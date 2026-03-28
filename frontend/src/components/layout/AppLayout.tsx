/**
 * @file App Layout
 *
 * Root layout component that assembles the three-panel UI:
 *   Sidebar (disk tree) | Main Panel (file list/grid/search) | Properties Panel
 *
 * **Responsibilities:**
 * - Manages the resizable three-panel layout via `@corvu/resizable`.
 * - Owns top-level search state (query, results, loading) and wires the
 *   debounced search to the `searchEntries` IPC call.
 * - Persists and restores the view mode (list vs. grid) preference.
 * - Renders the `JobPanel` for background job progress tracking.
 * - Renders the `Toolbar`, `SettingsDialog`, and panel toggle buttons.
 * - Registers global shortcut actions (search focus, cycle theme,
 *   toggle sidebar/properties, toggle grid view).
 *
 * **Architecture note:** The `ResizableContent` inner component exists
 * because `@corvu/resizable` exposes its context only to children of
 * `<Resizable>`. Panel toggle logic requires that context, so it lives
 * in `ResizableContent` while search/settings state lives in the outer
 * `AppLayout` component.
 *
 * @module components/layout/AppLayout
 */

import Resizable from "@corvu/resizable";
import { createSignal, Show, onMount, onCleanup, type Component } from "solid-js";
import { PanelLeftClose, PanelRightClose } from "lucide-solid";
import { Sidebar } from "./Sidebar";
import { MainPanel } from "./MainPanel";
import { PropertiesPanel } from "./PropertiesPanel";
import { Toolbar } from "./Toolbar";
import { SettingsDialog } from "../settings/SettingsDialog";
import { JobPanel } from "../jobs/JobPanel";
import { CommandPalette } from "../palette/CommandPalette";
import { searchEntries, getPreference, setPreference } from "../../lib/ipc";
import { createDebouncedSearch } from "../../lib/search";
import { isPanelCollapsed, toggleAction, SIDEBAR_INDEX, PROPERTIES_INDEX } from "../../lib/panels";
import { useShortcut } from "../../contexts/ShortcutContext";
import { useTheme } from "../../contexts/ThemeContext";
import type { SearchResult } from "../../lib/types";
import styles from "./AppLayout.module.css";

/**
 * Inner component rendered inside `<Resizable>` to gain access to the
 * resizable panel context (`ctx.sizes()`, `ctx.collapse()`, `ctx.expand()`).
 * Handles panel toggle logic and registers keyboard shortcut actions that
 * depend on the panel context.
 */
const ResizableContent: Component<{
  searchQuery: string;
  searchResults: SearchResult[];
  searchLoading: boolean;
  onSearchClear: () => void;
  onSearchInput: (value: string) => void;
  searchRef: (el: HTMLInputElement) => void;
  viewMode: "list" | "grid";
  onToggleView: () => void;
}> = (props) => {
  const { registerAction } = useShortcut();
  const { cycleTheme } = useTheme();
  const ctx = Resizable.useContext();

  const sidebarCollapsed = () => isPanelCollapsed(ctx.sizes(), SIDEBAR_INDEX);
  const propertiesCollapsed = () => isPanelCollapsed(ctx.sizes(), PROPERTIES_INDEX);

  const toggleSidebar = () => {
    if (toggleAction(ctx.sizes(), SIDEBAR_INDEX) === "expand") {
      ctx.expand(SIDEBAR_INDEX);
    } else {
      ctx.collapse(SIDEBAR_INDEX);
    }
  };

  const toggleProperties = () => {
    if (toggleAction(ctx.sizes(), PROPERTIES_INDEX) === "expand") {
      ctx.expand(PROPERTIES_INDEX);
    } else {
      ctx.collapse(PROPERTIES_INDEX);
    }
  };

  onMount(() => {
    const cleanups = [
      registerAction("search.focus", () => {
        /* forwarded via ref */ const el = document.querySelector<HTMLInputElement>('[data-search-input]');
        el?.focus();
      }),
      registerAction("view.cycleTheme", cycleTheme),
      registerAction("view.toggleSidebar", toggleSidebar),
      registerAction("view.toggleProperties", toggleProperties),
      registerAction("view.toggleGrid", props.onToggleView),
    ];
    onCleanup(() => cleanups.forEach((fn) => fn()));
  });

  return (
    <>
      <Resizable.Panel
        initialSize={0.2}
        minSize={0.12}
        collapsible
        collapsedSize={0}
        style={{ overflow: "hidden" }}
      >
        <Sidebar onCollapse={toggleSidebar} />
      </Resizable.Panel>

      <Resizable.Handle class={styles.handle}>
        <Show when={sidebarCollapsed()}>
          <button
            class={styles.expandButton}
            title="Expand sidebar"
            aria-label="Expand sidebar"
            onClick={(e) => { e.stopPropagation(); toggleSidebar(); }}
          >
            <PanelLeftClose size={12} />
          </button>
        </Show>
      </Resizable.Handle>

      <Resizable.Panel initialSize={0.55} minSize={0.25} style={{ overflow: "hidden" }}>
        <MainPanel
          searchQuery={props.searchQuery}
          searchResults={props.searchResults}
          searchLoading={props.searchLoading}
          onSearchClear={props.onSearchClear}
          viewMode={props.viewMode}
        />
      </Resizable.Panel>

      <Resizable.Handle class={styles.handle}>
        <Show when={propertiesCollapsed()}>
          <button
            class={styles.expandButton}
            title="Expand properties"
            aria-label="Expand properties"
            onClick={(e) => { e.stopPropagation(); toggleProperties(); }}
          >
            <PanelRightClose size={12} />
          </button>
        </Show>
      </Resizable.Handle>

      <Resizable.Panel
        initialSize={0.25}
        minSize={0.15}
        collapsible
        collapsedSize={0}
        style={{ overflow: "hidden" }}
      >
        <PropertiesPanel onCollapse={toggleProperties} />
      </Resizable.Panel>
    </>
  );
};

