/**
 * @file File Preview
 *
 * Renders a preview of a file's contents based on its type:
 * - **Text/Code**: Monospace `<pre>` block with the file content.
 * - **Images**: Centered `<img>` with a blob URL.
 * - **Binary/Large**: File metadata with a "cannot preview" message.
 *
 * Data is fetched via the `read_file` IPC command when the component mounts.
 * Image blob URLs are cleaned up via `onCleanup` to prevent memory leaks.
 *
 * Files larger than 10 MB skip the read and show the binary fallback.
 *
 * @module components/preview/FilePreview
 */

import { createSignal, onMount, onCleanup, Switch, Match, type Component } from "solid-js";
import type { PreviewData } from "../../lib/types";
import { readFile, getEntry } from "../../lib/ipc";
import { classifyForPreview, isArchive, MAX_PREVIEW_SIZE } from "../../lib/preview-utils";
import { CodePreview } from "./CodePreview";
import { ArchivePreview } from "./ArchivePreview";
import styles from "./FilePreview.module.css";

/**
 * File preview component. Fetches and displays file contents on mount.
 *
 * @param props.diskId   - UUID of the disk containing the file.
 * @param props.path     - Storage-relative path to the file.
 * @param props.name     - File basename (used for display and classification).
 * @param props.mimeType - MIME type from the backend, or null if unknown.
 */
export const FilePreview: Component<{
  diskId: string;
  path: string;
  name: string;
  mimeType: string | null;
}> = (props) => {
  const [data, setData] = createSignal<PreviewData>({ status: "loading" });
  let blobUrl: string | undefined;

  onMount(async () => {
    try {
      // Check file size first via stat to avoid loading huge files
      const entry = await getEntry(props.diskId, props.path);
      const kind = classifyForPreview(props.mimeType, props.name);

      if (entry.size > MAX_PREVIEW_SIZE || kind === "binary") {
        setData({
          status: "binary",
          size: entry.size,
          mimeType: props.mimeType,
          modified: entry.modified,
        });
        return;
      }

      const bytes = await readFile(props.diskId, props.path);
      const uint8 = new Uint8Array(bytes);

      if (kind === "image") {
        const mime = props.mimeType ?? "application/octet-stream";
        const blob = new Blob([uint8], { type: mime });
        blobUrl = URL.createObjectURL(blob);
        setData({ status: "image", blobUrl, mimeType: mime });
      } else {
        const content = new TextDecoder("utf-8", { fatal: false }).decode(uint8);
        setData({ status: "text", content });
      }
    } catch (e) {
      setData({ status: "error", message: String(e) });
    }
  });

  onCleanup(() => {
    if (blobUrl) URL.revokeObjectURL(blobUrl);
  });

  /**
   * Formats a byte count into a human-readable string (B, KB, or MB).
   * @param bytes - Raw byte count.
   * @returns Formatted size string.
   */
  const formatSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  // Archives get a dedicated preview that lists their contents
  if (isArchive(props.name)) {
    return (
      <ArchivePreview
        diskId={props.diskId}
        path={props.path}
        name={props.name}
      />
    );
  }

  return (
    <div class={styles.container}>
      <Switch>
        <Match when={data().status === "loading"}>
          <div class={styles.center}>Loading preview...</div>
        </Match>
        <Match when={data().status === "error"}>
          <div class={styles.center}>
            <span class={styles.errorText}>
              {(data() as { status: "error"; message: string }).message}
            </span>
          </div>
        </Match>
        <Match when={data().status === "text"}>
          <CodePreview
            content={(data() as { status: "text"; content: string }).content}
            fileName={props.name}
          />
        </Match>
        <Match when={data().status === "image"}>
          <div class={styles.center}>
            <img
              class={styles.image}
              src={(data() as { status: "image"; blobUrl: string }).blobUrl}
              alt={props.name}
            />
          </div>
        </Match>
        <Match when={data().status === "binary"}>
          <div class={styles.center}>
            <div class={styles.binaryInfo}>
              <div class={styles.binaryName}>{props.name}</div>
              <div class={styles.binaryMeta}>
                {(data() as { status: "binary"; mimeType: string | null }).mimeType ?? "Unknown type"}
                {" — "}
                {formatSize((data() as { status: "binary"; size: number }).size)}
              </div>
              <div class={styles.binaryHint}>No preview available for this file type</div>
            </div>
          </div>
        </Match>
      </Switch>
    </div>
  );
};
