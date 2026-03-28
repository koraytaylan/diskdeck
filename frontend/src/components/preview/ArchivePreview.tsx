/**
 * @file Archive Preview
 *
 * Renders a read-only listing of the entries inside a .zip or .tar.gz archive.
 * Calls `listArchive()` on mount and displays entry names, sizes, and
 * directory indicators in a simple table layout.
 *
 * @module components/preview/ArchivePreview
 */

import { createSignal, onMount, For, Show, type Component } from "solid-js";
import type { Entry } from "../../lib/types";
import { listArchive } from "../../lib/ipc";
import { Folder, File } from "lucide-solid";
import styles from "./ArchivePreview.module.css";

/**
 * Formats a byte count into a human-readable string (B, KB, or MB).
 * @param bytes - Raw byte count.
 * @returns Formatted size string.
 */
const formatSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

/**
 * Archive preview component. Fetches and displays archive contents on mount.
 *
 * @param props.diskId - UUID of the disk containing the archive.
 * @param props.path   - Storage-relative path to the archive file.
 * @param props.name   - File basename (used for the header).
 */
export const ArchivePreview: Component<{
  diskId: string;
  path: string;
  name: string;
}> = (props) => {
  const [entries, setEntries] = createSignal<Entry[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    try {
      const result = await listArchive(props.diskId, props.path);
      setEntries(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  });

  return (
    <div class={styles.container}>
      <div class={styles.header}>
        <span class={styles.headerTitle}>Archive: {props.name}</span>
        <span class={styles.headerCount}>
          <Show when={!loading() && !error()}>
            {entries().length} {entries().length === 1 ? "entry" : "entries"}
          </Show>
        </span>
      </div>
      <Show when={loading()}>
        <div class={styles.center}>Loading archive contents...</div>
      </Show>
      <Show when={error()}>
        <div class={styles.center}>
          <span class={styles.errorText}>{error()}</span>
        </div>
      </Show>
      <Show when={!loading() && !error()}>
        <div class={styles.list}>
          <div class={styles.listHeader}>
            <span class={styles.colIcon} />
            <span class={styles.colName}>Name</span>
            <span class={styles.colSize}>Size</span>
          </div>
          <div class={styles.scrollArea}>
            <For each={entries()}>
              {(entry) => (
                <div class={styles.row}>
                  <span class={styles.colIcon}>
                    <Show when={entry.is_dir} fallback={<File size={14} />}>
                      <Folder size={14} />
                    </Show>
                  </span>
                  <span class={styles.colName} title={entry.path}>
                    {entry.path}
                  </span>
                  <span class={styles.colSize}>
                    {entry.is_dir ? "--" : formatSize(entry.size)}
                  </span>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};
