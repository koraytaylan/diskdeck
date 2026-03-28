/**
 * @file Diff Dialog
 *
 * Modal dialog for comparing two directories across storage backends.
 * Displays a table of differences (added, removed, modified, unchanged)
 * with filter buttons and a count summary.
 *
 * **Behavior:**
 * - Source defaults to the current active disk and path.
 * - Destination requires selecting a disk from a dropdown and entering a path.
 * - "Compare" button invokes the `diffDirectories` IPC command.
 * - Results are shown in a table with status icons, name, source size, and dest size.
 * - Filter buttons allow viewing all entries or only a specific status.
 * - A count summary shows the breakdown of added/removed/modified/unchanged entries.
 *
 * @module components/diff/DiffDialog
 */

import { createSignal, For, Show, type Component } from "solid-js";
import { X } from "lucide-solid";
import { diffDirectories } from "../../lib/ipc";
import { formatSize } from "../../lib/file-utils";
import type { DiffEntry, DiffStatus, DiskConfig } from "../../lib/types";
import styles from "./DiffDialog.module.css";

/** Filter options for the diff results table. */
type DiffFilter = "all" | DiffStatus;

/**
 * Returns the display icon character for a diff status.
 * @param status - The diff status of the entry.
 * @returns A single character representing the status.
 */
function statusIcon(status: DiffStatus): string {
  switch (status) {
    case "added": return "+";
    case "removed": return "-";
    case "modified": return "~";
    case "unchanged": return "=";
  }
}

/**
 * Returns the CSS class for a diff status icon.
 * @param status - The diff status of the entry.
 * @returns CSS module class name string.
 */
function statusClass(status: DiffStatus): string {
  switch (status) {
    case "added": return styles.statusAdded;
    case "removed": return styles.statusRemoved;
    case "modified": return styles.statusModified;
    case "unchanged": return styles.statusUnchanged;
  }
}

/**
 * Modal dialog for comparing two directories and displaying differences.
 *
 * @param props.open          - Whether the dialog is visible.
 * @param props.onClose       - Callback to close the dialog.
 * @param props.disks         - All available disk configurations for the selectors.
 * @param props.srcDiskId     - Default source disk ID (current active disk).
 * @param props.srcPath       - Default source path (current browsing path).
 */
