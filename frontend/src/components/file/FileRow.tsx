/**
 * @file File Row
 *
 * A single row in the `FileList` component. Renders an entry's icon, name,
 * and dynamically configured columns (size, modified, kind, permissions, etc.).
 *
 * Columns are passed as a prop so the parent `FileList` controls which columns
 * are visible and their widths. The `name` column is always rendered first with
 * flex: 1. All other columns render as fixed-width cells.
 *
 * Supports:
 * - Selection highlighting
 * - Drag source (draggable attribute)
 * - Drop target highlighting (for directory rows)
 * - Inline rename mode (replaces the name label with an input field)
 *
 * **Inline rename behavior:**
 * When `props.renaming` is true, the name label is replaced by an input.
 * The input auto-focuses and pre-selects the filename (excluding extension).
 * Pressing Enter submits the rename; pressing Escape or blurring cancels.
 * The rename is executed via `OperationContext.rename()`.
 *
 * @module components/file/FileRow
 */

import { Show, createSignal, For, type Component } from "solid-js";
import type { Entry } from "../../lib/types";
import type { ColumnDef, ColumnRenderExtras } from "../../lib/columns";
import { getIcon } from "../../lib/file-utils";
import { getFolderSize } from "../../lib/ipc";
import { useOperation } from "../../contexts/OperationContext";

/**
 * Module-level cache for folder sizes. Once a folder's size is fetched,
 * it's stored here and reused across re-renders. This prevents flickering
 * when virtual scrolling recycles rows.
 */
const folderSizeCache = new Map<string, number>();
import { useFile } from "../../contexts/FileContext";
import styles from "./FileRow.module.css";

/** Shape of a visible column passed from FileList to FileRow. */
export interface VisibleColumn {
  /** Column definition. */
  def: ColumnDef;
  /** Current pixel width for this column. */
  width: number;
}

/**
 * Single row in the file list view.
 *
 * @param props.entry           - The file/directory entry to display.
 * @param props.columns         - Visible non-name columns with their current widths.
 * @param props.selected        - Whether this row is currently selected.
 * @param props.renaming        - Whether inline rename mode is active for this row.
 * @param props.dragging        - Whether this row is currently being dragged.
 * @param props.dropTarget      - Whether this row is a valid drop target being hovered.
 * @param props.onClick         - Click handler for selection.
 * @param props.onDoubleClick   - Double-click handler for navigation into directories.
 * @param props.onDragStart     - Drag start handler.
 * @param props.onDragEnd       - Drag end handler.
 * @param props.onDrop          - Drop handler.
 * @param props.onDragOver      - Drag over handler.
 * @param props.onDragLeave     - Drag leave handler.
 * @param props.onRenameComplete - Callback after successful rename.
 * @param props.onRenameCancel   - Callback when rename is cancelled.
 */
export const FileRow: Component<{
  entry: Entry;
  columns: VisibleColumn[];
  selected: boolean;
  renaming?: boolean;
  dragging?: boolean;
  dropTarget?: boolean;
  onClick: (e: MouseEvent) => void;
  onDoubleClick: (e: MouseEvent) => void;
  onDragStart?: (e: DragEvent) => void;
  onDragEnd?: () => void;
  onDrop?: (e: DragEvent) => void;
  onDragOver?: (e: DragEvent) => void;
  onDragLeave?: () => void;
  onRenameComplete?: () => void;
  onRenameCancel?: () => void;
}> = (props) => {
  const { rename } = useOperation();
  const { state } = useFile();
  const [editName, setEditName] = createSignal(props.entry.name);

  /** Folder size signal, populated from cache or fetched lazily. */
  const [folderSize, setFolderSize] = createSignal<number | undefined>(
    props.entry.is_dir ? folderSizeCache.get(props.entry.path) : undefined,
  );

  // Fetch folder size once if not cached
  if (props.entry.is_dir && state.diskId && !folderSizeCache.has(props.entry.path)) {
    const diskId = state.diskId;
    const path = props.entry.path;
    getFolderSize(diskId, path).then((size) => {
      folderSizeCache.set(path, size);
      setFolderSize(size);
    }).catch(() => { /* ignore errors */ });
  }

  const Icon = () => {
    const I = getIcon(props.entry);
    return <I size={14} />;
  };

  /** Build the extras object for column render functions that need async data. */
  const renderExtras = (): ColumnRenderExtras => ({
    folderSize: folderSize(),
    folderSizeLoading: props.entry.is_dir && folderSize() === undefined,
  });

  const handleRenameSubmit = async () => {
    const newName = editName().trim();
    if (!newName || newName === props.entry.name) {
      props.onRenameCancel?.();
      return;
    }
    if (state.diskId) {
      await rename(state.diskId, props.entry.path, newName);
      props.onRenameComplete?.();
    }
  };

  const handleRenameKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      handleRenameSubmit();
    } else if (e.key === "Escape") {
      props.onRenameCancel?.();
    }
  };

  return (
    <div
      class={styles.row}
      classList={{
        [styles.selected]: props.selected,
        [styles.dragging]: !!props.dragging,
        [styles.dropTarget]: !!props.dropTarget,
      }}
      draggable={!props.renaming}
      onClick={props.onClick}
      onDblClick={props.onDoubleClick}
      onDragStart={props.onDragStart}
      onDragEnd={props.onDragEnd}
      onDragOver={props.onDragOver}
      onDragLeave={props.onDragLeave}
      onDrop={props.onDrop}
    >
      <div class={styles.name}>
        <span class={styles.icon}>
          <Icon />
        </span>
        <Show
          when={props.renaming}
          fallback={<span class={styles.label}>{props.entry.name}</span>}
        >
          <input
            class={styles.renameInput}
            type="text"
            value={editName()}
            onInput={(e) => setEditName(e.currentTarget.value)}
            onKeyDown={handleRenameKeyDown}
            onBlur={handleRenameSubmit}
            ref={(el) => {
              requestAnimationFrame(() => {
                el.focus();
                const dot = el.value.lastIndexOf(".");
                el.setSelectionRange(0, dot > 0 ? dot : el.value.length);
              });
            }}
            onClick={(e) => e.stopPropagation()}
          />
        </Show>
      </div>
      <For each={props.columns}>
        {(col) => (
          <div
            class={styles.cell}
            style={{
              width: `${col.width}px`,
              "text-align": col.def.align,
            }}
          >
            {col.def.render(props.entry, renderExtras())}
          </div>
        )}
      </For>
    </div>
  );
};
