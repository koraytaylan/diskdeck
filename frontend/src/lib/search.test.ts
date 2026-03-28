import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createDebouncedSearch } from "./search";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("createDebouncedSearch", () => {
  it("calls onEmpty for whitespace-only input", () => {
    const onEmpty = vi.fn();
    const { search } = createDebouncedSearch({
      delay: 100,
      fetcher: vi.fn(),
      onResults: vi.fn(),
      onLoading: vi.fn(),
      onEmpty,
    });

    search("   ");
    expect(onEmpty).toHaveBeenCalledTimes(1);
  });

  it("debounces the fetcher call", () => {
    const fetcher = vi.fn().mockResolvedValue(["result"]);
    const { search } = createDebouncedSearch({
      delay: 300,
      fetcher,
      onResults: vi.fn(),
      onLoading: vi.fn(),
      onEmpty: vi.fn(),
    });

    search("hello");
    expect(fetcher).not.toHaveBeenCalled();

    vi.advanceTimersByTime(300);
    expect(fetcher).toHaveBeenCalledWith("hello");
  });

  it("forwards results to onResults", async () => {
    const onResults = vi.fn();
    const fetcher = vi.fn().mockResolvedValue(["a", "b"]);
    const { search } = createDebouncedSearch({
      delay: 100,
      fetcher,
      onResults,
      onLoading: vi.fn(),
      onEmpty: vi.fn(),
    });

    search("test");
    vi.advanceTimersByTime(100);
    // Flush the microtask queue so the async fetcher resolves
    await vi.runAllTimersAsync();

    expect(onResults).toHaveBeenCalledWith(["a", "b"]);
  });

  it("discards stale results when a newer search is started", async () => {
    const onResults = vi.fn();
    let resolveFirst!: (v: string[]) => void;
    let resolveSecond!: (v: string[]) => void;

    const fetcher = vi
      .fn()
      .mockImplementationOnce(
        () => new Promise<string[]>((r) => { resolveFirst = r; }),
      )
      .mockImplementationOnce(
        () => new Promise<string[]>((r) => { resolveSecond = r; }),
      );

    const { search } = createDebouncedSearch({
      delay: 100,
      fetcher,
      onResults,
      onLoading: vi.fn(),
      onEmpty: vi.fn(),
    });

    // First search: "r"
    search("r");
    vi.advanceTimersByTime(100);
    // fetcher called for "r"

    // Second search: "readme" — started before "r" resolves
    search("readme");
    vi.advanceTimersByTime(100);
    // fetcher called for "readme"

    // "r" resolves AFTER "readme" was dispatched — should be discarded
    resolveFirst(["objectSpread2.js", "package.json"]);
    await Promise.resolve(); // flush microtasks

    expect(onResults).not.toHaveBeenCalled();

    // "readme" resolves — should be accepted
    resolveSecond(["README.md"]);
    await Promise.resolve();

    expect(onResults).toHaveBeenCalledTimes(1);
    expect(onResults).toHaveBeenCalledWith(["README.md"]);
  });

  it("does not call onLoading(false) for stale results", async () => {
    const onLoading = vi.fn();
    let resolveFirst!: (v: string[]) => void;

    const fetcher = vi
      .fn()
      .mockImplementationOnce(
        () => new Promise<string[]>((r) => { resolveFirst = r; }),
      )
      .mockImplementationOnce(() => new Promise<string[]>(() => {})); // never resolves

    const { search } = createDebouncedSearch({
      delay: 100,
      fetcher,
      onResults: vi.fn(),
      onLoading,
      onEmpty: vi.fn(),
    });

    search("r");
    vi.advanceTimersByTime(100);

    search("readme");
    vi.advanceTimersByTime(100);

    // Clear the call history from the search() calls
    onLoading.mockClear();

    // "r" resolves after "readme" started — stale
    resolveFirst(["stale"]);
    await Promise.resolve();

    // onLoading(false) should NOT have been called by stale result
    expect(onLoading).not.toHaveBeenCalled();
  });

  it("clear() discards pending and in-flight searches", async () => {
    const onResults = vi.fn();
    const onEmpty = vi.fn();
    let resolveSearch!: (v: string[]) => void;

    const fetcher = vi
      .fn()
      .mockImplementation(
        () => new Promise<string[]>((r) => { resolveSearch = r; }),
      );

    const { search, clear } = createDebouncedSearch({
      delay: 100,
      fetcher,
      onResults,
      onLoading: vi.fn(),
      onEmpty,
    });

    search("test");
    vi.advanceTimersByTime(100);
    // fetcher is now in-flight

    onEmpty.mockClear();
    clear();
    expect(onEmpty).toHaveBeenCalledTimes(1);

    // The in-flight fetch resolves after clear — should be discarded
    resolveSearch(["should-not-appear"]);
    await Promise.resolve();

    expect(onResults).not.toHaveBeenCalled();
  });

  it("resets debounce timer on rapid input", () => {
    const fetcher = vi.fn().mockResolvedValue([]);
    const { search } = createDebouncedSearch({
      delay: 300,
      fetcher,
      onResults: vi.fn(),
      onLoading: vi.fn(),
      onEmpty: vi.fn(),
    });

    search("r");
    vi.advanceTimersByTime(200);
    search("re");
    vi.advanceTimersByTime(200);
    search("rea");
    vi.advanceTimersByTime(200);
    search("read");

    // Only 800ms total, but each call resets the 300ms timer
    expect(fetcher).not.toHaveBeenCalled();

    vi.advanceTimersByTime(300);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher).toHaveBeenCalledWith("read");
  });
});