export const DiffDialog: Component<{
  open: boolean;
  onClose: () => void;
  disks: DiskConfig[];
  srcDiskId: string;
  srcPath: string;
}> = (props) => {
  const [srcDiskId, setSrcDiskId] = createSignal(props.srcDiskId);
  const [srcPath, setSrcPath] = createSignal(props.srcPath);
  const [dstDiskId, setDstDiskId] = createSignal("");
  const [dstPath, setDstPath] = createSignal("/");
  const [results, setResults] = createSignal<DiffEntry[]>([]);
  const [filter, setFilter] = createSignal<DiffFilter>("all");
  const [error, setError] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [hasCompared, setHasCompared] = createSignal(false);

  /** Reset dialog state when reopened. */
  const reset = () => {
    setSrcDiskId(props.srcDiskId);
    setSrcPath(props.srcPath);
    setDstDiskId("");
    setDstPath("/");
    setResults([]);
    setFilter("all");
    setError("");
    setLoading(false);
    setHasCompared(false);
  };

  /** Execute the directory comparison. */
  const handleCompare = async () => {
    const src = srcDiskId();
    const dst = dstDiskId();
    if (!src || !dst) {
      setError("Please select both source and destination disks");
      return;
    }

    setError("");
    setLoading(true);
    setHasCompared(false);
    try {
      const diff = await diffDirectories(src, srcPath(), dst, dstPath());
      setResults(diff);
      setHasCompared(true);
      setFilter("all");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  /** Filtered results based on the active filter. */
  const filteredResults = (): DiffEntry[] => {
    const f = filter();
    const all = results();
    if (f === "all") return all;
    return all.filter((e) => e.status === f);
  };

  /** Count of entries by status. */
  const counts = () => {
    const all = results();
    return {
      added: all.filter((e) => e.status === "added").length,
      removed: all.filter((e) => e.status === "removed").length,
      modified: all.filter((e) => e.status === "modified").length,
      unchanged: all.filter((e) => e.status === "unchanged").length,
    };
  };

  /** Format a size value, returning "--" for null. */
  const fmtSize = (size: number | null): string => {
    if (size === null) return "--";
    if (size === 0) return "0 B";
    return formatSize(size);
  };

  /** Close dialog and reset state. */
  const handleClose = () => {
    reset();
    props.onClose();
  };

  /** Close on backdrop click. */
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) handleClose();
  };

  /** Close on Escape key. */
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") handleClose();
  };

  return (
    <Show when={props.open}>
      <div
        class={styles.backdrop}
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        role="dialog"
        aria-modal="true"
        aria-label="Compare Directories"
      >
        <div class={styles.dialog}>
          <div class={styles.header}>
            <span>Compare Directories</span>
            <button
              class={styles.closeButton}
              onClick={handleClose}
              title="Close"
            >
              <X size={14} />
            </button>
          </div>
          <div class={styles.body}>
            {/* Source selector */}
            <div class={styles.selectorRow}>
              <div class={styles.field}>
                <label class={styles.label}>Source Disk</label>
                <select
                  class={styles.select}
                  value={srcDiskId()}
                  onChange={(e) => setSrcDiskId(e.currentTarget.value)}
                >
                  <option value="">Select disk...</option>
                  <For each={props.disks}>
                    {(disk) => <option value={disk.id}>{disk.name}</option>}
                  </For>
                </select>
              </div>
              <div class={styles.field}>
                <label class={styles.label}>Source Path</label>
                <input
                  class={styles.input}
                  type="text"
                  value={srcPath()}
                  onInput={(e) => setSrcPath(e.currentTarget.value)}
                  placeholder="/"
                />
              </div>
            </div>

            {/* Destination selector */}
            <div class={styles.selectorRow}>
              <div class={styles.field}>
                <label class={styles.label}>Destination Disk</label>
                <select
                  class={styles.select}
                  value={dstDiskId()}
                  onChange={(e) => setDstDiskId(e.currentTarget.value)}
                >
                  <option value="">Select disk...</option>
                  <For each={props.disks}>
                    {(disk) => <option value={disk.id}>{disk.name}</option>}
                  </For>
                </select>
              </div>
              <div class={styles.field}>
                <label class={styles.label}>Destination Path</label>
                <input
                  class={styles.input}
                  type="text"
                  value={dstPath()}
                  onInput={(e) => setDstPath(e.currentTarget.value)}
                  placeholder="/"
                />
              </div>
            </div>

            {/* Compare button */}
            <div class={styles.actions}>
              <button
                class={styles.compareButton}
                onClick={handleCompare}
                disabled={loading() || !srcDiskId() || !dstDiskId()}
              >
                {loading() ? "Comparing..." : "Compare"}
              </button>
            </div>

            <Show when={error()}>
              <div class={styles.error}>{error()}</div>
            </Show>

            {/* Results section */}
            <Show when={hasCompared()}>
              {/* Summary */}
              <div class={styles.summary}>
                <span class={styles.summaryAdded}>{counts().added} added</span>
                {", "}
                <span class={styles.summaryRemoved}>{counts().removed} removed</span>
                {", "}
                <span class={styles.summaryModified}>{counts().modified} modified</span>
                {", "}
                <span class={styles.summaryUnchanged}>{counts().unchanged} unchanged</span>
              </div>

              {/* Filter buttons */}
              <div class={styles.filterRow}>
                <button
                  class={`${styles.filterButton} ${filter() === "all" ? styles.filterButtonActive : ""}`}
                  onClick={() => setFilter("all")}
                >
                  Show All ({results().length})
                </button>
                <button
                  class={`${styles.filterButton} ${filter() === "added" ? styles.filterButtonActive : ""}`}
                  onClick={() => setFilter("added")}
                >
                  Added ({counts().added})
                </button>
                <button
                  class={`${styles.filterButton} ${filter() === "removed" ? styles.filterButtonActive : ""}`}
                  onClick={() => setFilter("removed")}
                >
                  Removed ({counts().removed})
                </button>
                <button
                  class={`${styles.filterButton} ${filter() === "modified" ? styles.filterButtonActive : ""}`}
                  onClick={() => setFilter("modified")}
                >
                  Modified ({counts().modified})
                </button>
              </div>

              {/* Results table */}
              <Show
                when={filteredResults().length > 0}
                fallback={
                  <div class={styles.emptyState}>
                    {results().length === 0
                      ? "Both directories are identical"
                      : "No entries match the selected filter"}
                  </div>
                }
              >
                <div class={styles.resultsContainer}>
                  <table class={styles.resultTable}>
                    <thead>
                      <tr>
                        <th>Status</th>
                        <th>Name</th>
                        <th>Src Size</th>
                        <th>Dst Size</th>
                      </tr>
                    </thead>
                    <tbody>
                      <For each={filteredResults()}>
                        {(entry) => (
                          <tr>
                            <td>
                              <span class={`${styles.statusIcon} ${statusClass(entry.status)}`}>
                                {statusIcon(entry.status)}
                              </span>
                            </td>
                            <td class={styles.nameCell}>
                              {entry.is_dir ? `${entry.name}/` : entry.name}
                            </td>
                            <td class={styles.sizeCell}>{fmtSize(entry.src_size)}</td>
                            <td class={styles.sizeCell}>{fmtSize(entry.dst_size)}</td>
                          </tr>
                        )}
                      </For>
                    </tbody>
                  </table>
                </div>
              </Show>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};
