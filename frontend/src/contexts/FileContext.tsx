/**
 * @file File Context
 *
 * Manages directory navigation, entry listing, selection state, sorting,
 * and browser-style back/forward history for the currently active browser tab.
 *
 * **Tab-aware state management:**
 * FileContext caches per-tab browsing state (entries, path, history, selection)
 * and swaps it when the active tab changes. When a browser tab is activated,
 * any cached state is restored; if no cache exists the root directory is fetched.
 *
 * **Key behaviors:**
 *
 * *Navigation & History:*
 * - `navigate(diskId, path)` fetches entries for a path, ensures a browser tab
 *   exists for the disk, and pushes the path onto the history stack.
 * - `goBack()` / `goForward()` move through the history stack without
 *   pushing new entries (they call `fetchEntries` directly).
 * - `goUp()` navigates to the parent directory via `navigate()`.
 *
 * *Race condition guard (`fetchSeq`):*
 * A monotonically increasing sequence number prevents stale responses
 * from overwriting fresher data. If the user clicks folder A then
 * immediately folder B, the response for A is silently discarded.
 *
 * *Selection:*
 * Three selection modes: single click (replace), Ctrl/Cmd+click (toggle),
 * and Shift+click (range). Range selection uses the last item in
 * `selectedPaths` as the anchor and selects all items between anchor
 * and target in the current sort order.
 *
 * *Sorting:*
 * Directories always sort before files. Within each group, entries are
 * sorted by the active field (name, size, modified) and direction.
 * Re-triggers via `createMemo` on `state.entries`, `sortField`, or `sortDir`.
 *
 * Consumed by virtually every component that displays or interacts with files.
 *
 * @module contexts/FileContext
 */

import {
  createContext,
  useContext,
  type ParentComponent,
  createMemo,
  createEffect,
  on,
  onMount,
  onCleanup,
} from "solid-js";
import { createStore } from "solid-js/store";
import type { Entry, FsChangeEvent } from "../lib/types";
import { formatKind } from "../lib/columns";
import { listEntries, watchDirectory, unwatchDirectory } from "../lib/ipc";
import { listen } from "@tauri-apps/api/event";
import { buildBreadcrumbs, parentPath } from "../lib/paths";
import { useTab } from "./TabContext";
import { useDisk } from "./DiskContext";

/** Columns that can be used for sorting. */
type SortField = "name" | "size" | "modified" | "created" | "kind" | "permissions";

/** Sort direction. */
type SortDir = "asc" | "desc";

/** An entry in the recently visited paths list. */
export interface RecentPath {
  /** UUID of the disk this path belongs to. */
  diskId: string;
  /** Display name of the disk. */
  diskName: string;
  /** The directory path that was visited. */
  path: string;
}

/** Reactive store shape for file browsing state. */
interface FileState {
  /** UUID of the disk currently being browsed, or null. */
  diskId: string | null;
  /** Current directory path within the active disk. */
  currentPath: string;
  /** Raw (unsorted) entries in the current directory. */
  entries: Entry[];
  /** Set of selected entry paths. */
  selectedPaths: Set<string>;
  /** True while a `listEntries` call is in flight. */
  loading: boolean;
  /** Error message from the last failed fetch, or null. */
  error: string | null;
  /** Active sort column. */
  sortField: SortField;
  /** Active sort direction. */
  sortDir: SortDir;
  /** Browser-style navigation history (array of paths). */
  history: string[];
  /** Current position in the history array (-1 = no history). */
  historyIndex: number;
  /** Recently visited directory paths across all disks (most recent first). */
  recentPaths: RecentPath[];
}

