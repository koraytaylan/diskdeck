/**
 * @file Properties Panel
 *
 * Right-side panel that displays metadata about the current selection:
 * - **No selection, disk active:** Shows disk properties (name, type, created date).
 * - **Single entry selected:** Shows entry details (name, path, type, size, modified, permissions).
 *   For directories, a "Calculate Size" button triggers a recursive size calculation.
 * - **Multiple entries selected:** Shows a count ("N items selected").
 * - **No disk active:** Shows "No selection" hint.
 *
 * Includes a collapse button that delegates to the resizable panel context.
 *
 * @module components/layout/PropertiesPanel
 */

import { Show, For, createSignal, createEffect, on, type Component } from "solid-js";
import { PanelRightClose } from "lucide-solid";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { getFolderSize } from "../../lib/ipc";
import styles from "./PropertiesPanel.module.css";

/**
 * Format a byte count with 2 decimal places (more precise than the
 * file-utils version, appropriate for the properties panel).
 */
function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const val = bytes / Math.pow(1024, i);
  return `${i === 0 ? val : val.toFixed(2)} ${units[i]}`;
}

/** Format a Unix timestamp (seconds) to locale string, or "--" for null. */
function formatTimestamp(ts: number | null): string {
  if (ts === null) return "--";
  return new Date(ts * 1000).toLocaleString();
}

/** A single label/value row in the properties display. */
interface PropRow {
  label: string;
  value: string;
}

/** Folder size calculation state: idle, loading, or resolved with a size. */
type FolderSizeState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "done"; size: number }
  | { status: "error"; message: string };

export const PropertiesPanel: Component<{
  onCollapse: () => void;
}> = (props) => {
  const { selectedEntries, state: fileState } = useFile();
  const { activeDisk } = useDisk();

  const [folderSizeState, setFolderSizeState] = createSignal<FolderSizeState>({ status: "idle" });
  /** Cache of calculated folder sizes, keyed by entry path. */
  const [sizeCache, setSizeCache] = createSignal<Record<string, number>>({});

  const singleEntry = () => {
    const sel = selectedEntries();
    return sel.length === 1 ? sel[0] : null;
  };

  const multiCount = () => selectedEntries().length;

  /** Reset folder size state when the selected entry changes. */
  createEffect(on(
    () => singleEntry()?.path,
    (path) => {
      if (path && sizeCache()[path] !== undefined) {
        setFolderSizeState({ status: "done", size: sizeCache()[path] });
      } else {
        setFolderSizeState({ status: "idle" });
      }
    },
  ));

  /** Trigger folder size calculation for the currently selected directory. */
  const handleCalculateSize = async () => {
    const entry = singleEntry();
    const diskId = fileState.diskId;
    if (!entry || !entry.is_dir || !diskId) return;

    setFolderSizeState({ status: "loading" });
    try {
      const size = await getFolderSize(diskId, entry.path);
      setFolderSizeState({ status: "done", size });
      setSizeCache((prev) => ({ ...prev, [entry.path]: size }));
    } catch {
      setFolderSizeState({ status: "error", message: "Failed to calculate size" });
    }
  };

  const diskProps = (): PropRow[] => {
    const disk = activeDisk();
    if (!disk) return [];
    return [
      { label: "Disk", value: disk.name },
      { label: "Type", value: disk.disk_type.toUpperCase() },
      { label: "Created", value: new Date(disk.created_at).toLocaleDateString() },
    ];
  };

  const entryProps = (): PropRow[] => {
    const entry = singleEntry();
    if (!entry) return [];
    const rows: PropRow[] = [
      { label: "Name", value: entry.name },
      { label: "Path", value: entry.path },
      { label: "Type", value: entry.is_dir ? "Folder" : (entry.mime_type ?? "File") },
    ];
    if (!entry.is_dir) {
      rows.push({ label: "Size", value: formatBytes(entry.size) });
    }
    rows.push({ label: "Modified", value: formatTimestamp(entry.modified) });
    if (entry.permissions) {
      rows.push({ label: "Permissions", value: entry.permissions });
    }
    return rows;
  };

  return (
    <div class={styles.properties}>
      <div class={styles.header}>
        <span>Properties</span>
        <button
          class={styles.collapseButton}
          title="Collapse properties"
          onClick={props.onCollapse}
        >
          <PanelRightClose size={14} />
        </button>
      </div>
      <Show
        when={singleEntry()}
        fallback={
          <Show
            when={multiCount() > 1}
            fallback={
              <Show
                when={activeDisk()}
                fallback={<div class={styles.emptyHint}>No selection</div>}
              >
                <div class={styles.propList}>
                  <For each={diskProps()}>
                    {(row) => (
                      <div class={styles.propRow}>
                        <span class={styles.propLabel}>{row.label}</span>
                        <span class={styles.propValue}>{row.value}</span>
                      </div>
                    )}
                  </For>
                </div>
              </Show>
            }
          >
            <div class={styles.emptyHint}>{multiCount()} items selected</div>
          </Show>
        }
      >
        <div class={styles.propList}>
          <For each={entryProps()}>
            {(row) => (
              <div class={styles.propRow}>
                <span class={styles.propLabel}>{row.label}</span>
                <span class={styles.propValue}>{row.value}</span>
              </div>
            )}
          </For>
          <Show when={singleEntry()?.is_dir}>
            <div class={styles.propRow}>
              <span class={styles.propLabel}>Size</span>
              {(() => {
                const s = folderSizeState();
                if (s.status === "idle") {
                  return (
                    <button
                      class={styles.calculateSizeButton}
                      onClick={handleCalculateSize}
                    >
                      Calculate Size
                    </button>
                  );
                }
                if (s.status === "loading") {
                  return <span class={styles.propValue}>Calculating...</span>;
                }
                if (s.status === "error") {
                  return <span class={styles.propValue}>{s.message}</span>;
                }
                return <span class={styles.propValue}>{formatBytes(s.size)}</span>;
              })()}
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};
