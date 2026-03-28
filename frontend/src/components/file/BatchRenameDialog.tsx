/**
 * @file Batch Rename Dialog
 *
 * Modal dialog for renaming multiple files at once using find/replace.
 * Shows a live preview of the name changes before applying them.
 *
 * **Behavior:**
 * - Displays the count of selected files.
 * - Two text inputs: "Find" and "Replace with".
 * - Live preview shows each file that would be renamed (old name -> new name).
 * - Files whose names do not contain the find pattern are not shown in the preview.
 * - "Rename" button applies all renames via the `batchRename` IPC command.
 * - On success, closes the dialog and refreshes the file list.
 * - Errors are displayed in the dialog.
 *
 * @module components/file/BatchRenameDialog
 */

import { createSignal, For, Show, type Component } from "solid-js";
import { X } from "lucide-solid";
import { batchRename } from "../../lib/ipc";
import styles from "./BatchRenameDialog.module.css";

/** Computed preview of a single file's rename result. */
interface RenamePreview {
  /** Original file path. */
  path: string;
  /** Original filename (basename). */
  oldName: string;
  /** New filename after find/replace. */
  newName: string;
}

/**
 * Extract the basename from a file path.
 * @param path - Storage-relative path (e.g. "/docs/readme.md").
 * @returns The basename (e.g. "readme.md").
 */
function basename(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

/**
 * Modal dialog for batch renaming files using find/replace.
 *
 * @param props.open     - Whether the dialog is visible.
 * @param props.onClose  - Callback to close the dialog.
 * @param props.diskId   - UUID of the disk containing the selected files.
 * @param props.paths    - Array of selected file paths to rename.
 * @param props.onDone   - Callback invoked after a successful rename (triggers refresh).
 */
export const BatchRenameDialog: Component<{
  open: boolean;
  onClose: () => void;
  diskId: string;
  paths: string[];
  onDone: () => void;
}> = (props) => {
  const [find, setFind] = createSignal("");
  const [replace, setReplace] = createSignal("");
  const [error, setError] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  /** Reset form state when dialog is closed or opened. */
  const reset = () => {
    setFind("");
    setReplace("");
    setError("");
    setSubmitting(false);
  };

  /** Compute the live preview of renames based on current find/replace values. */
  const previews = (): RenamePreview[] => {
    const f = find();
    if (!f) return [];
    const r = replace();
    const results: RenamePreview[] = [];
    for (const path of props.paths) {
      const oldName = basename(path);
      const newName = oldName.replaceAll(f, r);
      if (newName !== oldName && newName.length > 0) {
        results.push({ path, oldName, newName });
      }
    }
    return results;
  };

  /** Submit the batch rename to the backend. */
  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const f = find();
    if (!f) {
      setError("Find pattern cannot be empty");
      return;
    }
    const r = replace();
    if (r.includes("/") || r.includes("\\")) {
      setError("Replace pattern cannot contain path separators");
      return;
    }

    setError("");
    setSubmitting(true);
    try {
      await batchRename(props.diskId, props.paths, f, r);
      reset();
      props.onDone();
      props.onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  /** Close dialog and reset state. */
  const handleClose = () => {
    reset();
    props.onClose();
  };

  /** Close on backdrop click. */
  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) handleClose();
  };

  /** Close on Escape key. */
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") handleClose();
  };

  return (
    <Show when={props.open}>
      <div
        class={styles.backdrop}
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        role="dialog"
        aria-modal="true"
        aria-label="Batch Rename"
      >
        <div class={styles.dialog}>
          <div class={styles.header}>
            <span>Batch Rename ({props.paths.length} files)</span>
            <button
              class={styles.closeButton}
              onClick={handleClose}
              title="Close"
            >
              <X size={14} />
            </button>
          </div>
          <form class={styles.form} onSubmit={handleSubmit}>
            <div class={styles.field}>
              <label class={styles.label}>Find</label>
              <input
                class={styles.input}
                type="text"
                placeholder="Text to find..."
                value={find()}
                onInput={(e) => setFind(e.currentTarget.value)}
                autofocus
              />
            </div>
            <div class={styles.field}>
              <label class={styles.label}>Replace with</label>
              <input
                class={styles.input}
                type="text"
                placeholder="Replacement text..."
                value={replace()}
                onInput={(e) => setReplace(e.currentTarget.value)}
              />
            </div>

            <div class={styles.field}>
              <span class={styles.label}>Preview</span>
              <Show
                when={previews().length > 0}
                fallback={
                  <span class={styles.noChanges}>
                    {find() ? "No files match the find pattern" : "Enter a find pattern to see preview"}
                  </span>
                }
              >
                <div class={styles.previewSection}>
                  <For each={previews()}>
                    {(p) => (
                      <div class={styles.previewRow}>
                        <span class={styles.previewOld}>{p.oldName}</span>
                        <span class={styles.previewArrow}>&rarr;</span>
                        <span class={styles.previewNew}>{p.newName}</span>
                      </div>
                    )}
                  </For>
                </div>
              </Show>
            </div>

            <Show when={error()}>
              <div class={styles.error}>{error()}</div>
            </Show>

            <div class={styles.actions}>
              <button
                type="button"
                class={styles.cancelButton}
                onClick={handleClose}
              >
                Cancel
              </button>
              <button
                type="submit"
                class={styles.submitButton}
                disabled={submitting() || previews().length === 0}
              >
                {submitting() ? "Renaming..." : "Rename"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Show>
  );
};
