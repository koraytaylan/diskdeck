/**
 * @file Disk Tree
 *
 * Renders the list of registered disks in the sidebar. Each disk is
 * represented by a `DiskNode` which can be expanded into a folder tree.
 * When no disks exist, shows an empty-state hint.
 *
 * Also manages the edit-disk dialog state: when a `DiskNode` triggers
 * an edit, this component opens the `AddDiskDialog` in edit mode.
 *
 * @module components/disk/DiskTree
 */

import { createSignal, For, Show, type Component } from "solid-js";
import type { DiskConfig } from "../../lib/types";
import { useDisk } from "../../contexts/DiskContext";
import { DiskNode } from "./DiskNode";
import { AddDiskDialog } from "./AddDiskDialog";
import styles from "./DiskTree.module.css";

/** Container component that lists all disks and manages the edit dialog. */
export const DiskTree: Component = () => {
  const { state } = useDisk();
  const [editingDisk, setEditingDisk] = createSignal<DiskConfig | undefined>();

  return (
    <div class={styles.tree}>
      <Show
        when={state.disks.length > 0}
        fallback={
          <div class={styles.empty}>
            No disks configured.
            <br />
            Click + to add one.
          </div>
        }
      >
        <For each={state.disks}>
          {(disk) => (
            <DiskNode
              disk={disk}
              onEdit={(d) => setEditingDisk(d)}
            />
          )}
        </For>
      </Show>
      <AddDiskDialog
        open={!!editingDisk()}
        onClose={() => setEditingDisk(undefined)}
        editDisk={editingDisk()}
      />
    </div>
  );
};
