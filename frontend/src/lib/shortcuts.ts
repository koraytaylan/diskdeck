/**
 * @file Keyboard Shortcut System
 *
 * Provides the static data and pure functions that power DiskDeck's
 * customizable keyboard shortcuts.
 *
 * **Binding format:**
 * A key combo is represented as `string[]` where the elements are
 * `KeyboardEvent.key` values. Modifier keys ("Control", "Shift", "Alt",
 * "Meta") come first (order doesn't matter), followed by exactly one
 * non-modifier key (e.g. `["Control", "Shift", "n"]`).
 *
 * **Action IDs** use a dot-separated namespace convention:
 * - `file.*`   - File operations (copy, paste, delete, etc.)
 * - `nav.*`    - Navigation (back, forward, up, address bar focus)
 * - `search.*` - Search
 * - `view.*`   - UI toggles (sidebar, properties panel, theme, grid)
 *
 * **Persistence:**
 * `ShortcutContext` stores only the *delta* (non-default bindings) via
 * `setPreference("shortcuts", ...)`. The data stored is a JSON object
 * mapping action IDs to their custom key combos.
 *
 * Related modules:
 * - `contexts/ShortcutContext.tsx` - Runtime registry and keydown dispatch.
 * - `components/settings/SettingsDialog.tsx` - Rebinding UI.
 *
 * @module lib/shortcuts
 */

/**
 * Default keyboard shortcut mappings.
 * Keys are dot-namespaced action IDs; values are key combo arrays.
 * These serve as the baseline when no user customizations exist.
 */
export const defaultShortcuts: Record<string, string[]> = {
  "file.copy": ["Control", "c"],
  "file.paste": ["Control", "v"],
  "file.cut": ["Control", "x"],
  "file.delete": ["Delete"],
  "file.rename": ["F2"],
  "file.selectAll": ["Control", "a"],
  "file.newFolder": ["Control", "Shift", "n"],
  "file.preview": [" "],
  "file.batchRename": ["Control", "Shift", "r"],
  "file.diff": ["Control", "Shift", "d"],
  "nav.back": ["Alt", "ArrowLeft"],
  "nav.forward": ["Alt", "ArrowRight"],
  "nav.up": ["Control", "ArrowUp"],
  "nav.open": ["Control", "ArrowDown"],
  "nav.addressBar": ["Control", "l"],
  "tab.close": ["Control", "w"],
  "tab.new": ["Control", "t"],
  "search.focus": ["Control", "f"],
  "view.toggleSidebar": ["Control", "b"],
  "view.toggleProperties": ["Control", "Shift", "b"],
  "view.cycleTheme": ["Control", "Shift", "t"],
  "view.toggleGrid": ["Control", "Shift", "g"],
  "palette.open": ["Control", "p"],
  "view.diskUsage": ["Control", "Shift", "u"],
};

/**
 * Human-readable labels for each action ID, displayed in the
 * settings dialog and context menu shortcut hints.
 */
export const shortcutLabels: Record<string, string> = {
  "file.copy": "Copy",
  "file.paste": "Paste",
  "file.cut": "Cut",
  "file.delete": "Delete",
  "file.rename": "Rename",
  "file.selectAll": "Select All",
  "file.newFolder": "New Folder",
  "file.preview": "Quick Preview",
  "file.batchRename": "Batch Rename",
  "file.diff": "Compare Directories",
  "nav.back": "Navigate Back",
  "nav.forward": "Navigate Forward",
  "nav.up": "Navigate Up",
  "nav.open": "Open Selected",
  "nav.addressBar": "Focus Address Bar",
  "tab.close": "Close Tab",
  "tab.new": "New Tab",
  "search.focus": "Search",
  "view.toggleSidebar": "Toggle Sidebar",
  "view.toggleProperties": "Toggle Properties",
  "view.cycleTheme": "Cycle Theme",
  "view.toggleGrid": "Toggle Grid View",
  "palette.open": "Command Palette",
  "view.diskUsage": "Disk Usage",
};

/**
 * Groupings of shortcut actions for the settings dialog UI.
 * Each category is rendered as a labeled section with its actions listed below.
 */
