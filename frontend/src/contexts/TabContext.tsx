/**
 * @file Tab Context
 *
 * Manages the tab bar state. Tabs are either "browser" tabs (one per disk,
 * showing the file list) or "preview" tabs (opened by double-clicking files).
 *
 * Browser tabs are deduplicated by diskId -- selecting the same disk twice
 * activates the existing tab. Preview tabs are deduplicated by diskId:path.
 *
 * @module contexts/TabContext
 */

import {
  createContext,
  useContext,
  createMemo,
  type ParentComponent,
} from "solid-js";
import { createStore, produce } from "solid-js/store";
import type { Tab } from "../lib/types";

/** Internal state shape for the tab store. */
interface TabState {
  /** All open tabs (browser and preview). */
  tabs: Tab[];
  /** ID of the currently active/visible tab, or null when no tabs are open. */
  activeTabId: string | null;
}

/** Public API exposed by the TabContext provider. */
interface TabContextValue {
  /** Reactive tab state (tabs array and active tab ID). */
  state: TabState;
  /** The currently active tab (derived memo), or null when no tabs exist. */
  activeTab: () => Tab | null;
  /**
   * Open or activate a browser tab for a disk.
   * Deduplicated by diskId — calling with the same diskId activates the
   * existing tab instead of creating a duplicate.
   */
  openBrowserTab: (diskId: string, diskName: string) => void;
  /**
   * Force-create a new browser tab for a disk, even if one already exists.
   * Used by "Open in New Tab" and "Duplicate" actions.
   * @param initialPath - The path to navigate to in the new tab (default "/").
   */
  openNewBrowserTab: (diskId: string, diskName: string, initialPath?: string) => void;
  /** Update the currentPath stored on a browser tab (called by FileContext on navigation). */
  updateTabPath: (tabId: string, path: string) => void;
  /**
   * Open or activate a file preview tab.
   * Deduplicated by `preview:${diskId}:${path}`.
   */
  openPreview: (diskId: string, path: string, name: string, mimeType: string | null) => void;
  /** Duplicate a tab (creates a new tab with the same disk/path). */
  duplicateTab: (tabId: string) => void;
  /** Close a tab by ID. */
  closeTab: (tabId: string) => void;
  /** Close all tabs except the given one. */
  closeOtherTabs: (tabId: string) => void;
  /** Close all tabs. */
  closeAllTabs: () => void;
  /** Switch the active tab. */
  activateTab: (tabId: string) => void;
}

const TabContext = createContext<TabContextValue>();

/**
 * Provider component for tab state. Must wrap any component that uses
 * `useTab()`. Placed before FileProvider in the App tree so that
 * FileContext can access tab state.
 */
export const TabProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<TabState>({
    tabs: [],
    activeTabId: null,
  });

  /** Derived memo: the currently active tab object, or null. */
  const activeTab = createMemo(() => {
    if (!state.activeTabId) return null;
    return state.tabs.find((t) => t.id === state.activeTabId) ?? null;
  });

  /** Counter for generating unique browser tab IDs when duplicating. */
  let tabSeq = 0;

  const openBrowserTab = (diskId: string, diskName: string) => {
    // Dedup: if a browser tab for this disk already exists, activate it
    const existing = state.tabs.find(
      (t) => t.kind === "browser" && t.diskId === diskId,
    );
    if (existing) {
      setState("activeTabId", existing.id);
      return;
    }
    const tabId = `browser:${diskId}:${++tabSeq}`;
    setState(
      produce((s) => {
        s.tabs.push({ kind: "browser", id: tabId, diskId, diskName, currentPath: "/" });
        s.activeTabId = tabId;
      }),
    );
  };

  const openNewBrowserTab = (diskId: string, diskName: string, initialPath = "/") => {
    const tabId = `browser:${diskId}:${++tabSeq}`;
    setState(
      produce((s) => {
        s.tabs.push({ kind: "browser", id: tabId, diskId, diskName, currentPath: initialPath });
        s.activeTabId = tabId;
      }),
    );
  };

  const updateTabPath = (tabId: string, path: string) => {
    setState(
      produce((s) => {
        const tab = s.tabs.find((t) => t.id === tabId);
        if (tab && tab.kind === "browser") {
          tab.currentPath = path;
        }
      }),
    );
  };

  const openPreview = (
    diskId: string,
    path: string,
    name: string,
    mimeType: string | null,
  ) => {
    const tabId = `preview:${diskId}:${path}`;
    const existing = state.tabs.find((t) => t.id === tabId);
    if (existing) {
      setState("activeTabId", tabId);
      return;
    }
    setState(
      produce((s) => {
        s.tabs.push({ kind: "preview", id: tabId, diskId, path, name, mimeType });
        s.activeTabId = tabId;
      }),
    );
  };

  const closeTab = (tabId: string) => {
    setState(
      produce((s) => {
        const idx = s.tabs.findIndex((t) => t.id === tabId);
        if (idx < 0) return;
        s.tabs.splice(idx, 1);
        if (s.activeTabId === tabId) {
          // Activate adjacent tab or null
          if (s.tabs.length > 0) {
            s.activeTabId = idx > 0 ? s.tabs[idx - 1].id : s.tabs[0].id;
          } else {
            s.activeTabId = null;
          }
        }
      }),
    );
  };

  const duplicateTab = (tabId: string) => {
    const tab = state.tabs.find((t) => t.id === tabId);
    if (!tab) return;
    if (tab.kind === "browser") {
      openNewBrowserTab(tab.diskId, tab.diskName, tab.currentPath);
    } else {
      // For preview tabs, just activate the existing one (no point duplicating)
      setState("activeTabId", tabId);
    }
  };

  const closeOtherTabs = (tabId: string) => {
    setState(
      produce((s) => {
        s.tabs = s.tabs.filter((t) => t.id === tabId);
        s.activeTabId = tabId;
      }),
    );
  };

  const closeAllTabs = () => {
    setState({ tabs: [], activeTabId: null });
  };

  const activateTab = (tabId: string) => {
    setState("activeTabId", tabId);
  };

  return (
    <TabContext.Provider
      value={{
        state,
        activeTab,
        openBrowserTab,
        openNewBrowserTab,
        updateTabPath,
        openPreview,
        duplicateTab,
        closeTab,
        closeOtherTabs,
        closeAllTabs,
        activateTab,
      }}
    >
      {props.children}
    </TabContext.Provider>
  );
};

/**
 * Hook to access tab state and actions. Must be used within a `TabProvider`.
 * @throws Error if called outside a TabProvider.
 * @returns The tab context value with state and action methods.
 */
export function useTab(): TabContextValue {
  const ctx = useContext(TabContext);
  if (!ctx) throw new Error("useTab must be used within TabProvider");
  return ctx;
}
