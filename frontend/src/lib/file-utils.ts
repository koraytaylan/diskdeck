/**
 * @file File Display Utilities
 *
 * Pure helper functions for presenting file-system entries in the UI:
 * - Human-readable byte sizes
 * - Locale-aware date formatting
 * - Lucide icon selection based on MIME type / file extension
 *
 * Used by `FileRow`, `FileGridItem`, and `PropertiesPanel` to format
 * entry metadata for display.
 *
 * @module lib/file-utils
 */

import {
  Folder,
  File,
  FileText,
  FileImage,
  FileVideo,
  FileAudio,
  FileArchive,
  FileCode,
} from "lucide-solid";
import type { Entry } from "./types";

/**
 * Format a byte count into a human-readable string with appropriate units.
 * Returns "--" for zero bytes (used for directories where size is meaningless).
 *
 * @example formatSize(0)       // "--"
 * @example formatSize(1024)    // "1.0 KB"
 * @example formatSize(5242880) // "5.0 MB"
 *
 * @param bytes - File size in bytes.
 * @returns Formatted size string.
 */
export function formatSize(bytes: number): string {
  if (bytes === 0) return "--";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, i);
  return `${i === 0 ? val : val.toFixed(1)} ${units[i]}`;
}

/**
 * Format a Unix timestamp (seconds since epoch) into a locale-aware date string.
 * Returns "--" when the timestamp is null (modification date unavailable).
 *
 * @param ts - Unix timestamp in **seconds** (not milliseconds), or null.
 * @returns Formatted date string, e.g. "Mar 20, 2026, 02:30 PM".
 */
export function formatDate(ts: number | null): string {
  if (ts === null) return "--";
  // Backend sends seconds; JS Date expects milliseconds
  const d = new Date(ts * 1000);
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Select an appropriate Lucide icon component for a file-system entry.
 *
 * Resolution order:
 * 1. Directories always get the `Folder` icon.
 * 2. MIME type prefix match (image/, video/, audio/, text/).
 * 3. File extension match against known code extensions.
 * 4. File extension match against known archive extensions.
 * 5. Fallback: generic `File` icon.
 *
 * @param entry - The file-system entry to pick an icon for.
 * @returns A Lucide SolidJS icon component (not an instance -- caller renders it).
 */
export function getIcon(entry: Entry) {
  if (entry.is_dir) return Folder;
  const mime = entry.mime_type ?? "";
  if (mime.startsWith("image/")) return FileImage;
  if (mime.startsWith("video/")) return FileVideo;
  if (mime.startsWith("audio/")) return FileAudio;
  if (mime.startsWith("text/")) return FileText;
  const ext = entry.name.split(".").pop()?.toLowerCase() ?? "";
  const codeExts = [
    "js", "ts", "tsx", "jsx", "rs", "py", "go", "java", "c", "cpp", "h",
    "css", "html", "json", "yaml", "yml", "toml", "xml", "sh", "bash",
  ];
  if (codeExts.includes(ext)) return FileCode;
  const archiveExts = ["zip", "tar", "gz", "bz2", "xz", "7z", "rar"];
  if (archiveExts.includes(ext)) return FileArchive;
  return File;
}
