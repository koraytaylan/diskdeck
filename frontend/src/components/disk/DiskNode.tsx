/**
 * @file Disk Node
 *
 * Renders a single disk in the sidebar as an expandable tree node with
 * lazy-loaded subdirectory children. Contains two sub-components:
 *
 * - `TreeFolder` -- Recursive component for rendering a folder in the tree.
 *   Shows expand/collapse chevron, folder icon, and label. Accepts drag-and-drop.
 *
 * - `DiskNode` -- Top-level node for a disk. Shows the disk icon (varies by
 *   type), handles expand/collapse, lazy-loads root children on first click,
 *   and provides a context menu for edit/delete.
 *
 * **Lazy loading:**
 * Children are fetched via `listEntries` only when a node is first expanded.
 * The `FolderNode.children` field starts as `null` (not loaded) and is
 * populated on demand. Once loaded, toggling collapse/expand does not re-fetch.
 *
 * **Tree mutation & re-rendering:**
 * `FolderNode` objects are mutated in place (not replaced) for performance.
 * Since SolidJS stores cannot track in-place mutations on plain objects,
 * a `version` signal is bumped after each mutation to force re-evaluation
 * of derived accessors like `isExpanded()` and `children()`.
 *
 * **Syncing with main-panel navigation:**
 * A `createEffect` watches `fileState.diskId` and `fileState.currentPath`.
 * When the user navigates via the main panel (not the sidebar), `expandToPath`
 * automatically expands ancestor folders in the tree so the current directory
 * is visible. The `navigatingFromSidebar` flag prevents this effect from
 * firing during sidebar-initiated navigation (avoiding redundant fetches).
 *
 * **Drag-and-drop:**
 * Both the disk root row and individual folder rows accept drops. Drops
 * are validated via `isDropAllowed` and execute copy or move based on
 * the Alt/Option modifier.
 *
 * @module components/disk/DiskNode
 */

import { createSignal, createEffect, on, For, Show, type Component } from "solid-js";
import {
  HardDrive,
  Cloud,
  Server,
  ChevronRight,
  ChevronDown,
  Folder,
} from "lucide-solid";
import type { DiskConfig, Entry } from "../../lib/types";
import { useDisk } from "../../contexts/DiskContext";
import { useFile } from "../../contexts/FileContext";
import { useShortcut } from "../../contexts/ShortcutContext";
import { useTab } from "../../contexts/TabContext";
import { addBookmark, removeBookmark, listEntries, copyEntries, moveEntries, crossCopyEntries, crossMoveEntries } from "../../lib/ipc";
import { refreshBookmarks, findBookmark } from "../bookmarks/BookmarkList";
import { getDragData, isValidDrop, dropEffect, isDropAllowed } from "../../lib/drag";
import { showContextMenu } from "../shared/ContextMenu";
import styles from "./DiskNode.module.css";

/** Maps disk types to their corresponding Lucide icon components. */
const diskIcons = {
  local: HardDrive,
  s3: Cloud,
  gcs: Cloud,
  azure: Cloud,
  sftp: Server,
  ftp: Server,
} as const;

/**
 * In-memory tree node representing a subdirectory.
 * Mutated in place for performance (see `version` signal for re-rendering).
 */
interface FolderNode {
  /** The directory entry this node represents. */
  entry: Entry;
  /** Child folder nodes, or null if not yet loaded. */
  children: FolderNode[] | null;
  /** Whether this node is currently expanded in the tree. */
  expanded: boolean;
}

/**
 * Recursive tree folder component. Renders a single folder row with
 * expand/collapse toggle, drag-and-drop support, and nested children.
 */
