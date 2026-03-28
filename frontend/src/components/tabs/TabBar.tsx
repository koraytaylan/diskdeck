/**
 * @file Tab Bar
 *
 * Horizontal tab strip at the top of the main panel. Shows dynamic browser
 * tabs (one per disk) and preview tabs opened by double-clicking files.
 *
 * - Click a tab to activate it.
 * - Middle-click any tab to close it.
 * - Click the X button to close any tab.
 * - When no tabs are open, the tab bar is hidden.
 *
 * @module components/tabs/TabBar
 */

import { For, Show, type Component } from "solid-js";
import { Folder, File, X } from "lucide-solid";
import { useTab } from "../../contexts/TabContext";
import { showContextMenu } from "../shared/ContextMenu";
import styles from "./TabBar.module.css";

/**
 * Tab bar component rendered at the top of the main panel.
 * Displays all open tabs (browser and preview) with click-to-activate
 * and close controls. Hidden when no tabs exist.
 */
export const TabBar: Component = () => {
  const { state, activateTab, closeTab, closeOtherTabs, closeAllTabs, duplicateTab } = useTab();

  return (
    <Show when={state.tabs.length > 0}>
      <div class={styles.bar} role="tablist">
        <For each={state.tabs}>
          {(tab) => {
            const isActive = () => state.activeTabId === tab.id;

            /** Build a short label like "Local: /Documents" or "Local: /" */
            const label = () => {
              if (tab.kind !== "browser") return tab.name;
              const p = tab.currentPath;
              if (!p || p === "/") return tab.diskName;
              const parts = p.split("/").filter(Boolean);
              const folder = parts[parts.length - 1];
              return parts.length > 1
                ? `${tab.diskName}: /.../${folder}`
                : `${tab.diskName}: /${folder}`;
            };

            const title = () =>
              tab.kind === "browser"
                ? `${tab.diskName}: ${tab.currentPath}`
                : tab.path;

            const handleTabContextMenu = (e: MouseEvent) => {
              e.preventDefault();
              e.stopPropagation();
              const items = [
                ...(tab.kind === "browser"
                  ? [{ label: "Duplicate Tab", action: () => duplicateTab(tab.id) }]
                  : []),
                { label: "Close", action: () => closeTab(tab.id) },
                {
                  label: "Close Others",
                  action: () => closeOtherTabs(tab.id),
                  disabled: state.tabs.length <= 1,
                },
                {
                  label: "Close All",
                  action: () => closeAllTabs(),
                },
              ];
              showContextMenu(e.clientX, e.clientY, items);
            };

            return (
              <div
                class={styles.tab}
                classList={{ [styles.active]: isActive() }}
                onClick={() => activateTab(tab.id)}
                onAuxClick={(e) => {
                  if (e.button === 1) {
                    e.preventDefault();
                    closeTab(tab.id);
                  }
                }}
                onContextMenu={handleTabContextMenu}
                title={title()}
                role="tab"
                tabIndex={0}
                aria-selected={isActive()}
              >
                <span class={styles.tabIcon}>
                  {tab.kind === "preview" ? <File size={12} /> : <Folder size={12} />}
                </span>
                <span class={styles.tabLabel}>{label()}</span>
                <span
                  class={styles.closeButton}
                  role="button"
                  tabIndex={0}
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(tab.id);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") { e.stopPropagation(); closeTab(tab.id); }
                  }}
                  aria-label={`Close ${label()}`}
                >
                  <X size={10} />
                </span>
              </div>
            );
          }}
        </For>
      </div>
    </Show>
  );
};
