/**
 * @file Settings Dialog
 *
 * Modal dialog with three tabs:
 *
 * **Appearance tab:**
 * Displays a theme picker with three options (dark, mirage, light).
 * Selecting a theme immediately applies it via `ThemeContext.setTheme()`.
 *
 * **Shortcuts tab:**
 * Lists all keyboard shortcuts grouped by category. Each shortcut shows
 * its current binding and allows rebinding via a "capture mode":
 * 1. User clicks a shortcut binding button -> enters capture mode.
 * 2. The next non-modifier keypress is captured as the new binding.
 * 3. If the new binding conflicts with another action, the conflicting
 *    action's binding is cleared and a warning is shown for 3 seconds.
 * 4. Escape during capture cancels without changes.
 * 5. Individual shortcuts can be reset to defaults via a per-row reset button.
 * 6. "Reset All to Defaults" restores all bindings at once.
 *
 * **Profiles tab:**
 * Export all disk configurations as JSON (credentials redacted) for sharing
 * with team members, and import profiles from a JSON string. Imported disks
 * get fresh UUIDs and require credential entry via Edit Disk.
 *
 * The `data-shortcut-capture` attribute is set on the capture button so
 * the global shortcut handler in `ShortcutContext` knows to skip events
 * during capture mode.
 *
 * @module components/settings/SettingsDialog
 */

import { Show, For, createSignal, onCleanup, type Component } from "solid-js";
import { X, RotateCcw, Moon, Sunset, Sun, Download, Upload } from "lucide-solid";
import { useShortcut } from "../../contexts/ShortcutContext";
import { useTheme, type Theme } from "../../contexts/ThemeContext";
import { useDisk } from "../../contexts/DiskContext";
import {
  defaultShortcuts,
  shortcutLabels,
  shortcutCategories,
  formatShortcut,
  serializeKeys,
} from "../../lib/shortcuts";
import { exportProfiles, importProfiles } from "../../lib/ipc";
import styles from "./SettingsDialog.module.css";

/** Set of modifier key names, used to ignore modifier-only keypresses during capture. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/** Available theme choices for the appearance tab. */
const themeOptions: { value: Theme; label: string; Icon: typeof Moon }[] = [
  { value: "dark", label: "Dark", Icon: Moon },
  { value: "mirage", label: "Mirage", Icon: Sunset },
  { value: "light", label: "Light", Icon: Sun },
];

/**
 * Settings dialog with appearance and shortcuts tabs.
 *
 * @param props.open    - Whether the dialog is visible.
 * @param props.onClose - Callback to close the dialog.
 */