/** Public API exposed by the file context. */
interface FileContextValue {
  state: FileState;
  /** Navigate to a directory, fetch its entries, and push to history. */
  navigate: (diskId: string, path: string) => Promise<void>;
  /** Re-fetch entries for the current directory (e.g. after a paste/delete). */
  refresh: () => Promise<void>;
  /** Navigate back in history. */
  goBack: () => void;
  /** Navigate forward in history. */
  goForward: () => void;
  /** Navigate to the parent directory. */
  goUp: () => void;
  canGoBack: () => boolean;
  canGoForward: () => boolean;
  canGoUp: () => boolean;
  /**
   * Select an entry. Supports three modes via flags:
   * - Default: replace selection with this entry.
   * - `multi=true`: toggle this entry in the existing selection (Ctrl+click).
   * - `range=true`: select all entries between the last selection and this entry (Shift+click).
   */
  selectEntry: (path: string, multi?: boolean, range?: boolean) => void;
  /** Clear all selected entries. */
  clearSelection: () => void;
  /** Toggle sort by a field. If already sorting by this field, flip direction. */
  setSort: (field: SortField) => void;
  /** Memo: entries sorted by current field/direction, with directories first. */
  sortedEntries: () => Entry[];
  /** Memo: subset of entries that are currently selected. */
  selectedEntries: () => Entry[];
  /** Memo: breadcrumb segments for the current path. */
  breadcrumbs: () => { label: string; path: string }[];
  /** Recently visited directory paths, most recent first (max 20). */
  recentPaths: () => RecentPath[];
}

const FileContext = createContext<FileContextValue>();

/**
 * Provider component that manages file browsing state for the active browser tab.
 * Handles directory listing, navigation history, selection, sorting, and
 * per-tab state caching/restoration on tab switches.
 */
