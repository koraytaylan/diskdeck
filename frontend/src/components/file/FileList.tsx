/**
 * @file File List (Virtualized)
 *
 * Renders the directory contents as a virtualized vertical list with
 * configurable, resizable column headers. Uses a custom virtual
 * window calculation (`computeVirtualWindow`) instead of a library to
 * ensure correct behavior on window resize.
 *
 * **Columns:**
 * Column visibility and widths are configurable. Right-clicking the header
 * area opens a Finder-like column picker to toggle optional columns. Column
 * borders are draggable to resize. Configuration is persisted in localStorage.
 *
 * **Virtual scrolling:**
 * A `ResizeObserver` tracks the scroll container height. On each scroll
 * event or resize, `computeVirtualWindow` calculates which rows are
 * visible (plus an overscan buffer of 10 rows) and only those rows are
 * rendered. A spacer div provides the correct total scrollable height.
 *
 * **Selection:**
 * Click handlers delegate to `FileContext.selectEntry()` with appropriate
 * modifier flags (Shift for range, Ctrl/Cmd for multi-toggle).
 * Double-click on a directory navigates into it.
 *
 * **Drag-and-drop:**
 * - Rows are draggable; dragging serializes selected paths via `setDragData`.
 * - Directory rows accept drops (highlighted with a drop-target style).
 * - The scroll area background also accepts drops (into the current directory).
 *
 * @module components/file/FileList
 */

import { Show, For, createSignal, createMemo, onMount, onCleanup, type Component } from "solid-js";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { useTab } from "../../contexts/TabContext";
import { computeVirtualWindow } from "../../lib/virtual";
import { setDragData, getDragData, isValidDrop, dropEffect, isDropAllowed } from "../../lib/drag";
import { copyEntries, moveEntries } from "../../lib/ipc";
import {
  ALL_COLUMNS,
  TOGGLEABLE_COLUMNS,
  loadColumnConfig,
  saveColumnConfig,
  type ColumnConfig,
} from "../../lib/columns";
import { showContextMenu, type MenuItem } from "../shared/ContextMenu";
import { FileRow, type VisibleColumn } from "./FileRow";
import styles from "./FileList.module.css";
import { ArrowDown, ArrowUp } from "lucide-solid";

/** Fixed row height in pixels (must match CSS). */
const ROW_HEIGHT = 28;
/** Number of extra rows rendered above/below the viewport for smooth scrolling. */
const OVERSCAN = 10;

/**
 * Virtualized file list component.
 *
 * @param props.renamingPath     - Path of the entry currently being renamed (or null).
 * @param props.onRenameComplete - Callback when rename is successfully submitted.
 * @param props.onRenameCancel   - Callback when rename is cancelled.
 */
