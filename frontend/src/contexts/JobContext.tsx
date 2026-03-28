/**
 * @file Job Context
 *
 * Manages the reactive state for all backend jobs (copy, move, delete,
 * and future batch operations). Jobs are tracked via Tauri "job-update"
 * events emitted by the backend as spawned tasks progress.
 *
 * On mount, hydrates from `listJobs()` IPC call, then keeps state
 * in sync via the event listener.
 *
 * **Store shape:**
 * A flat array of `JobInfo` snapshots, sorted by `created_at` descending
 * (newest first). Each incoming event upserts by `id`.
 *
 * **Cleanup:**
 * `clearFinished` removes non-running jobs both locally and on the backend.
 * The Tauri event listener is unsubscribed on unmount.
 *
 * @module contexts/JobContext
 */

import {
  createContext,
  useContext,
  createMemo,
  onMount,
  onCleanup,
  type ParentComponent,
} from "solid-js";
import { createStore, produce } from "solid-js/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { JobInfo } from "../lib/types";
import { listJobs, cancelJob, clearFinishedJobs } from "../lib/ipc";

/** Internal reactive store shape. */
interface JobState {
  /** All tracked jobs, ordered by created_at descending. */
  jobs: JobInfo[];
}

/** Public API exposed by the job context. */
interface JobContextValue {
  /** All tracked jobs, ordered by created_at descending. */
  jobs: () => JobInfo[];
  /** Number of currently running jobs. */
  activeCount: () => number;
  /** Cancel a running job by ID. */
  cancel: (jobId: string) => Promise<void>;
  /** Clear all finished (completed/failed/cancelled) jobs. */
  clearFinished: () => Promise<void>;
}

const JobContext = createContext<JobContextValue>();

/**
 * Provider component that tracks all backend jobs in a reactive store.
 *
 * Hydrates on mount via `listJobs()` and subscribes to real-time
 * `"job-update"` Tauri events for continuous state synchronisation.
 */
export const JobProvider: ParentComponent = (props) => {
  const [state, setState] = createStore<JobState>({ jobs: [] });

  /**
   * Upsert a job into the store by ID.
   * If a job with the same ID exists it is replaced; otherwise the new
   * job is appended. The array is re-sorted after every upsert.
   */
  const upsertJob = (job: JobInfo) => {
    setState(
      produce((s) => {
        const idx = s.jobs.findIndex((j) => j.id === job.id);
        if (idx >= 0) {
          s.jobs[idx] = job;
        } else {
          s.jobs.push(job);
        }
        // Keep sorted by created_at descending (newest first)
        s.jobs.sort((a, b) => b.created_at - a.created_at);
      }),
    );
  };

  let unlisten: UnlistenFn | undefined;

  onMount(async () => {
    // Hydrate existing jobs from the backend
    try {
      const existing = await listJobs();
      setState("jobs", existing.sort((a, b) => b.created_at - a.created_at));
    } catch (e) {
      console.warn("Failed to hydrate jobs:", e);
    }

    // Subscribe to real-time job progress events
    unlisten = await listen<JobInfo>("job-update", (event) => {
      upsertJob(event.payload);
    });
  });

  onCleanup(() => {
    unlisten?.();
  });

  /** Derived memo: all jobs (triggers reactivity on array changes). */
  const jobs = createMemo(() => state.jobs);

  /** Derived memo: count of jobs with status "running". */
  const activeCount = createMemo(
    () => state.jobs.filter((j) => j.status === "running").length,
  );

  /**
   * Request the backend to cancel a running job.
   * The actual status change arrives via a subsequent "job-update" event.
   */
  const cancel = async (jobId: string) => {
    await cancelJob(jobId);
  };

  /**
   * Clear all finished jobs from both the backend and local store.
   * Running jobs are left untouched.
   */
  const clearFinished = async () => {
    await clearFinishedJobs();
    // Optimistically remove finished jobs from local state
    setState("jobs", (prev) => prev.filter((j) => j.status === "running"));
  };

  return (
    <JobContext.Provider value={{ jobs, activeCount, cancel, clearFinished }}>
      {props.children}
    </JobContext.Provider>
  );
};

/**
 * Hook to access job tracking state and actions.
 * Must be called within a `<JobProvider>` subtree.
 *
 * @returns JobContextValue with jobs list, active count, cancel, and clearFinished.
 * @throws Error if called outside JobProvider.
 */
export function useJobs(): JobContextValue {
  const ctx = useContext(JobContext);
  if (!ctx) throw new Error("useJobs must be used within JobProvider");
  return ctx;
}
