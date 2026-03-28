/**
 * @file Search Bar
 *
 * Controlled text input for searching entries across all disks.
 * Renders a search icon, a text input, and a clear button (visible
 * only when the input has content).
 *
 * The `data-search-input` attribute on the input element allows
 * `AppLayout` to programmatically focus it via a DOM query when
 * the search shortcut is triggered.
 *
 * Pressing Escape clears the search and exits search mode.
 *
 * @module components/search/SearchBar
 */

import { Show, type Component } from "solid-js";
import { Search, X } from "lucide-solid";
import styles from "./SearchBar.module.css";

/**
 * Search input component.
 *
 * @param props.value   - Current input value (controlled).
 * @param props.onInput - Callback on every input change.
 * @param props.onClear - Callback to clear the search.
 * @param props.ref     - Optional ref callback for the input element.
 */
export const SearchBar: Component<{
  value: string;
  onInput: (value: string) => void;
  onClear: () => void;
  ref?: (el: HTMLInputElement) => void;
}> = (props) => {
  return (
    <div class={styles.wrapper}>
      <Search size={13} style={{ color: "var(--ui-fg)", "flex-shrink": "0" }} />
      <input
        ref={props.ref}
        class={styles.input}
        type="text"
        data-search-input
        placeholder="Search..."
        value={props.value}
        onInput={(e) => props.onInput(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            props.onClear();
          }
        }}
      />
      <Show when={props.value}>
        <button class={styles.clearButton} onClick={props.onClear} title="Clear search" aria-label="Clear search">
          <X size={12} />
        </button>
      </Show>
    </div>
  );
};
