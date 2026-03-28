/**
 * @file Sidebar
 *
 * Left panel of the three-panel layout. Contains:
 * - A header with "Disks" title, an "Add Disk" button, and a collapse button.
 * - The `DiskTree` component listing all registered storage backends.
 * - An `AddDiskDialog` that opens when the "+" button is clicked.
 *
 * The sidebar is collapsible via the `onCollapse` prop, which delegates
 * to the resizable panel context in `AppLayout`.
 *
 * @module components/layout/Sidebar
 */

import { createSignal, type Component } from "solid-js";
import { Plus, PanelLeftClose } from "lucide-solid";
import { BookmarkList } from "../bookmarks/BookmarkList";
import { DiskTree } from "../disk/DiskTree";
import { AddDiskDialog } from "../disk/AddDiskDialog";
import styles from "./Sidebar.module.css";

/**
 * Sidebar panel component.
 *
 * @param props.onCollapse - Callback to collapse this panel (wired to resizable context).
 */
export const Sidebar: Component<{
  onCollapse: () => void;
}> = (props) => {
  const [dialogOpen, setDialogOpen] = createSignal(false);

  return (
    <div class={styles.sidebar}>
      <div class={styles.header}>
        <span>Deck</span>
        <div class={styles.headerActions}>
          <button
            class={styles.headerButton}
            title="Add Disk"
            aria-label="Add Disk"
            onClick={() => setDialogOpen(true)}
          >
            <Plus size={14} />
          </button>
          <button
            class={styles.headerButton}
            title="Collapse sidebar"
            aria-label="Collapse sidebar"
            onClick={props.onCollapse}
          >
            <PanelLeftClose size={14} />
          </button>
        </div>
      </div>
      <BookmarkList />
      <DiskTree />
      <AddDiskDialog
        open={dialogOpen()}
        onClose={() => setDialogOpen(false)}
      />
    </div>
  );
};
