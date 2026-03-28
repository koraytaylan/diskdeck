/**
 * @file Shortcut Context
 *
 * Central registry for keyboard shortcuts. Manages the binding state,
 * dispatches keydown events to registered action handlers, and persists
 * user-customized bindings to the backend preference store.
 *
 * **Architecture:**
 * - **Bindings** (stored in a SolidJS store) map action IDs to key combos.
 *   Initialized from `defaultShortcuts`, then overridden with any persisted
 *   user customizations loaded from the backend on mount.
 * - **Handlers** (stored in a plain `Map`) map action IDs to callback functions.
 *   Components register handlers via `registerAction()` in `onMount` and
 *   deregister via the returned cleanup function in `onCleanup`.
 * - A global `keydown` listener matches incoming events against bindings
 *   and invokes the corresponding handler if one is registered.
 *
 * **Capture mode:**
 * When the `SettingsDialog` is capturing a new key combo, the target element
 * has a `data-shortcut-capture` attribute. The global handler skips events
 * from such elements to avoid triggering actions during rebinding.
 *
 * **Persistence:**
 * Only the *delta* from defaults is persisted (as JSON under the "shortcuts"
 * preference key). On load, only known action IDs with valid array values
 * are applied; unknown or corrupt data is silently ignored.
 *
 * Consumed by:
 * - `AppLayout` / `MainPanel` (register action handlers)
 * - `SettingsDialog` (read/write bindings, conflict detection)
 * - `ContextMenu` items (display formatted shortcut hints)
 *
 * @module contexts/ShortcutContext
 */

import {
  createContext,
  useContext,
  onMount,
  onCleanup,
  type ParentComponent,
} from "solid-js";
import { createStore } from "solid-js/store";
import {
  defaultShortcuts,
  matchesShortcut,
  formatShortcut,
  serializeKeys,
} from "../lib/shortcuts";
import { getPreference, setPreference } from "../lib/ipc";

/** A zero-argument callback that handles a shortcut action. */
type ActionHandler = () => void;

/** Reactive store shape for shortcut bindings. */
interface ShortcutState {
  /** Map of action ID -> key combo (e.g. "file.copy" -> ["Control", "c"]). */
  bindings: Record<string, string[]>;
}

/** Public API exposed by the shortcut context. */
interface ShortcutContextValue {
  /** Reactive store containing current bindings. */
  state: ShortcutState;
  /**
   * Register a handler for an action ID. Returns a cleanup function
   * that deregisters the handler (call it in `onCleanup`).
   */
  registerAction: (actionId: string, handler: ActionHandler) => () => void;
  /** Get the raw key combo for an action, or undefined if unbound. */
  getBinding: (actionId: string) => string[] | undefined;
  /** Get the human-readable formatted shortcut string (e.g. "^C"). */
  getFormattedBinding: (actionId: string) => string;
  /** Set a new key combo for an action and persist. */
  setBinding: (actionId: string, keys: string[]) => void;
  /** Reset a single action to its default binding and persist. */
  resetBinding: (actionId: string) => void;
  /** Reset all actions to their default bindings and persist. */
  resetAllBindings: () => void;
  /**
   * Check if a proposed key combo conflicts with an existing binding.
   * Returns the conflicting action ID, or undefined if no conflict.
   */
  getConflict: (actionId: string, keys: string[]) => string | undefined;
  /**
   * Programmatically trigger a registered action by its ID.
   * Used by the command palette to execute actions without a keyboard event.
   * Does nothing if no handler is registered for the given action.
   */
  triggerAction: (actionId: string) => void;
}

/** Backend preference key under which custom bindings are stored. */
const PREF_KEY = "shortcuts";

const ShortcutContext = createContext<ShortcutContextValue>();

/**
 * Provider component that initializes the shortcut system, listens for
 * global keydown events, and provides binding management to the tree.
 */
