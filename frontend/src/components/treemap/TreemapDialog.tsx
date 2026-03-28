/**
 * @file Treemap Dialog
 *
 * Modal dialog that displays a visual breakdown of disk space consumption
 * for a directory. Shows a horizontal stacked bar chart where each segment
 * represents a child entry, with width proportional to its size.
 *
 * **Features:**
 * - Stacked bar chart with color-coded segments (accent for directories,
 *   muted for files).
 * - Hover tooltip showing name, formatted size, and percentage.
 * - Sorted entry list below the bar with inline size bars.
 * - Click on a directory segment or list row to drill into it.
 * - Breadcrumb navigation to move back up the directory tree.
 *
 * @module components/treemap/TreemapDialog
 */

import { createSignal, createEffect, on, For, Show, type Component } from "solid-js";
import { X, Folder, File } from "lucide-solid";
import { getSizeBreakdown } from "../../lib/ipc";
import { formatSize } from "../../lib/file-utils";
import type { SizeEntry } from "../../lib/types";
import styles from "./TreemapDialog.module.css";

/**
 * Format a percentage value for display.
 * @param value - Fraction between 0 and 1.
 * @returns Formatted percentage string (e.g. "42.1%").
 */
function formatPercent(value: number): string {
  if (value < 0.001) return "<0.1%";
  return `${(value * 100).toFixed(1)}%`;
}

/**
 * Disk usage treemap dialog component.
 *
 * @param props.open     - Whether the dialog is visible.
 * @param props.onClose  - Callback to close the dialog.
 * @param props.diskId   - UUID of the disk to analyze.
 * @param props.initialPath - Starting directory path.
 */
