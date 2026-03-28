/**
 * @file Toolbar
 *
 * Horizontal toolbar at the top of the application. Contains:
 * - Back / Forward / Up navigation buttons (consuming `FileContext`)
 * - Breadcrumb path showing the active disk name + current directory
 * - Integrated search bar
 * - View mode toggle (list <-> grid)
 * - Settings button
 * - Theme cycle button with an icon matching the current theme
 *
 * All props are passed down from `AppLayout`; the toolbar itself is
 * stateless aside from reading context values.
 *
 * @module components/layout/Toolbar
 */

import { For, type Component } from "solid-js";
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Sun,
  Moon,
  Sunset,
  LayoutGrid,
  List,
  Settings,
} from "lucide-solid";
import { useTheme } from "../../contexts/ThemeContext";
import { useFile } from "../../contexts/FileContext";
import { useDisk } from "../../contexts/DiskContext";
import { SearchBar } from "../search/SearchBar";
import styles from "./Toolbar.module.css";

/** Maps each theme to its corresponding Lucide icon component. */
const themeIcons = {
  dark: Moon,
  mirage: Sunset,
  light: Sun,
} as const;

/**
 * Top toolbar component.
 *
 * @param props.searchQuery   - Current search input value (controlled).
 * @param props.onSearchInput - Callback when the search input changes.
 * @param props.onSearchClear - Callback to clear the search.
 * @param props.searchRef     - Ref callback for the search input element (for programmatic focus).
 * @param props.viewMode      - Current view mode ("list" or "grid").
 * @param props.onToggleView  - Callback to toggle between list and grid view.
 * @param props.onOpenSettings - Callback to open the settings dialog.
 */
export const Toolbar: Component<{
  searchQuery: string;
  onSearchInput: (value: string) => void;
  onSearchClear: () => void;
  searchRef?: (el: HTMLInputElement) => void;
  viewMode: "list" | "grid";
  onToggleView: () => void;
  onOpenSettings: () => void;
}> = (props) => {
  const { theme, cycleTheme } = useTheme();
  const { goBack, goForward, goUp, canGoBack, canGoForward, canGoUp, breadcrumbs, navigate } =
    useFile();
  const { state: diskState, activeDisk } = useDisk();

  const ThemeIcon = () => {
    const Icon = themeIcons[theme()];
    return <Icon size={14} />;
  };

  const handleCrumbClick = (path: string) => {
    if (diskState.activeDiskId) {
      navigate(diskState.activeDiskId, path);
    }
  };

  return (
    <div class={styles.toolbar}>
      <div class={styles.navGroup}>
        <button
          class={styles.navButton}
          disabled={!canGoBack()}
          title="Back"
          aria-label="Back"
          onClick={goBack}
        >
          <ArrowLeft size={14} />
        </button>
        <button
          class={styles.navButton}
          disabled={!canGoForward()}
          title="Forward"
          aria-label="Forward"
          onClick={goForward}
        >
          <ArrowRight size={14} />
        </button>
        <button
          class={styles.navButton}
          disabled={!canGoUp()}
          title="Up"
          aria-label="Up"
          onClick={goUp}
        >
          <ArrowUp size={14} />
        </button>
      </div>

      <div class={styles.breadcrumb}>
        <For each={breadcrumbs()}>
          {(crumb, i) => (
            <>
              {i() > 0 && <span class={styles.separator}>/</span>}
              <button
                class={styles.crumbButton}
                onClick={() => handleCrumbClick(crumb.path)}
              >
                {i() === 0 ? (activeDisk()?.name ?? crumb.label) : crumb.label}
              </button>
            </>
          )}
        </For>
      </div>

      <SearchBar
        value={props.searchQuery}
        onInput={props.onSearchInput}
        onClear={props.onSearchClear}
        ref={props.searchRef}
      />

      <button
        class={styles.navButton}
        onClick={props.onToggleView}
        title={props.viewMode === "list" ? "Grid view" : "List view"}
        aria-label={props.viewMode === "list" ? "Grid view" : "List view"}
      >
        {props.viewMode === "list" ? <LayoutGrid size={14} /> : <List size={14} />}
      </button>

      <button
        class={styles.navButton}
        onClick={props.onOpenSettings}
        title="Settings"
        aria-label="Settings"
      >
        <Settings size={14} />
      </button>

      <button
        class={styles.themeButton}
        onClick={cycleTheme}
        title={`Theme: ${theme()}`}
        aria-label={`Theme: ${theme()}`}
      >
        <ThemeIcon />
      </button>
    </div>
  );
};
