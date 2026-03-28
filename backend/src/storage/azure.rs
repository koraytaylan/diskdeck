//! # Azure Blob Storage backend
//!
//! Implements [`StorageBackend`] for Azure Blob Storage containers.
//!
//! ## Azure Blob API mapping
//!
//! Azure Blob Storage, like S3, is a flat key-value store. DiskDeck maps its
//! virtual directory hierarchy onto Azure's blob namespace:
//!
//! - **Listing**: Uses `list_blobs().prefix(...).delimiter("/")` which returns
//!   `BlobPrefix` items (virtual folders) and `Blob` items (files).
//! - **Creating folders**: Writes a zero-byte block blob with a trailing-slash
//!   name (same convention as S3).
//! - **Deleting folders**: Lists all blobs under the prefix and deletes each one.
//! - **Renaming**: Copy-from-URL then delete, since Azure Blob has no rename API.
//! - **Copying**: Uses `copy_from_url` with the source blob's public URL,
//!   constructed from the account name, container, and key.
//!
//! ## Key normalization
//!
//! Same pattern as the S3 backend: DiskDeck virtual paths have a leading `/`
//! which is stripped before Azure API calls and restored when building [`Entry`]
//! paths.
//!
//! ## Streaming reads
//!
//! `read()` collects all chunks from Azure's streaming response into a single
//! `Vec<u8>`. For very large files this loads everything into memory.

use async_trait::async_trait;
use azure_storage::prelude::*;
use azure_storage_blobs::prelude::*;
use futures::StreamExt;

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::search::SearchQuery;

use super::{StorageBackend, StorageResult};

/// Azure Blob Storage backend.
///
/// Holds a container client (scoped to one container within a storage account)
/// along with the account and container names for URL construction.
pub struct AzureBackend {
    /// Pre-configured client for the target container.
    container_client: ContainerClient,
    /// Storage account name, used when building copy-source URLs.
    account: String,
    /// Container name, used when building copy-source URLs.
    container: String,
}

impl AzureBackend {
    /// Creates a new AzureBackend from storage account credentials.
    ///
    /// # Arguments
    ///
    /// * `account` — Azure storage account name.
    /// * `access_key` — Storage account access key (base64-encoded).
    /// * `container` — Blob container name to operate on.
    ///
    /// # Errors
    ///
    /// Returns `DiskDeckError` if the container client cannot be constructed
    /// (typically from invalid credential format).
    pub fn new(account: &str, access_key: &str, container: &str) -> Result<Self, DiskDeckError> {
        let creds =
            StorageCredentials::access_key(account.to_string(), access_key.to_string());
        let container_client =
            BlobServiceClient::new(account, creds).container_client(container);
        Ok(Self {
            container_client,
            account: account.to_string(),
            container: container.to_string(),
        })
    }

    /// Strips the leading `/` from a virtual path to produce a valid blob name.
    fn normalize_key(path: &str) -> &str {
        path.strip_prefix('/').unwrap_or(path)
    }

    /// Converts a blob name back to a virtual absolute-style path.
    fn key_to_path(key: &str) -> String {
        format!("/{}", key)
    }

    /// Extracts the last path component from a blob name, handling trailing slashes.
    fn key_name(key: &str) -> String {
        let trimmed = key.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
    }
}

#[async_trait]
impl StorageBackend for AzureBackend {
    /// Lists blobs and virtual directories at the given prefix.
    ///
    /// Uses Azure's `list_blobs` with a delimiter to get both blob prefixes
    /// (folders) and blobs (files) in a single paginated stream.
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>> {
        let mut prefix = Self::normalize_key(path).to_string();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }

        let mut entries = Vec::new();
        let mut stream = self
            .container_client
            .list_blobs()
            .prefix(prefix.clone())
            .delimiter("/")
            .into_stream();

