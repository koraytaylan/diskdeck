/**
 * @file Context Menu
 *
 * A global right-click context menu system. Unlike typical component-scoped
 * menus, this uses a module-level signal so any component can trigger the
 * menu via `showContextMenu()` without prop drilling.
 *
 * **Architecture:**
 * - `showContextMenu(x, y, items)` -- Imperative function to open the menu
 *   at a given position with the specified items. Adjusts position to stay
 *   within the viewport (boundary-aware positioning).
 * - `hideContextMenu()` -- Closes the menu.
 * - `<ContextMenu />` -- Renders the actual menu DOM. Should be placed
 *   once in the component tree (inside `MainPanel`).
 *
 * **Dismissal:**
 * When the menu is shown, global listeners are added for:
 * - Click anywhere (closes menu)
 * - Escape key (closes menu)
 * - Scroll on any element (closes menu, using capture phase)
 *
 * These listeners are cleaned up when the `<Show>` block unmounts.
 *
 * @module components/shared/ContextMenu
 */

import {
  createSignal,
  For,
  Show,
  onCleanup,
  type Component,
} from "solid-js";
import styles from "./ContextMenu.module.css";

/**
 * A single item in the context menu.
 *
 * @property label     - Display text (empty string for separators).
 * @property shortcut  - Optional shortcut hint displayed right-aligned.
 * @property action    - Callback invoked when the item is clicked.
 * @property disabled  - If true, the item is grayed out and not clickable.
 * @property separator - If true, renders a horizontal divider instead of a button.
 */
export interface MenuItem {
  label: string;
  shortcut?: string;
  action: () => void;
  disabled?: boolean;
  separator?: boolean;
  checked?: boolean;
}

/** Internal state for the context menu position and items. */
interface ContextMenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

/**
 * Module-level signal holding the current context menu state.
 * Null means the menu is closed.
 */
const [menuState, setMenuState] = createSignal<ContextMenuState | null>(null);

/**
 * Open the context menu at the given screen coordinates with the specified items.
 * Adjusts position to prevent the menu from overflowing the viewport edges.
 *
 * @param x     - Desired X position (typically `e.clientX`).
 * @param y     - Desired Y position (typically `e.clientY`).
 * @param items - Menu items to display.
 */
export function showContextMenu(
  x: number,
  y: number,
  items: MenuItem[],
) {
  // Boundary-aware positioning: shift menu if it would overflow the viewport
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const menuW = 200;
  const menuH = items.length * 30;
  const posX = x + menuW > vw ? vw - menuW - 4 : x;
  const posY = y + menuH > vh ? vh - menuH - 4 : y;
  setMenuState({ x: posX, y: posY, items });
}

/** Close the context menu. */
export function hideContextMenu() {
  setMenuState(null);
}

/**
 * Context menu renderer component. Place once in the component tree.
 * Renders conditionally based on the module-level `menuState` signal.
 */
export const ContextMenu: Component = () => {
  const handleClickOutside = () => hideContextMenu();
  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === "Escape") hideContextMenu();
  };

  // Attach global listeners when menu is shown
  const cleanup = () => {
    document.removeEventListener("click", handleClickOutside);
    document.removeEventListener("keydown", handleEscape);
    document.removeEventListener("scroll", handleClickOutside, true);
  };

  return (
    <Show when={menuState()}>
      {(state) => {
        // Add listeners on mount
        requestAnimationFrame(() => {
          document.addEventListener("click", handleClickOutside);
          document.addEventListener("keydown", handleEscape);
          document.addEventListener("scroll", handleClickOutside, true);
        });
        onCleanup(cleanup);

        return (
          <div
            class={styles.menu}
            role="menu"
            style={{ left: `${state().x}px`, top: `${state().y}px` }}
          >
            <For each={state().items}>
              {(item) => (
                <Show
                  when={!item.separator}
                  fallback={<div class={styles.separator} role="separator" />}
                >
                  <button
                    role="menuitem"
                    class={styles.item}
                    classList={{ [styles.disabled]: item.disabled }}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!item.disabled) {
                        item.action();
                        hideContextMenu();
                      }
                    }}
                    disabled={item.disabled}
                  >
                    <Show when={item.checked !== undefined}>
                      <span class={styles.checkmark}>{item.checked ? "\u2713" : "\u00A0"}</span>
                    </Show>
                    <span class={styles.label}>{item.label}</span>
                    <Show when={item.shortcut}>
                      <span class={styles.shortcut}>{item.shortcut}</span>
                    </Show>
                  </button>
                </Show>
              )}
            </For>
          </div>
        );
      }}
    </Show>
  );
};
