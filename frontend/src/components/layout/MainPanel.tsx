/**
 * @file Main Panel
 *
 * The center panel of the three-panel layout. Responsible for:
 * - Displaying the file list (or grid) for the active browser tab's directory.
 * - Switching between browser tabs and preview tabs.
 * - Showing an empty state ("Select a disk to browse files") when no tabs exist.
 * - Switching to search results when a search query is active.
 * - Showing loading and empty folder states.
 * - Handling the right-click context menu with file operations.
 * - Managing inline rename state and batch rename dialog.
 * - Accepting drag-and-drop onto the background (drops into the current directory).
 * - Registering keyboard shortcut actions for file operations (copy, cut,
 *   paste, delete, rename, batch rename, select all, new folder) and
 *   navigation (back, forward, up).
 * - Syncing DiskContext.activeDiskId with the active browser tab.
 *
 * **Rendering priority (top to bottom):**
 * 1. No active tab -> "Select a disk to browse files" empty state
 * 2. Preview tab -> `FilePreview`
 * 3. Browser tab:
 *    a. If a search query is active -> `SearchResults`
 *    b. Else if loading -> "Loading..." state
 *    c. Else if no entries -> "Empty folder" state
 *    d. Else if grid mode -> `FileGrid`
 *    e. Else -> `FileList`
 *
 * @module components/layout/MainPanel
 */

import { Show, createSignal, createEffect, on, onMount, onCleanup, type Component } from "solid-js";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { useOperation } from "../../contexts/OperationContext";
import { useShortcut } from "../../contexts/ShortcutContext";
import { useTab } from "../../contexts/TabContext";
import { getDragData, isValidDrop, dropEffect, isDropAllowed } from "../../lib/drag";
import { addBookmark, removeBookmark, copyEntries, moveEntries, crossCopyEntries, crossMoveEntries, extractArchive } from "../../lib/ipc";
import { refreshBookmarks, findBookmark } from "../bookmarks/BookmarkList";
import { isArchive } from "../../lib/preview-utils";
import { FileList } from "../file/FileList";
import { FileGrid } from "../file/FileGrid";
import { BatchRenameDialog } from "../file/BatchRenameDialog";
import { DiffDialog } from "../diff/DiffDialog";
import { TreemapDialog } from "../treemap/TreemapDialog";
import { SearchResults } from "../search/SearchResults";
import { TabBar } from "../tabs/TabBar";
import { FilePreview } from "../preview/FilePreview";
import {
  ContextMenu,
  showContextMenu,
  type MenuItem,
} from "../shared/ContextMenu";
import type { SearchResult } from "../../lib/types";
import styles from "./MainPanel.module.css";

/**
 * Main content panel component.
 *
 * @param props.searchQuery    - Current search query string (empty = not searching).
 * @param props.searchResults  - Search results from the backend.
 * @param props.searchLoading  - Whether a search is currently in flight.
 * @param props.onSearchClear  - Callback to exit search mode.
 * @param props.viewMode       - "list" or "grid" display mode.
 */
