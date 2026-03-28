/**
 * @file Search Results
 *
 * Displays search results grouped by disk. Each group shows the disk name
 * and icon, followed by matching entries. Clicking a result navigates to
 * the parent directory of the matched entry on the appropriate disk.
 *
 * States:
 * - Loading: "Searching..." indicator.
 * - Empty results with a query: "No results for ..." message.
 * - Results available: Grouped disk sections with clickable entries.
 *
 * @module components/search/SearchResults
 */

import { For, Show, type Component } from "solid-js";
import { HardDrive, Cloud, Folder, File, FileText, FileImage } from "lucide-solid";
import type { SearchResult } from "../../lib/types";
import styles from "./SearchResults.module.css";

/**
 * Simplified icon picker for search result entries.
 * Uses a reduced set of icons compared to `file-utils.getIcon()`
 * since search results don't carry full MIME type information.
 */
function fileIcon(name: string, isDir: boolean) {
  if (isDir) return Folder;
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"].includes(ext)) return FileImage;
  if (["txt", "md", "json", "yml", "yaml", "toml", "xml", "csv", "log"].includes(ext)) return FileText;
  return File;
}

/**
 * Extract the parent directory from a full path.
 * Used to navigate to the containing folder when a search result is clicked.
 */
function parentPath(fullPath: string): string {
  const idx = fullPath.lastIndexOf("/");
  if (idx <= 0) return "/";
  return fullPath.slice(0, idx);
}

/**
 * Grouped search results display.
 *
 * @param props.results    - Array of result groups (one per disk with matches).
 * @param props.loading    - Whether a search is currently in flight.
 * @param props.query      - The current search query (used for empty-state message).
 * @param props.onNavigate - Callback when a result is clicked. Receives diskId and
 *                           the parent path of the matched entry.
 */
export const SearchResults: Component<{
  results: SearchResult[];
  loading: boolean;
  query: string;
  onNavigate: (diskId: string, path: string) => void;
}> = (props) => {
  return (
    <div class={styles.container}>
      <Show when={props.loading}>
        <div class={styles.searching}>Searching...</div>
      </Show>
      <Show when={!props.loading && props.results.length === 0 && props.query.length > 0}>
        <div class={styles.emptyState}>No results for "{props.query}"</div>
      </Show>
      <Show when={!props.loading && props.results.length > 0}>
        <For each={props.results}>
          {(group) => {
            const DiskIcon = group.disk_id.startsWith("s3") ? Cloud : HardDrive;
            return (
              <div class={styles.diskGroup}>
                <div class={styles.diskHeader}>
                  <span class={styles.diskIcon}>
                    <DiskIcon size={12} />
                  </span>
                  {group.disk_name}
                  <span class={styles.resultCount}>{group.entries.length}</span>
                </div>
                <For each={group.entries}>
                  {(entry) => {
                    const Icon = fileIcon(entry.name, entry.is_dir);
                    return (
                      <div
                        class={styles.resultRow}
                        onClick={() =>
                          props.onNavigate(group.disk_id, parentPath(entry.path))
                        }
                      >
                        <span class={styles.resultIcon}>
                          <Icon size={14} />
                        </span>
                        <span class={styles.resultName}>{entry.name}</span>
                        <span class={styles.resultPath}>{entry.path}</span>
                      </div>
                    );
                  }}
                </For>
              </div>
            );
          }}
        </For>
      </Show>
    </div>
  );
};
