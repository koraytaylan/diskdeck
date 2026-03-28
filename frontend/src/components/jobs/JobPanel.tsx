/**
 * @file Job Panel
 *
 * Collapsible bottom panel that displays all active and recent jobs.
 * Shows a summary bar when collapsed ("N active jobs") and a scrollable
 * list of `JobRow` components when expanded.
 *
 * The panel is only rendered when there is at least one job to display.
 * A "clear finished" button (trash icon) appears when there are
 * completed/failed/cancelled jobs.
 *
 * @module components/jobs/JobPanel
 */

import { createSignal, Show, For, type Component } from "solid-js";
import { ChevronUp, ChevronDown, Trash2 } from "lucide-solid";
import { useJobs } from "../../contexts/JobContext";
import { JobRow } from "./JobRow";
import styles from "./JobPanel.module.css";

/**
 * Collapsible bottom panel showing a summary of active jobs and,
 * when expanded, a scrollable list of all tracked jobs.
 */
export const JobPanel: Component = () => {
  const { jobs, activeCount, clearFinished } = useJobs();
  const [expanded, setExpanded] = createSignal(false);

  /** Whether there are any jobs to display at all. */
  const hasJobs = () => jobs().length > 0;

  /** Whether any jobs have reached a terminal state. */
  const hasFinished = () => jobs().some((j) => j.status !== "running");

  return (
    <Show when={hasJobs()}>
      <div class={styles.panel}>
        <button
          class={styles.header}
          onClick={() => setExpanded((v) => !v)}
          aria-label={expanded() ? "Collapse job panel" : "Expand job panel"}
          aria-expanded={expanded()}
        >
          <span class={styles.summary}>
            {activeCount() > 0
              ? `${activeCount()} active job${activeCount() !== 1 ? "s" : ""}`
              : "Jobs"}
            {jobs().length > activeCount() &&
              ` \u00b7 ${jobs().length - activeCount()} finished`}
          </span>
          <div class={styles.headerActions}>
            <Show when={hasFinished()}>
              <button
                class={styles.clearButton}
                onClick={(e) => {
                  e.stopPropagation();
                  clearFinished();
                }}
                title="Clear finished jobs"
                aria-label="Clear finished jobs"
              >
                <Trash2 size={12} />
              </button>
            </Show>
            {expanded() ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
          </div>
        </button>
        <Show when={expanded()}>
          <div class={styles.list}>
            <For each={jobs()}>{(job) => <JobRow job={job} />}</For>
          </div>
        </Show>
      </div>
    </Show>
  );
};
