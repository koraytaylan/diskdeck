/**
 * @file Command Palette
 *
 * A VS Code-style command palette overlay activated by Cmd/Ctrl+P.
 * Provides fuzzy search across three categories:
 *
 * - **Actions**: All registered keyboard shortcut actions from `shortcutLabels`.
 * - **Disks**: All configured storage backends from `DiskContext`.
 * - **Recent paths**: Recently navigated directories from `FileContext`.
 *
 * Results are filtered by fuzzy substring match and sorted by match position
 * (prefix matches rank higher). Keyboard navigation (Up/Down/Enter/Escape)
 * and click selection are supported.
 *
 * @module components/palette/CommandPalette
 */

import { createSignal, createMemo, Show, For, onMount, type Component } from "solid-js";
import { useShortcut } from "../../contexts/ShortcutContext";
import { useDisk } from "../../contexts/DiskContext";
import { useFile } from "../../contexts/FileContext";
import { useTab } from "../../contexts/TabContext";
import { shortcutLabels } from "../../lib/shortcuts";
import { fuzzyMatch, fuzzyScore } from "../../lib/fuzzy";
import styles from "./CommandPalette.module.css";

/** The three types of results the palette can display. */
type PaletteCategory = "Action" | "Disk" | "Path";

/** A single result item in the command palette. */
interface PaletteItem {
  /** Display category for the badge. */
  category: PaletteCategory;
  /** Text displayed to the user and matched against the query. */
  label: string;
  /** Formatted shortcut hint (only for actions). */
  shortcutHint: string;
  /** Callback executed when this item is selected. */
  onSelect: () => void;
}

/** Maximum number of visible results in the palette. */
const MAX_RESULTS = 15;

/** Props for the CommandPalette component. */
interface CommandPaletteProps {
  /** Whether the palette is currently visible. */
  open: boolean;
  /** Callback to close the palette. */
  onClose: () => void;
}

/**
 * Command palette overlay component.
 * Renders a modal with a search input and categorized results list.
 */
export const CommandPalette: Component<CommandPaletteProps> = (props) => {
  const { getFormattedBinding, triggerAction } = useShortcut();
  const { state: diskState } = useDisk();
  const { navigate, recentPaths } = useFile();
  const { openBrowserTab } = useTab();

  const [query, setQuery] = createSignal("");
  const [selectedIndex, setSelectedIndex] = createSignal(0);

  let inputRef: HTMLInputElement | undefined;

  /** Build the full list of palette items from all three sources. */
  const allItems = createMemo((): PaletteItem[] => {
    const items: PaletteItem[] = [];

    // Actions from shortcutLabels
    for (const [actionId, label] of Object.entries(shortcutLabels)) {
      items.push({
        category: "Action",
        label,
        shortcutHint: getFormattedBinding(actionId),
        onSelect: () => {
          props.onClose();
          triggerAction(actionId);
        },
      });
    }

    // Disks
    for (const disk of diskState.disks) {
      items.push({
        category: "Disk",
        label: disk.name,
        shortcutHint: "",
        onSelect: () => {
          props.onClose();
          openBrowserTab(disk.id, disk.name);
        },
      });
    }

    // Recent paths
    for (const recent of recentPaths()) {
      const displayLabel = `${recent.diskName || recent.diskId}: ${recent.path}`;
      items.push({
        category: "Path",
        label: displayLabel,
        shortcutHint: "",
        onSelect: () => {
          props.onClose();
          navigate(recent.diskId, recent.path);
        },
      });
    }

    return items;
  });

  /** Filtered and scored results based on the current query. */
  const filteredItems = createMemo((): PaletteItem[] => {
    const q = query().trim();
    const source = allItems();

    if (!q) return source.slice(0, MAX_RESULTS);

    return source
      .filter((item) => fuzzyMatch(q, item.label))
      .sort((a, b) => fuzzyScore(q, a.label) - fuzzyScore(q, b.label))
      .slice(0, MAX_RESULTS);
  });

  /** Handle keyboard navigation within the palette. */
  const handleKeyDown = (e: KeyboardEvent) => {
    const items = filteredItems();
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, items.length - 1));
        scrollSelectedIntoView();
        break;
      case "ArrowUp":
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
        scrollSelectedIntoView();
        break;
      case "Enter":
        e.preventDefault();
        if (items[selectedIndex()]) {
          items[selectedIndex()].onSelect();
        }
        break;
      case "Escape":
        e.preventDefault();
        props.onClose();
        break;
    }
  };

  /** Scroll the currently selected item into the visible area. */
  const scrollSelectedIntoView = () => {
    requestAnimationFrame(() => {
      const container = inputRef?.closest(`.${styles.palette}`)?.querySelector(`.${styles.results}`);
      const selected = container?.querySelector(`.${styles.itemSelected}`);
      selected?.scrollIntoView({ block: "nearest" });
    });
  };

  /** Reset state and focus input when the palette opens. */
  const handleOpen = () => {
    setQuery("");
    setSelectedIndex(0);
    requestAnimationFrame(() => inputRef?.focus());
  };

  // Auto-focus when opened
  onMount(() => {
    if (props.open) handleOpen();
  });

  // Watch for open state changes
  createMemo(() => {
    if (props.open) handleOpen();
  });

  /** Badge CSS class for a given category. */
  const badgeClass = (category: PaletteCategory): string => {
    switch (category) {
      case "Action": return `${styles.badge} ${styles.badgeAction}`;
      case "Disk": return `${styles.badge} ${styles.badgeDisk}`;
      case "Path": return `${styles.badge} ${styles.badgePath}`;
    }
  };

  return (
    <Show when={props.open}>
      <div class={styles.overlay} onClick={() => props.onClose()} aria-hidden="true" />
      <div class={styles.palette} role="dialog" aria-label="Command palette">
        <input
          ref={inputRef}
          class={styles.input}
          type="text"
          placeholder="Type a command, disk name, or path..."
          value={query()}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setSelectedIndex(0);
          }}
          onKeyDown={handleKeyDown}
          aria-label="Command palette search"
        />
        <div class={styles.results} role="listbox">
          <Show when={filteredItems().length === 0}>
            <div class={styles.empty}>No matching results</div>
          </Show>
          <For each={filteredItems()}>
            {(item, index) => (
              <div
                class={`${styles.item} ${index() === selectedIndex() ? styles.itemSelected : ""}`}
                role="option"
                aria-selected={index() === selectedIndex()}
                onClick={() => item.onSelect()}
                onMouseEnter={() => setSelectedIndex(index())}
              >
                <span class={badgeClass(item.category)}>{item.category}</span>
                <span class={styles.label}>{item.label}</span>
                <Show when={item.shortcutHint}>
                  <span class={styles.shortcutHint}>{item.shortcutHint}</span>
                </Show>
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
};