export const shortcutCategories: { label: string; actions: string[] }[] = [
  { label: "File", actions: ["file.copy", "file.paste", "file.cut", "file.delete", "file.rename", "file.batchRename", "file.diff", "file.selectAll", "file.newFolder", "file.preview"] },
  { label: "Navigation", actions: ["nav.back", "nav.forward", "nav.up", "nav.open", "nav.addressBar"] },
  { label: "Tabs", actions: ["tab.close", "tab.new"] },
  { label: "Search", actions: ["search.focus"] },
  { label: "View", actions: ["view.toggleSidebar", "view.toggleProperties", "view.cycleTheme", "view.toggleGrid", "view.diskUsage"] },
  { label: "General", actions: ["palette.open"] },
];

/** The four modifier key names recognized by the KeyboardEvent API. */
const MODIFIERS = ["Alt", "Control", "Meta", "Shift"];

/**
 * Serialize a key combo into a canonical, comparable string.
 * Modifiers are sorted alphabetically and the main key is lowercased,
 * joined by "+". This ensures that `["Shift", "Control", "n"]` and
 * `["Control", "Shift", "n"]` produce the same string.
 *
 * Used for conflict detection when rebinding shortcuts.
 *
 * @param keys - Key combo array.
 * @returns Canonical string, e.g. "control+shift+n".
 */
export function serializeKeys(keys: string[]): string {
  const mods = keys.filter((k) => MODIFIERS.includes(k)).sort();
  const main = keys.find((k) => !MODIFIERS.includes(k));
  return [...mods, main?.toLowerCase() ?? ""].join("+");
}

/**
 * Format a key combo for human display using macOS-style symbols.
 * Maps modifier names to Unicode glyphs and uppercases single-char keys.
 *
 * @example formatShortcut(["Control", "c"])           // "^C"
 * @example formatShortcut(["Control", "Shift", "n"])  // "^ShiftN"
 *
 * @param keys - Key combo array.
 * @returns Display string suitable for UI labels and menus.
 */
export function formatShortcut(keys: string[]): string {
  return keys
    .map((k) => {
      switch (k) {
        case "Meta": return "\u2318";
        case "Control": return "\u2303";
        case "Shift": return "\u21e7";
        case "Alt": return "\u2325";
        case "ArrowLeft": return "\u2190";
        case "ArrowRight": return "\u2192";
        case "ArrowUp": return "\u2191";
        case "ArrowDown": return "\u2193";
        case "Delete": return "Del";
        case "Backspace": return "\u232b";
        case "Enter": return "\u23ce";
        case "Escape": return "Esc";
        case " ": return "Space";
        default: return k.length === 1 ? k.toUpperCase() : k;
      }
    })
    .join("");
}

/**
 * Check if a keyboard event matches a key combo definition.
 *
 * Matching rules:
 * - **Platform-adaptive modifier**: "Control" in a binding matches either
 *   `ctrlKey` or `metaKey` (Cmd on macOS). This means `["Control", "c"]`
 *   matches both Ctrl+C (Windows/Linux) and Cmd+C (macOS). "Meta" in a
 *   binding behaves identically. This follows the convention that desktop
 *   apps use Cmd as the primary modifier on macOS and Ctrl everywhere else.
 * - Shift and Alt must match exactly.
 * - The non-modifier key is compared case-insensitively against both
 *   `e.key` and `e.code` (the latter handles special keys like "F2").
 * - Returns false if the combo contains no non-modifier key.
 *
 * @param e    - The native KeyboardEvent.
 * @param keys - Key combo definition to match against.
 * @returns True if the event matches the combo.
 */
export function matchesShortcut(e: KeyboardEvent, keys: string[]): boolean {
  const modifiers = new Set(keys.filter((k) =>
    ["Control", "Shift", "Alt", "Meta"].includes(k),
  ));
  const mainKey = keys.find(
    (k) => !["Control", "Shift", "Alt", "Meta"].includes(k),
  );

  if (!mainKey) return false;

  // "Control" or "Meta" in a binding means "the platform primary modifier"
  // — Cmd on macOS (metaKey), Ctrl on Windows/Linux (ctrlKey).
  // Accept either ctrlKey or metaKey for either modifier keyword.
  const primaryRequired = modifiers.has("Control") || modifiers.has("Meta");
  const primaryPressed = e.ctrlKey || e.metaKey;
  if (primaryPressed !== primaryRequired) return false;

  const shiftRequired = modifiers.has("Shift");
  const altRequired = modifiers.has("Alt");
  if (e.shiftKey !== shiftRequired) return false;
  if (e.altKey !== altRequired) return false;

  return e.key.toLowerCase() === mainKey.toLowerCase() || e.code === mainKey;
}
