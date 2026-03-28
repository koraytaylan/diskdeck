/**
 * @file Drag-and-Drop Data Model
 *
 * Provides the data transfer protocol for internal drag-and-drop of files
 * and folders within DiskDeck. Uses a custom MIME type to avoid conflicts
 * with native OS drag events.
 *
 * **Drag flow:**
 * 1. `FileRow` / `FileGridItem` calls `setDragData()` in `onDragStart` to
 *    serialize the selected paths into `dataTransfer`.
 * 2. Drop targets (`DiskNode`, `MainPanel`, `FileList`, `FileGrid`) call
 *    `isValidDrop()` in `onDragOver` to check if the event is ours.
 * 3. On drop, `getDragData()` deserializes the payload and `isDropAllowed()`
 *    validates the target (no self-drops, no dropping into own children).
 * 4. `dropEffect()` checks the Alt/Option modifier to decide copy vs. move.
 *
 * **Cross-disk support:** `isDropAllowed` permits cross-disk drops (no path
 * conflicts are possible across different disks). Same-disk self-drop and
 * ancestor checks still apply.
 *
 * @module lib/drag
 */

/** Custom MIME type used to identify DiskDeck internal drag payloads. */
const MIME = "application/x-diskdeck-paths";

/**
 * Data transferred during an internal drag operation.
 *
 * @property diskId - UUID of the disk the dragged entries belong to.
 * @property paths  - Array of storage-relative paths being dragged.
 */
export interface DragPayload {
  diskId: string;
  paths: string[];
}

/**
 * Serialize a drag payload into the dataTransfer object.
 * Called from `onDragStart` handlers in file list/grid rows.
 *
 * @param e      - The native DragEvent.
 * @param diskId - UUID of the source disk.
 * @param paths  - Paths of the entries being dragged.
 */
export function setDragData(e: DragEvent, diskId: string, paths: string[]): void {
  if (!e.dataTransfer) return;
  e.dataTransfer.setData(MIME, JSON.stringify({ diskId, paths }));
  e.dataTransfer.effectAllowed = "copyMove";
}

/**
 * Deserialize and validate a drag payload from a drop event.
 * Returns null if the event does not contain a valid DiskDeck payload
 * (e.g. it is a native OS file drop or malformed JSON).
 *
 * @param e - The native DragEvent from an `onDrop` handler.
 * @returns Parsed DragPayload, or null if invalid.
 */
export function getDragData(e: DragEvent): DragPayload | null {
  if (!e.dataTransfer) return null;
  const raw = e.dataTransfer.getData(MIME);
  if (!raw) return null;
  try {
    const data = JSON.parse(raw);
    if (data && typeof data.diskId === "string" && Array.isArray(data.paths)) {
      return data as DragPayload;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Check if a drag event carries our custom MIME type.
 * Used in `onDragOver` handlers to decide whether to accept the drop
 * (call `e.preventDefault()`) or ignore it.
 *
 * @param e - The native DragEvent.
 * @returns True if this is a DiskDeck internal drag.
 */
export function isValidDrop(e: DragEvent): boolean {
  if (!e.dataTransfer) return false;
  return e.dataTransfer.types.includes(MIME);
}

/**
 * Determine the intended drop effect based on modifier keys.
 * Convention: Alt/Option = copy, otherwise = move.
 *
 * @param e - The native DragEvent.
 * @returns "copy" or "move".
 */
export function dropEffect(e: DragEvent): "copy" | "move" {
  return e.altKey ? "copy" : "move";
}

/**
 * Validate whether dropping the payload onto a target path is allowed.
 *
 * Rules:
 * - Cross-disk drops are always allowed (no path conflicts possible).
 * - Cannot drop an entry onto itself (same-disk only).
 * - Cannot drop an entry into one of its own descendants (prevents
 *   circular directory structures, e.g. moving /a into /a/b).
 *
 * @param payload      - The parsed drag payload.
 * @param targetDiskId - UUID of the disk the drop target belongs to.
 * @param targetPath   - Storage-relative path of the drop target directory.
 * @returns True if the drop is allowed.
 */
export function isDropAllowed(
  payload: DragPayload,
  targetDiskId: string,
  targetPath: string,
): boolean {
  // Cross-disk drops are always allowed (no path conflicts possible)
  if (payload.diskId !== targetDiskId) return true;
  // Normalize target so startsWith check works for child detection
  const normalizedTarget = targetPath.endsWith("/") ? targetPath : targetPath + "/";
  for (const p of payload.paths) {
    if (p === targetPath) return false;
    const normalizedSource = p.endsWith("/") ? p : p + "/";
    if (normalizedTarget.startsWith(normalizedSource)) return false;
  }
  return true;
}
