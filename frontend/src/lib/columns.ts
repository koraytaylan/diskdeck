/**
 * @file Column Definitions
 *
 * Defines the configurable column system for the file list view. Each column
 * has an ID, label, default width, minimum width, alignment, visibility default,
 * a render function that extracts display text from an `Entry`, and an optional
 * sort field for header click-to-sort.
 *
 * The `name` column is always visible and cannot be toggled. All other columns
 * can be shown or hidden via a right-click column picker on the header area.
 *
 * Column widths and visibility are persisted in `localStorage` under the key
 * `diskdeck:columns`.
 *
 * @module lib/columns
 */

import type { Entry } from "./types";
import { formatSize, formatDate } from "./file-utils";

/** Definition of a single column in the file list header/row. */
export interface ColumnDef {
  /** Unique identifier for the column. */
  id: string;
  /** Display label shown in the column header. */
  label: string;
  /** Default width in pixels (ignored for the flex `name` column). */
  defaultWidth: number;
  /** Minimum width in pixels when resizing. */
  minWidth: number;
  /** Text alignment within the column cell. */
  align: "left" | "right";
  /** Whether this column is visible by default on first launch. */
  defaultVisible: boolean;
  /** Returns the display value for an entry. */
  render: (entry: Entry, extras?: ColumnRenderExtras) => string;
  /** Sort field name (must match FileContext SortField if sortable). */
  sortField?: string;
}

/**
 * Extra data passed to column render functions that require async or
 * externally computed values (e.g. folder sizes).
 */
export interface ColumnRenderExtras {
  /** Lazily computed folder size, provided by the row component. */
  folderSize?: number;
  /** Whether the folder size is still loading. */
  folderSizeLoading?: boolean;
}

/** localStorage key for persisting column configuration. */
export const COLUMNS_STORAGE_KEY = "diskdeck:columns";

/**
 * Derive a human-readable "kind" description from an entry's metadata.
 *
 * Resolution order:
 * 1. Directories return "Folder".
 * 2. MIME type is mapped to a friendly label (e.g. "image/png" -> "PNG Image").
 * 3. Files with no MIME type fall back to extension-based labels.
 * 4. Unknown files return "Document".
 *
 * @param entry - The file-system entry to describe.
 * @returns A user-facing kind string like "Folder", "PDF Document", "PNG Image".
 */
export function formatKind(entry: Entry): string {
  if (entry.is_dir) return "Folder";

  const mime = entry.mime_type;
  if (mime) {
    const subtypeRaw = mime.split("/")[1] ?? "";
    // Strip vendor/x- prefixes for cleaner labels (e.g. "x-tar" -> "tar")
    const subtype = subtypeRaw.replace(/^(x-|vnd\.)/, "");

    if (mime.startsWith("image/")) {
      return `${subtype.toUpperCase()} Image`;
    }
    if (mime.startsWith("video/")) {
      return `${subtype.toUpperCase()} Video`;
    }
    if (mime.startsWith("audio/")) {
      return `${subtype.toUpperCase()} Audio`;
    }
    if (mime.startsWith("text/")) {
      if (subtype === "plain") return "Plain Text";
      if (subtype === "html") return "HTML Document";
      if (subtype === "css") return "CSS Stylesheet";
      if (subtype === "csv") return "CSV Document";
      return `${subtype.toUpperCase()} Text`;
    }
    if (mime === "application/pdf") return "PDF Document";
    if (mime === "application/json") return "JSON Document";
    if (mime === "application/xml") return "XML Document";
    if (mime === "application/zip") return "ZIP Archive";
    if (mime === "application/gzip" || mime === "application/x-gzip") return "GZIP Archive";
    if (mime === "application/x-tar") return "TAR Archive";
    if (mime === "application/javascript") return "JavaScript";
    if (mime === "application/typescript") return "TypeScript";
  }

  // Fallback: derive from file extension (only if the name contains a dot)
  const dotIndex = entry.name.lastIndexOf(".");
  const ext = dotIndex > 0 ? entry.name.slice(dotIndex + 1).toLowerCase() : "";
  if (ext) {
    const extensionKinds: Record<string, string> = {
      pdf: "PDF Document",
      doc: "Word Document",
      docx: "Word Document",
      xls: "Excel Spreadsheet",
      xlsx: "Excel Spreadsheet",
      ppt: "PowerPoint Presentation",
      pptx: "PowerPoint Presentation",
      zip: "ZIP Archive",
      tar: "TAR Archive",
      gz: "GZIP Archive",
      "7z": "7-Zip Archive",
      rar: "RAR Archive",
      js: "JavaScript",
      ts: "TypeScript",
      tsx: "TSX Document",
      jsx: "JSX Document",
      rs: "Rust Source",
      py: "Python Script",
      go: "Go Source",
      java: "Java Source",
      c: "C Source",
      cpp: "C++ Source",
      h: "C Header",
      css: "CSS Stylesheet",
      html: "HTML Document",
      json: "JSON Document",
      yaml: "YAML Document",
      yml: "YAML Document",
      toml: "TOML Document",
      xml: "XML Document",
      md: "Markdown Document",
      txt: "Plain Text",
      sh: "Shell Script",
      bash: "Shell Script",
      png: "PNG Image",
      jpg: "JPEG Image",
      jpeg: "JPEG Image",
      gif: "GIF Image",
      svg: "SVG Image",
      webp: "WebP Image",
      mp4: "MP4 Video",
      mov: "QuickTime Video",
      avi: "AVI Video",
      mp3: "MP3 Audio",
      wav: "WAV Audio",
      flac: "FLAC Audio",
    };
    if (extensionKinds[ext]) return extensionKinds[ext];
    return `${ext.toUpperCase()} File`;
  }

  return "Document";
}