const TreeFolder: Component<{
  node: FolderNode;
  diskId: string;
  diskName: string;
  depth: number;
  activePath: string | null;
  version: number;
  onToggle: (node: FolderNode) => void;
  onSelect: (path: string) => void;
  onDrop: (path: string, e: DragEvent) => void;
}> = (props) => {
  const { openNewBrowserTab: openNewTab } = useTab();
  const { triggerAction } = useShortcut();
  const { navigate } = useFile();
  const { openBrowserTab } = useTab();
  const { selectDisk } = useDisk();
  const [isDropTarget, setIsDropTarget] = createSignal(false);
  const isActivePath = () => props.activePath === props.node.entry.path;
  const isExpanded = () => { props.version; return props.node.expanded; };
  const children = () => { props.version; return props.node.expanded ? props.node.children : null; };

  const handleFolderContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const existing = findBookmark(props.diskId, props.node.entry.path);
    showContextMenu(e.clientX, e.clientY, [
      {
        label: "Open in New Tab",
        action: () => openNewTab(props.diskId, props.diskName, props.node.entry.path),
      },
      existing
        ? {
            label: "Remove Bookmark",
            action: async () => {
              try { await removeBookmark(existing.id); refreshBookmarks(); } catch { /* ignore */ }
            },
          }
        : {
            label: "Bookmark",
            action: async () => {
              try { await addBookmark(props.diskId, props.diskName, props.node.entry.path, props.node.entry.name); refreshBookmarks(); } catch { /* ignore */ }
            },
          },
      {
        label: "Disk Usage",
        action: async () => {
          selectDisk(props.diskId);
          openBrowserTab(props.diskId, props.diskName);
          await navigate(props.diskId, props.node.entry.path);
          triggerAction("view.diskUsage");
        },
      },
    ]);
  };

  const handleDragOver = (e: DragEvent) => {
    if (!isValidDrop(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
    setIsDropTarget(true);
  };

  const handleDragLeave = () => {
    setIsDropTarget(false);
  };

  const handleDrop = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDropTarget(false);
    props.onDrop(props.node.entry.path, e);
  };

  return (
    <div>
      <button
        class={styles.folderRow}
        classList={{
          [styles.activeFolder]: isActivePath(),
          [styles.dropTarget]: isDropTarget(),
        }}
        style={{ "padding-left": `${(props.depth + 1) * 16 + 4}px` }}
        onClick={(e) => {
          if (e.metaKey || e.ctrlKey) {
            // Cmd/Ctrl+click: open folder in a new tab
            openNewTab(props.diskId, props.diskName, props.node.entry.path);
          } else {
            props.onToggle(props.node);
            props.onSelect(props.node.entry.path);
          }
        }}
        onContextMenu={handleFolderContextMenu}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        <span class={styles.chevron} aria-expanded={isExpanded()}>
          <Show when={isExpanded()} fallback={<ChevronRight size={12} />}>
            <ChevronDown size={12} />
          </Show>
        </span>
        <Folder size={14} />
        <span class={styles.label}>{props.node.entry.name}</span>
      </button>
      <Show when={children()}>
        {(nodes) => (
          <For each={nodes()}>
            {(child) => (
              <TreeFolder
                node={child}
                diskId={props.diskId}
                diskName={props.diskName}
                depth={props.depth + 1}
                activePath={props.activePath}
                version={props.version}
                onToggle={props.onToggle}
                onSelect={props.onSelect}
                onDrop={props.onDrop}
              />
            )}
          </For>
        )}
      </Show>
    </div>
  );
};

/**
 * Top-level disk node in the sidebar tree.
 * Handles disk selection, expand/collapse, lazy loading of root children,
 * context menu (edit/delete), and drag-and-drop onto the disk root.
 *
 * @param props.disk   - The disk configuration to render.
 * @param props.onEdit - Callback when the user selects "Edit" from the context menu.
 */
