/**
 * @file Path Utilities
 *
 * Pure functions for manipulating storage-relative paths. These paths always
 * use forward slashes and start with "/", regardless of the underlying OS.
 *
 * Used by:
 * - `FileContext` to build breadcrumbs for the toolbar.
 * - `DiskNode` to expand the sidebar tree to match the current navigation path.
 * - `Toolbar` to render clickable breadcrumb segments.
 *
 * @module lib/paths
 */

/**
 * A single breadcrumb segment displayed in the toolbar navigation bar.
 *
 * @property label - Display text (basename of the segment, or "/" for root).
 * @property path  - Full storage-relative path up to this segment.
 */
export interface Breadcrumb {
  label: string;
  path: string;
}

/**
 * Build an array of breadcrumb segments from a storage-relative path.
 *
 * Always returns at least one element (the root "/"). For example:
 * - `"/"`          => `[{ label: "/", path: "/" }]`
 * - `"/docs/code"` => `[{ label: "/", path: "/" }, { label: "docs", path: "/docs" }, { label: "code", path: "/docs/code" }]`
 *
 * @param currentPath - The current storage-relative directory path.
 * @returns Ordered array of breadcrumb segments from root to current directory.
 */
export function buildBreadcrumbs(currentPath: string): Breadcrumb[] {
  if (!currentPath || currentPath === "/") {
    return [{ label: "/", path: "/" }];
  }
  const parts = currentPath.split("/").filter(Boolean);
  const crumbs: Breadcrumb[] = [{ label: "/", path: "/" }];
  let accumulated = "";
  for (const part of parts) {
    accumulated += "/" + part;
    crumbs.push({ label: part, path: accumulated });
  }
  return crumbs;
}

/**
 * Compute the parent directory path (one level up).
 * Returns "/" for root-level paths or empty/null input.
 *
 * @example parentPath("/docs/code") // => "/docs"
 * @example parentPath("/docs")      // => "/"
 * @example parentPath("/")          // => "/"
 *
 * @param path - A storage-relative path.
 * @returns The parent directory path.
 */
export function parentPath(path: string): string {
  if (!path || path === "/") return "/";
  // Strip trailing slash before splitting, then remove the last segment
  const parts = path.replace(/\/$/, "").split("/");
  parts.pop();
  return parts.join("/") || "/";
}

/**
 * Split a storage path into an ordered list of all ancestor paths,
 * from the shallowest to the deepest.
 *
 * Used by `DiskNode.expandToPath()` to expand each ancestor folder
 * in the sidebar tree when the user navigates from the main panel.
 *
 * @example ancestorPaths("/a/b/c") // => ["/a", "/a/b", "/a/b/c"]
 * @example ancestorPaths("/")      // => []
 *
 * @param targetPath - The full storage-relative path.
 * @returns Array of ancestor paths, not including root "/".
 */
export function ancestorPaths(targetPath: string): string[] {
  const segments = targetPath.split("/").filter(Boolean);
  const result: string[] = [];
  let acc = "";
  for (const seg of segments) {
    acc += "/" + seg;
    result.push(acc);
  }
  return result;
}