export const FileList: Component<{
  renamingPath?: string | null;
  onRenameComplete?: () => void;
  onRenameCancel?: () => void;
}> = (props) => {
  const {
    state,
    sortedEntries,
    selectEntry,
    clearSelection,
    setSort,
    navigate,
    refresh,
  } = useFile();
  const { openPreview, openNewBrowserTab } = useTab();
  const { activeDisk } = useDisk();

  const [rootHeight, setRootHeight] = createSignal(400);
  const [scrollOffset, setScrollOffset] = createSignal(0);
  const [dragging, setDragging] = createSignal(false);
  const [dropTargetPath, setDropTargetPath] = createSignal<string | null>(null);
  let scrollRef!: HTMLDivElement;

  // --- Column configuration state ---
  const initial = loadColumnConfig();
  const [visibleIds, setVisibleIds] = createSignal<string[]>(initial.visibleIds);
  const [columnWidths, setColumnWidths] = createSignal<Record<string, number>>(initial.widths);

  /** Persist current column config to localStorage. */
  const persistColumns = (config: ColumnConfig) => {
    saveColumnConfig(config);
  };

  /** Toggle a column's visibility and persist. */
  const toggleColumn = (columnId: string) => {
    const current = visibleIds();
    let updated: string[];
    if (current.includes(columnId)) {
      updated = current.filter((id) => id !== columnId);
    } else {
      // Insert in definition order: find the correct position among visible IDs
      const allIds = ALL_COLUMNS.map((c) => c.id);
      updated = [...current, columnId].sort(
        (a, b) => allIds.indexOf(a) - allIds.indexOf(b),
      );
    }
    setVisibleIds(updated);
    persistColumns({ visibleIds: updated, widths: columnWidths() });
  };

  /** Update a column's width and persist. */
  const updateColumnWidth = (columnId: string, width: number) => {
    const col = ALL_COLUMNS.find((c) => c.id === columnId);
    const minW = col?.minWidth ?? 40;
    const clamped = Math.max(minW, width);
    const updated = { ...columnWidths(), [columnId]: clamped };
    setColumnWidths(updated);
    persistColumns({ visibleIds: visibleIds(), widths: updated });
  };

  /** Computed list of visible non-name columns with their current widths and defs. */
  const visibleDataColumns = createMemo((): VisibleColumn[] => {
    const ids = visibleIds();
    const widths = columnWidths();
    return ALL_COLUMNS
      .filter((c) => c.id !== "name" && ids.includes(c.id))
      .map((def) => ({
        def,
        width: widths[def.id] ?? def.defaultWidth,
      }));
  });

  // --- Resize handle logic ---
  const handleResizeStart = (columnId: string, startX: number) => {
    const startWidth = columnWidths()[columnId] ?? ALL_COLUMNS.find((c) => c.id === columnId)?.defaultWidth ?? 80;

    const onMouseMove = (e: MouseEvent) => {
      const delta = e.clientX - startX;
      updateColumnWidth(columnId, startWidth + delta);
    };

    const onMouseUp = () => {
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
    };

    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
  };

  // --- Column picker context menu ---
  const showColumnPicker = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const items: MenuItem[] = TOGGLEABLE_COLUMNS.map((col) => ({
      label: col.label,
      checked: visibleIds().includes(col.id),
      action: () => toggleColumn(col.id),
    }));
    showContextMenu(e.clientX, e.clientY, items);
  };

  onMount(() => {
    if (scrollRef) {
      const ro = new ResizeObserver((entries) => {
        for (const entry of entries) {
          setRootHeight(entry.contentRect.height);
        }
      });
      ro.observe(scrollRef);
      onCleanup(() => ro.disconnect());
    }
  });

  // Inline virtual list calculation. We use our own `computeVirtualWindow`
  // instead of @solid-primitives/virtual@0.2.3 because that library reads
  // rootHeight once via access() and never re-tracks it, causing stale
  // calculations when the window resizes.
  const virtual = () => {
    const items = sortedEntries();
    const win = computeVirtualWindow(
      items.length,
      rootHeight(),
      ROW_HEIGHT,
      scrollOffset(),
      OVERSCAN,
    );
    return {
      containerHeight: win.containerHeight,
      viewerTop: win.viewerTop,
      visibleItems: items.slice(win.firstIdx, win.lastIdx),
    };
  };

  const onScroll = (e: Event) => {
    const target = e.target as HTMLElement;
    if (target?.scrollTop !== undefined) setScrollOffset(target.scrollTop);
  };

  const handleRowClick = (path: string, e: MouseEvent) => {
    if (e.shiftKey) {
      selectEntry(path, false, true);
    } else if (e.metaKey || e.ctrlKey) {
      selectEntry(path, true);
    } else {
      selectEntry(path);
    }
  };

  /** Right-click selects the row (if not already selected) so the context
   *  menu built by MainPanel sees the correct selection. */
  const handleRowContextMenu = (path: string) => {
    if (!state.selectedPaths.has(path)) {
      selectEntry(path);
    }
  };

  const handleRowDoubleClick = (entry: { path: string; is_dir: boolean; name: string; mime_type: string | null }, e: MouseEvent) => {
    if (!state.diskId) return;
    if (entry.is_dir) {
      if (e.metaKey || e.ctrlKey) {
        // Cmd+double-click (macOS) / Ctrl+double-click (Windows): open folder in new tab
        const disk = activeDisk();
        openNewBrowserTab(state.diskId, disk?.name ?? state.diskId, entry.path);
      } else {
        navigate(state.diskId, entry.path);
      }
    } else {
      openPreview(state.diskId, entry.path, entry.name, entry.mime_type);
    }
  };

  const handleBackgroundClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      clearSelection();
    }
  };

  // --- Drag and drop handlers ---
  const handleDragStart = (entry: { path: string }, e: DragEvent) => {
    if (!state.diskId) return;
    const paths = state.selectedPaths.has(entry.path)
      ? [...state.selectedPaths]
      : [entry.path];
    setDragData(e, state.diskId, paths);
    setDragging(true);
  };

  const handleDragEnd = () => {
    setDragging(false);
    setDropTargetPath(null);
  };

  const handleRowDragOver = (entry: { path: string; is_dir: boolean }, e: DragEvent) => {
    if (!entry.is_dir || !isValidDrop(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
    setDropTargetPath(entry.path);
  };

  const handleRowDragLeave = () => {
    setDropTargetPath(null);
  };

  const handleRowDrop = async (entry: { path: string; is_dir: boolean }, e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDropTargetPath(null);
    setDragging(false);
    if (!entry.is_dir || !state.diskId) return;
    const payload = getDragData(e);
    if (!payload || !isDropAllowed(payload, state.diskId, entry.path)) return;
    const effect = dropEffect(e);
    if (effect === "copy") {
      await copyEntries(state.diskId, payload.paths, entry.path);
    } else {
      await moveEntries(state.diskId, payload.paths, entry.path);
    }
    refresh();
  };

  const handleScrollAreaDragOver = (e: DragEvent) => {
    if (!isValidDrop(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
  };

  const handleScrollAreaDrop = async (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    setDropTargetPath(null);
    if (!state.diskId) return;
    const payload = getDragData(e);
    if (!payload || !isDropAllowed(payload, state.diskId, state.currentPath)) return;
    const effect = dropEffect(e);
    if (effect === "copy") {
      await copyEntries(state.diskId, payload.paths, state.currentPath);
    } else {
      await moveEntries(state.diskId, payload.paths, state.currentPath);
    }
    refresh();
  };

  const SortIndicator: Component<{ field: string }> = (indicatorProps) => (
    <Show when={state.sortField === indicatorProps.field}>
      <Show when={state.sortDir === "asc"} fallback={<ArrowDown size={10} />}>
        <ArrowUp size={10} />
      </Show>
    </Show>
  );

  return (
    <div class={styles.container}>
      <div class={styles.header} onContextMenu={showColumnPicker}>
        <button
          class={styles.colName}
          classList={{ [styles.activeCol]: state.sortField === "name" }}
          onClick={() => setSort("name")}
        >
          Name <SortIndicator field="name" />
        </button>
        <For each={visibleDataColumns()}>
          {(col) => (
            <>
              <div
                class={styles.resizeHandle}
                onMouseDown={(e) => {
                  e.preventDefault();
                  handleResizeStart(col.def.id, e.clientX);
                }}
              />
              <button
                class={styles.colHeader}
                classList={{
                  [styles.activeCol]: state.sortField === col.def.sortField,
                  [styles.colHeaderLeft]: col.def.align === "left",
                }}
                style={{ width: `${col.width}px` }}
                onClick={() => {
                  if (col.def.sortField) setSort(col.def.sortField as Parameters<typeof setSort>[0]);
                }}
              >
                {col.def.label} <Show when={col.def.sortField}><SortIndicator field={col.def.sortField!} /></Show>
              </button>
            </>
          )}
        </For>
      </div>
      <div
        ref={scrollRef}
        class={styles.scrollArea}
        onScroll={onScroll}
        onClick={handleBackgroundClick}
        onDragOver={handleScrollAreaDragOver}
        onDrop={handleScrollAreaDrop}
      >
        <div
          style={{
            height: `${virtual().containerHeight}px`,
            position: "relative",
          }}
        >
          <div
            style={{
              position: "absolute",
              top: `${virtual().viewerTop}px`,
              left: 0,
              width: "100%",
            }}
          >
            {virtual().visibleItems.map((entry) => (
              <div
                style={{ height: `${ROW_HEIGHT}px` }}
                onContextMenu={() => handleRowContextMenu(entry.path)}
              >
                <FileRow
                  entry={entry}
                  columns={visibleDataColumns()}
                  selected={state.selectedPaths.has(entry.path)}
                  renaming={props.renamingPath === entry.path}
                  dragging={dragging() && state.selectedPaths.has(entry.path)}
                  dropTarget={dropTargetPath() === entry.path}
                  onClick={(e) => handleRowClick(entry.path, e)}
                  onDoubleClick={(e) => handleRowDoubleClick(entry, e)}
                  onDragStart={(e) => handleDragStart(entry, e)}
                  onDragEnd={handleDragEnd}
                  onDragOver={(e) => handleRowDragOver(entry, e)}
                  onDragLeave={handleRowDragLeave}
                  onDrop={(e) => handleRowDrop(entry, e)}
                  onRenameComplete={props.onRenameComplete}
                  onRenameCancel={props.onRenameCancel}
                />
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
