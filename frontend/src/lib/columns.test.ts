import { describe, it, expect } from "vitest";
import {
  formatKind,
  ALL_COLUMNS,
  TOGGLEABLE_COLUMNS,
  getColumnById,
  defaultColumnConfig,
  loadColumnConfig,
  saveColumnConfig,
  COLUMNS_STORAGE_KEY,
} from "./columns";
import type { Entry } from "./types";

/** Helper to create a minimal Entry for testing. */
function makeEntry(overrides: Partial<Entry> = {}): Entry {
  return {
    path: "/test.txt",
    name: "test.txt",
    size: 100,
    modified: 1700000000,
    is_dir: false,
    permissions: null,
    mime_type: null,
    ...overrides,
  };
}

describe("formatKind", () => {
  it("returns 'Folder' for directories", () => {
    expect(formatKind(makeEntry({ is_dir: true, name: "docs" }))).toBe("Folder");
  });

  describe("MIME-type based classification", () => {
    it("classifies image/* MIME types", () => {
      expect(formatKind(makeEntry({ mime_type: "image/png" }))).toBe("PNG Image");
      expect(formatKind(makeEntry({ mime_type: "image/jpeg" }))).toBe("JPEG Image");
      expect(formatKind(makeEntry({ mime_type: "image/gif" }))).toBe("GIF Image");
      expect(formatKind(makeEntry({ mime_type: "image/svg+xml" }))).toBe("SVG+XML Image");
      expect(formatKind(makeEntry({ mime_type: "image/webp" }))).toBe("WEBP Image");
    });

    it("classifies video/* MIME types", () => {
      expect(formatKind(makeEntry({ mime_type: "video/mp4" }))).toBe("MP4 Video");
      expect(formatKind(makeEntry({ mime_type: "video/quicktime" }))).toBe("QUICKTIME Video");
    });

    it("classifies audio/* MIME types", () => {
      expect(formatKind(makeEntry({ mime_type: "audio/mpeg" }))).toBe("MPEG Audio");
      expect(formatKind(makeEntry({ mime_type: "audio/wav" }))).toBe("WAV Audio");
    });

    it("classifies text/* MIME types", () => {
      expect(formatKind(makeEntry({ mime_type: "text/plain" }))).toBe("Plain Text");
      expect(formatKind(makeEntry({ mime_type: "text/html" }))).toBe("HTML Document");
      expect(formatKind(makeEntry({ mime_type: "text/css" }))).toBe("CSS Stylesheet");
      expect(formatKind(makeEntry({ mime_type: "text/csv" }))).toBe("CSV Document");
      expect(formatKind(makeEntry({ mime_type: "text/xml" }))).toBe("XML Text");
    });

    it("classifies application/* MIME types", () => {
      expect(formatKind(makeEntry({ mime_type: "application/pdf" }))).toBe("PDF Document");
      expect(formatKind(makeEntry({ mime_type: "application/json" }))).toBe("JSON Document");
      expect(formatKind(makeEntry({ mime_type: "application/xml" }))).toBe("XML Document");
      expect(formatKind(makeEntry({ mime_type: "application/zip" }))).toBe("ZIP Archive");
      expect(formatKind(makeEntry({ mime_type: "application/gzip" }))).toBe("GZIP Archive");
      expect(formatKind(makeEntry({ mime_type: "application/x-tar" }))).toBe("TAR Archive");
      expect(formatKind(makeEntry({ mime_type: "application/javascript" }))).toBe("JavaScript");
      expect(formatKind(makeEntry({ mime_type: "application/typescript" }))).toBe("TypeScript");
    });
  });

  describe("extension-based fallback", () => {
    it("classifies common file extensions when no MIME type", () => {
      expect(formatKind(makeEntry({ name: "doc.pdf" }))).toBe("PDF Document");
      expect(formatKind(makeEntry({ name: "photo.png" }))).toBe("PNG Image");
      expect(formatKind(makeEntry({ name: "app.js" }))).toBe("JavaScript");
      expect(formatKind(makeEntry({ name: "main.rs" }))).toBe("Rust Source");
      expect(formatKind(makeEntry({ name: "script.py" }))).toBe("Python Script");
      expect(formatKind(makeEntry({ name: "archive.zip" }))).toBe("ZIP Archive");
      expect(formatKind(makeEntry({ name: "data.json" }))).toBe("JSON Document");
      expect(formatKind(makeEntry({ name: "readme.md" }))).toBe("Markdown Document");
      expect(formatKind(makeEntry({ name: "style.css" }))).toBe("CSS Stylesheet");
    });

    it("returns uppercase extension + 'File' for unknown extensions", () => {
      expect(formatKind(makeEntry({ name: "data.xyz" }))).toBe("XYZ File");
      expect(formatKind(makeEntry({ name: "config.ini" }))).toBe("INI File");
    });
  });

  it("returns 'Document' for files with no MIME type and no extension", () => {
    expect(formatKind(makeEntry({ name: "Makefile" }))).toBe("Document");
  });
});

