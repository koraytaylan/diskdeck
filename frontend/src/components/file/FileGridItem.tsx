/**
 * @file File Grid Item
 *
 * A single cell in the `FileGrid` component. Displays a large icon and
 * the entry name. Supports selection, drag-and-drop, drop targeting,
 * and inline rename -- mirroring `FileRow` behavior but in a grid layout.
 *
 * The icon is rendered at 40px (vs. 14px in list view) for better visual
 * identification in grid mode.
 *
 * @module components/file/FileGridItem
 */

import { Show, createSignal, type Component } from "solid-js";
import type { Entry } from "../../lib/types";
import { getIcon } from "../../lib/file-utils";
import { useOperation } from "../../contexts/OperationContext";
import { useFile } from "../../contexts/FileContext";
import styles from "./FileGridItem.module.css";

/**
 * Single cell in the grid view.
 * Props mirror `FileRow` (see `FileRow.tsx` for detailed prop documentation).
 */
export const FileGridItem: Component<{
  entry: Entry;
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

  const Icon = () => {
    const I = getIcon(props.entry);
    return <I size={40} />;
  };

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
      class={styles.cell}
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
  );
};
