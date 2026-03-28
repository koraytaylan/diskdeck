/**
 * @file Bookmark List
 *
 * Sidebar section that displays pinned path bookmarks above the disk tree.
 * Each bookmark is a quick-access link to a specific directory on a disk.
 *
 * - Clicking a bookmark opens the disk's browser tab and navigates to the path.
 * - Right-clicking shows a context menu with a "Remove Bookmark" option.
 * - The section is hidden when no bookmarks exist.
 * - Bookmarks are loaded on mount via `listBookmarks()`.
 *
 * @module components/bookmarks/BookmarkList
 */

import { createSignal, onMount, For, Show, type Component } from "solid-js";
import { Folder } from "lucide-solid";
import type { Bookmark } from "../../lib/types";
import { listBookmarks, removeBookmark } from "../../lib/ipc";
import { useTab } from "../../contexts/TabContext";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { showContextMenu } from "../shared/ContextMenu";
import styles from "./BookmarkList.module.css";

/**
 * Module-level signal for bookmark state. Shared between BookmarkList
 * (which renders them) and external callers (which add new ones via
 * `refreshBookmarks()`).
 */
const [bookmarks, setBookmarks] = createSignal<Bookmark[]>([]);

/** Returns the bookmark for a given disk+path, or undefined if not bookmarked. */
export function findBookmark(diskId: string, path: string): Bookmark | undefined {
  return bookmarks().find((b) => b.disk_id === diskId && b.path === path);
}

/** Re-fetch bookmarks from the backend. Call after adding/removing a bookmark. */
export async function refreshBookmarks(): Promise<void> {
  try {
    const result = await listBookmarks();
    setBookmarks(result);
  } catch {
    // Silently ignore — bookmarks are non-critical
  }
}

/**
 * Bookmark list sidebar section.
 *
 * Renders a "BOOKMARKS" header and a list of pinned paths.
 * Shows an empty-state hint when no bookmarks exist.
 * Each item navigates to the bookmarked disk and path on click.
 */
export const BookmarkList: Component = () => {
  const { openBrowserTab } = useTab();
  const { navigate } = useFile();
  const { selectDisk } = useDisk();

  /** Fetch bookmarks from the backend on mount. */
  onMount(() => { refreshBookmarks(); });

  /** Navigate to the bookmarked disk and path. */
  const handleClick = (bookmark: Bookmark) => {
    selectDisk(bookmark.disk_id);
    openBrowserTab(bookmark.disk_id, bookmark.disk_name);
    navigate(bookmark.disk_id, bookmark.path);
  };

  /** Show context menu with "Remove Bookmark" option. */
  const handleContextMenu = (e: MouseEvent, bookmark: Bookmark) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e.clientX, e.clientY, [
      {
        label: "Remove Bookmark",
        action: async () => {
          try {
            await removeBookmark(bookmark.id);
            setBookmarks((prev) => prev.filter((b) => b.id !== bookmark.id));
          } catch {
            // Silently ignore removal errors
          }
        },
      },
    ]);
  };

  return (
    <div class={styles.section}>
      <div class={styles.header}>
        <span>Bookmarks</span>
      </div>
      <Show
        when={bookmarks().length > 0}
        fallback={
          <div class={styles.emptyHint}>Right-click a folder to bookmark it</div>
        }
      >
        <For each={bookmarks()}>
          {(bookmark) => (
            <button
              class={styles.item}
              title={`${bookmark.disk_name}: ${bookmark.path}`}
              onClick={() => handleClick(bookmark)}
              onContextMenu={(e) => handleContextMenu(e, bookmark)}
            >
              <Folder size={14} />
              <span class={styles.label}>{bookmark.label}</span>
            </button>
          )}
        </For>
      </Show>
    </div>
  );
};