describe("ALL_COLUMNS", () => {
  it("has 'name' as the first column", () => {
    expect(ALL_COLUMNS[0].id).toBe("name");
  });

  it("includes all expected column IDs", () => {
    const ids = ALL_COLUMNS.map((c) => c.id);
    expect(ids).toContain("name");
    expect(ids).toContain("size");
    expect(ids).toContain("modified");
    expect(ids).toContain("created");
    expect(ids).toContain("kind");
    expect(ids).toContain("permissions");
  });

  it("has unique IDs", () => {
    const ids = ALL_COLUMNS.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("TOGGLEABLE_COLUMNS", () => {
  it("excludes the name column", () => {
    const ids = TOGGLEABLE_COLUMNS.map((c) => c.id);
    expect(ids).not.toContain("name");
  });

  it("includes all non-name columns", () => {
    expect(TOGGLEABLE_COLUMNS.length).toBe(ALL_COLUMNS.length - 1);
  });
});

describe("getColumnById", () => {
  it("finds existing columns", () => {
    expect(getColumnById("name")?.label).toBe("Name");
    expect(getColumnById("size")?.label).toBe("Size");
  });

  it("returns undefined for unknown IDs", () => {
    expect(getColumnById("nonexistent")).toBeUndefined();
  });
});

describe("defaultColumnConfig", () => {
  it("includes only default-visible columns", () => {
    const config = defaultColumnConfig();
    const expectedVisible = ALL_COLUMNS.filter((c) => c.defaultVisible).map((c) => c.id);
    expect(config.visibleIds).toEqual(expectedVisible);
  });

  it("sets default widths for all columns", () => {
    const config = defaultColumnConfig();
    for (const col of ALL_COLUMNS) {
      expect(config.widths[col.id]).toBe(col.defaultWidth);
    }
  });
});

describe("loadColumnConfig / saveColumnConfig", () => {
  beforeEach(() => {
    localStorage.removeItem(COLUMNS_STORAGE_KEY);
  });

  it("returns defaults when nothing is stored", () => {
    const config = loadColumnConfig();
    const defaults = defaultColumnConfig();
    expect(config.visibleIds).toEqual(defaults.visibleIds);
  });

  it("round-trips through save and load", () => {
    const custom = { visibleIds: ["name", "size", "kind"], widths: { name: 0, size: 120, kind: 150 } };
    saveColumnConfig(custom);
    const loaded = loadColumnConfig();
    expect(loaded.visibleIds).toEqual(custom.visibleIds);
    expect(loaded.widths.size).toBe(120);
    expect(loaded.widths.kind).toBe(150);
  });

  it("ensures 'name' is always in visibleIds", () => {
    saveColumnConfig({ visibleIds: ["size", "modified"], widths: {} });
    const loaded = loadColumnConfig();
    expect(loaded.visibleIds).toContain("name");
  });

  it("returns defaults for invalid JSON", () => {
    localStorage.setItem(COLUMNS_STORAGE_KEY, "not valid json");
    const config = loadColumnConfig();
    const defaults = defaultColumnConfig();
    expect(config.visibleIds).toEqual(defaults.visibleIds);
  });

  it("returns defaults for structurally invalid data", () => {
    localStorage.setItem(COLUMNS_STORAGE_KEY, JSON.stringify({ visibleIds: "not-array" }));
    const config = loadColumnConfig();
    const defaults = defaultColumnConfig();
    expect(config.visibleIds).toEqual(defaults.visibleIds);
  });
});
