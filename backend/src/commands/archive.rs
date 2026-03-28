//! # Archive commands
//!
//! Implements Tauri IPC commands for browsing and extracting compressed
//! archives (.zip, .tar.gz, .tgz).
//!
//! ## Supported formats
//!
//! - **ZIP** — handled via the `zip` crate.
//! - **tar.gz / .tgz** — handled via `flate2` (gzip decompression) + `tar`.
//!
//! ## Operations
//!
//! - [`list_archive`] — lists entries inside an archive without extracting.
//! - [`extract_archive`] — extracts all entries to the archive's parent directory
//!   as a background job with progress tracking.
//!
//! Both commands delegate to `_inner` functions that accept a backend trait object,
//! making them testable without Tauri's `State` or `AppHandle`.

use std::io::{Cursor, Read};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::DiskDeckError;
use crate::models::entry::Entry;
use crate::models::job::{JobKind, JobStatus};
use crate::state::{AppState, JobRegistry};
use crate::storage::StorageBackend;

/// Detects the archive format from a file path's extension.
///
/// Returns `"zip"` for `.zip` files, `"tar.gz"` for `.tar.gz` and `.tgz` files,
/// or `None` if the extension is not a recognized archive format.
fn detect_format(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".zip") {
        Some("zip")
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Some("tar.gz")
    } else {
        None
    }
}

/// Computes the parent directory of an archive path.
///
/// For example, `/docs/archive.zip` returns `/docs`.
/// A root-level archive like `/archive.zip` returns `/`.
fn parent_dir(path: &str) -> String {
    if let Some(pos) = path.rfind('/') {
        if pos == 0 {
            "/".to_string()
        } else {
            path[..pos].to_string()
        }
    } else {
        "/".to_string()
    }
}

/// Lists entries inside a zip archive from raw bytes.
///
/// Each entry is mapped to an [`Entry`] struct with path, name, size, and
/// directory flag. Paths are presented relative to the archive root.
fn list_zip_entries(data: &[u8]) -> Result<Vec<Entry>, DiskDeckError> {
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| DiskDeckError::Storage(format!("Failed to read zip archive: {e}")))?;

    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| DiskDeckError::Storage(format!("Failed to read zip entry: {e}")))?;
        let raw_name = file.name().to_string();
        let is_dir = file.is_dir();
        let size = file.size();

        // Derive display name from the last non-empty path component
        let name = raw_name
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&raw_name)
            .to_string();

        if name.is_empty() {
            continue;
        }

        entries.push(Entry {
            path: format!("/{}", raw_name.trim_start_matches('/')),
            name,
            size,
            modified: None,
                    created: None,
            is_dir,
            permissions: None,
            mime_type: None,
        });
    }
    Ok(entries)
}

/// Lists entries inside a tar.gz archive from raw bytes.
///
/// Decompresses the gzip layer first, then iterates tar entries. Each entry
/// is mapped to an [`Entry`] struct.
fn list_tar_gz_entries(data: &[u8]) -> Result<Vec<Entry>, DiskDeckError> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(decoder);

    let entries_iter = archive
        .entries()
        .map_err(|e| DiskDeckError::Storage(format!("Failed to read tar.gz archive: {e}")))?;

    let mut entries = Vec::new();
    for entry_result in entries_iter {
        let entry = entry_result
            .map_err(|e| DiskDeckError::Storage(format!("Failed to read tar entry: {e}")))?;
        let raw_path = entry
            .path()
            .map_err(|e| DiskDeckError::Storage(format!("Invalid tar entry path: {e}")))?
            .to_string_lossy()
            .to_string();
        let is_dir = entry.header().entry_type().is_dir();
        let size = entry.header().size().unwrap_or(0);

        let name = raw_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(&raw_path)
            .to_string();

        if name.is_empty() {
            continue;
        }

        entries.push(Entry {
            path: format!("/{}", raw_path.trim_start_matches('/')),
            name,
            size,
            modified: None,
                    created: None,
            is_dir,
            permissions: None,
            mime_type: None,
        });
    }
    Ok(entries)
}

/// Core logic for listing archive contents, testable without Tauri State.
///
/// Reads the archive bytes from the backend, detects the format by extension,
/// and delegates to the appropriate format-specific listing function.
///
/// # Errors
///
/// Returns [`DiskDeckError::Storage`] if the archive format is unrecognized
/// or if the archive data is malformed.
pub(crate) async fn list_archive_inner(
    backend: &dyn StorageBackend,
    path: &str,
) -> Result<Vec<Entry>, DiskDeckError> {
    let format = detect_format(path)
        .ok_or_else(|| DiskDeckError::Storage("Unsupported archive format".to_string()))?;
    let data = backend.read(path).await?;
    match format {
        "zip" => list_zip_entries(&data),
        "tar.gz" => list_tar_gz_entries(&data),
        _ => Err(DiskDeckError::Storage("Unsupported archive format".to_string())),
    }
}