/**
 * Top-level layout component. Owns search state, view mode, and settings
 * dialog visibility. Renders the toolbar, the resizable three-panel
 * layout, the settings dialog, and the job panel.
 */
export const AppLayout: Component = () => {
  const [searchQuery, setSearchQuery] = createSignal("");
  const [searchResults, setSearchResults] = createSignal<SearchResult[]>([]);
  const [searchLoading, setSearchLoading] = createSignal(false);
  const [viewMode, setViewMode] = createSignal<"list" | "grid">("list");
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [paletteOpen, setPaletteOpen] = createSignal(false);

  const { registerAction } = useShortcut();

  // Ref for the search input element, kept for future focus management
  const searchInputRefHolder = { current: undefined as HTMLInputElement | undefined };

  onMount(() => {
    const cleanup = registerAction("palette.open", () =>
      setPaletteOpen((prev) => !prev),
    );
    onCleanup(cleanup);

    getPreference("viewMode").then((v) => {
      if (v === "grid") setViewMode("grid");
    }).catch((e) => console.warn(e));
  });

  const toggleView = () => {
    const next = viewMode() === "list" ? "grid" : "list";
    setViewMode(next);
    setPreference("viewMode", next).catch((e) => console.warn(e));
  };

  const debouncedSearch = createDebouncedSearch<SearchResult[]>({
    delay: 300,
    fetcher: (query) =>
      searchEntries({ pattern: query, disk_ids: null, recursive: true }),
    onResults: (results) => setSearchResults(results),
    onLoading: (loading) => setSearchLoading(loading),
    onEmpty: () => {
      setSearchResults([]);
      setSearchLoading(false);
    },
  });

  const handleSearchInput = (value: string) => {
    setSearchQuery(value);
    debouncedSearch.search(value);
  };

  const handleSearchClear = () => {
    setSearchQuery("");
    debouncedSearch.clear();
  };

  return (
    <div class={styles.root}>
      <Toolbar
        searchQuery={searchQuery()}
        onSearchInput={handleSearchInput}
        onSearchClear={handleSearchClear}
        searchRef={(el) => { searchInputRefHolder.current = el; }}
        viewMode={viewMode()}
        onToggleView={toggleView}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      <Resizable class={styles.panels}>
        <ResizableContent
          searchQuery={searchQuery()}
          searchResults={searchResults()}
          searchLoading={searchLoading()}
          onSearchClear={handleSearchClear}
          onSearchInput={handleSearchInput}
          searchRef={(el) => { searchInputRefHolder.current = el; }}
          viewMode={viewMode()}
          onToggleView={toggleView}
        />
      </Resizable>
      <SettingsDialog
        open={settingsOpen()}
        onClose={() => setSettingsOpen(false)}
      />
      <JobPanel />
      <CommandPalette
        open={paletteOpen()}
        onClose={() => setPaletteOpen(false)}
      />
    </div>
  );
};