export const SettingsDialog: Component<{
  open: boolean;
  onClose: () => void;
}> = (props) => {
  const { state, setBinding, resetBinding, resetAllBindings, getConflict } =
    useShortcut();
  const { theme, setTheme } = useTheme();
  const { loadDisks } = useDisk();

  const [tab, setTab] = createSignal<"appearance" | "shortcuts" | "profiles">("appearance");
  const [profileMsg, setProfileMsg] = createSignal<string | null>(null);
  const [capturingAction, setCapturingAction] = createSignal<string | null>(
    null,
  );
  const [conflictMsg, setConflictMsg] = createSignal<string | null>(null);

  let conflictTimer: ReturnType<typeof setTimeout> | undefined;

  onCleanup(() => {
    if (conflictTimer) clearTimeout(conflictTimer);
  });

  const showConflict = (msg: string) => {
    if (conflictTimer) clearTimeout(conflictTimer);
    setConflictMsg(msg);
    conflictTimer = setTimeout(() => setConflictMsg(null), 3000);
  };

  const isDefault = (actionId: string): boolean => {
    const current = state.bindings[actionId];
    const def = defaultShortcuts[actionId];
    if (!current || !def) return true;
    return serializeKeys(current) === serializeKeys(def);
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      setCapturingAction(null);
      props.onClose();
    }
  };

  const handleDialogKeyDown = (e: KeyboardEvent) => {
    const capturing = capturingAction();
    if (capturing) {
      e.preventDefault();
      e.stopPropagation();

      // Escape cancels capture
      if (e.key === "Escape") {
        setCapturingAction(null);
        return;
      }

      // Ignore modifier-only presses
      if (MODIFIERS.has(e.key)) return;

      // Build key combo
      const keys: string[] = [];
      if (e.ctrlKey) keys.push("Control");
      if (e.shiftKey) keys.push("Shift");
      if (e.altKey) keys.push("Alt");
      if (e.metaKey) keys.push("Meta");
      keys.push(e.key);

      // Check for conflict
      const conflict = getConflict(capturing, keys);
      if (conflict) {
        const label = shortcutLabels[conflict] ?? conflict;
        setBinding(conflict, []);
        showConflict(`Replaced binding for "${label}"`);
      }

      setBinding(capturing, keys);
      setCapturingAction(null);
    } else if (e.key === "Escape") {
      props.onClose();
    }
  };

  const handleResetAll = () => {
    resetAllBindings();
    setConflictMsg(null);
  };

  return (
    <Show when={props.open}>
      <div class={styles.backdrop} onClick={handleBackdropClick}>
        <div
          class={styles.dialog}
          onKeyDown={handleDialogKeyDown}
          tabIndex={-1}
          ref={(el) => requestAnimationFrame(() => el.focus())}
        >
          <div class={styles.header}>
            <span>Settings</span>
            <button class={styles.closeButton} onClick={props.onClose}>
              <X size={14} />
            </button>
          </div>

          <div class={styles.tabBar}>
            <button
              class={styles.tabButton}
              classList={{ [styles.tabActive]: tab() === "appearance" }}
              onClick={() => setTab("appearance")}
            >
              Appearance
            </button>
            <button
              class={styles.tabButton}
              classList={{ [styles.tabActive]: tab() === "shortcuts" }}
              onClick={() => {
                setTab("shortcuts");
                setCapturingAction(null);
              }}
            >
              Shortcuts
            </button>
            <button
              class={styles.tabButton}
              classList={{ [styles.tabActive]: tab() === "profiles" }}
              onClick={() => setTab("profiles")}
            >
              Profiles
            </button>
          </div>

          <div class={styles.body}>
            <Show when={tab() === "appearance"}>
              <div class={styles.sectionLabel}>Theme</div>
              <div class={styles.themeGroup}>
                <For each={themeOptions}>
                  {(opt) => (
                    <button
                      class={styles.themeOption}
                      classList={{
                        [styles.themeOptionActive]: theme() === opt.value,
                      }}
                      onClick={() => setTheme(opt.value)}
                    >
                      <opt.Icon size={14} />
                      {opt.label}
                    </button>
                  )}
                </For>
              </div>
            </Show>

            <Show when={tab() === "shortcuts"}>
              <For each={shortcutCategories}>
                {(category) => (
                  <>
                    <div class={styles.categoryLabel}>{category.label}</div>
                    <For each={category.actions}>
                      {(actionId) => {
                        const binding = () => state.bindings[actionId] ?? [];
                        const isCapturing = () =>
                          capturingAction() === actionId;

                        return (
                          <div class={styles.shortcutRow}>
                            <span class={styles.actionLabel}>
                              {shortcutLabels[actionId] ?? actionId}
                            </span>
                            <div class={styles.rowRight}>
                              <button
                                class={styles.keyCombo}
                                classList={{
                                  [styles.capturing]: isCapturing(),
                                  [styles.unset]:
                                    !isCapturing() && binding().length === 0,
                                }}
                                data-shortcut-capture={
                                  isCapturing() ? "" : undefined
                                }
                                onClick={() => setCapturingAction(actionId)}
                              >
                                {isCapturing()
                                  ? "Press keys..."
                                  : binding().length > 0
                                    ? formatShortcut(binding())
                                    : "Unset"}
                              </button>
                              <Show when={!isDefault(actionId)}>
                                <button
                                  class={styles.resetButton}
                                  title="Reset to default"
                                  onClick={() => resetBinding(actionId)}
                                >
                                  <RotateCcw size={12} />
                                </button>
                              </Show>
                            </div>
                          </div>
                        );
                      }}
                    </For>
                  </>
                )}
              </For>

              <Show when={conflictMsg()}>
                <div class={styles.conflictWarning}>{conflictMsg()}</div>
              </Show>

              <button class={styles.resetAllButton} onClick={handleResetAll}>
                Reset All to Defaults
              </button>
            </Show>

            <Show when={tab() === "profiles"}>
              <div class={styles.sectionLabel}>Export / Import</div>
              <p class={styles.profileDescription}>
                Export disk configurations for team sharing. Credentials are
                stripped — recipients must enter their own via Edit Disk.
              </p>
              <div class={styles.profileActions}>
                <button
                  class={styles.profileButton}
                  onClick={async () => {
                    try {
                      const json = await exportProfiles();
                      await navigator.clipboard.writeText(json);
                      setProfileMsg("Profiles copied to clipboard");
                    } catch (e) {
                      setProfileMsg(`Export failed: ${String(e)}`);
                    }
                  }}
                >
                  <Download size={14} />
                  Export to Clipboard
                </button>
                <button
                  class={styles.profileButton}
                  onClick={async () => {
                    const json = window.prompt("Paste profile JSON:");
                    if (!json) return;
                    try {
                      const created = await importProfiles(json);
                      await loadDisks();
                      setProfileMsg(`Imported ${created.length} profile(s)`);
                    } catch (e) {
                      setProfileMsg(`Import failed: ${String(e)}`);
                    }
                  }}
                >
                  <Upload size={14} />
                  Import from JSON
                </button>
              </div>
              <Show when={profileMsg()}>
                <div class={styles.profileMessage}>{profileMsg()}</div>
              </Show>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};
