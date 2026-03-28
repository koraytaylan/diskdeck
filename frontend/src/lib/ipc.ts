/**
 * @file Tauri IPC Wrappers
 *
 * Thin, typed wrappers around `@tauri-apps/api/core.invoke()` for every
 * backend command exposed by the Rust Tauri application. Each function maps
 * 1:1 to a `#[tauri::command]` handler on the backend.
 *
 * **Return type convention for mutating file operations:**
 * `copyEntries`, `moveEntries`, and `deleteEntries` return a `string` --
 * this is a *job ID*. The backend spawns these as background tasks and
 * emits `JobInfo` events via `app.emit("job-update", ...)` as they
 * progress. The frontend tracks these in `JobContext`. Use `cancelJob`
 * to abort a running job, or `listJobs` to hydrate state on startup.
 *
 * **Error handling:**
 * All functions propagate Tauri invoke errors as rejected promises.
 * Callers (contexts, components) are responsible for catching and
 * displaying errors.
 *
 * @module lib/ipc
 */

import { invoke } from "@tauri-apps/api/core";
import type { Bookmark, DiffEntry, DiskConfig, Entry, IndexStatus, JobInfo, SearchQuery, SearchResult, SizeEntry } from "./types";

// ─── Disk Commands ───────────────────────────────────────────────────────────

/**
 * Fetch all configured disks from the backend database.
 * @returns Array of all disk configurations, ordered by creation time.
 */
export async function listDisks(): Promise<DiskConfig[]> {
  return invoke<DiskConfig[]>("list_disks");
}

/**
 * Register a new storage backend (disk).
 * The backend validates the config, persists it, and returns the new disk
 * with a generated `id`.
 *
 * @param name     - User-facing display name.
 * @param diskType - Storage backend type (e.g. "local", "s3").
 * @param config   - Backend-specific configuration key/value pairs.
 * @returns The newly created DiskConfig with its assigned ID.
 */
export async function createDisk(
  name: string,
  diskType: string,
  config: Record<string, unknown>,
): Promise<DiskConfig> {
  return invoke<DiskConfig>("create_disk", {
    name,
    diskType,
    config,
  });
}

/**
 * Update an existing disk's name and/or configuration.
 * The disk type cannot be changed after creation.
 *
 * @param diskId - UUID of the disk to update.
 * @param name   - New display name.
 * @param config - New backend-specific configuration.
 * @returns The updated DiskConfig.
 */
export async function updateDisk(
  diskId: string,
  name: string,
  config: Record<string, unknown>,
): Promise<DiskConfig> {
  return invoke<DiskConfig>("update_disk", { diskId, name, config });
}

/**
 * Permanently remove a disk and its associated index data.
 * @param diskId - UUID of the disk to delete.
 */
export async function deleteDisk(diskId: string): Promise<void> {
  return invoke<void>("delete_disk", { diskId });
}

// ─── File Commands ───────────────────────────────────────────────────────────

/**
 * List the immediate children of a directory on a disk.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative directory path (e.g. "/" or "/docs").
 * @returns Array of entries (files and sub-directories) in the directory.
 */
export async function listEntries(
  diskId: string,
  path: string,
): Promise<Entry[]> {
  return invoke<Entry[]>("list_entries", { diskId, path });
}

/**
 * Fetch metadata for a single entry (file or directory).
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative path to the entry.
 * @returns The entry metadata.
 */
export async function getEntry(
  diskId: string,
  path: string,
): Promise<Entry> {
  return invoke<Entry>("get_entry", { diskId, path });
}

/**
 * Copy one or more entries to a destination directory.
 * The operation runs asynchronously on the backend; progress is reported
 * via Tauri events keyed by the returned operation ID.
 *
 * @param diskId - UUID of the disk (source and destination must be same disk).
 * @param paths  - Array of source entry paths to copy.
 * @param dest   - Destination directory path.
 * @returns An operation ID string for progress tracking / cancellation.
 */
export async function copyEntries(
  diskId: string,
  paths: string[],
  dest: string,
): Promise<string> {
  return invoke<string>("copy_entries", { diskId, paths, dest });
}