/// An archive entry's metadata and content, collected synchronously so that
/// non-Send archive iterators (zip's `ZipFile`, tar's `Entries`) do not need
/// to be held across await points.
struct ArchiveEntryData {
    /// Destination path for this entry on the backend.
    path: String,
    /// Whether this entry is a directory.
    is_dir: bool,
    /// File content bytes (empty for directories).
    content: Vec<u8>,
}

/// Collects all entries from a zip archive synchronously into owned structs.
///
/// Returns an error message string if any entry cannot be read.
fn collect_zip_entries(data: &[u8], dest: &str) -> Result<Vec<ArchiveEntryData>, String> {
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("Failed to read zip: {e}"))?;

    let mut collected = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {e}"))?;

        let raw_name = file.name().to_string();
        let entry_path = format!(
            "{}/{}",
            dest.trim_end_matches('/'),
            raw_name.trim_start_matches('/')
        );
        let is_dir = file.is_dir();

        let mut content = Vec::new();
        if !is_dir {
            file.read_to_end(&mut content)
                .map_err(|e| format!("Failed to read zip entry data: {e}"))?;
        }

        collected.push(ArchiveEntryData {
            path: entry_path,
            is_dir,
            content,
        });
    }
    Ok(collected)
}

/// Collects all entries from a tar.gz archive synchronously into owned structs.
///
/// Returns an error message string if any entry cannot be read.
fn collect_tar_gz_entries(data: &[u8], dest: &str) -> Result<Vec<ArchiveEntryData>, String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(decoder);

    let entries_iter = archive
        .entries()
        .map_err(|e| format!("Failed to read tar.gz: {e}"))?;

    let mut collected = Vec::new();
    for entry_result in entries_iter {
        let mut entry = entry_result
            .map_err(|e| format!("Failed to read tar entry: {e}"))?;

        let raw_path = entry
            .path()
            .map_err(|e| format!("Invalid tar entry path: {e}"))?
            .to_string_lossy()
            .to_string();

        let entry_path = format!(
            "{}/{}",
            dest.trim_end_matches('/'),
            raw_path.trim_start_matches('/')
        );

        let is_dir = entry.header().entry_type().is_dir();
        let mut content = Vec::new();
        if !is_dir {
            entry.read_to_end(&mut content)
                .map_err(|e| format!("Failed to read tar entry data: {e}"))?;
        }

        collected.push(ArchiveEntryData {
            path: entry_path,
            is_dir,
            content,
        });
    }
    Ok(collected)
}

/// Core logic for extracting archive contents, testable without Tauri's AppHandle.
///
/// Reads the archive bytes from the backend, collects all entries synchronously
/// (to avoid holding non-Send archive iterators across await points), then writes
/// each file/directory to the backend asynchronously with progress tracking.
///
/// # Arguments
///
/// * `backend` — The storage backend to read from and write to.
/// * `registry` — The job registry for progress/status updates.
/// * `job_id` — The job ID for progress tracking.
/// * `cancel_flag` — Cooperative cancellation flag.
/// * `path` — Path to the archive file on the backend.
/// * `dest` — Destination directory where contents will be extracted.
pub(crate) async fn extract_archive_inner(
    backend: &dyn StorageBackend,
    registry: &Arc<JobRegistry>,
    job_id: &str,
    cancel_flag: &std::sync::atomic::AtomicBool,
    path: &str,
    dest: &str,
) -> JobStatus {
    let format = match detect_format(path) {
        Some(f) => f,
        None => {
            registry
                .finish(
                    job_id,
                    JobStatus::Failed,
                    Some("Unsupported archive format".to_string()),
                )
                .await;
            return JobStatus::Failed;
        }
    };

    let data = match backend.read(path).await {
        Ok(d) => d,
        Err(e) => {
            registry
                .finish(job_id, JobStatus::Failed, Some(e.to_string()))
                .await;
            return JobStatus::Failed;
        }
    };

    // Collect all entries synchronously to avoid holding non-Send iterators across awaits
    let collected = match format {
        "zip" => collect_zip_entries(&data, dest),
        "tar.gz" => collect_tar_gz_entries(&data, dest),
        _ => Err("Unsupported archive format".to_string()),
    };

    let entries = match collected {
        Ok(e) => e,
        Err(msg) => {
            registry
                .finish(job_id, JobStatus::Failed, Some(msg))
                .await;
            return JobStatus::Failed;
        }
    };

    // Write collected entries to the backend asynchronously with progress
    for (i, entry_data) in entries.iter().enumerate() {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            registry.finish(job_id, JobStatus::Cancelled, None).await;
            return JobStatus::Cancelled;
        }

        registry
            .update_progress(job_id, i as u32, &entry_data.path)
            .await;

        if entry_data.is_dir {
            if let Err(e) = backend.create_dir(&entry_data.path).await {
                registry
                    .finish(job_id, JobStatus::Failed, Some(e.to_string()))
                    .await;
                return JobStatus::Failed;
            }
        } else {
            // Ensure parent directory exists
            let parent = parent_dir(&entry_data.path);
            if parent != "/" {
                let _ = backend.create_dir(&parent).await;
            }
            if let Err(e) = backend.write(&entry_data.path, &entry_data.content).await {
                registry
                    .finish(job_id, JobStatus::Failed, Some(e.to_string()))
                    .await;
                return JobStatus::Failed;
            }
        }
    }

    registry.finish(job_id, JobStatus::Completed, None).await;
    JobStatus::Completed
}