export const FileProvider: ParentComponent = (props) => {
  const { activeTab, openBrowserTab, updateTabPath } = useTab();
  const { state: diskState } = useDisk();

  const [state, setState] = createStore<FileState>({
    diskId: null,
    currentPath: "/",
    entries: [],
    selectedPaths: new Set(),
    loading: false,
    error: null,
    sortField: "name",
    sortDir: "asc",
    history: [],
    historyIndex: -1,
    recentPaths: [],
  });

  /** Maximum number of recent paths to retain. */
  const MAX_RECENT_PATHS = 20;

  // --- Per-tab state cache ---
  // When switching between browser tabs, we save the current tab's state
  // and restore the new tab's cached state (or fetch fresh if uncached).

  /** Snapshot of browsing state for a single browser tab. */
  interface CachedTabState {
    currentPath: string;
    entries: Entry[];
    selectedPaths: Set<string>;
    history: string[];
    historyIndex: number;
  }
  const tabCache = new Map<string, CachedTabState>();
  let currentTabId: string | null = null;

  /**
   * Reactive effect: watches active tab changes to swap per-tab state.
   * Saves the outgoing tab's state to cache and restores (or fetches)
   * the incoming tab's state.
   */
  // Track the active tab ID (not object) to avoid spurious effect triggers.
  // SolidJS `on()` compares by value — tracking the ID string prevents
  // re-firing when the same tab object is re-found by `find()`.
  const activeTabId = createMemo(() => activeTab()?.id ?? null);

  createEffect(on(activeTabId, (tabId) => {
    // Skip if the tab hasn't actually changed
    if (tabId === currentTabId) return;

    // Save current tab state to cache
    if (currentTabId) {
      tabCache.set(currentTabId, {
        currentPath: state.currentPath,
        entries: [...state.entries],
        selectedPaths: new Set(state.selectedPaths),
        history: [...state.history],
        historyIndex: state.historyIndex,
      });
    }

    const tab = activeTab();
    if (!tab || tab.kind !== "browser") {
      currentTabId = tabId;
      return;
    }

    currentTabId = tabId;
    const cached = tabCache.get(tab.id);
    if (cached) {
      // Restore cached state immediately for a snappy tab switch
      setState({
        diskId: tab.diskId,
        currentPath: cached.currentPath,
        entries: cached.entries,
        selectedPaths: cached.selectedPaths,
        history: cached.history,
        historyIndex: cached.historyIndex,
        loading: false,
        error: null,
      });
      // If this path was modified by a completed job while we were away,
      // refresh to pick up the changes
      const key = staleKey(tab.diskId, cached.currentPath);
      if (stalePaths.has(key)) {
        stalePaths.delete(key);
        fetchEntries(tab.diskId, cached.currentPath);
      }
    } else {
      // New tab — fetch its initial path (may be "/" or a specific folder)
      const initialPath = tab.currentPath || "/";
      setState({
        diskId: tab.diskId,
        currentPath: initialPath,
        entries: [],
        selectedPaths: new Set(),
        history: [],
        historyIndex: -1,
        loading: false,
        error: null,
      });
      fetchEntries(tab.diskId, initialPath).then(() => {
        if (!state.error) {
          pushHistory(initialPath);
        }
      });
    }
  }));

  /**
   * Push a path onto the history stack, truncating any forward history.
   * This mimics browser behavior: navigating to a new page clears the
   * "forward" stack.
   */
  const pushHistory = (path: string) => {
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    setState("history", [...trimmed, path]);
    setState("historyIndex", trimmed.length);
  };

  /**
   * Race condition guard: incremented before every fetch. If a newer
   * fetch starts before an older one resolves, the older response is
   * silently discarded by comparing its captured `seq` to `fetchSeq`.
   */
  let fetchSeq = 0;

  /**
   * Core fetch function. Calls `listEntries` and updates state only
   * if no newer navigation has been started (race-condition safe).
   * Does NOT push to history -- callers decide whether to push.
   */
  const fetchEntries = async (diskId: string, path: string) => {
    const seq = ++fetchSeq;
    setState("loading", true);
    setState("error", null);
    try {
      const entries = await listEntries(diskId, path);
      if (seq !== fetchSeq) return;
      setState("entries", entries);
      setState("diskId", diskId);
      setState("currentPath", path);
      setState("selectedPaths", new Set());
      // Keep the tab's stored path in sync for tab title display
      if (currentTabId) updateTabPath(currentTabId, path);
      // Track this path in the recent paths list
      const tab = activeTab();
      const diskName = tab?.kind === "browser" ? tab.diskName : "";
      const newRecent: RecentPath = { diskId, diskName, path };
      const deduped = state.recentPaths.filter(
        (r) => !(r.diskId === diskId && r.path === path),
      );
      setState("recentPaths", [newRecent, ...deduped].slice(0, MAX_RECENT_PATHS));
    } catch (e: unknown) {
      if (seq !== fetchSeq) return;
      setState("error", String(e));
    } finally {
      if (seq === fetchSeq) setState("loading", false);
    }
  };

  /**
   * Navigate to a directory path on a disk. Ensures a browser tab exists
   * for the disk, fetches its entries, and pushes the path to history.
   */
  const navigate = async (diskId: string, path: string) => {
    const tab = activeTab();
    // Only open/switch tabs if the current tab isn't already browsing this disk.
    // This prevents in-tab folder navigation from jumping to a different tab.
    if (!tab || tab.kind !== "browser" || tab.diskId !== diskId) {
      const diskName = (tab && tab.kind === "browser" && tab.diskId === diskId)
        ? tab.diskName
        : diskId;
      openBrowserTab(diskId, diskName);
    }

    await fetchEntries(diskId, path);
    if (!state.error) {
      pushHistory(path);
    }
  };

  /** Re-fetch entries for the current directory (e.g. after paste/delete). */
  const refresh = async () => {
    if (state.diskId) {
      await fetchEntries(state.diskId, state.currentPath);
    }
  };

  // Back/forward do NOT push to history -- they move the index and re-fetch
  const goBack = () => {
    if (state.historyIndex > 0 && state.diskId) {
      const newIndex = state.historyIndex - 1;
      setState("historyIndex", newIndex);
      fetchEntries(state.diskId, state.history[newIndex]);
    }
  };

  const goForward = () => {
    if (state.historyIndex < state.history.length - 1 && state.diskId) {
      const newIndex = state.historyIndex + 1;
      setState("historyIndex", newIndex);
      fetchEntries(state.diskId, state.history[newIndex]);
    }
  };

  const goUp = () => {
    if (!state.diskId || state.currentPath === "/") return;
    navigate(state.diskId, parentPath(state.currentPath));
  };

  const canGoBack = () => state.historyIndex > 0;
  const canGoForward = () => state.historyIndex < state.history.length - 1;
  const canGoUp = () => state.currentPath !== "/" && state.diskId !== null;

  /**
   * Select an entry. Supports single-click (replace), Ctrl/Cmd+click (toggle),
   * and Shift+click (range) modes.
   */
  const selectEntry = (path: string, multi = false, range = false) => {
    // Shift+click range selection: select all items between last selection and target
    if (range && state.entries.length > 0) {
      const sorted = sortedEntries();
      const lastSelected = [...state.selectedPaths].pop();
      const lastIdx = lastSelected
        ? sorted.findIndex((e) => e.path === lastSelected)
        : 0;
      const curIdx = sorted.findIndex((e) => e.path === path);
      if (lastIdx >= 0 && curIdx >= 0) {
        const start = Math.min(lastIdx, curIdx);
        const end = Math.max(lastIdx, curIdx);
        const newSet = new Set(state.selectedPaths);
        for (let i = start; i <= end; i++) {
          newSet.add(sorted[i].path);
        }
        setState("selectedPaths", newSet);
        return;
      }
    }

    // Ctrl/Cmd+click: toggle individual entry in multi-selection
    if (multi) {
      const newSet = new Set(state.selectedPaths);
      if (newSet.has(path)) {
        newSet.delete(path);
      } else {
        newSet.add(path);
      }
      setState("selectedPaths", newSet);
    } else {
      // Default: replace entire selection with single entry
      setState("selectedPaths", new Set([path]));
    }
  };

  const clearSelection = () => setState("selectedPaths", new Set());

  /**
   * Toggle sort by a field. Clicking the already-active column flips
   * the direction; clicking a different column activates it ascending.
   */
  const setSort = (field: SortField) => {
    if (state.sortField === field) {
      setState("sortDir", state.sortDir === "asc" ? "desc" : "asc");
    } else {
      setState("sortField", field);
      setState("sortDir", "asc");
    }
  };

  /**
   * Memo: sorted entries with directories always grouped before files.
   * Re-evaluates when `state.entries`, `state.sortField`, or `state.sortDir` change.
   */
  const sortedEntries = createMemo(() => {
    const dirs = state.entries.filter((e) => e.is_dir);
    const files = state.entries.filter((e) => !e.is_dir);

    const cmp = (a: Entry, b: Entry): number => {
      let result = 0;
      switch (state.sortField) {
        case "name":
          result = a.name.localeCompare(b.name, undefined, {
            sensitivity: "base",
          });
          break;
        case "size":
          result = a.size - b.size;
          break;
        case "modified":
          result = (a.modified ?? 0) - (b.modified ?? 0);
          break;
        case "created":
          result = (a.created ?? 0) - (b.created ?? 0);
          break;
        case "kind":
          result = formatKind(a).localeCompare(formatKind(b), undefined, {
            sensitivity: "base",
          });
          break;
        case "permissions":
          result = (a.permissions ?? "").localeCompare(b.permissions ?? "");
          break;
      }
      return state.sortDir === "asc" ? result : -result;
    };

    return [...dirs.sort(cmp), ...files.sort(cmp)];
  });

  /** Memo: entries whose paths are in `selectedPaths`. */
  const selectedEntries = createMemo(() =>
    state.entries.filter((e) => state.selectedPaths.has(e.path)),
  );

  /** Memo: breadcrumbs for the current path, re-computed on path change. */
  const breadcrumbs = createMemo(() => buildBreadcrumbs(state.currentPath));

  // ─── Watch mode: auto-refresh on filesystem changes ───────────────────
  // For local disks, watch the currently viewed directory and auto-refresh
  // when files change. Only the current directory is watched (non-recursive).
  let currentWatchId: string | null = null;

  /** Check if the active disk is a local backend. */
  const isLocalDisk = (diskId: string): boolean => {
    return diskState.disks.some(
      (d) => d.id === diskId && d.disk_type === "local",
    );
  };

  /** Start watching the current directory. Best-effort: errors are ignored. */
  const startWatch = async (diskId: string, path: string) => {
    if (!isLocalDisk(diskId)) return;
    try {
      currentWatchId = await watchDirectory(diskId, path);
    } catch {
      currentWatchId = null;
    }
  };

  /** Stop the current watch. Best-effort: errors are ignored. */
  const stopWatch = async () => {
    if (currentWatchId) {
      const id = currentWatchId;
      currentWatchId = null;
      try {
        await unwatchDirectory(id);
      } catch {
        // Already unwatched or invalid
      }
    }
  };

  // Re-watch when the current disk/path changes
  createEffect(
    on(
      () => ({ diskId: state.diskId, path: state.currentPath }),
      async ({ diskId, path }) => {
        await stopWatch();
        if (diskId) {
          await startWatch(diskId, path);
        }
      },
    ),
  );

  // Track disk+path pairs that have been modified by completed jobs.
  // When the user navigates to or is already viewing a stale path, refresh.
  const stalePaths = new Set<string>();

  const staleKey = (diskId: string, path: string) => `${diskId}:${path}`;

  // Listen for fs-change and job-update events inside onMount so the
  // cleanup in onCleanup is guaranteed to run after the listeners resolve.
  let unlistenFsChange: (() => void) | undefined;
  let unlistenJobUpdate: (() => void) | undefined;

  onMount(async () => {
    unlistenFsChange = await listen<FsChangeEvent>("fs-change", (event) => {
      const { disk_id, path } = event.payload;
      if (disk_id === state.diskId && path === state.currentPath) {
        refresh();
      }
    });

    unlistenJobUpdate = await listen<{ status: string; disk_id: string; target_path: string }>("job-update", (event) => {
      const { status, disk_id, target_path } = event.payload;
      if (status !== "completed" || !disk_id) return;

      const key = staleKey(disk_id, target_path);

      if (disk_id === state.diskId && target_path === state.currentPath) {
        // Currently viewing the affected path — refresh immediately
        refresh();
      } else {
        // Not viewing it right now — mark as stale for later
        stalePaths.add(key);
        // Also invalidate tab cache so tab switches get fresh data
        for (const [tabId, cached] of tabCache.entries()) {
          if (cached.currentPath === target_path) {
            tabCache.delete(tabId);
          }
        }
      }
    });
  });

  // Cleanup on unmount
  onCleanup(() => {
    stopWatch();
    unlistenFsChange?.();
    unlistenJobUpdate?.();
  });

  return (
    <FileContext.Provider
      value={{
        state,
        navigate,
        refresh,
        goBack,
        goForward,
        goUp,
        canGoBack,
        canGoForward,
        canGoUp,
        selectEntry,
        clearSelection,
        setSort,
        sortedEntries,
        selectedEntries,
        breadcrumbs,
        recentPaths: () => state.recentPaths,
      }}
    >
      {props.children}
    </FileContext.Provider>
  );
};

/**
 * Hook to access file browsing state and navigation controls.
 * Must be called within a `<FileProvider>` subtree.
 *
 * @returns FileContextValue with state, navigation, selection, and sorting.
 * @throws Error if called outside FileProvider.
 */
export function useFile(): FileContextValue {
  const ctx = useContext(FileContext);
  if (!ctx) throw new Error("useFile must be used within FileProvider");
  return ctx;
}
