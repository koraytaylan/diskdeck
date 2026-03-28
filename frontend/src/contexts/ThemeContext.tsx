/**
 * @file Theme Context
 *
 * Manages the application color theme with three variants: dark, mirage, and light.
 * Theme selection is persisted to `localStorage` and applied as a `data-theme`
 * attribute on `<html>`, which CSS variables in `global.css` key off of.
 *
 * **Persistence:**
 * Uses `localStorage` (key: "diskdeck-theme") for immediate, synchronous reads
 * at startup. This is separate from the backend preference store because the
 * theme must be available *before* the Tauri IPC bridge is ready, to avoid
 * a flash of unstyled content.
 *
 * **Cycling order:** dark -> mirage -> light -> dark ...
 *
 * Consumed by:
 * - `Toolbar` (theme icon button)
 * - `SettingsDialog` (theme picker)
 * - `AppLayout` (registers `view.cycleTheme` shortcut)
 *
 * @module contexts/ThemeContext
 */

import {
  createContext,
  useContext,
  createSignal,
  type ParentComponent,
} from "solid-js";

/** The three supported color themes. */
export type Theme = "dark" | "mirage" | "light";

/** localStorage key for theme persistence. */
const STORAGE_KEY = "diskdeck-theme";

/** Cycle order for the `cycleTheme()` action. */
const THEME_ORDER: Theme[] = ["dark", "mirage", "light"];

/** Shape of the value provided by ThemeContext. */
interface ThemeContextValue {
  /** Reactive accessor for the current theme. */
  theme: () => Theme;
  /** Set the theme to a specific value. */
  setTheme: (t: Theme) => void;
  /** Advance to the next theme in the cycle order. */
  cycleTheme: () => void;
}

const ThemeContext = createContext<ThemeContextValue>();

/**
 * Provider component that initializes theme state from localStorage,
 * applies the CSS `data-theme` attribute, and exposes theme controls
 * to the component tree.
 *
 * **Important:** The theme is applied synchronously during component
 * creation (not in `onMount`) so that CSS variables resolve before
 * the first paint, preventing a flash of the wrong theme.
 */
export const ThemeProvider: ParentComponent = (props) => {
  const stored = localStorage.getItem(STORAGE_KEY) as Theme | null;
  const initial: Theme =
    stored && THEME_ORDER.includes(stored) ? stored : "dark";

  const [theme, setThemeSignal] = createSignal<Theme>(initial);

  /** Apply theme to the DOM and persist to localStorage. */
  const applyTheme = (t: Theme) => {
    document.documentElement.setAttribute("data-theme", t);
    localStorage.setItem(STORAGE_KEY, t);
  };

  // Apply immediately (not in onMount) so CSS variables resolve before first paint
  applyTheme(initial);

  const setTheme = (t: Theme) => {
    setThemeSignal(t);
    applyTheme(t);
  };

  const cycleTheme = () => {
    const idx = THEME_ORDER.indexOf(theme());
    const next = THEME_ORDER[(idx + 1) % THEME_ORDER.length];
    setTheme(next);
  };

  return (
    <ThemeContext.Provider value={{ theme, setTheme, cycleTheme }}>
      {props.children}
    </ThemeContext.Provider>
  );
};

/**
 * Hook to access theme state and controls.
 * Must be called within a `<ThemeProvider>` subtree.
 *
 * @returns ThemeContextValue with `theme()`, `setTheme()`, and `cycleTheme()`.
 * @throws Error if called outside ThemeProvider.
 */
export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