use super::file::get_backend;

/// Emits the current job state to the frontend as a `"job-update"` event.
async fn emit_job_update(app: &AppHandle, registry: &Arc<JobRegistry>, job_id: &str) {
    if let Some(info) = registry.get(job_id).await {
        let _ = app.emit("job-update", &info);
    }
}

/// Marks a disk's search index as stale so it will be re-indexed.
fn mark_stale(state: &AppState, disk_id: &str) {
    if let Err(e) = state.store.set_index_meta(disk_id, "stale", 0) {
        log::warn!("Index mark stale error: {e}");
    }
}

/// Lists the entries inside a .zip or .tar.gz archive.
///
/// Returns a flat list of [`Entry`] structs representing the archive contents.
/// Each entry includes path, name, size, and whether it is a directory.
/// The archive is not extracted — this is a read-only listing operation.
///
/// # Arguments
///
/// * `disk_id` — UUID of the disk containing the archive.
/// * `path` — Storage-relative path to the archive file.
///
/// # Errors
///
/// Returns an error if the archive format is unsupported, the file cannot be
/// read, or the archive data is malformed.
#[tauri::command]
pub async fn list_archive(
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<Vec<Entry>, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    list_archive_inner(backend.as_ref(), &path).await
}

/// Extracts a .zip or .tar.gz archive to its parent directory on the backend.
///
/// The extraction runs as a background job with progress tracking. Returns the
/// job ID immediately while extraction proceeds in a spawned async task.
///
/// Each archive entry is written to `<parent_dir>/<entry_path>`. Directories
/// are created as needed. The disk's search index is marked stale on completion
/// so that newly extracted files become searchable.
///
/// # Arguments
///
/// * `disk_id` — UUID of the disk containing the archive.
/// * `path` — Storage-relative path to the archive file.
///
/// # Returns
///
/// The job ID (UUID string) for progress tracking and cancellation.
#[tauri::command]
pub async fn extract_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    disk_id: String,
    path: String,
) -> Result<String, DiskDeckError> {
    let backend = get_backend(&state, &disk_id).await?;
    let dest = parent_dir(&path);

    let archive_name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let desc = format!("Extracting {archive_name}");

    // Use a rough estimate for total entries; the job will update progress as it goes
    let (job_id, cancel_flag) = state.jobs.create(JobKind::Copy, 0, desc, disk_id.clone(), dest.clone()).await;
    let registry = state.jobs.clone();

    emit_job_update(&app, &registry, &job_id).await;

    let app2 = app.clone();
    let jid = job_id.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        extract_archive_inner(
            backend.as_ref(),
            &registry,
            &jid,
            &cancel_flag,
            &path,
            &dest,
        )
        .await;
        emit_job_update(&app2, &registry, &jid).await;

        let state = app_handle.state::<AppState>();
        mark_stale(&state, &disk_id);
    });

    Ok(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::JobRegistry;
    use crate::storage::memory::MemoryBackend;
    use std::io::Write;

    /// Creates a zip archive in memory with the given file entries.
    /// Each entry is a tuple of (path, content_bytes).
    fn create_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (path, content) in entries {
            if path.ends_with('/') {
                writer
                    .add_directory(*path, options)
                    .expect("failed to add directory");
            } else {
                writer
                    .start_file(*path, options)
                    .expect("failed to start file");
                writer.write_all(content).expect("failed to write content");
            }
        }

        writer
            .finish()
            .expect("failed to finish zip")
            .into_inner()
    }

    /// Creates a tar.gz archive in memory with the given file entries.
    fn create_test_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(buf, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);

        for (path, content) in entries {
            if path.ends_with('/') {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_cksum();
                builder
                    .append_data(&mut header, *path, &[][..])
                    .expect("failed to add tar directory");
            } else {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_entry_type(tar::EntryType::Regular);
                header.set_cksum();
                builder
                    .append_data(&mut header, *path, *content)
                    .expect("failed to add tar file");
            }
        }

        let encoder = builder.into_inner().expect("failed to finish tar");
        encoder.finish().expect("failed to finish gzip")
    }

    #[test]
    fn detect_format_zip() {
        assert_eq!(detect_format("/archive.zip"), Some("zip"));
        assert_eq!(detect_format("/ARCHIVE.ZIP"), Some("zip"));
    }

    #[test]
    fn detect_format_tar_gz() {
        assert_eq!(detect_format("/archive.tar.gz"), Some("tar.gz"));
        assert_eq!(detect_format("/archive.tgz"), Some("tar.gz"));
        assert_eq!(detect_format("/ARCHIVE.TGZ"), Some("tar.gz"));
    }

    #[test]
    fn detect_format_unknown() {
        assert_eq!(detect_format("/file.txt"), None);
        assert_eq!(detect_format("/image.png"), None);
    }

    #[test]
    fn parent_dir_of_nested_path() {
        assert_eq!(parent_dir("/docs/archive.zip"), "/docs");
    }

    #[test]
    fn parent_dir_of_root_level_file() {
        assert_eq!(parent_dir("/archive.zip"), "/");
    }

    #[test]
    fn parent_dir_of_deep_path() {
        assert_eq!(parent_dir("/a/b/c/file.tar.gz"), "/a/b/c");
    }

    #[tokio::test]
    async fn list_zip_archive() {
        let backend = MemoryBackend::new();
        let zip_data = create_test_zip(&[
            ("hello.txt", b"Hello, World!"),
            ("subdir/", &[]),
            ("subdir/nested.txt", b"nested content"),
        ]);
        backend.write("/test.zip", &zip_data).await.unwrap();

        let entries = list_archive_inner(&backend, "/test.zip").await.unwrap();
        assert!(entries.len() >= 2);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"hello.txt"));
        assert!(names.contains(&"nested.txt"));

        // Verify file sizes
        let hello = entries.iter().find(|e| e.name == "hello.txt").unwrap();
        assert_eq!(hello.size, 13);
        assert!(!hello.is_dir);

        // Verify directory flag
        let subdir = entries.iter().find(|e| e.name == "subdir");
        if let Some(dir) = subdir {
            assert!(dir.is_dir);
        }
    }

    #[tokio::test]
    async fn list_tar_gz_archive() {
        let backend = MemoryBackend::new();
        let tar_data = create_test_tar_gz(&[
            ("readme.md", b"# README"),
            ("src/", &[]),
            ("src/main.rs", b"fn main() {}"),
        ]);
        backend.write("/project.tar.gz", &tar_data).await.unwrap();

        let entries = list_archive_inner(&backend, "/project.tar.gz").await.unwrap();
        assert!(entries.len() >= 2);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"readme.md"));
        assert!(names.contains(&"main.rs"));

        let readme = entries.iter().find(|e| e.name == "readme.md").unwrap();
        assert_eq!(readme.size, 8);
        assert!(!readme.is_dir);
    }

    #[tokio::test]
    async fn list_tgz_archive() {
        let backend = MemoryBackend::new();
        let tar_data = create_test_tar_gz(&[("file.txt", b"content")]);
        backend.write("/archive.tgz", &tar_data).await.unwrap();

        let entries = list_archive_inner(&backend, "/archive.tgz").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
    }

    #[tokio::test]
    async fn list_archive_unsupported_format() {
        let backend = MemoryBackend::new();
        backend.write("/file.txt", b"not an archive").await.unwrap();

        let result = list_archive_inner(&backend, "/file.txt").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_archive_malformed_zip() {
        let backend = MemoryBackend::new();
        backend.write("/bad.zip", b"not valid zip data").await.unwrap();

        let result = list_archive_inner(&backend, "/bad.zip").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn extract_zip_archive() {
        let backend = MemoryBackend::new();
        let zip_data = create_test_zip(&[
            ("hello.txt", b"Hello!"),
            ("subdir/", &[]),
            ("subdir/nested.txt", b"nested"),
        ]);
        backend.write("/test.zip", &zip_data).await.unwrap();

        let registry = Arc::new(JobRegistry::new());
        let (job_id, cancel_flag) = registry
            .create(JobKind::Copy, 0, "Extracting test.zip".into(), "d".into(), "/".into())
            .await;

        let status = extract_archive_inner(
            &backend,
            &registry,
            &job_id,
            &cancel_flag,
            "/test.zip",
            "/",
        )
        .await;

        assert_eq!(status, JobStatus::Completed);

        // Verify extracted files exist
        let hello = backend.read("/hello.txt").await.unwrap();
        assert_eq!(hello, b"Hello!");

        let nested = backend.read("/subdir/nested.txt").await.unwrap();
        assert_eq!(nested, b"nested");
    }

    #[tokio::test]
    async fn extract_tar_gz_archive() {
        let backend = MemoryBackend::new();
        let tar_data = create_test_tar_gz(&[
            ("readme.md", b"# README"),
            ("src/", &[]),
            ("src/lib.rs", b"pub fn hello() {}"),
        ]);
        backend.write("/project.tar.gz", &tar_data).await.unwrap();

        let registry = Arc::new(JobRegistry::new());
        let (job_id, cancel_flag) = registry
            .create(JobKind::Copy, 0, "Extracting project.tar.gz".into(), "d".into(), "/".into())
            .await;

        let status = extract_archive_inner(
            &backend,
            &registry,
            &job_id,
            &cancel_flag,
            "/project.tar.gz",
            "/",
        )
        .await;

        assert_eq!(status, JobStatus::Completed);

        let readme = backend.read("/readme.md").await.unwrap();
        assert_eq!(readme, b"# README");

        let lib = backend.read("/src/lib.rs").await.unwrap();
        assert_eq!(lib, b"pub fn hello() {}");
    }

    #[tokio::test]
    async fn extract_to_subdirectory() {
        let backend = MemoryBackend::new();
        backend.create_dir("/output").await.unwrap();

        let zip_data = create_test_zip(&[("file.txt", b"data")]);
        backend.write("/output/archive.zip", &zip_data).await.unwrap();

        let registry = Arc::new(JobRegistry::new());
        let (job_id, cancel_flag) = registry
            .create(JobKind::Copy, 0, "Extracting archive.zip".into(), "d".into(), "/".into())
            .await;

        let status = extract_archive_inner(
            &backend,
            &registry,
            &job_id,
            &cancel_flag,
            "/output/archive.zip",
            "/output",
        )
        .await;

        assert_eq!(status, JobStatus::Completed);
        let content = backend.read("/output/file.txt").await.unwrap();
        assert_eq!(content, b"data");
    }

    #[tokio::test]
    async fn extract_cancellation() {
        let backend = MemoryBackend::new();
        let zip_data = create_test_zip(&[
            ("a.txt", b"aaa"),
            ("b.txt", b"bbb"),
            ("c.txt", b"ccc"),
        ]);
        backend.write("/cancel.zip", &zip_data).await.unwrap();

        let registry = Arc::new(JobRegistry::new());
        let (job_id, cancel_flag) = registry
            .create(JobKind::Copy, 0, "Extracting cancel.zip".into(), "d".into(), "/".into())
            .await;

        // Set cancel flag before extraction starts
        cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);

        let status = extract_archive_inner(
            &backend,
            &registry,
            &job_id,
            &cancel_flag,
            "/cancel.zip",
            "/",
        )
        .await;

        assert_eq!(status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn extract_unsupported_format_fails() {
        let backend = MemoryBackend::new();
        backend.write("/file.txt", b"not an archive").await.unwrap();

        let registry = Arc::new(JobRegistry::new());
        let (job_id, cancel_flag) = registry
            .create(JobKind::Copy, 0, "Extracting file.txt".into(), "d".into(), "/".into())
            .await;

        let status = extract_archive_inner(
            &backend,
            &registry,
            &job_id,
            &cancel_flag,
            "/file.txt",
            "/",
        )
        .await;

        assert_eq!(status, JobStatus::Failed);
    }

    #[test]
    fn list_zip_entries_with_content() {
        let data = create_test_zip(&[
            ("a.txt", b"alpha"),
            ("b.txt", b"beta"),
        ]);
        let entries = list_zip_entries(&data).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a.txt");
        assert_eq!(entries[1].name, "b.txt");
    }

    #[test]
    fn list_tar_gz_entries_with_content() {
        let data = create_test_tar_gz(&[
            ("x.txt", b"x-content"),
            ("y.txt", b"y-content"),
        ]);
        let entries = list_tar_gz_entries(&data).unwrap();
        assert_eq!(entries.len(), 2);
    }
}