export const MainPanel: Component<{
  searchQuery: string;
  searchResults: SearchResult[];
  searchLoading: boolean;
  onSearchClear: () => void;
  viewMode: "list" | "grid";
}> = (props) => {
  const { state: fileState, refresh, selectedEntries, navigate, goBack, goForward, goUp, selectEntry, sortedEntries } = useFile();
  const { state: diskState, selectDisk } = useDisk();
  const { activeTab, openPreview, closeTab, duplicateTab } = useTab();
  const { clipCopy, clipCut, paste, hasClipboard, remove, mkdir } =
    useOperation();
  const { registerAction, getFormattedBinding } = useShortcut();
  const [renamingPath, setRenamingPath] = createSignal<string | null>(null);
  const [batchRenameOpen, setBatchRenameOpen] = createSignal(false);
  const [diffOpen, setDiffOpen] = createSignal(false);
  const [treemapOpen, setTreemapOpen] = createSignal(false);

  /**
   * Reactive effect: keep DiskContext.activeDiskId in sync with the active
   * browser tab. When a browser tab becomes active, update the sidebar
   * highlighting to match.
   */
  createEffect(on(() => activeTab(), (tab) => {
    if (tab && tab.kind === "browser") {
      selectDisk(tab.diskId);
    }
  }));

  const selectedPaths = () => [...fileState.selectedPaths];

  /** Create a new folder in the current directory of the active browser tab. */
  const handleNewFolder = async () => {
    const diskId = fileState.diskId;
    if (!diskId) return;
    const name = prompt("Folder name:");
    if (!name) return;
    const path = fileState.currentPath === "/"
      ? `/${name}`
      : `${fileState.currentPath}/${name}`;
    await mkdir(diskId, path);
    refresh();
  };

  /** Delete selected entries in the active browser tab. */
  const handleDelete = async () => {
    const diskId = fileState.diskId;
    if (!diskId) return;
    const paths = selectedPaths();
    if (paths.length === 0) return;
    const confirm = window.confirm(
      `Delete ${paths.length} item${paths.length > 1 ? "s" : ""}?`,
    );
    if (!confirm) return;
    await remove(diskId, paths);
  };

  /** Copy selected entries to clipboard. */
  const handleCopy = () => {
    const diskId = fileState.diskId;
    if (!diskId) return;
    clipCopy(diskId, selectedPaths());
  };

  /** Cut selected entries to clipboard. */
  const handleCut = () => {
    const diskId = fileState.diskId;
    if (!diskId) return;
    clipCut(diskId, selectedPaths());
  };

  /** Paste clipboard contents into the current directory. */
  const handlePaste = async () => {
    const diskId = fileState.diskId;
    if (!diskId) return;
    await paste(diskId, fileState.currentPath);
  };

  /** Start inline rename for the single selected entry. */
  const handleRename = () => {
    const sel = selectedEntries();
    if (sel.length === 1) {
      setRenamingPath(sel[0].path);
    }
  };

  /** Open batch rename dialog when multiple files are selected. */
  const handleBatchRename = () => {
    if (fileState.selectedPaths.size >= 2) {
      setBatchRenameOpen(true);
    }
  };

  /** Open the directory comparison dialog. */
  const handleDiff = () => {
    setDiffOpen(true);
  };

  /** Open the disk usage treemap dialog. */
  const handleDiskUsage = () => {
    setTreemapOpen(true);
  };

  /** Select all entries in the current directory. */
  const handleSelectAll = () => {
    const all = sortedEntries();
    for (const entry of all) {
      selectEntry(entry.path, true);
    }
  };

  /** Open selected item: enter folder or preview file (Cmd/Ctrl+Down). */
  const handleOpen = () => {
    const sel = selectedEntries();
    if (sel.length !== 1 || !fileState.diskId) return;
    const entry = sel[0];
    if (entry.is_dir) {
      navigate(fileState.diskId, entry.path);
    } else {
      openPreview(fileState.diskId, entry.path, entry.name, entry.mime_type);
    }
  };

  /** Quick preview: open selected file in a preview tab (Space). */
  const handlePreview = () => {
    const sel = selectedEntries();
    if (sel.length !== 1 || !fileState.diskId) return;
    const entry = sel[0];
    if (!entry.is_dir) {
      openPreview(fileState.diskId, entry.path, entry.name, entry.mime_type);
    }
  };

  /** Close the current tab (Cmd/Ctrl+W). */
  const handleCloseTab = () => {
    const tab = activeTab();
    if (tab) closeTab(tab.id);
  };

  /** Duplicate the current tab (Cmd/Ctrl+T). */
  const handleNewTab = () => {
    const tab = activeTab();
    if (tab) duplicateTab(tab.id);
  };

  // Register shortcut actions
  onMount(() => {
    const cleanups = [
      registerAction("file.copy", handleCopy),
      registerAction("file.cut", handleCut),
      registerAction("file.paste", handlePaste),
      registerAction("file.delete", handleDelete),
      registerAction("file.rename", handleRename),
      registerAction("file.batchRename", handleBatchRename),
      registerAction("file.diff", handleDiff),
      registerAction("file.selectAll", handleSelectAll),
      registerAction("file.newFolder", handleNewFolder),
      registerAction("file.preview", handlePreview),
      registerAction("nav.back", goBack),
      registerAction("nav.forward", goForward),
      registerAction("nav.up", goUp),
      registerAction("nav.open", handleOpen),
      registerAction("tab.close", handleCloseTab),
      registerAction("tab.new", handleNewTab),
      registerAction("view.diskUsage", handleDiskUsage),
    ];
    onCleanup(() => cleanups.forEach((fn) => fn()));
  });

  /** Build and show the right-click context menu for the browser content area. */
  const handleContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    const items: MenuItem[] = [];

    if (fileState.selectedPaths.size > 0) {
      items.push(
        { label: "Copy", shortcut: getFormattedBinding("file.copy"), action: handleCopy },
        { label: "Cut", shortcut: getFormattedBinding("file.cut"), action: handleCut },
        { label: "Delete", shortcut: getFormattedBinding("file.delete"), action: handleDelete },
      );
      if (fileState.selectedPaths.size === 1) {
        items.push({
          label: "Rename",
          shortcut: getFormattedBinding("file.rename"),
          action: handleRename,
        });
      }
      if (fileState.selectedPaths.size >= 2) {
        items.push({
          label: "Batch Rename",
          shortcut: getFormattedBinding("file.batchRename"),
          action: handleBatchRename,
        });
      }

      // Add "Extract Here" for single selected archive files
      if (fileState.selectedPaths.size === 1) {
        const sel = selectedEntries();
        if (sel.length === 1 && !sel[0].is_dir && isArchive(sel[0].name)) {
          items.push({
            label: "Extract Here",
            action: async () => {
              if (!fileState.diskId) return;
              try {
                await extractArchive(fileState.diskId, sel[0].path);
              } catch {
                // Error handled by job system
              }
            },
          });
        }
      }

      items.push({ label: "", action: () => {}, separator: true });
    }

    items.push(
      {
        label: "New Folder",
        shortcut: getFormattedBinding("file.newFolder"),
        action: handleNewFolder,
      },
      {
        label: "Paste",
        shortcut: getFormattedBinding("file.paste"),
        action: handlePaste,
        disabled: !hasClipboard(),
      },
      { label: "", action: () => {}, separator: true },
      { label: "Refresh", action: () => refresh() },
      {
        label: "Disk Usage",
        shortcut: getFormattedBinding("view.diskUsage"),
        action: handleDiskUsage,
      },
    );

    // Add bookmark toggle when browsing a directory
    const tab = activeTab();
    if (tab && tab.kind === "browser" && fileState.diskId) {
      const currentPath = fileState.currentPath;
      const folderName = currentPath === "/"
        ? tab.diskName
        : currentPath.split("/").filter(Boolean).pop() ?? tab.diskName;
      const existing = findBookmark(fileState.diskId, currentPath);
      items.push(
        { label: "", action: () => {}, separator: true },
        existing
          ? {
              label: "Remove Bookmark",
              action: async () => {
                try {
                  await removeBookmark(existing.id);
                  refreshBookmarks();
                } catch { /* ignore */ }
              },
            }
          : {
              label: "Bookmark This Folder",
              action: async () => {
                try {
                  await addBookmark(fileState.diskId!, tab.diskName, currentPath, folderName);
                  refreshBookmarks();
                } catch { /* ignore */ }
              },
            },
      );
    }

    // Add "Compare Directories..." option
    if (fileState.diskId) {
      items.push(
        { label: "", action: () => {}, separator: true },
        {
          label: "Compare Directories...",
          shortcut: getFormattedBinding("file.diff"),
          action: handleDiff,
        },
      );
    }

    showContextMenu(e.clientX, e.clientY, items);
  };

  // --- Background drag and drop ---
  /** Allow drag-over on the main content area. */
  const handleMainDragOver = (e: DragEvent) => {
    if (!isValidDrop(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
  };

  /** Handle drop onto the main content area background (supports cross-disk). */
  const handleMainDrop = async (e: DragEvent) => {
    e.preventDefault();
    const diskId = fileState.diskId;
    if (!diskId) return;
    const payload = getDragData(e);
    if (!payload || !isDropAllowed(payload, diskId, fileState.currentPath)) return;

    const isCrossDisk = payload.diskId !== diskId;
    const effect = dropEffect(e);

    if (effect === "copy") {
      if (isCrossDisk) {
        await crossCopyEntries(payload.diskId, diskId, payload.paths, fileState.currentPath);
      } else {
        await copyEntries(diskId, payload.paths, fileState.currentPath);
      }
    } else {
      if (isCrossDisk) {
        await crossMoveEntries(payload.diskId, diskId, payload.paths, fileState.currentPath);
      } else {
        await moveEntries(diskId, payload.paths, fileState.currentPath);
      }
    }
  };

  const isSearching = () => props.searchQuery.trim().length > 0;

  /** Navigate to a search result's parent directory on the appropriate disk. */
  const handleSearchNavigate = (diskId: string, path: string) => {
    props.onSearchClear();
    selectDisk(diskId);
    navigate(diskId, path);
  };

  return (
    <div class={styles.main}>
      <TabBar />
      <Show
        when={activeTab()}
        fallback={<div class={styles.emptyState}>Select a disk to browse files</div>}
      >
        {(tab) => (
          <Show
            when={tab().kind === "browser"}
            fallback={
              (() => {
                const t = tab();
                if (t.kind !== "preview") return null;
                return (
                  <FilePreview
                    diskId={t.diskId}
                    path={t.path}
                    name={t.name}
                    mimeType={t.mimeType}
                  />
                );
              })()
            }
          >
            <div
              class={styles.browserContent}
              onContextMenu={handleContextMenu}
              onDragOver={handleMainDragOver}
              onDrop={handleMainDrop}
            >
              <Show
                when={!isSearching()}
                fallback={
                  <SearchResults
                    results={props.searchResults}
                    loading={props.searchLoading}
                    query={props.searchQuery}
                    onNavigate={handleSearchNavigate}
                  />
                }
              >
                <Show
                  when={!fileState.loading}
                  fallback={<div class={styles.emptyState}>Loading...</div>}
                >
                  <Show
                    when={!fileState.error}
                    fallback={
                      <div class={styles.errorState}>
                        <div class={styles.errorMessage}>{fileState.error}</div>
                        <button
                          class={styles.retryButton}
                          onClick={() => refresh()}
                        >
                          Retry
                        </button>
                      </div>
                    }
                  >
                  <Show
                    when={fileState.entries.length > 0}
                    fallback={
                      <div class={styles.emptyState}>This folder is empty</div>
                    }
                  >
                    <Show
                      when={props.viewMode === "grid"}
                      fallback={
                        <FileList
                          renamingPath={renamingPath()}
                          onRenameComplete={() => {
                            setRenamingPath(null);
                            refresh();
                          }}
                          onRenameCancel={() => setRenamingPath(null)}
                        />
                      }
                    >
                      <FileGrid
                        renamingPath={renamingPath()}
                        onRenameComplete={() => {
                          setRenamingPath(null);
                          refresh();
                        }}
                        onRenameCancel={() => setRenamingPath(null)}
                      />
                    </Show>
                  </Show>
                  </Show>
                </Show>
              </Show>
              <ContextMenu />
            </div>
          </Show>
        )}
      </Show>
      <BatchRenameDialog
        open={batchRenameOpen()}
        onClose={() => setBatchRenameOpen(false)}
        diskId={fileState.diskId ?? ""}
        paths={selectedPaths()}
        onDone={refresh}
      />
      <DiffDialog
        open={diffOpen()}
        onClose={() => setDiffOpen(false)}
        disks={diskState.disks}
        srcDiskId={fileState.diskId ?? ""}
        srcPath={fileState.currentPath}
      />
      <TreemapDialog
        open={treemapOpen()}
        onClose={() => setTreemapOpen(false)}
        diskId={fileState.diskId ?? ""}
        initialPath={fileState.currentPath}
      />
    </div>
  );
};