/**
 * Move (rename) one or more entries to a destination directory.
 * Returns an operation ID for progress tracking, same as `copyEntries`.
 *
 * @param diskId - UUID of the disk (same-disk moves only).
 * @param paths  - Array of source entry paths to move.
 * @param dest   - Destination directory path.
 * @returns An operation ID string for progress tracking / cancellation.
 */
export async function moveEntries(
  diskId: string,
  paths: string[],
  dest: string,
): Promise<string> {
  return invoke<string>("move_entries", { diskId, paths, dest });
}

/**
 * Delete one or more entries permanently.
 * Returns an operation ID for progress tracking.
 *
 * @param diskId - UUID of the disk containing the entries.
 * @param paths  - Array of entry paths to delete.
 * @returns An operation ID string for progress tracking / cancellation.
 */
export async function deleteEntries(
  diskId: string,
  paths: string[],
): Promise<string> {
  return invoke<string>("delete_entries", { diskId, paths });
}

/**
 * Copy entries from one disk to another.
 * Like `copyEntries`, the operation runs asynchronously on the backend
 * and returns a job ID for progress tracking / cancellation.
 *
 * @param srcDiskId - UUID of the source disk.
 * @param dstDiskId - UUID of the destination disk.
 * @param paths     - Array of source entry paths to copy.
 * @param dest      - Destination directory path on the target disk.
 * @returns A job ID string for progress tracking / cancellation.
 */
export async function crossCopyEntries(
  srcDiskId: string,
  dstDiskId: string,
  paths: string[],
  dest: string,
): Promise<string> {
  return invoke<string>("cross_copy_entries", { srcDiskId, dstDiskId, paths, dest });
}

/**
 * Move entries from one disk to another.
 * Like `moveEntries`, the operation runs asynchronously on the backend
 * and returns a job ID for progress tracking / cancellation.
 *
 * @param srcDiskId - UUID of the source disk.
 * @param dstDiskId - UUID of the destination disk.
 * @param paths     - Array of source entry paths to move.
 * @param dest      - Destination directory path on the target disk.
 * @returns A job ID string for progress tracking / cancellation.
 */
export async function crossMoveEntries(
  srcDiskId: string,
  dstDiskId: string,
  paths: string[],
  dest: string,
): Promise<string> {
  return invoke<string>("cross_move_entries", { srcDiskId, dstDiskId, paths, dest });
}

/**
 * Read the raw bytes of a file from a disk.
 * Used by the file preview system to fetch file contents for display.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative path to the file.
 * @returns The file contents as an array of bytes.
 */
export async function readFile(
  diskId: string,
  path: string,
): Promise<number[]> {
  return invoke<number[]>("read_file", { diskId, path });
}

/**
 * Write raw bytes to a file on a disk.
 * Creates the file if it does not exist, or overwrites if it does.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative path to the file.
 * @param data   - File contents as an array of bytes (mirrors Rust `Vec<u8>`).
 */
export async function writeFile(
  diskId: string,
  path: string,
  data: number[],
): Promise<void> {
  return invoke<void>("write_file", { diskId, path, data });
}

// ─── Job Commands ─────────────────────────────────────────────────────────────

/**
 * Fetch all jobs currently tracked by the backend (running and finished).
 * Used to hydrate the `JobContext` store on mount.
 *
 * @returns Array of job snapshots, including completed/failed/cancelled jobs.
 */
export async function listJobs(): Promise<JobInfo[]> {
  return invoke<JobInfo[]>("list_jobs");
}

/**
 * Cancel a running job by ID.
 * The backend sets a cancellation flag; the job stops at the next item
 * boundary (already-processed items are not rolled back).
 *
 * @param jobId - The job ID returned by copy/move/delete.
 */
export async function cancelJob(jobId: string): Promise<void> {
  return invoke<void>("cancel_job", { jobId });
}

/**
 * Remove all finished jobs (completed, failed, cancelled) from the
 * backend's tracking store.
 */
export async function clearFinishedJobs(): Promise<void> {
  return invoke<void>("clear_finished_jobs");
}

/**
 * Rename a single file or directory.
 *
 * @param diskId  - UUID of the disk containing the entry.
 * @param path    - Current storage-relative path of the entry.
 * @param newName - New basename (not a full path).
 */
