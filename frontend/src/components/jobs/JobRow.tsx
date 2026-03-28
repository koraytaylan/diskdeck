/**
 * @file Job Row
 *
 * Renders a single job entry within the `JobPanel` list. Each row shows:
 * - A kind icon (copy / move / delete)
 * - A progress description ("Copying 3/10")
 * - The current item filename (while running)
 * - A thin progress bar (while running)
 * - A status icon (spinner / check / warning / ban)
 * - A cancel button (while running)
 *
 * Finished jobs are rendered at reduced opacity to visually distinguish
 * them from active work.
 *
 * @module components/jobs/JobRow
 */

import { Show, type Component } from "solid-js";
import {
  Copy,
  FolderInput,
  Trash2,
  X,
  Check,
  AlertTriangle,
  Ban,
  Loader,
} from "lucide-solid";
import type { JobInfo } from "../../lib/types";
import { useJobs } from "../../contexts/JobContext";
import styles from "./JobRow.module.css";

/** Maps each job kind to its corresponding Lucide icon component. */
const kindIcons = {
  copy: Copy,
  move: FolderInput,
  delete: Trash2,
} as const;

/** Human-readable verb for each job kind (used in progress labels). */
const kindLabels = {
  copy: "Copying",
  move: "Moving",
  delete: "Deleting",
} as const;

/**
 * A single row representing one backend job.
 *
 * @param props.job - The `JobInfo` snapshot to render.
 */
export const JobRow: Component<{ job: JobInfo }> = (props) => {
  const { cancel } = useJobs();

  /** Accessor for the current job snapshot. */
  const job = () => props.job;

  /** Completion percentage (0-100), clamped to 0 when total is 0. */
  const pct = () => {
    if (job().total === 0) return 0;
    return Math.round((job().completed / job().total) * 100);
  };

  /** Renders the icon corresponding to the job's kind. */
  const KindIcon = () => {
    const Icon = kindIcons[job().kind];
    return <Icon size={12} />;
  };

  /** Extracts the basename from the current item path for display. */
  const fileName = () => {
    const f = job().current_item;
    if (!f) return "";
    const parts = f.split("/");
    return parts[parts.length - 1] || f;
  };

  /** Whether the job is still in progress. */
  const isRunning = () => job().status === "running";

  return (
    <div class={styles.row} classList={{ [styles.finished]: !isRunning() }}>
      <div class={styles.icon}>
        <KindIcon />
      </div>

      <div class={styles.content}>
        <div class={styles.description}>
          <span class={styles.label}>
            {kindLabels[job().kind]} {job().description}
          </span>
          <Show when={isRunning()}>
            <span class={styles.currentItem}>
              {job().completed}/{job().total}
              {fileName() ? ` — ${fileName()}` : ""}
            </span>
          </Show>
          <Show when={job().status === "completed"}>
            <span class={styles.completedLabel}>{job().total}/{job().total} done</span>
          </Show>
          <Show when={job().status === "failed" && job().error}>
            <span class={styles.error}>{job().error}</span>
          </Show>
          <Show when={job().status === "cancelled"}>
            <span class={styles.cancelledLabel}>{job().completed}/{job().total} cancelled</span>
          </Show>
        </div>

        <Show when={isRunning()}>
          <div class={styles.track}>
            <div class={styles.fill} style={{ width: `${pct()}%` }} />
          </div>
        </Show>
      </div>

      <div class={styles.status}>
        {job().status === "running" && (
          <Loader size={12} class={styles.spinner} />
        )}
        {job().status === "completed" && (
          <Check size={12} class={styles.success} />
        )}
        {job().status === "failed" && (
          <AlertTriangle size={12} class={styles.failure} />
        )}
        {job().status === "cancelled" && (
          <Ban size={12} class={styles.cancelled} />
        )}
      </div>

      <Show when={isRunning()}>
        <button
          class={styles.cancelButton}
          onClick={() => cancel(job().id)}
          title="Cancel"
          aria-label="Cancel job"
        >
          <X size={12} />
        </button>
      </Show>
    </div>
  );
};