/**
 * All available column definitions in display order.
 * The `name` column is always first and always visible.
 */
export const ALL_COLUMNS: readonly ColumnDef[] = [
  {
    id: "name",
    label: "Name",
    defaultWidth: 0, // Not used -- name column uses flex: 1
    minWidth: 120,
    align: "left",
    defaultVisible: true,
    render: (entry) => entry.name,
    sortField: "name",
  },
  {
    id: "size",
    label: "Size",
    defaultWidth: 100,
    minWidth: 60,
    align: "right",
    defaultVisible: true,
    render: (entry, extras) => {
      if (entry.is_dir) {
        if (extras?.folderSizeLoading) return "...";
        if (extras?.folderSize !== undefined) return formatSize(extras.folderSize);
        return "--";
      }
      return formatSize(entry.size);
    },
    sortField: "size",
  },
  {
    id: "modified",
    label: "Modified",
    defaultWidth: 180,
    minWidth: 80,
    align: "right",
    defaultVisible: true,
    render: (entry) => formatDate(entry.modified),
    sortField: "modified",
  },
  {
    id: "created",
    label: "Created",
    defaultWidth: 180,
    minWidth: 80,
    align: "right",
    defaultVisible: false,
    render: (entry) => formatDate(entry.created),
    sortField: "created",
  },
  {
    id: "kind",
    label: "Kind",
    defaultWidth: 120,
    minWidth: 60,
    align: "left",
    defaultVisible: false,
    render: (entry) => formatKind(entry),
    sortField: "kind",
  },
  {
    id: "permissions",
    label: "Permissions",
    defaultWidth: 90,
    minWidth: 60,
    align: "left",
    defaultVisible: false,
    render: (entry) => entry.permissions ?? "--",
    sortField: "permissions",
  },
] as const;

/** Column IDs that can be toggled on/off (excludes "name" which is always shown). */
export const TOGGLEABLE_COLUMNS = ALL_COLUMNS.filter((c) => c.id !== "name");

/**
 * Look up a column definition by ID.
 *
 * @param id - Column identifier.
 * @returns The matching ColumnDef, or undefined if not found.
 */
export function getColumnById(id: string): ColumnDef | undefined {
  return ALL_COLUMNS.find((c) => c.id === id);
}

/** Persisted column configuration shape stored in localStorage. */
export interface ColumnConfig {
  /** IDs of visible columns (in order). */
  visibleIds: string[];
  /** Mapping of column ID to pixel width. */
  widths: Record<string, number>;
}

/**
 * Build the default column configuration from column definitions.
 *
 * @returns Default ColumnConfig with all default-visible columns and default widths.
 */
export function defaultColumnConfig(): ColumnConfig {
  const visibleIds = ALL_COLUMNS.filter((c) => c.defaultVisible).map((c) => c.id);
  const widths: Record<string, number> = {};
  for (const col of ALL_COLUMNS) {
    widths[col.id] = col.defaultWidth;
  }
  return { visibleIds, widths };
}

/**
 * Load column configuration from localStorage, falling back to defaults
 * if nothing is stored or the stored value is invalid.
 *
 * @returns The loaded or default ColumnConfig.
 */
export function loadColumnConfig(): ColumnConfig {
  try {
    const raw = localStorage.getItem(COLUMNS_STORAGE_KEY);
    if (!raw) return defaultColumnConfig();
    const parsed = JSON.parse(raw) as Partial<ColumnConfig>;
    if (!Array.isArray(parsed.visibleIds) || typeof parsed.widths !== "object") {
      return defaultColumnConfig();
    }
    // Ensure "name" is always included
    const visibleIds = parsed.visibleIds.includes("name")
      ? parsed.visibleIds
      : ["name", ...parsed.visibleIds];
    // Merge with defaults so new columns get their default widths
    const defaults = defaultColumnConfig();
    const widths = { ...defaults.widths, ...parsed.widths };
    return { visibleIds, widths };
  } catch {
    return defaultColumnConfig();
  }
}

/**
 * Save column configuration to localStorage.
 *
 * @param config - The ColumnConfig to persist.
 */
export function saveColumnConfig(config: ColumnConfig): void {
  localStorage.setItem(COLUMNS_STORAGE_KEY, JSON.stringify(config));
}
