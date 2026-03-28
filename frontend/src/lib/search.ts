/**
 * @file Debounced Search Pattern
 *
 * Provides a generic, framework-agnostic debounced search factory that
 * handles three common problems with async search UIs:
 *
 * 1. **Debouncing** -- Avoids firing a request on every keystroke by
 *    waiting `delay` ms after the last input.
 * 2. **Stale result rejection** -- Uses a generation counter so that
 *    if the user types "foo" then quickly "bar", the results for "foo"
 *    are silently discarded even if they arrive after "bar"'s results.
 * 3. **Empty query handling** -- Immediately clears results (no delay)
 *    when the input is blank.
 *
 * Used by `AppLayout` to wire the search bar to `searchEntries()` IPC.
 *
 * @module lib/search
 */

/**
 * Create a debounced search controller.
 *
 * @typeParam T - The type of the search result payload (e.g. `SearchResult[]`).
 *
 * @param opts.delay     - Debounce delay in milliseconds.
 * @param opts.fetcher   - Async function that performs the actual search.
 * @param opts.onResults - Callback invoked with results when the fetch succeeds
 *                         and the result is still current (not stale).
 * @param opts.onLoading - Called with `true` when a search starts and `false`
 *                         when it finishes (only if still current).
 * @param opts.onEmpty   - Called when the query is blank or the fetch fails,
 *                         to reset the UI to its empty state.
 *
 * @returns An object with:
 *   - `search(value)` -- Call on every input change. Debounces, then fetches.
 *   - `clear()`       -- Immediately cancel any pending search and reset.
 */
export function createDebouncedSearch<T>(opts: {
  delay: number;
  fetcher: (query: string) => Promise<T>;
  onResults: (results: T) => void;
  onLoading: (loading: boolean) => void;
  onEmpty: () => void;
}) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Monotonically increasing counter; each call to search() bumps it. */
  let generation = 0;

  function search(value: string) {
    if (timer) clearTimeout(timer);
    generation++;

    const trimmed = value.trim();
    if (!trimmed) {
      opts.onEmpty();
      return;
    }

    opts.onLoading(true);
    // Capture the current generation so we can check freshness when the fetch resolves
    const myGen = generation;

    timer = setTimeout(async () => {
      try {
        const results = await opts.fetcher(trimmed);
        // Discard results if a newer search was started while we were fetching
        if (myGen === generation) {
          opts.onResults(results);
        }
      } catch {
        if (myGen === generation) {
          opts.onEmpty();
        }
      } finally {
        if (myGen === generation) {
          opts.onLoading(false);
        }
      }
    }, opts.delay);
  }

  /** Cancel any in-flight search and reset to the empty state. */
  function clear() {
    if (timer) clearTimeout(timer);
    generation++;
    opts.onEmpty();
  }

  return { search, clear };
}