        while let Some(response) = stream.next().await {
            let response = response.map_err(|e| DiskDeckError::Storage(e.to_string()))?;

            // Folders (blob prefixes)
            for bp in response.blobs.prefixes() {
                let name = Self::key_name(&bp.name);
                if !name.is_empty() {
                    entries.push(Entry {
                        path: Self::key_to_path(bp.name.trim_end_matches('/')),
                        name,
                        size: 0,
                        modified: None,
                        created: None,
                        is_dir: true,
                        permissions: None,
                        mime_type: None,
                    });
                }
            }

            // Files (blobs)
            for blob in response.blobs.blobs() {
                // Skip the prefix marker blob itself
                if blob.name == prefix {
                    continue;
                }
                let name = Self::key_name(&blob.name);
                if name.is_empty() {
                    continue;
                }
                let modified = Some(blob.properties.last_modified.unix_timestamp());
                let mime = mime_guess::from_path(&name)
                    .first()
                    .map(|m| m.to_string());

                entries.push(Entry {
                    path: Self::key_to_path(&blob.name),
                    name,
                    size: blob.properties.content_length,
                    modified,
                    created: None,
                    is_dir: false,
                    permissions: None,
                    mime_type: mime,
                });
            }
        }

        Ok(entries)
    }

    /// Reads a blob's full contents by collecting all chunks from the download stream.
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let key = Self::normalize_key(path);
        let mut data = Vec::new();
        let mut stream = self
            .container_client
            .blob_client(key)
            .get()
            .into_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DiskDeckError::Storage(e.to_string()))?;
            let body = chunk
                .data
                .collect()
                .await
                .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
            data.extend_from_slice(&body);
        }

        Ok(data)
    }

    /// Uploads data as a block blob, creating or overwriting.
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let key = Self::normalize_key(path);
        self.container_client
            .blob_client(key)
            .put_block_blob(data.to_vec())
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Deletes a blob or all blobs under a "folder" prefix.
    ///
    /// First checks if the path is a folder prefix (has child blobs). If yes,
    /// deletes all children individually. Otherwise deletes the single blob.
    async fn delete(&self, path: &str) -> StorageResult<()> {
        let key = Self::normalize_key(path);

        // Check if it's a "folder" by listing with prefix
        let folder_prefix = if key.ends_with('/') {
            key.to_string()
        } else {
            format!("{}/", key)
        };

        let mut keys_to_delete = Vec::new();
        let mut stream = self
            .container_client
            .list_blobs()
            .prefix(folder_prefix)
            .into_stream();

        while let Some(response) = stream.next().await {
            let response = response.map_err(|e| DiskDeckError::Storage(e.to_string()))?;
            for blob in response.blobs.blobs() {
                keys_to_delete.push(blob.name.clone());
            }
        }

        if keys_to_delete.is_empty() {
            // Single blob deletion
            self.container_client
                .blob_client(key)
                .delete()
                .await
                .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        } else {
            for k in keys_to_delete {
                self.container_client
                    .blob_client(k)
                    .delete()
                    .await
                    .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Copies a blob using Azure's server-side `copy_from_url`.
    ///
    /// The source URL is constructed from the account, container, and source key.
    /// This is a synchronous (blocking) copy for blobs in the same account.
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()> {
        let src_key = Self::normalize_key(src);
        let dst_key = Self::normalize_key(dst);
        let source_url = format!(
            "https://{}.blob.core.windows.net/{}/{}",
            self.account, self.container, src_key
        );
        let url =
            url::Url::parse(&source_url).map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        self.container_client
            .blob_client(dst_key)
            .copy_from_url(url)
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Renames by copying then deleting (Azure Blob has no native rename).
    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()> {
        self.copy(src, dst).await?;
        self.delete(src).await?;
        Ok(())
    }

    /// Gets blob properties (metadata) for a single blob.
    async fn stat(&self, path: &str) -> StorageResult<Entry> {
        let key = Self::normalize_key(path);
        let resp = self
            .container_client
            .blob_client(key)
            .get_properties()
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;

        let name = Self::key_name(key);
        let modified = Some(resp.blob.properties.last_modified.unix_timestamp());
        let mime = mime_guess::from_path(&name)
            .first()
            .map(|m| m.to_string());

        Ok(Entry {
            path: Self::key_to_path(key),
            name,
            size: resp.blob.properties.content_length,
            modified,
            created: None,
            is_dir: false,
            permissions: None,
            mime_type: mime,
        })
    }

    /// Checks existence by attempting to get blob properties.
    async fn exists(&self, path: &str) -> StorageResult<bool> {
        let key = Self::normalize_key(path);
        match self
            .container_client
            .blob_client(key)
            .get_properties()
            .await
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Brute-force search: lists all blobs in the container and filters by name.
    ///
    /// This is the live-search fallback for unindexed Azure disks.
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>> {
        let mut entries = Vec::new();
        let pattern = query.pattern.to_lowercase();

        let mut stream = self.container_client.list_blobs().into_stream();
        while let Some(response) = stream.next().await {
            let response = response.map_err(|e| DiskDeckError::Storage(e.to_string()))?;
            for blob in response.blobs.blobs() {
                let name = Self::key_name(&blob.name);
                if name.to_lowercase().contains(&pattern) {
                    let modified = Some(blob.properties.last_modified.unix_timestamp());
                    let mime = mime_guess::from_path(&name)
                        .first()
                        .map(|m| m.to_string());

                    entries.push(Entry {
                        path: Self::key_to_path(&blob.name),
                        name,
                        size: blob.properties.content_length,
                        modified,
                        created: None,
                        is_dir: false,
                        permissions: None,
                        mime_type: mime,
                    });
                }
            }
        }

        Ok(entries)
    }

    /// Creates a "folder" by writing a zero-byte block blob with a trailing-slash name.
    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let mut key = Self::normalize_key(path).to_string();
        if !key.ends_with('/') {
            key.push('/');
        }
        self.container_client
            .blob_client(key)
            .put_block_blob(Vec::<u8>::new())
            .await
            .map_err(|e| DiskDeckError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_key_strips_leading_slash() {
        assert_eq!(AzureBackend::normalize_key("/photos/sunset.jpg"), "photos/sunset.jpg");
    }

    #[test]
    fn normalize_key_no_slash() {
        assert_eq!(AzureBackend::normalize_key("photos/sunset.jpg"), "photos/sunset.jpg");
    }

    #[test]
    fn normalize_key_just_slash() {
        assert_eq!(AzureBackend::normalize_key("/"), "");
    }

    #[test]
    fn normalize_key_empty() {
        assert_eq!(AzureBackend::normalize_key(""), "");
    }

    #[test]
    fn key_to_path_adds_slash() {
        assert_eq!(AzureBackend::key_to_path("photos/sunset.jpg"), "/photos/sunset.jpg");
    }

    #[test]
    fn key_to_path_empty() {
        assert_eq!(AzureBackend::key_to_path(""), "/");
    }

    #[test]
    fn key_name_simple() {
        assert_eq!(AzureBackend::key_name("photos/sunset.jpg"), "sunset.jpg");
    }

    #[test]
    fn key_name_trailing_slash() {
        assert_eq!(AzureBackend::key_name("documents/reports/"), "reports");
    }

    #[test]
    fn key_name_no_slash() {
        assert_eq!(AzureBackend::key_name("file.txt"), "file.txt");
    }

    #[test]
    fn key_name_empty() {
        assert_eq!(AzureBackend::key_name(""), "");
    }

    #[test]
    fn new_creates_backend() {
        let result = AzureBackend::new("myaccount", "dGVzdGtleQ==", "mycontainer");
        assert!(result.is_ok());
        let backend = result.unwrap();
        assert_eq!(backend.account, "myaccount");
        assert_eq!(backend.container, "mycontainer");
    }
}
