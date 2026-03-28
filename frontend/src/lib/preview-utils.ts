/**
 * @file Preview Utilities
 *
 * Classifies files by their MIME type or extension to determine
 * how they should be previewed (text, image, or binary fallback).
 *
 * @module lib/preview-utils
 */

/** Maximum file size (in bytes) that will be loaded for preview. */
export const MAX_PREVIEW_SIZE = 10 * 1024 * 1024; // 10 MB

/** File extensions recognized as browsable/extractable archives. */
const ARCHIVE_EXTENSIONS = ["zip", "tar.gz", "tgz"];

/**
 * Checks whether a filename has an archive extension (.zip, .tar.gz, .tgz).
 *
 * @param fileName - The file's basename.
 * @returns `true` if the file is a supported archive format.
 */
export function isArchive(fileName: string): boolean {
  const lower = fileName.toLowerCase();
  return ARCHIVE_EXTENSIONS.some((ext) => lower.endsWith(`.${ext}`));
}

/**
 * Classifies a file for preview rendering.
 *
 * The classification logic checks MIME type first, then falls back to file
 * extension matching. Files that cannot be confidently classified as text
 * or image default to "binary" (which shows a metadata-only fallback).
 *
 * @param mimeType - The MIME type reported by the backend, or null if unknown.
 * @param fileName - The file's basename, used for extension-based fallback.
 * @returns "text" for text/code files, "image" for image files, "binary" otherwise.
 */
export function classifyForPreview(
  mimeType: string | null,
  fileName: string,
): "text" | "image" | "binary" {
  if (mimeType) {
    if (mimeType.startsWith("image/")) return "image";
    if (
      mimeType.startsWith("text/") ||
      mimeType === "application/json" ||
      mimeType === "application/xml" ||
      mimeType === "application/javascript"
    )
      return "text";
  }
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  const textExts = [
    "txt", "md", "json", "yaml", "yml", "toml", "xml", "html", "css",
    "js", "ts", "tsx", "jsx", "rs", "py", "go", "java", "c", "cpp", "h",
    "sh", "bash", "zsh", "log", "csv", "ini", "cfg", "conf", "env",
    "gitignore", "dockerfile", "makefile", "lock", "svg",
  ];
  if (textExts.includes(ext)) return "text";
  const imageExts = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];
  if (imageExts.includes(ext)) return "image";
  return "binary";
}
