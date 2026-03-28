/**
 * @file Operation Context
 *
 * Manages the internal clipboard (copy/cut) and executes file operations
 * (copy, move, delete, rename, mkdir) against the backend. Also tracks
 * the status of in-flight operations for UI feedback.
 *
 * **Clipboard model:**
 * - `clipCopy` / `clipCut` store a snapshot of `{ diskId, paths, mode }`.
 * - `paste` reads the clipboard and invokes the appropriate IPC call.
 * - After a successful "cut" paste, the clipboard is cleared (the source
 *   files were moved). After a "copy" paste, the clipboard persists
 *   so the user can paste again.
 * - Cross-disk paste is supported: when `clip.diskId !== diskId`, the
 *   paste delegates to `crossCopyEntries` / `crossMoveEntries`.
 *
 * **Operation tracking:**
 * Bulk file operations (copy, move, delete) are fire-and-forget: the IPC
 * call returns a job ID and progress is tracked by the `JobContext` via
 * backend "job-update" events. Rename and mkdir are still awaited inline
 * since they are synchronous on the backend.
 *
 * Consumed by:
 * - `MainPanel` (copy, cut, paste, delete, rename, new folder via shortcuts and context menu)
 * - `FileRow` / `FileGridItem` (inline rename)
 *
 * @module contexts/OperationContext
 */

import {
  createContext,
  useContext,
  type ParentComponent,
} from "solid-js";
import { createStore } from "solid-js/store";
import {
  copyEntries,
  moveEntries,
  crossCopyEntries,
  crossMoveEntries,
  deleteEntries,
  renameEntry,
  createFolder,
} from "../lib/ipc";

/** Whether the clipboard was populated via copy or cut. */
type ClipboardMode = "copy" | "cut";

/** Snapshot of entries placed on the internal clipboard. */
interface ClipboardState {
  /** UUID of the source disk. */
  diskId: string;
  /** Paths of the entries that were copied/cut. */
  paths: string[];
  /** Whether this is a copy or cut operation. */
  mode: ClipboardMode;
}

/** Reactive store shape. */
interface OperationState {
  /** Current clipboard contents, or null if empty. */
  clipboard: ClipboardState | null;
}

/** Public API exposed by the operation context. */
interface OperationContextValue {
  state: OperationState;
  /** Place entries on the clipboard in "copy" mode. */
  clipCopy: (diskId: string, paths: string[]) => void;
  /** Place entries on the clipboard in "cut" mode. */
  clipCut: (diskId: string, paths: string[]) => void;
  /** Execute the clipboard operation (copy or move) into the destination. */
  paste: (diskId: string, dest: string) => Promise<void>;
  /** Check if the clipboard has content. */
  hasClipboard: () => boolean;
  /** Delete entries from a disk. */
  remove: (diskId: string, paths: string[]) => Promise<void>;
  /** Rename a single entry. */
  rename: (diskId: string, path: string, newName: string) => Promise<void>;
  /** Create a new empty directory. */
  mkdir: (diskId: string, path: string) => Promise<void>;
}

const OperationContext = createContext<OperationContextValue>();

/**
 * Provider component that manages the clipboard and file operations.
 *
 * Bulk operations (copy, move, delete) are fire-and-forget: the IPC call
 * spawns a backend job and returns immediately. Progress is tracked by
 * the `JobContext`. Rename and mkdir are still awaited inline.
 */
export const OperationProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<OperationState>({
    clipboard: null,
  });

  const clipCopy = (diskId: string, paths: string[]) => {
    setState("clipboard", { diskId, paths, mode: "copy" });
  };

  const clipCut = (diskId: string, paths: string[]) => {
    setState("clipboard", { diskId, paths, mode: "cut" });
  };

  const hasClipboard = () => state.clipboard !== null;

  const paste = async (diskId: string, dest: string) => {
    const clip = state.clipboard;
    if (!clip) return;

    const isCrossDisk = clip.diskId !== diskId;

    if (clip.mode === "copy") {
      if (isCrossDisk) {
        crossCopyEntries(clip.diskId, diskId, clip.paths, dest).catch((e) =>
          console.warn("Cross-copy failed to start:", e),
        );
      } else {
        copyEntries(diskId, clip.paths, dest).catch((e) =>
          console.warn("Copy failed to start:", e),
        );
      }
    } else {
      // Cut = move
      if (isCrossDisk) {
        crossMoveEntries(clip.diskId, diskId, clip.paths, dest).catch((e) =>
          console.warn("Cross-move failed to start:", e),
        );
      } else {
        moveEntries(diskId, clip.paths, dest).catch((e) =>
          console.warn("Move failed to start:", e),
        );
      }
      // Clear clipboard after cut-paste (source files will be moved)
      setState("clipboard", null);
    }
  };

  const remove = async (diskId: string, paths: string[]) => {
    deleteEntries(diskId, paths).catch((e) =>
      console.warn("Failed to start delete job:", e),
    );
  };

  const rename = async (diskId: string, path: string, newName: string) => {
    await renameEntry(diskId, path, newName);
  };

  const mkdir = async (diskId: string, path: string) => {
    await createFolder(diskId, path);
  };

  return (
    <OperationContext.Provider
      value={{
        state,
        clipCopy,
        clipCut,
        paste,
        hasClipboard,
        remove,
        rename,
        mkdir,
      }}
    >
      {props.children}
    </OperationContext.Provider>
  );
};

/**
 * Hook to access clipboard and file operation methods.
 * Must be called within an `<OperationProvider>` subtree.
 *
 * @returns OperationContextValue with clipboard and mutation methods.
 * @throws Error if called outside OperationProvider.
 */
export function useOperation(): OperationContextValue {
  const ctx = useContext(OperationContext);
  if (!ctx)
    throw new Error("useOperation must be used within OperationProvider");
  return ctx;
}
