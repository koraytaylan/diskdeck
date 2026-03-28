/**
 * @file Disk Context
 *
 * Manages the list of registered storage backends (disks) and tracks
 * which disk is currently active (selected in the sidebar).
 *
 * **State lifecycle:**
 * 1. On mount, fetches all disks from the backend via `listDisks()`.
 * 2. User interactions (add, edit, delete) update both the backend and
 *    the local store optimistically -- the IPC call is awaited, and only
 *    on success is the local state updated.
 * 3. `activeDiskId` tracks the currently selected disk; changing it
 *    causes `FileContext` and the sidebar to update accordingly.
 *
 * Consumed by:
 * - `DiskTree` / `DiskNode` (renders disk list, handles selection)
 * - `AddDiskDialog` (creates/edits disks)
 * - `Toolbar` (shows active disk name in breadcrumbs)
 * - `MainPanel` / `PropertiesPanel` (reads `activeDiskId`)
 *
 * @module contexts/DiskContext
 */

import {
  createContext,
  useContext,
  onMount,
  type ParentComponent,
} from "solid-js";
import { createStore } from "solid-js/store";
import type { DiskConfig } from "../lib/types";
import { listDisks, createDisk, updateDisk, deleteDisk } from "../lib/ipc";

/** Reactive store shape for disk state. */
interface DiskState {
  /** All registered disks, loaded from the backend on mount. */
  disks: DiskConfig[];
  /** UUID of the currently selected disk, or null if none is selected. */
  activeDiskId: string | null;
  /** True while the initial disk list is being fetched. */
  loading: boolean;
}

/** Public API exposed by the disk context. */
interface DiskContextValue {
  /** Reactive store containing disk list, active selection, and loading state. */
  state: DiskState;
  /** Re-fetch all disks from the backend (used for manual refresh). */
  loadDisks: () => Promise<void>;
  /** Create a new disk on the backend and add it to the local list. */
  addDisk: (
    name: string,
    diskType: string,
    config: Record<string, unknown>,
  ) => Promise<DiskConfig>;
  /** Update an existing disk's name/config on the backend and locally. */
  editDisk: (
    diskId: string,
    name: string,
    config: Record<string, unknown>,
  ) => Promise<DiskConfig>;
  /** Delete a disk from the backend and remove it locally. Clears active selection if needed. */
  removeDisk: (diskId: string) => Promise<void>;
  /** Set the active disk by ID, or null to deselect. */
  selectDisk: (diskId: string | null) => void;
  /** Derived accessor: returns the full DiskConfig for the active disk, or undefined. */
  activeDisk: () => DiskConfig | undefined;
}

const DiskContext = createContext<DiskContextValue>();

/**
 * Provider component that fetches the disk list on mount and exposes
 * CRUD operations plus active-disk selection to the component tree.
 */
export const DiskProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<DiskState>({
    disks: [],
    activeDiskId: null,
    loading: false,
  });

  const loadDisks = async () => {
    setState("loading", true);
    try {
      const disks = await listDisks();
      setState("disks", disks);
    } finally {
      setState("loading", false);
    }
  };

  const addDisk = async (
    name: string,
    diskType: string,
    config: Record<string, unknown>,
  ): Promise<DiskConfig> => {
    const disk = await createDisk(name, diskType, config);
    setState("disks", (prev) => [...prev, disk]);
    return disk;
  };

  const editDisk = async (
    diskId: string,
    name: string,
    config: Record<string, unknown>,
  ): Promise<DiskConfig> => {
    const updated = await updateDisk(diskId, name, config);
    setState("disks", (prev) =>
      prev.map((d) => (d.id === diskId ? updated : d)),
    );
    return updated;
  };

  const removeDisk = async (diskId: string) => {
    await deleteDisk(diskId);
    setState("disks", (prev) => prev.filter((d) => d.id !== diskId));
    // Clear active selection if the deleted disk was active
    if (state.activeDiskId === diskId) {
      setState("activeDiskId", null);
    }
  };

  const selectDisk = (diskId: string | null) => {
    setState("activeDiskId", diskId);
  };

  const activeDisk = () =>
    state.disks.find((d) => d.id === state.activeDiskId);

  onMount(() => {
    loadDisks();
  });

  return (
    <DiskContext.Provider
      value={{ state, loadDisks, addDisk, editDisk, removeDisk, selectDisk, activeDisk }}
    >
      {props.children}
    </DiskContext.Provider>
  );
};

/**
 * Hook to access disk state and CRUD operations.
 * Must be called within a `<DiskProvider>` subtree.
 *
 * @returns DiskContextValue with state, disk operations, and selection controls.
 * @throws Error if called outside DiskProvider.
 */
export function useDisk(): DiskContextValue {
  const ctx = useContext(DiskContext);
  if (!ctx) throw new Error("useDisk must be used within DiskProvider");
  return ctx;
}