export async function renameEntry(
  diskId: string,
  path: string,
  newName: string,
): Promise<void> {
  return invoke<void>("rename_entry", { diskId, path, newName });
}

/**
 * Create a new empty directory.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Full storage-relative path for the new folder (e.g. "/docs/new-folder").
 */
export async function createFolder(
  diskId: string,
  path: string,
): Promise<void> {
  return invoke<void>("create_folder", { diskId, path });
}

// ─── Search Commands ─────────────────────────────────────────────────────────

/**
 * Search for entries matching a pattern across one or more disks.
 * Uses the backend's search index for fast lookups.
 *
 * @param query - Search parameters (pattern, disk filter, recursion flag).
 * @returns Array of result groups, one per disk that had matches.
 */
export async function searchEntries(
  query: SearchQuery,
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search_entries", { query });
}

/**
 * Trigger a full re-index of a disk's search index.
 * The operation is asynchronous; monitor `IndexStatus` for completion.
 *
 * @param diskId - UUID of the disk to re-index.
 */
export async function reindexDisk(diskId: string): Promise<void> {
  return invoke<void>("reindex_disk", { diskId });
}

/**
 * Get the current index status for all registered disks.
 * @returns Array of index statuses (one per disk).
 */
export async function getIndexStatus(): Promise<IndexStatus[]> {
  return invoke<IndexStatus[]>("get_index_status");
}

// ─── Bookmark Commands ───────────────────────────────────────────────────────

/**
 * Fetch all bookmarks from the backend database.
 * @returns Array of bookmarks, ordered by creation time (newest first).
 */
export async function listBookmarks(): Promise<Bookmark[]> {
  return invoke<Bookmark[]>("list_bookmarks");
}

/**
 * Create a new bookmark for a directory path on a disk.
 *
 * @param diskId   - UUID of the target disk.
 * @param diskName - Display name of the disk.
 * @param path     - Storage-relative directory path.
 * @param label    - User-facing label (non-empty, max 255 chars).
 * @returns The newly created Bookmark.
 */
export async function addBookmark(
  diskId: string,
  diskName: string,
  path: string,
  label: string,
): Promise<Bookmark> {
  return invoke<Bookmark>("add_bookmark", { diskId, diskName, path, label });
}

/**
 * Remove a bookmark by its ID.
 * @param bookmarkId - UUID of the bookmark to remove.
 */
export async function removeBookmark(bookmarkId: string): Promise<void> {
  return invoke<void>("remove_bookmark", { bookmarkId });
}

// ─── Profile Commands ─────────────────────────────────────────────────────

/**
 * Export all disk configurations as JSON with credentials stripped.
 * The returned string can be saved to a file or shared with team members.
 *
 * @returns Pretty-printed JSON string of all disk profiles (credentials redacted).
 */
export async function exportProfiles(): Promise<string> {
  return invoke<string>("export_profiles");
}

/**
 * Import disk configurations from a JSON string.
 * Each imported disk receives a fresh UUID. Credentials are NOT imported --
 * the user must fill them in via the Edit Disk dialog.
 *
 * @param json - JSON string containing an array of DiskConfig objects.
 * @returns Array of newly created DiskConfig objects (with redacted credentials).
 */
export async function importProfiles(json: string): Promise<DiskConfig[]> {
  return invoke<DiskConfig[]>("import_profiles", { json });
}

// ─── Diff Commands ──────────────────────────────────────────────────────────

/**
 * Compare two directories (potentially on different disks) and return
 * a list of differences (added, removed, modified, unchanged).
 *
 * @param srcDiskId - UUID of the source disk.
 * @param srcPath   - Directory path on the source disk.
 * @param dstDiskId - UUID of the destination disk.
 * @param dstPath   - Directory path on the destination disk.
 * @returns Array of diff entries describing the comparison result.
 */
export async function diffDirectories(
  srcDiskId: string,
  srcPath: string,
  dstDiskId: string,
  dstPath: string,
): Promise<DiffEntry[]> {
  return invoke<DiffEntry[]>("diff_directories", {
    srcDiskId,
    srcPath,
    dstDiskId,
    dstPath,
  });
}

// ─── Preference Commands ─────────────────────────────────────────────────────