export const TreemapDialog: Component<{
  open: boolean;
  onClose: () => void;
  diskId: string;
  initialPath: string;
}> = (props) => {
  const [entries, setEntries] = createSignal<SizeEntry[]>([]);
  const [currentPath, setCurrentPath] = createSignal("/");
  const [loading, setLoading] = createSignal(false);
  const [tooltipData, setTooltipData] = createSignal<{
    name: string;
    size: string;
    percent: string;
    x: number;
    y: number;
  } | null>(null);

  /** Fetch size breakdown for the given path. */
  const fetchBreakdown = async (path: string) => {
    setLoading(true);
    setEntries([]);
    try {
      const result = await getSizeBreakdown(props.diskId, path);
      setEntries(result);
      setCurrentPath(path);
    } catch {
      setEntries([]);
    } finally {
      setLoading(false);
    }
  };

  /** Re-fetch when the dialog opens or the initial path changes. */
  createEffect(on(() => props.open, (open) => {
    if (open) {
      fetchBreakdown(props.initialPath);
    }
  }));

  /** Total size of all entries in the current directory. */
  const totalSize = () => entries().reduce((sum, e) => sum + e.size, 0);

  /** Build breadcrumb segments from the current path. */
  const breadcrumbs = () => {
    const path = currentPath();
    const segments: { label: string; path: string }[] = [{ label: "/", path: "/" }];
    if (path === "/") return segments;
    const parts = path.split("/").filter(Boolean);
    let acc = "";
    for (const part of parts) {
      acc += "/" + part;
      segments.push({ label: part, path: acc });
    }
    return segments;
  };

  /** Navigate into a subdirectory. */
  const drillInto = (entry: SizeEntry) => {
    if (entry.is_dir) {
      fetchBreakdown(entry.path);
    }
  };

  /** Navigate to a breadcrumb path. */
  const navigateTo = (path: string) => {
    fetchBreakdown(path);
  };

  /** Show tooltip on bar segment hover. */
  const handleSegmentHover = (entry: SizeEntry, e: MouseEvent) => {
    const total = totalSize();
    const percent = total > 0 ? entry.size / total : 0;
    setTooltipData({
      name: entry.name,
      size: formatSize(entry.size),
      percent: formatPercent(percent),
      x: e.clientX + 12,
      y: e.clientY - 8,
    });
  };

  /** Hide tooltip. */
  const handleSegmentLeave = () => {
    setTooltipData(null);
  };

  /** Close dialog and reset state. */
  const handleClose = () => {
    setTooltipData(null);
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
        aria-label="Disk Usage"
      >
        <div class={styles.dialog}>
          <div class={styles.header}>
            <span>Disk Usage</span>
            <button
              class={styles.closeButton}
              onClick={handleClose}
              title="Close"
            >
              <X size={14} />
            </button>
          </div>

          <div class={styles.breadcrumb}>
            <For each={breadcrumbs()}>
              {(seg, i) => (
                <>
                  <Show when={i() > 0}>
                    <span class={styles.breadcrumbSeparator}>/</span>
                  </Show>
                  <button
                    class={styles.breadcrumbSegment}
                    onClick={() => navigateTo(seg.path)}
                  >
                    {seg.label}
                  </button>
                </>
              )}
            </For>
          </div>

          <div class={styles.body}>
            <Show when={!loading()} fallback={<div class={styles.loading}>Calculating sizes...</div>}>
              <Show when={entries().length > 0} fallback={<div class={styles.emptyState}>This directory is empty</div>}>
                {/* Stacked bar chart */}
                <div class={styles.barContainer}>
                  <For each={entries()}>
                    {(entry) => {
                      const total = totalSize();
                      const widthPct = total > 0 ? (entry.size / total) * 100 : 0;
                      return (
                        <div
                          class={`${styles.barSegment} ${entry.is_dir ? styles.barSegmentDir : styles.barSegmentFile}`}
                          style={{ width: `${widthPct}%` }}
                          onMouseMove={(e) => handleSegmentHover(entry, e)}
                          onMouseLeave={handleSegmentLeave}
                          onClick={() => drillInto(entry)}
                          title={entry.name}
                        />
                      );
                    }}
                  </For>
                </div>

                {/* Sorted entry list */}
                <div class={styles.entryList}>
                  <For each={entries()}>
                    {(entry) => {
                      const total = totalSize();
                      const percent = total > 0 ? entry.size / total : 0;
                      return (
                        <div
                          class={`${styles.entryRow} ${entry.is_dir ? styles.entryRowClickable : ""}`}
                          onClick={() => drillInto(entry)}
                        >
                          <span class={`${styles.entryIcon} ${entry.is_dir ? styles.entryIconDir : ""}`}>
                            <Show when={entry.is_dir} fallback={<File size={14} />}>
                              <Folder size={14} />
                            </Show>
                          </span>
                          <span class={styles.entryName}>{entry.name}</span>
                          <span class={styles.entrySize}>{formatSize(entry.size)}</span>
                          <span class={styles.entryPercent}>{formatPercent(percent)}</span>
                          <div class={styles.entryBar}>
                            <div
                              class={`${styles.entryBarFill} ${entry.is_dir ? styles.entryBarFillDir : styles.entryBarFillFile}`}
                              style={{ width: `${percent * 100}%` }}
                            />
                          </div>
                        </div>
                      );
                    }}
                  </For>
                </div>

                {/* Total */}
                <div class={styles.totalRow}>
                  <span>Total</span>
                  <span>{formatSize(totalSize())}</span>
                </div>
              </Show>
            </Show>
          </div>
        </div>

        {/* Floating tooltip */}
        <Show when={tooltipData()}>
          {(data) => (
            <div
              class={styles.tooltip}
              style={{ left: `${data().x}px`, top: `${data().y}px` }}
            >
              <span class={styles.tooltipName}>{data().name}</span>
              <span class={styles.tooltipSize}>{data().size}</span>
              <span class={styles.tooltipPercent}>{data().percent}</span>
            </div>
          )}
        </Show>
      </div>
    </Show>
  );
};
