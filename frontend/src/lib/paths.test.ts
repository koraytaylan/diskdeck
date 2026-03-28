import { describe, it, expect } from "vitest";
import { buildBreadcrumbs, parentPath, ancestorPaths } from "./paths";

describe("buildBreadcrumbs", () => {
  it("returns root only for /", () => {
    expect(buildBreadcrumbs("/")).toEqual([{ label: "/", path: "/" }]);
  });

  it("returns root only for empty string", () => {
    expect(buildBreadcrumbs("")).toEqual([{ label: "/", path: "/" }]);
  });

  it("builds segments for a nested path", () => {
    expect(buildBreadcrumbs("/a/b/c")).toEqual([
      { label: "/", path: "/" },
      { label: "a", path: "/a" },
      { label: "b", path: "/a/b" },
      { label: "c", path: "/a/b/c" },
    ]);
  });

  it("builds segments for a single-level path", () => {
    expect(buildBreadcrumbs("/docs")).toEqual([
      { label: "/", path: "/" },
      { label: "docs", path: "/docs" },
    ]);
  });

  it("handles paths with trailing slash", () => {
    const crumbs = buildBreadcrumbs("/a/b/");
    // trailing slash produces an empty segment that gets filtered out
    expect(crumbs).toEqual([
      { label: "/", path: "/" },
      { label: "a", path: "/a" },
      { label: "b", path: "/a/b" },
    ]);
  });
});

describe("parentPath", () => {
  it("returns / for root", () => {
    expect(parentPath("/")).toBe("/");
  });

  it("returns / for empty string", () => {
    expect(parentPath("")).toBe("/");
  });

  it("returns / for a top-level path", () => {
    expect(parentPath("/a")).toBe("/");
  });

  it("returns parent for nested path", () => {
    expect(parentPath("/a/b/c")).toBe("/a/b");
  });

  it("handles trailing slash", () => {
    expect(parentPath("/a/b/")).toBe("/a");
  });
});

describe("ancestorPaths", () => {
  it("returns empty array for root", () => {
    expect(ancestorPaths("/")).toEqual([]);
  });

  it("returns single ancestor for top-level path", () => {
    expect(ancestorPaths("/a")).toEqual(["/a"]);
  });

  it("returns all ancestors for nested path", () => {
    expect(ancestorPaths("/a/b/c")).toEqual(["/a", "/a/b", "/a/b/c"]);
  });

  it("returns empty array for empty string", () => {
    expect(ancestorPaths("")).toEqual([]);
  });
});