export const DiskNode: Component<{
  disk: DiskConfig;
  onEdit: (disk: DiskConfig) => void;
}> = (props) => {
  const { state: diskState, selectDisk, removeDisk } = useDisk();
  const { state: fileState, navigate, refresh } = useFile();
  const { triggerAction } = useShortcut();
  const { openBrowserTab, openNewBrowserTab } = useTab();
  const [diskDropTarget, setDiskDropTarget] = createSignal(false);
  const [expanded, setExpanded] = createSignal(false);
  const [folders, setFolders] = createSignal<FolderNode[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [version, setVersion] = createSignal(0);

  // Flag to suppress the createEffect that syncs the tree with main-panel
  // navigation. Set to true before sidebar-initiated navigations and reset
  // after they complete, preventing redundant expandToPath calls.
  let navigatingFromSidebar = false;

  const isActive = () => diskState.activeDiskId === props.disk.id;

  const Icon = diskIcons[props.disk.disk_type] ?? HardDrive;

  /** Bump the version counter to force re-renders after in-place FolderNode mutations. */
  const bump = () => setVersion((v) => v + 1);

  const loadChildren = async (path: string): Promise<FolderNode[]> => {
    try {
      const entries = await listEntries(props.disk.id, path);
      return entries
        .filter((e) => e.is_dir)
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((e) => ({ entry: e, children: null, expanded: false }));
    } catch {
      return [];
    }
  };

  /** Handle click on the disk row: select, expand/collapse, and open browser tab. */
  const handleDiskClick = async (e: MouseEvent) => {
    if (e.metaKey || e.ctrlKey) {
      // Cmd/Ctrl+click: force open in a new tab
      openNewBrowserTab(props.disk.id, props.disk.name);
      return;
    }
    selectDisk(props.disk.id);
    openBrowserTab(props.disk.id, props.disk.name);
    if (!expanded()) {
      if (!loaded()) {
        const children = await loadChildren("/");
        setFolders(children);
        setLoaded(true);
      }
      setExpanded(true);
    } else {
      setExpanded(false);
    }
    navigatingFromSidebar = true;
    await navigate(props.disk.id, "/");
    navigatingFromSidebar = false;
  };

  const handleToggle = async (node: FolderNode) => {
    if (!node.expanded) {
      if (node.children === null) {
        node.children = await loadChildren(node.entry.path);
      }
      node.expanded = true;
    } else {
      node.expanded = false;
    }
    bump();
  };

  /** Handle click on a subfolder in the tree: select disk, open browser tab, navigate. */
  const handleFolderSelect = async (path: string) => {
    selectDisk(props.disk.id);
    openBrowserTab(props.disk.id, props.disk.name);
    navigatingFromSidebar = true;
    await navigate(props.disk.id, path);
    navigatingFromSidebar = false;
  };

  // Expand tree to match current navigation path (from main panel)
  const expandToPath = async (targetPath: string) => {
    if (targetPath === "/") return;

    const segments = targetPath.split("/").filter(Boolean);
    const ancestorPaths: string[] = [];
    let acc = "";
    for (const seg of segments) {
      acc += "/" + seg;
      ancestorPaths.push(acc);
    }

    if (!loaded()) {
      const children = await loadChildren("/");
      setFolders(children);
      setLoaded(true);
    }
    setExpanded(true);

    let currentNodes = folders();
    for (const ancestorPath of ancestorPaths) {
      const node = currentNodes.find((n) => n.entry.path === ancestorPath);
      if (!node) break;

      if (node.children === null) {
        node.children = await loadChildren(node.entry.path);
      }
      node.expanded = true;
      currentNodes = node.children ?? [];
    }

    bump();
  };

  /**
   * Reactive effect: when fileState.diskId or fileState.currentPath change
   * (tracked via the accessor function), expand the tree to match.
   * Only fires for this disk and only when the navigation originated
   * from the main panel (not the sidebar itself).
   */
  createEffect(
    on(
      () => ({ diskId: fileState.diskId, path: fileState.currentPath }),
      ({ diskId, path }) => {
        if (diskId === props.disk.id && !navigatingFromSidebar) {
          expandToPath(path);
        }
      },
    ),
  );

  const handleContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e.clientX, e.clientY, [
      {
        label: "Open in New Tab",
        action: () => {
          openNewBrowserTab(props.disk.id, props.disk.name);
        },
      },
      (() => {
        const existing = findBookmark(props.disk.id, "/");
        return existing
          ? {
              label: "Remove Bookmark",
              action: async () => {
                try { await removeBookmark(existing.id); refreshBookmarks(); } catch { /* ignore */ }
              },
            }
          : {
              label: "Bookmark",
              action: async () => {
                try { await addBookmark(props.disk.id, props.disk.name, "/", props.disk.name); refreshBookmarks(); } catch { /* ignore */ }
              },
            };
      })(),
      {
        label: "Disk Usage",
        action: async () => {
          selectDisk(props.disk.id);
          openBrowserTab(props.disk.id, props.disk.name);
          await navigate(props.disk.id, "/");
          triggerAction("view.diskUsage");
        },
      },
      { label: "", action: () => {}, separator: true },
      {
        label: "Edit",
        action: () => props.onEdit(props.disk),
      },
      {
        label: "Delete",
        action: () => {
          // Defer confirm to next tick so the context menu fully closes first.
          // This prevents WKWebView's non-blocking confirm from interfering
          // with the disk node's render lifecycle.
          setTimeout(async () => {
            if (window.confirm(`Remove disk "${props.disk.name}"?`)) {
              await removeDisk(props.disk.id);
            }
          }, 0);
        },
      },
    ]);
  };

  // --- Drag and drop handlers for sidebar ---
  const handleDiskDragOver = (e: DragEvent) => {
    if (!isValidDrop(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = dropEffect(e);
    setDiskDropTarget(true);
  };

  const handleDiskDragLeave = () => {
    setDiskDropTarget(false);
  };

  /** Handle drop onto the disk root (supports cross-disk transfers). */
  const handleDiskDrop = async (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setDiskDropTarget(false);
    const payload = getDragData(e);
    if (!payload || !isDropAllowed(payload, props.disk.id, "/")) return;
    const isCrossDisk = payload.diskId !== props.disk.id;
    const effect = dropEffect(e);
    if (effect === "copy") {
      isCrossDisk
        ? await crossCopyEntries(payload.diskId, props.disk.id, payload.paths, "/")
        : await copyEntries(props.disk.id, payload.paths, "/");
    } else {
      isCrossDisk
        ? await crossMoveEntries(payload.diskId, props.disk.id, payload.paths, "/")
        : await moveEntries(props.disk.id, payload.paths, "/");
    }
    refresh();
  };

  /** Handle drop onto a subfolder in the tree (supports cross-disk transfers). */
  const handleFolderDrop = async (path: string, e: DragEvent) => {
    const payload = getDragData(e);
    if (!payload || !isDropAllowed(payload, props.disk.id, path)) return;
    const isCrossDisk = payload.diskId !== props.disk.id;
    const effect = dropEffect(e);
    if (effect === "copy") {
      isCrossDisk
        ? await crossCopyEntries(payload.diskId, props.disk.id, payload.paths, path)
        : await copyEntries(props.disk.id, payload.paths, path);
    } else {
      isCrossDisk
        ? await crossMoveEntries(payload.diskId, props.disk.id, payload.paths, path)
        : await moveEntries(props.disk.id, payload.paths, path);
    }
    refresh();
  };

  // The active path for highlighting (only when this disk is active)
  const activePath = () =>
    fileState.diskId === props.disk.id ? fileState.currentPath : null;

  return (
    <div class={styles.node}>
      <button
        class={styles.diskRow}
        classList={{
          [styles.active]: isActive(),
          [styles.dropTarget]: diskDropTarget(),
        }}
        onClick={handleDiskClick}
        onContextMenu={handleContextMenu}
        onDragOver={handleDiskDragOver}
        onDragLeave={handleDiskDragLeave}
        onDrop={handleDiskDrop}
      >
        <span class={styles.chevron} aria-expanded={expanded()}>
          <Show when={expanded()} fallback={<ChevronRight size={12} />}>
            <ChevronDown size={12} />
          </Show>
        </span>
        <Icon size={14} />
        <span class={styles.label}>{props.disk.name}</span>
      </button>
      <Show when={expanded()}>
        <For each={folders()}>
          {(node) => (
            <TreeFolder
              node={node}
              diskId={props.disk.id}
              diskName={props.disk.name}
              depth={1}
              activePath={activePath()}
              version={version()}
              onToggle={handleToggle}
              onSelect={handleFolderSelect}
              onDrop={handleFolderDrop}
            />
          )}
        </For>
      </Show>
    </div>
  );
};