/**
 * Read a persisted user preference from the backend key/value store.
 * Used for settings like theme, view mode, and custom shortcut bindings.
 *
 * @param key - Preference key (e.g. "viewMode", "shortcuts").
 * @returns The stored value string, or null if the key does not exist.
 */
export async function getPreference(key: string): Promise<string | null> {
  return invoke<string | null>("get_preference", { key });
}

/**
 * Write a user preference to the backend key/value store.
 *
 * @param key   - Preference key.
 * @param value - Value to persist (always a string; callers JSON-stringify complex values).
 */
export async function setPreference(key: string, value: string): Promise<void> {
  return invoke<void>("set_preference", { key, value });
}

// ─── Folder Size Command ─────────────────────────────────────────────────────

/**
 * Calculate the total size of a directory recursively.
 * Walks the entire directory tree and sums all file sizes.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative path to the directory.
 * @returns Total size in bytes.
 */
export async function getFolderSize(
  diskId: string,
  path: string,
): Promise<number> {
  return invoke<number>("get_folder_size", { diskId, path });
}

// ─── Batch Rename Command ────────────────────────────────────────────────────

/**
 * Rename multiple files by applying find/replace to their filenames.
 * Files whose names do not contain the find pattern are skipped.
 *
 * @param diskId  - UUID of the disk containing the files.
 * @param paths   - Array of file paths to rename.
 * @param find    - Substring to search for in filenames.
 * @param replace - Replacement string (must not contain path separators).
 * @returns Array of [oldPath, newPath] tuples for successfully renamed entries.
 */
export async function batchRename(
  diskId: string,
  paths: string[],
  find: string,
  replace: string,
): Promise<[string, string][]> {
  return invoke<[string, string][]>("batch_rename", { diskId, paths, find, replace });
}

// ─── Archive Commands ───────────────────────────────────────────────────────

/**
 * List the entries inside a .zip or .tar.gz archive without extracting it.
 * Returns a flat list of entries representing the archive contents.
 *
 * @param diskId - UUID of the disk containing the archive.
 * @param path   - Storage-relative path to the archive file.
 * @returns Array of entries (files and directories) inside the archive.
 */
export async function listArchive(
  diskId: string,
  path: string,
): Promise<Entry[]> {
  return invoke<Entry[]>("list_archive", { diskId, path });
}

/**
 * Extract a .zip or .tar.gz archive to its parent directory.
 * The operation runs asynchronously on the backend; progress is reported
 * via Tauri events keyed by the returned job ID.
 *
 * @param diskId - UUID of the disk containing the archive.
 * @param path   - Storage-relative path to the archive file.
 * @returns A job ID string for progress tracking / cancellation.
 */
export async function extractArchive(
  diskId: string,
  path: string,
): Promise<string> {
  return invoke<string>("extract_archive", { diskId, path });
}

// ─── Size Breakdown Command ──────────────────────────────────────────────────

/**
 * Get the size breakdown of a directory's immediate children.
 * Each child reports its size; directories report recursive totals.
 * Results are sorted by size descending.
 *
 * @param diskId - UUID of the target disk.
 * @param path   - Storage-relative directory path.
 * @returns Array of size entries sorted by size descending.
 */
export async function getSizeBreakdown(
  diskId: string,
  path: string,
): Promise<SizeEntry[]> {
  return invoke<SizeEntry[]>("get_size_breakdown", { diskId, path });
}

// ─── Watch Commands ──────────────────────────────────────────────────────────

/**
 * Start watching a local directory for filesystem changes.
 * The backend emits "fs-change" events when files are created, modified,
 * or deleted in the watched directory.
 *
 * Only works for local disks. Remote backends return an error.
 *
 * @param diskId - UUID of the local disk.
 * @param path   - Storage-relative directory path to watch.
 * @returns A watch ID that can be passed to `unwatchDirectory`.
 */
export async function watchDirectory(
  diskId: string,
  path: string,
): Promise<string> {
  return invoke<string>("watch_directory", { diskId, path });
}

/**
 * Stop watching a directory.
 * Drops the OS-level filesystem monitor associated with the watch ID.
 *
 * @param watchId - The watch ID returned by `watchDirectory`.
 */
export async function unwatchDirectory(watchId: string): Promise<void> {
  return invoke<void>("unwatch_directory", { watchId });
}