export const ShortcutProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<ShortcutState>({
    bindings: { ...defaultShortcuts },
  });

  /** Runtime registry of action handlers (not reactive -- plain Map). */
  const handlers = new Map<string, ActionHandler>();

  /**
   * Persist only the non-default bindings to the backend.
   * This keeps the stored data minimal and forward-compatible
   * (new default shortcuts are picked up automatically).
   */
  const persistBindings = () => {
    const delta: Record<string, string[]> = {};
    for (const [id, keys] of Object.entries(state.bindings)) {
      if (serializeKeys(keys) !== serializeKeys(defaultShortcuts[id] ?? [])) {
        delta[id] = keys;
      }
    }
    setPreference(PREF_KEY, JSON.stringify(delta)).catch((e) => console.warn(e));
  };

  /** Load user-customized bindings from the backend on startup. */
  const loadBindings = async () => {
    try {
      const raw = await getPreference(PREF_KEY);
      if (!raw) return;
      const stored = JSON.parse(raw) as Record<string, string[]>;
      for (const [id, keys] of Object.entries(stored)) {
        // Only apply known action IDs with valid array values
        if (Array.isArray(keys) && id in defaultShortcuts) {
          setState("bindings", id, keys);
        }
      }
    } catch {
      // Ignore corrupt data -- fall back to defaults
    }
  };

  onMount(() => {
    loadBindings();
  });

  const registerAction = (actionId: string, handler: ActionHandler) => {
    handlers.set(actionId, handler);
    return () => {
      handlers.delete(actionId);
    };
  };

  const getBinding = (actionId: string) => state.bindings[actionId];

  const getFormattedBinding = (actionId: string) => {
    const binding = state.bindings[actionId];
    return binding ? formatShortcut(binding) : "";
  };

  const setBinding = (actionId: string, keys: string[]) => {
    setState("bindings", actionId, keys);
    persistBindings();
  };

  const resetBinding = (actionId: string) => {
    const def = defaultShortcuts[actionId];
    if (def) {
      setState("bindings", actionId, [...def]);
      persistBindings();
    }
  };

  const resetAllBindings = () => {
    for (const [id, keys] of Object.entries(defaultShortcuts)) {
      setState("bindings", id, [...keys]);
    }
    setPreference(PREF_KEY, "{}").catch((e) => console.warn(e));
  };

  /** Look up and invoke a registered handler by action ID. */
  const triggerAction = (actionId: string) => {
    const handler = handlers.get(actionId);
    if (handler) handler();
  };

  const getConflict = (actionId: string, keys: string[]): string | undefined => {
    const serialized = serializeKeys(keys);
    for (const [id, existing] of Object.entries(state.bindings)) {
      if (id === actionId) continue;
      if (existing.length > 0 && serializeKeys(existing) === serialized) {
        return id;
      }
    }
    return undefined;
  };

  /**
   * Global keydown handler. Skips events from input elements and
   * shortcut-capture elements, then iterates bindings to find a match.
   */
  const handleKeyDown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement;
    // Don't intercept when typing in inputs or during shortcut capture
    const tag = target?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (target?.hasAttribute("data-shortcut-capture")) return;

    for (const [actionId, keys] of Object.entries(state.bindings)) {
      if (keys.length > 0 && matchesShortcut(e, keys)) {
        const handler = handlers.get(actionId);
        if (handler) {
          e.preventDefault();
          e.stopPropagation();
          handler();
          return;
        }
      }
    }
  };

  if (typeof window !== "undefined") {
    window.addEventListener("keydown", handleKeyDown);
    onCleanup(() => window.removeEventListener("keydown", handleKeyDown));
  }

  return (
    <ShortcutContext.Provider
      value={{
        state,
        registerAction,
        getBinding,
        getFormattedBinding,
        setBinding,
        resetBinding,
        resetAllBindings,
        getConflict,
        triggerAction,
      }}
    >
      {props.children}
    </ShortcutContext.Provider>
  );
};

/**
 * Hook to access the shortcut registry and binding management.
 * Must be called within a `<ShortcutProvider>` subtree.
 *
 * @returns ShortcutContextValue with binding state, registration, and mutation methods.
 * @throws Error if called outside ShortcutProvider.
 */
export function useShortcut(): ShortcutContextValue {
  const ctx = useContext(ShortcutContext);
  if (!ctx)
    throw new Error("useShortcut must be used within ShortcutProvider");
  return ctx;
}
