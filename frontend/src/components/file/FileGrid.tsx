/**
 * @file File Grid (Virtualized)
 *
 * Renders directory contents as a virtualized grid of icon+name cells.
 * Uses `computeGridVirtualWindow` which adapts the column count to the
 * available width and virtualizes by grid row (not individual cell).
 *
 * **Virtual scrolling:**
 * A `ResizeObserver` tracks both the height and width of the scroll
 * container. `computeGridVirtualWindow` determines how many columns
 * fit, then calculates visible grid rows (plus overscan). Visible items
 * are sliced from the flat sorted-entries array and grouped into rows.
 *
 * Selection, navigation, and drag-and-drop behavior mirror `FileList`.
 *
 * @module components/file/FileGrid
 */

import { createSignal, onMount, onCleanup, type Component } from "solid-js";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { useTab } from "../../contexts/TabContext";
import { computeGridVirtualWindow } from "../../lib/virtual";
import { setDragData, getDragData, isValidDrop, dropEffect, isDropAllowed } from "../../lib/drag";
import { copyEntries, moveEntries } from "../../lib/ipc";
import { FileGridItem } from "./FileGridItem";
import styles from "./FileGrid.module.css";

/** Width of a single grid cell in pixels. */
const CELL_WIDTH = 120;
/** Height of a single grid cell (row) in pixels. */
const CELL_HEIGHT = 100;
/** Number of extra grid rows rendered above/below the viewport. */
const OVERSCAN = 3;

export const FileGrid: Component<{
  renamingPath?: string | null;
  onRenameComplete?: () => void;
  onRenameCancel?: () => void;
}> = (props) => {
  const {
    state,
    sortedEntries,
    selectEntry,
    clearSelection,
    navigate,
    refresh,
  } = useFile();
  const { openPreview, openNewBrowserTab } = useTab();
  const { activeDisk } = useDisk();

  const [rootHeight, setRootHeight] = createSignal(400);
  const [rootWidth, setRootWidth] = createSignal(600);
  const [scrollOffset, setScrollOffset] = createSignal(0);
  const [dragging, setDragging] = createSignal(false);
  const [dropTargetPath, setDropTargetPath] = createSignal<string | null>(null);
  let scrollRef!: HTMLDivElement;

  onMount(() => {
    if (scrollRef) {
      const ro = new ResizeObserver((entries) => {
        for (const entry of entries) {
          setRootHeight(entry.contentRect.height);
          setRootWidth(entry.contentRect.width);
        }
      });
      ro.observe(scrollRef);
      onCleanup(() => ro.disconnect());
    }
  });

  const virtual = () => {
    const items = sortedEntries();
    const win = computeGridVirtualWindow(
      items.length,
      rootHeight(),
      rootWidth(),
      CELL_WIDTH,
      CELL_HEIGHT,
      scrollOffset(),
      OVERSCAN,
    );
    const visibleItems = items.slice(win.firstItemIdx, win.lastItemIdx);
    // Group into rows of colCount
    const rows: typeof items[] = [];
    for (let i = 0; i < visibleItems.length; i += win.colCount) {
      rows.push(visibleItems.slice(i, i + win.colCount));
    }
    return {
      containerHeight: win.containerHeight,
      viewerTop: win.viewerTop,
      rows,
    };
  };

  const onScroll = (e: Event) => {
    const target = e.target as HTMLElement;
    if (target?.scrollTop !== undefined) setScrollOffset(target.scrollTop);
  };

  const handleItemClick = (path: string, e: MouseEvent) => {
    if (e.shiftKey) {
      selectEntry(path, false, true);
    } else if (e.metaKey || e.ctrlKey) {
      selectEntry(path, true);
    } else {
      selectEntry(path);
    }
  };

  /** Right-click selects the item (if not already selected) so the context
   *  menu built by MainPanel sees the correct selection. */
  const handleItemContextMenu = (path: string) => {
    if (!state.selectedPaths.has(path)) {
      selectEntry(path);
    }
  };

  const handleItemDoubleClick = (entry: { path: string; is_dir: boolean; name: string; mime_type: string | null }, e: MouseEvent) => {
    if (!state.diskId) return;
    if (entry.is_dir) {
      if (e.metaKey || e.ctrlKey) {
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

  const handleItemDragOver = (entry: { path: string; is_dir: boolean }, e: DragEvent) => {
    if (!entry.is_dir || !isValidDrop(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
    setDropTargetPath(entry.path);
  };

  const handleItemDragLeave = () => {
    setDropTargetPath(null);
  };

  const handleItemDrop = async (entry: { path: string; is_dir: boolean }, e: DragEvent) => {
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

  return (
    <div class={styles.container}>
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
            {virtual().rows.map((row) => (
              <div class={styles.row} style={{ height: `${CELL_HEIGHT}px` }}>
                {row.map((entry) => (
                  <div onContextMenu={() => handleItemContextMenu(entry.path)}>
                  <FileGridItem
                    entry={entry}
                    selected={state.selectedPaths.has(entry.path)}
                    renaming={props.renamingPath === entry.path}
                    dragging={dragging() && state.selectedPaths.has(entry.path)}
                    dropTarget={dropTargetPath() === entry.path}
                    onClick={(e) => handleItemClick(entry.path, e)}
                    onDoubleClick={(e) => handleItemDoubleClick(entry, e)}
                    onDragStart={(e) => handleDragStart(entry, e)}
                    onDragEnd={handleDragEnd}
                    onDragOver={(e) => handleItemDragOver(entry, e)}
                    onDragLeave={handleItemDragLeave}
                    onDrop={(e) => handleItemDrop(entry, e)}
                    onRenameComplete={props.onRenameComplete}
                    onRenameCancel={props.onRenameCancel}
                  />
                  </div>
                ))}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};
