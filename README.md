# DiskDeck

A cross-platform file explorer that connects to multiple storage backends through a unified interface. Browse local files, AWS S3 buckets, Google Cloud Storage buckets, Azure Blob containers, SFTP servers, and FTP/FTPS servers from a single application — with tabs, file previews, cross-disk transfers, and a familiar file manager experience.

---

## Table of Contents

- [Motivation](#motivation)
- [Constraints & Requirements](#constraints--requirements)
  - [Workspace Isolation](#workspace-isolation)
  - [Technology Stack](#technology-stack)
  - [Security](#security)
  - [UI & Theming](#ui--theming)
  - [Architecture](#architecture)
- [Design Choices & Justifications](#design-choices--justifications)
  - [Why Tauri v2 Over Electron](#why-tauri-v2-over-electron)
  - [Why SolidJS Over React](#why-solidjs-over-react)
  - [Why a Trait-Based Storage Abstraction](#why-a-trait-based-storage-abstraction)
  - [Why CSS Modules Over Tailwind or CSS-in-JS](#why-css-modules-over-tailwind-or-css-in-js)
  - [Why Ayu Theme](#why-ayu-theme)
  - [Why Corvu Resizable](#why-corvu-resizable)
  - [Why Virtual Scrolling](#why-virtual-scrolling)
  - [Why Context API Over External State Libraries](#why-context-api-over-external-state-libraries)
  - [Why Lucide Icons](#why-lucide-icons)
  - [Why SQLite With SQLCipher Encryption](#why-sqlite-with-sqlcipher-encryption)
- [Project Structure](#project-structure)
- [How the Two Sides Connect](#how-the-two-sides-connect)
- [Storage Backends](#storage-backends)
  - [StorageBackend Trait](#storagebackend-trait)
  - [Local Filesystem Backend](#local-filesystem-backend)
  - [AWS S3 Backend](#aws-s3-backend)
  - [Google Cloud Storage Backend](#google-cloud-storage-backend)
- [Adding a New Backend](#adding-a-new-backend)
- [Frontend Architecture](#frontend-architecture)
  - [Context Providers](#context-providers)
  - [Component Hierarchy](#component-hierarchy)
  - [Tab System](#tab-system)
  - [Smart Auto-Refresh](#smart-auto-refresh)
  - [Keyboard Shortcuts](#keyboard-shortcuts)
  - [Search System](#search-system)
- [Security Model](#security-model)
- [Prerequisites](#prerequisites)
- [Development](#development)
- [Testing](#testing)
- [Build](#build)
- [Contributing](#contributing)
  - [Definition of Done](#definition-of-done)
  - [Rules](#rules)
- [License](#license)
- [Roadmap](#roadmap)

---

## Motivation

Modern development workflows span multiple storage systems: local project directories, S3 buckets for assets, GCS buckets, cloud backups, remote servers via SFTP. Switching between the AWS Console, Finder/Explorer, and terminal clients for each is disjointed. DiskDeck provides a single interface where every storage backend looks and behaves the same way, with native performance and no browser overhead.

---

## Constraints & Requirements

### Workspace Isolation

The backend and frontend are **fully isolated workspaces**, as if maintained by two separate teams. This is a hard constraint, not the default Tauri `src`/`src-tauri` structure.

- `backend/` is a self-contained Rust/Tauri project with its own `Cargo.toml`, `tauri.conf.json`, and test suite.
- `frontend/` is a self-contained SolidJS/TypeScript project with its own `package.json`, `tsconfig.json`, `vite.config.ts`, and test suite.
- Each side is independently buildable and testable without the other.
- The **only coupling point** is the IPC contract: `frontend/src/lib/types.ts` mirrors `backend/src/models/`. Both sides agree on the shape of data flowing through Tauri's `invoke()` and `emit()` APIs.
- No shared code, no monorepo tooling, no workspace-level `package.json` or `Cargo.toml`.

### Technology Stack

| Layer | Choice | Non-negotiable? |
|-------|--------|-----------------|
| Desktop framework | Tauri v2 | Yes |
| Backend language | Rust | Yes (via Tauri) |
| Frontend framework | SolidJS + TypeScript | Yes |
| Build tool | Vite | Yes |
| Package manager | pnpm | Yes |
| Database | SQLite (rusqlite bundled) | Yes |
| Credential storage | SQLCipher-encrypted DB (key in OS keychain via `keyring` crate) | Yes |
| Syntax highlighting | Shiki (lazy-loaded, ayu-dark theme) | Yes |
| Icons | Lucide (lucide-solid) | Yes |
| CSS approach | CSS Modules | Yes |
| Theme | Ayu (Dark / Mirage / Light) | Yes |
| Virtual scrolling | @solid-primitives/virtual | Yes |
| Split panes | @corvu/resizable | Yes |
| Keyboard primitives | @solid-primitives/keyboard | Yes |
| State management | SolidJS createStore + Context API | Yes (no external state library) |

### Security

- **SQLCipher-encrypted database**: The database (`diskdeck.db`) is encrypted with AES-256 via SQLCipher. It stores disk configs (including credentials), preferences, bookmarks, and the search index. The encryption key is the only secret held in the OS keychain — retrieved once on app startup, with zero keychain access when using disks.
- **OS keychain (DB key only)**: The 32-byte database encryption key is stored in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) via the `keyring` crate. No per-disk keychain entries exist. Credentials never appear in plaintext files or environment variables.
- **Credential redaction**: Sensitive config fields are blanked before sending disk configs over IPC to the frontend, preventing exposure in renderer memory or devtools.
- **Path traversal prevention**: The local filesystem backend canonicalizes all paths and validates they remain within the configured root directory.
- **Content Security Policy**: Configured in `tauri.conf.json` — `'unsafe-inline'` removed from `script-src`, explicit `img-src`, `font-src`, `connect-src` restrictions.
- **Minimal Tauri capabilities**: Only `core:default` and `opener:default` permissions granted. No `dangerousRemoteDomainIpcAccess`.
- **Input validation**: Disk names (max 255 chars), preference keys/values, search patterns, and file rename targets all validated server-side.
- **Error sanitization**: Internal error details (database messages, SDK errors) are logged server-side only. User-facing IPC responses return generic messages.
- **Structured logging**: Backend uses `tracing-subscriber` with an env-filter for leveled, filterable output.

### UI & Theming

- **Three-panel layout**: Collapsible left sidebar (disk tree), center file browser with tabs, collapsible right properties panel. Panels are resizable via drag handles.
- **Tab system**: Multiple browser tabs (one per disk, or multiple per disk), plus file preview tabs. Tabs are closable, scrollable, and support right-click context menus.
- **Ayu color scheme**: Three variants (Dark, Mirage, Light) using CSS custom properties. Theme persists across sessions via `localStorage`.
- **Compact design**: Dense information display suited for power users. 28px row height in the file list, 12px base font size, minimal padding.
- **Native feel**: Platform-adaptive keyboard shortcuts (Cmd on macOS, Ctrl on Windows/Linux), right-click context menus, breadcrumb navigation, inline rename.

### Architecture

- **Trait-based storage**: A single `StorageBackend` async trait that all backends implement. Adding a new backend requires zero UI changes.
- **Async throughout**: Tokio-based async runtime for all I/O. No blocking the main thread.
- **Background jobs**: Bulk operations (copy, move, delete, cross-disk transfers) run as spawned Tokio tasks with progress events and cooperative cancellation.
- **IPC as the only bridge**: Frontend and backend communicate exclusively through Tauri IPC. No shared memory, no direct function calls, no FFI.
- **Tests at every level**: 333 Rust tests (84% line coverage), 190 Vitest frontend tests (92% utility coverage), clippy with `-D warnings`.

---

## Design Choices & Justifications

### Why Tauri v2 Over Electron

Tauri produces binaries ~10x smaller than Electron, uses the OS webview instead of bundling Chromium, and provides a Rust backend with memory safety guarantees. Tauri v2 specifically adds a capabilities-based permission system, scoping what the frontend can access. The tradeoff is a smaller ecosystem, but for a file explorer the backend does the heavy lifting in Rust anyway.

### Why SolidJS Over React

SolidJS uses fine-grained reactivity without a virtual DOM. When a file list has thousands of entries and the user toggles a single selection, SolidJS updates only the affected DOM node instead of diffing the entire list. This matters for a file explorer where lists are large and interactions are frequent. The bundle size is also significantly smaller (~7KB vs React's ~40KB), which reduces webview load time.

### Why a Trait-Based Storage Abstraction

The `StorageBackend` trait (`list`, `read`, `write`, `delete`, `copy`, `rename`, `stat`, `exists`, `search`, `create_dir`) provides a uniform interface over fundamentally different systems. Local FS operations are POSIX path-based; S3 and GCS are prefix-based with no real directories; SFTP is session-based. The trait lets the UI treat them identically while each backend translates operations to its native protocol.

The trait uses `async-trait` with `Arc<dyn StorageBackend>` for dynamic dispatch. This is a deliberate tradeoff: dynamic dispatch has a small runtime cost, but it allows the backend registry to hold heterogeneous backends in a single `HashMap` without generics or enum dispatch. For I/O-bound operations, the dispatch overhead is negligible.

### Why CSS Modules Over Tailwind or CSS-in-JS

CSS Modules provide scoped class names with zero runtime cost and no build-time class generation. For a desktop app where bundle size matters less than rendering performance, avoiding a CSS-in-JS runtime is beneficial. Compared to Tailwind, CSS Modules allow the Ayu theme to be expressed as CSS custom properties that cascade naturally through `[data-theme]` selectors, without fighting utility class specificity.

### Why Ayu Theme

Ayu is an established editor color scheme with three carefully designed variants (Dark, Mirage, Light) that provide distinct visual identities while maintaining readability. The `ayu` npm package provides exact hex values, which are extracted into 40+ CSS custom properties per variant covering editor backgrounds, UI chrome, accent colors, and syntax highlighting colors (reused for file type color coding and code preview highlighting).

### Why Corvu Resizable

Corvu is a SolidJS-native headless component library. Its `Resizable` component provides collapsible panels with proper keyboard accessibility and ARIA attributes. Using a SolidJS-native library avoids the React compatibility shims that other split-pane libraries require.

### Why Virtual Scrolling

A local filesystem directory can contain tens of thousands of files. Rendering all of them as DOM nodes would be prohibitively slow. `@solid-primitives/virtual` renders only the visible rows (plus a small overscan buffer), keeping DOM node count constant regardless of directory size. At 28px per row in a 800px viewport, roughly 30 rows are rendered instead of potentially 50,000.

### Why Context API Over External State Libraries

SolidJS's `createStore` + `createContext` provides reactive state management without additional dependencies. The app has seven distinct state domains (theme, shortcuts, disks, tabs, files, operations, jobs), each with its own context provider. This keeps state localized and avoids a single global store. The reactive graph ensures components re-render only when their specific state slice changes.

### Why Lucide Icons

Lucide provides 1700+ SVG icons as individual SolidJS components that are tree-shaken at build time. Only the icons actually imported end up in the bundle. The file explorer needs many distinct icons (folder, file types, navigation arrows, cloud, hard drive, etc.), and Lucide covers all of them with a consistent visual style.

### Why SQLite With SQLCipher Encryption

All persistent state — disk configs (including credentials), bookmarks, preferences, and the FTS5 search index — lives in a single SQLCipher-encrypted SQLite database. The database encryption key is the only secret stored in the OS keychain (retrieved once on startup). This gives DiskDeck full offline persistence with strong at-rest encryption, zero per-disk keychain prompts, and a single file (`diskdeck.db`) that is easy to back up or delete.

---

## Project Structure

```
diskdeck/
├── README.md
├── dev.sh                               # Dev launcher: bypasses keychain with local DB key
│
├── backend/                              # Rust / Tauri workspace
│   ├── Cargo.toml                        # Rust dependencies
│   ├── Cargo.lock
│   ├── tauri.conf.json                   # Tauri app config
│   ├── build.rs                          # Tauri build script
│   ├── capabilities/
│   │   └── default.json                  # Tauri v2 permissions (minimal)
│   ├── icons/                            # Platform app icons
│   └── src/
│       ├── main.rs                       # Entry point
│       ├── lib.rs                        # Tauri app builder, command registration
│       ├── error.rs                      # DiskDeckError enum, sanitized serialization
│       ├── state.rs                      # AppState + JobRegistry (job tracking)
│       ├── watcher.rs                   # Filesystem watcher for live directory updates
│       ├── commands/
│       │   ├── mod.rs
│       │   ├── archive.rs                # Browse and extract .zip/.tar.gz archives
│       │   ├── bookmarks.rs              # Bookmark CRUD (list, add, remove)
│       │   ├── diff.rs                   # Compare two directories across backends
│       │   ├── disk.rs                   # Disk CRUD, backend factory, config redaction
│       │   ├── file.rs                   # File ops, bulk jobs, cross-disk transfers
│       │   ├── preferences.rs            # Key-value preferences with validation
│       │   ├── profiles.rs              # Export/import disk configs for team sharing
│       │   └── search.rs                 # FTS5-first search with live fallback
│       ├── indexer.rs                    # Background FTS5 indexer (recursive walk)
│       ├── storage/
│       │   ├── mod.rs                    # StorageBackend trait definition
│       │   ├── local.rs                  # Local FS backend (tokio::fs)
│       │   ├── s3.rs                     # AWS S3 backend (aws-sdk-s3)
│       │   ├── gcs.rs                    # Google Cloud Storage backend (reqwest + GCS JSON API)
│       │   ├── azure.rs                  # Azure Blob Storage backend (azure_storage_blobs)
│       │   ├── sftp.rs                   # SFTP backend (russh + russh-sftp)
│       │   ├── ftp.rs                    # FTP/FTPS backend (suppaftp)
│       │   └── memory.rs                 # In-memory backend (test-only)
│       ├── db/
│       │   └── mod.rs                    # SQLCipher-encrypted store (DiskStore) — configs with credentials
│       ├── security/
│       │   ├── mod.rs
│       │   └── keyring.rs                # OS keychain: DB encryption key only
│       └── models/
│           ├── mod.rs
│           ├── bookmark.rs               # Bookmark (pinned path)
│           ├── diff.rs                   # DiffStatus, DiffEntry (directory comparison)
│           ├── disk.rs                   # DiskConfig, DiskType enum
│           ├── entry.rs                  # Entry (file/folder metadata)
│           ├── search.rs                 # SearchQuery, SearchResult
│           └── job.rs                    # JobKind, JobStatus, JobInfo
│
└── frontend/                             # SolidJS / TypeScript workspace
    ├── package.json
    ├── pnpm-lock.yaml
    ├── tsconfig.json / tsconfig.app.json / tsconfig.node.json
    ├── vite.config.ts
    ├── index.html
    └── src/
        ├── index.tsx                     # SolidJS mount point
        ├── App.tsx                       # Provider tree: Theme > Shortcuts > Disks > Tabs > Files > Operations > Jobs > Layout
        ├── assets/styles/
        │   ├── theme.css                 # Ayu CSS custom properties (3 variants, 40+ vars each)
        │   ├── reset.css                 # CSS reset + global user-select: none
        │   └── global.css                # Base typography, scrollbars
        ├── lib/
        │   ├── types.ts                  # IPC contract types (mirrors backend models)
        │   ├── ipc.ts                    # Typed Tauri invoke() wrappers (incl. cross-disk transfers)
        │   ├── file-utils.ts             # getIcon(), formatSize(), formatDate()
        │   ├── columns.ts               # Column definitions, formatKind(), config persistence (localStorage)
        │   ├── drag.ts                   # Drag-and-drop: data model, drop validation (cross-disk aware)
        │   ├── fuzzy.ts                  # Fuzzy matching utilities for command palette
        │   ├── shortcuts.ts              # Key bindings, platform-adaptive matching (Cmd/Ctrl)
        │   ├── search.ts                 # Debounced search with stale result rejection
        │   ├── panels.ts                 # Panel collapse/expand logic
        │   ├── virtual.ts                # Virtual scroll window computation
        │   ├── preview-utils.ts          # MIME/extension classification for preview
        │   └── languages.ts              # File extension → Shiki language ID mapping
        ├── contexts/
        │   ├── ThemeContext.tsx           # Dark/Mirage/Light, localStorage persistence
        │   ├── ShortcutContext.tsx        # Global keyboard shortcut registry, capture mode
        │   ├── DiskContext.tsx            # Disk list, active disk, CRUD
        │   ├── TabContext.tsx             # Dynamic browser + preview tabs, dedup, close/duplicate
        │   ├── FileContext.tsx            # Per-tab navigation, entries, selection, sorting, history
        │   ├── OperationContext.tsx       # Clipboard (copy/cut), file operations (cross-disk aware)
        │   └── JobContext.tsx             # Real-time job tracking via Tauri events
        └── components/
            ├── diff/
            │   ├── DiffDialog.tsx        # Modal: compare two directories, show diff table
            │   └── DiffDialog.module.css
            ├── bookmarks/
            │   ├── BookmarkList.tsx       # Sidebar bookmark section (list, click, remove)
            │   └── BookmarkList.module.css
            ├── layout/
            │   ├── AppLayout.tsx          # Three-panel layout + search state + settings
            │   ├── Sidebar.tsx            # Disk tree container + bookmark list
            │   ├── MainPanel.tsx          # TabBar + browser/preview conditional rendering
            │   ├── PropertiesPanel.tsx    # Selected entry metadata
            │   └── Toolbar.tsx            # Nav buttons, breadcrumbs, search, view/theme toggle
            ├── disk/
            │   ├── DiskTree.tsx           # Renders list of disks or empty state
            │   ├── DiskNode.tsx           # Expandable tree node, drag-drop, context menu
            │   └── AddDiskDialog.tsx      # Multi-backend form (Local, S3, GCS, Azure, SFTP, FTP)
            ├── file/
            │   ├── FileList.tsx           # Virtualized list view, resizable/configurable columns, column picker, selection
            │   ├── FileRow.tsx            # List row: icon + name + dynamic columns, inline rename
            │   ├── FileGrid.tsx           # Virtualized grid view, large icons
            │   ├── FileGridItem.tsx       # Grid cell: large icon + filename, inline rename
            │   └── BatchRenameDialog.tsx  # Modal: find/replace rename for multiple files
            ├── tabs/
            │   └── TabBar.tsx             # Tab strip: close, middle-click, context menu, ARIA
            ├── preview/
            │   ├── FilePreview.tsx        # Routes to CodePreview/image/archive/binary based on type
            │   ├── CodePreview.tsx        # Shiki syntax highlighting + line numbers
            │   └── ArchivePreview.tsx     # Read-only listing of .zip/.tar.gz contents
            ├── palette/
            │   └── CommandPalette.tsx     # Cmd/Ctrl+P fuzzy finder for actions, disks, recent paths
            ├── jobs/
            │   ├── JobPanel.tsx           # Collapsible bottom panel for job tracking
            │   └── JobRow.tsx             # Job progress, status icon, cancel button
            ├── settings/
            │   └── SettingsDialog.tsx     # Theme picker, shortcut rebinding with capture mode
            ├── treemap/
            │   └── TreemapDialog.tsx      # Disk usage visualization (stacked bar + sorted list)
            ├── search/
            │   ├── SearchBar.tsx          # Input with clear button
            │   └── SearchResults.tsx      # Results grouped by disk
            └── shared/
                └── ContextMenu.tsx        # Right-click menu, boundary-aware positioning
```

---

## How the Two Sides Connect

```
┌──────────────────┐         Tauri IPC          ┌──────────────────┐
│    Frontend       │  ◄──── invoke() ────►     │    Backend        │
│    (SolidJS)      │  ◄──── emit()/listen() ►  │    (Rust)         │
│                   │                            │                   │
│  lib/types.ts     │  ◄─── IPC Contract ───►   │  models/*.rs      │
│  lib/ipc.ts       │      (shared shapes)       │  commands/*.rs    │
└──────────────────┘                            └──────────────────┘
```

- **Build**: `tauri.conf.json` in `backend/` points `frontendDist` to `../frontend/dist`. The `beforeDevCommand` runs `cd frontend && pnpm dev` and the `beforeBuildCommand` runs `cd frontend && pnpm install && pnpm build`. Tauri orchestrates the build, but each side compiles independently.
- **IPC Contract**: TypeScript types in `frontend/src/lib/types.ts` mirror Rust structs in `backend/src/models/`. These matching type definitions are the only coupling. Both sides can evolve independently as long as the contract holds.
- **Event channel**: The backend emits `"job-update"` events via Tauri's event system for real-time job progress. Each event includes `disk_id` and `target_path` so the frontend knows exactly which directory was affected. The frontend's `JobContext` listens for these events and updates the reactive job store. `FileContext` uses the target info for [smart auto-refresh](#smart-auto-refresh). The backend also emits `"fs-change"` events when a watched local directory changes; `FileContext` listens for these to auto-refresh the file list.
- **Independent dev**: The frontend can run in a browser (`pnpm dev`, open `http://localhost:1420`) for layout and styling work; IPC calls will fail but the UI renders. The backend tests run with `cargo test` and require no frontend.

---

## Storage Backends

### StorageBackend Trait

Every storage system implements the same async trait, defined in `backend/src/storage/mod.rs`:

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn list(&self, path: &str) -> StorageResult<Vec<Entry>>;
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>>;
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()>;
    async fn delete(&self, path: &str) -> StorageResult<()>;
    async fn copy(&self, src: &str, dst: &str) -> StorageResult<()>;
    async fn rename(&self, src: &str, dst: &str) -> StorageResult<()>;
    async fn stat(&self, path: &str) -> StorageResult<Entry>;
    async fn exists(&self, path: &str) -> StorageResult<bool>;
    async fn search(&self, query: &SearchQuery) -> StorageResult<Vec<Entry>>;
    async fn create_dir(&self, path: &str) -> StorageResult<()>;
}
```

Backends are stored as `Arc<dyn StorageBackend>` in a `RwLock<HashMap<String, Arc<dyn StorageBackend>>>`, keyed by disk UUID. This allows concurrent reads from multiple backends while serializing writes to the registry.

Cross-disk transfers work by calling `read()` on the source backend and `write()` on the destination backend, with recursive directory traversal handled by the `cross_copy_single()` helper.

### Local Filesystem Backend

`backend/src/storage/local.rs` — wraps `tokio::fs` with:

- **Path traversal prevention**: All paths are canonicalized and checked to remain within the configured root directory. Attempts to escape via `../` are rejected.
- **Directories first**: Listings sort directories before files, then alphabetically within each group.
- **Recursive operations**: Copy and delete handle directory trees recursively.
- **Cross-platform permissions**: Unix file modes are reported; Windows returns `None`.
- **MIME detection**: `mime_guess` crate infers MIME types from file extensions.
- **Search**: Uses `tokio::task::spawn_blocking` to walk the filesystem on a thread pool, avoiding blocking the async runtime.
- **22 unit tests**: Covering list, read/write, delete, copy (file + directory), rename, stat, search, path traversal, create_dir, cross-backend copy, and edge cases.

### AWS S3 Backend

`backend/src/storage/s3.rs` — wraps `aws-sdk-s3` v1 with:

- **Folder emulation**: Uses `list_objects_v2` with `delimiter='/'` and `common_prefixes` to present S3's flat keyspace as a hierarchical directory structure.
- **Key normalization**: Strips leading slashes for S3 keys, adds them back for the Entry path format.
- **Rename via copy+delete**: S3 has no native rename; implemented as `copy_object` followed by `delete_object`.
- **Recursive delete**: Listing all objects under a prefix and deleting them individually.
- **Search**: Lists all objects in the bucket and filters by name pattern (client-side).
- **Directory creation**: Creates a zero-byte object with a trailing slash key.
- **Credential-based construction**: `from_credentials()` builds an S3 client from explicit access key ID, secret access key, and region. Credentials are read from the encrypted database config.

### Google Cloud Storage Backend

`backend/src/storage/gcs.rs` — wraps the GCS JSON API via `reqwest` with `rustls-tls`:

- **Folder emulation**: Uses the GCS JSON API with `delimiter=/` and prefix listing to present GCS's flat keyspace as a hierarchical directory structure.
- **Key normalization**: Strips leading slashes for GCS keys, adds them back for the Entry path format.
- **Rename via copy+delete**: GCS has no native rename; implemented as `rewriteObject` followed by `delete`.
- **Recursive delete**: Listing all objects under a prefix and deleting them individually.
- **Search**: Lists all objects in the bucket and filters by name pattern (client-side).
- **Directory creation**: Creates a zero-byte object with a trailing slash key.
- **Service account authentication**: Accepts a GCP service account JSON key. Generates short-lived OAuth2 access tokens by signing JWTs with the service account's RSA private key. Tokens are cached and refreshed automatically.
- **No OpenSSL dependency**: Uses `reqwest` with `rustls-tls` for portable TLS without system library requirements.

### Adding a New Backend

To add a new storage backend (e.g., Dropbox):

1. Create `backend/src/storage/<name>.rs` implementing `StorageBackend`
2. Add `pub mod <name>;` to `backend/src/storage/mod.rs`
3. Add a variant to `DiskType` in `backend/src/models/disk.rs`
4. Handle the new type in `create_disk` in `backend/src/commands/disk.rs`
5. Add UI fields in `frontend/src/components/disk/AddDiskDialog.tsx`

No changes to the file browser, context menu, toolbar, properties panel, search, tabs, preview, or any other UI component.

| Backend | Status | Crate |
|---------|--------|-------|
| Local filesystem | Implemented | `tokio::fs` |
| AWS S3 | Implemented | `aws-sdk-s3` v1 |
| Google Cloud Storage | Implemented | `reqwest` + GCS JSON API |
| Azure Blob Storage | Implemented | `azure_storage_blobs` |
| SFTP | Implemented | `russh` + `russh-sftp` |
| FTP/FTPS | Implemented | `suppaftp` |

---

## Frontend Architecture

### Context Providers

The app wraps the component tree in seven context providers, each managing a distinct state domain:

```
ThemeProvider          → theme signal, cycleTheme(), localStorage persistence
  ShortcutProvider     → key bindings, registerAction(), triggerAction(), global keydown listener
    DiskProvider       → disk list, activeDiskId, addDisk(), removeDisk()
      TabProvider      → browser tabs (per-disk), preview tabs, dedup, close/duplicate
        FileProvider   → per-tab: navigation, entries, selection, sorting, history cache, recentPaths
          OperationProvider → clipboard state, paste/copy/cut (cross-disk aware)
            JobProvider     → real-time job tracking via Tauri "job-update" events
              AppLayout     → three-panel layout, search state
```

**Why this order matters**: TabProvider is before FileProvider because FileContext needs to read the active tab to switch per-tab state. OperationProvider is after FileProvider because paste needs to know the current disk and path. JobProvider is innermost because it has no dependencies on other contexts.

### Component Hierarchy

```
AppLayout
├── Toolbar
│   ├── Nav buttons (Back / Forward / Up)
│   ├── Breadcrumb path (clickable segments)
│   ├── SearchBar (Cmd/Ctrl+F to focus, 300ms debounce)
│   ├── View toggle button (List / Grid)
│   ├── Settings button
│   └── Theme toggle button
├── Sidebar
│   ├── BookmarkList (pinned paths, click-to-navigate, right-click remove)
│   ├── DiskTree
│   │   └── DiskNode[] (lazy-loaded folder tree, context menu, drag-drop)
│   └── "Add Disk" button → AddDiskDialog modal
├── MainPanel
│   ├── TabBar (browser + preview tabs, context menu, middle-click close)
│   ├── [Browser content] — when a browser tab is active:
│   │   ├── FileList (virtualized list, sortable columns) — OR —
│   │   ├── FileGrid (virtualized grid, large icons) — OR —
│   │   ├── SearchResults (grouped by disk) — when searching
│   │   └── ContextMenu (right-click, boundary-aware positioning)
│   └── [FilePreview] — when a preview tab is active:
│       └── CodePreview (Shiki syntax highlighting + line numbers) — OR —
│       └── Image preview (blob URL) — OR —
│       └── Binary metadata fallback
├── PropertiesPanel
│   └── Entry metadata / disk info / multi-selection count
├── SettingsDialog
│   └── Theme picker + shortcut rebinding with capture mode
├── CommandPalette (Cmd/Ctrl+P fuzzy finder overlay)
├── BatchRenameDialog (find/replace for multiple files)
├── DiffDialog (compare two directories across backends)
├── TreemapDialog (disk usage visualization with drill-down)
└── JobPanel (collapsible bottom)
    └── JobRow[] (progress bar, status icon, cancel button per job)
```

### Tab System

The main panel supports multiple tabs, similar to a web browser or IDE:

- **Browser tabs**: One per disk by default (deduplicated by diskId). Navigate folders within a tab. Can duplicate via context menu or Cmd/Ctrl+click on sidebar.
- **Preview tabs**: Opened by double-clicking a file. Deduplicated by `diskId:path`. Shows syntax-highlighted code, images, or binary metadata.
- **Tab bar**: Click to switch, middle-click or X to close, right-click for context menu (Duplicate, Close, Close Others, Close All).
- **Per-tab state**: Each browser tab has its own navigation path, history stack, and selection. Switching tabs saves/restores state via an in-memory cache in `FileContext`.
- **Cmd/Ctrl+click**: On sidebar disks/folders or on folders in the file list, opens the target in a new tab.
- **Tab title**: Shows `DiskName: /.../FolderName` for browser tabs, updating as you navigate.

### Smart Auto-Refresh

When a background job completes (copy, move, delete, extract), the file list must update to reflect the changes. Naively refreshing on every job completion wastes bandwidth and disrupts the user's current view. DiskDeck uses a targeted staleness-tracking system instead.

**How it works:**

Every `"job-update"` event from the backend carries `disk_id` and `target_path` — the directory where the job's effects land. When a job completes, `FileContext` follows this decision tree:

```
Job completes with { disk_id, target_path }
│
├─ Currently viewing that disk + path?
│  └─ YES → refresh the file list immediately
│
└─ NO → mark disk + path as stale
         │
         ├─ Invalidate any tab cache entry for that path
         │
         └─ Later, when user switches to a tab:
            │
            ├─ Tab cache exists AND path is stale?
            │  └─ Restore cached UI instantly, then fetch fresh data
            │
            ├─ Tab cache exists AND path is NOT stale?
            │  └─ Restore cached UI only (no fetch needed)
            │
            └─ No cache (new tab)?
               └─ Fetch fresh data (always current)
```

**Scenarios this handles correctly:**

| Scenario | Behavior |
|----------|----------|
| Copy file to the folder you're viewing | Refreshes immediately — new file appears |
| Copy file to folder C while viewing folder B | C is marked stale; switching to C's tab fetches fresh data |
| Copy file to folder C, then navigate away from C before the job finishes | Tab cache for C is invalidated; returning to C fetches fresh data |
| Delete files in the current folder | Refreshes immediately — deleted files disappear |
| Local disk: external app modifies files | `"fs-change"` watcher event triggers immediate refresh |

**Additional refresh sources:**

- **Filesystem watcher** (`"fs-change"` events): For local disks, OS-level file monitoring triggers an immediate refresh when the currently viewed directory changes externally.
- **Manual refresh**: The "Refresh" context menu item and future keyboard shortcut always force a fresh fetch regardless of staleness state.

### Keyboard Shortcuts

All shortcuts are managed through `ShortcutContext`, which maintains a registry of action IDs mapped to key combinations and handler functions.

**Platform-adaptive**: All `Control` bindings automatically work with `Cmd` on macOS and `Ctrl` on Windows/Linux. The `matchesShortcut` function treats Control and Meta as interchangeable.

| Action | Default Binding | Description |
|--------|----------------|-------------|
| `file.copy` | Cmd/Ctrl+C | Copy selected entries to clipboard |
| `file.cut` | Cmd/Ctrl+X | Cut selected entries |
| `file.paste` | Cmd/Ctrl+V | Paste clipboard contents (cross-disk aware) |
| `file.delete` | Delete | Delete selected entries (with confirmation) |
| `file.rename` | F2 | Inline rename of selected entry |
| `file.selectAll` | Cmd/Ctrl+A | Select all entries in current directory |
| `file.newFolder` | Cmd/Ctrl+Shift+N | Create new folder (prompt for name) |
| `file.batchRename` | Cmd/Ctrl+Shift+R | Batch rename selected files (find/replace) |
| `file.diff` | Cmd/Ctrl+Shift+D | Compare two directories across backends |
| `file.preview` | Space | Quick preview selected file in a new tab |
| `nav.back` | Alt+Left | Navigate back in history |
| `nav.forward` | Alt+Right | Navigate forward in history |
| `nav.up` | Cmd/Ctrl+Up | Navigate to parent directory |
| `nav.open` | Cmd/Ctrl+Down | Open selected item (enter folder / preview file) |
| `tab.close` | Cmd/Ctrl+W | Close current tab |
| `tab.new` | Cmd/Ctrl+T | Duplicate current tab |
| `search.focus` | Cmd/Ctrl+F | Focus the search bar |
| `view.toggleGrid` | Cmd/Ctrl+Shift+G | Toggle between list and grid view |
| `view.cycleTheme` | Cmd/Ctrl+Shift+T | Cycle through Dark/Mirage/Light |
| `palette.open` | Cmd/Ctrl+P | Open the command palette (fuzzy finder for actions, disks, recent paths) |
| `view.diskUsage` | Cmd/Ctrl+Shift+U | Open disk usage treemap for the current directory |

Components register their handlers via `registerAction(actionId, handler)`, which returns a cleanup function called on unmount. The ShortcutContext's global `keydown` listener matches events against bindings and dispatches to the registered handler. Input/textarea/select elements are excluded to avoid intercepting normal typing.

All shortcuts are rebindable via the Settings dialog. Context menu items display the formatted shortcut hints dynamically.

### Search System

Search uses an FTS5 full-text search index backed by the SQLite database, with live fallback for unindexed disks:

1. **User types in SearchBar** (Cmd/Ctrl+F to focus) with 300ms debounce
2. **AppLayout invokes `searchEntries` IPC** with `{ pattern, disk_ids: null, recursive: true }`
3. **Backend checks each disk's index status**:
   - **Indexed disks** (status = "ready"): FTS5 prefix query on the `search_index` virtual table — instant results
   - **Unindexed disks** (status = "stale" or "indexing"): falls back to live `backend.search(query)` (filesystem walk / S3 list)
4. **Results grouped by disk** are returned as `Vec<SearchResult>`, each containing the disk name and matching entries
5. **SearchResults component** renders results grouped under disk headers with file type icons
6. **Clicking a result** clears the search, opens the disk's tab, and navigates to the parent directory

**Background indexing**: On app startup, a background task walks all disks via `StorageBackend::list()` and populates the FTS5 index. Each disk's status is tracked in `index_meta` (stale → indexing → ready). Re-indexing can be triggered per-disk via the `reindex_disk` IPC command.

**Incremental updates**: File mutation commands (delete, rename, create folder) update the search index immediately. Complex operations (copy, move) mark the disk as "stale" for background re-indexing.

---

## Security Model

| Threat | Mitigation |
|--------|-----------|
| Path traversal | LocalBackend canonicalizes paths and validates they remain within the root directory |
| Credential leakage | Credentials stored in SQLCipher-encrypted database (AES-256); redacted before IPC serialization |
| Database tampering | DB encrypted with SQLCipher; encryption key stored in OS keychain with its own platform protection |
| XSS via IPC | CSP restricts script sources (no `unsafe-inline`); all IPC data serialized through serde |
| Error information leak | Internal errors sanitized before sending to frontend; full details logged server-side only |
| Input injection | Disk names, preference keys, search patterns, file rename targets validated and length-limited |
| Excessive permissions | Tauri capabilities scoped to `core:default` + `opener:default` only |
| Remote code execution | No `dangerousRemoteDomainIpcAccess`; no remote content loaded |
| Stale connections | SFTP/FTP backends reset connection on error, reconnect on next operation |

---

## Prerequisites

- **Rust** 1.77+ with `cargo`
- **Node.js** 20+ with `pnpm`
- **Tauri CLI**: `cargo install tauri-cli --version "^2"`
- Platform-specific Tauri dependencies: see [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

---

## Development

### Recommended: `dev.sh` (no keychain prompts)

```bash
./dev.sh
```

The database is encrypted with SQLCipher and the encryption key is normally stored in the OS keychain. During development, each recompilation changes the binary's hash, causing macOS to prompt for keychain access on every run. The `dev.sh` script solves this by generating a persistent dev key in `.dev-db-key` (gitignored, `chmod 600`) and passing it via the `DISKDECK_DB_KEY` environment variable, bypassing the keychain entirely.

First run generates the key file; subsequent runs reuse it. Zero prompts.

### Alternative: Direct Tauri dev

```bash
cd backend
cargo tauri dev
```

This works on all platforms. On macOS you will be prompted for keychain access on every launch during development (production builds with stable code signatures do not have this issue).

### Frontend only

```bash
cd frontend
pnpm dev
# Open http://localhost:1420
# IPC calls will fail — use for layout and styling work only
```

### Backend only

```bash
cd backend
cargo build
```

---

## Testing

### Backend (333 tests, 84% line coverage)

```bash
cd backend

# Run all tests:
cargo test

# Run clippy with warnings as errors:
cargo clippy -- -D warnings

# Coverage report (requires cargo-llvm-cov):
cargo llvm-cov --lib
```

Test coverage by module:
- **100%**: state.rs (JobRegistry), error.rs, storage/memory.rs (test backend)
- **96-98%**: db/mod.rs (SQLite + FTS5), storage/local.rs (22 tests)
- **85-92%**: commands/search.rs, commands/disk.rs, commands/preferences.rs, indexer.rs
- **74%**: commands/file.rs (inner functions tested, Tauri command wrappers excluded)
- **45-63%**: Cloud backends (S3, GCS, Azure, SFTP, FTP — trait methods need live servers)
- **0%**: lib.rs (Tauri app bootstrap — requires running app)

### Frontend (190 tests, 92% utility coverage)

```bash
cd frontend

# Run all tests:
pnpm test

# Coverage report:
npx vitest run --coverage

# Type check:
npx tsc --noEmit

# Production build:
pnpm build
```

Test coverage by module:
- **100%**: panels.ts, paths.ts, virtual.ts, preview-utils.ts
- **92-97%**: drag.ts, search.ts, languages.ts
- **78%**: shortcuts.ts (platform-adaptive matching, formatting)

---

## Build

```bash
cd backend
cargo tauri build
```

This builds the frontend (via `beforeBuildCommand`), compiles the Rust backend in release mode, and produces platform-specific installers in `backend/target/release/bundle/`.

---

## Contributing

### Build & Test

```bash
# Backend (run from backend/):
cargo build
cargo test
cargo clippy -- -D warnings

# Frontend (run from frontend/):
pnpm test
npx tsc --noEmit
pnpm build

# Full app (run from backend/):
cargo tauri dev
```

All three backend checks (build, test, clippy with `-D warnings`) must pass. Frontend must pass `tsc --noEmit` and `pnpm test`.

### Definition of Done

Every unit of work (feature, bug fix, refactor) must satisfy ALL of the following before it is considered complete:

1. **Tests**: New code must have tests. Bug fixes must include a regression test. No exceptions.
2. **Documentation (code)**: All new public functions, types, modules, and components must have doc comments (`///` in Rust, `/** */` JSDoc in TypeScript). Module-level docs (`//!` or `@file`) for new files. Non-obvious logic gets inline comments.
3. **Documentation (README)**: New features must be reflected in the README — roadmap updated (moved from Planned to Implemented), project structure tree updated if new files were added, and any new keyboard shortcuts, config fields, or user-facing behavior documented in the appropriate section.
4. **Clean Code**: Code must be written as if it will be handed to someone with zero context tomorrow. Self-documenting names, no magic numbers, no dead code, no commented-out code. If something needs a comment to explain *what* it does (not *why*), rename it instead.
5. **Industry best practices**: Use current, well-established patterns for the language/framework. Better to skip a feature than to implement it with a hacky workaround. No deprecated APIs, no known-vulnerable patterns, no reinventing what a well-maintained library already provides.
6. **All checks green**: `cargo test`, `cargo clippy -- -D warnings`, `npx tsc --noEmit`, `pnpm test` — all must pass before the work is done.

### Rules

- **Workspace isolation is a hard constraint.** Backend and frontend are independent workspaces. The only coupling is the IPC contract: `frontend/src/lib/types.ts` mirrors `backend/src/models/`. Never introduce shared code, monorepo tooling, or cross-workspace imports.
- **No external state libraries in the frontend.** Use SolidJS `createSignal`/`createStore` + Context API only.
- **All errors go through `DiskDeckError`** (`backend/src/error.rs`). It uses `thiserror` but implements `serde::Serialize` manually (not derive) because Tauri IPC requires it. New error variants must follow this pattern.
- **Clippy must pass with `-D warnings`.** No `#[allow]` unless there's a concrete reason.
- **CSS Modules only** — `.module.css` files, accessed as `styles.camelCaseName`. No Tailwind, no CSS-in-JS, no inline styles.
- **Path traversal prevention in LocalBackend is security-critical.** Paths like `../../etc/passwd` must never escape the configured root. All paths are canonicalized and validated before any I/O. Never weaken this check.
- **New Tauri commands** require changes in three places: the `#[tauri::command]` fn, registration in `lib.rs` → `invoke_handler`, and a typed wrapper in `frontend/src/lib/ipc.ts`.
- **New storage backends** require changes in five places: the implementation file, `storage/mod.rs`, `DiskType` enum, `backend_from_config*()` factory functions, and `AddDiskDialog.tsx` form fields. Nothing else should need to change.

By contributing to this repository, you agree that your contributions will be licensed under the MIT License.

---

## Roadmap

### Implemented

- Local filesystem backend with path traversal protection
- AWS S3 backend with folder emulation
- Azure Blob Storage backend with server-side copy and folder emulation
- SFTP backend with lazy connection and auto-reconnect
- FTP/FTPS backend with Unix LIST parser and TLS upgrade
- Three-panel resizable layout (sidebar, file browser, properties)
- Ayu theme system (Dark / Mirage / Light) with persistence
- Virtualized file list with sortable columns
- Virtualized grid view with large icons
- File operations: copy, cut, paste, delete, rename, new folder
- Cross-disk transfers: copy/move files between any two backends via read+write
- Right-click context menus (file list, tab bar, sidebar disks/folders)
- Inline rename (F2)
- Cross-backend search with FTS5 index and live fallback
- Platform-adaptive keyboard shortcuts (Cmd on macOS, Ctrl on Windows/Linux)
- Breadcrumb navigation with per-tab history (back/forward/up)
- Selection: single click, Ctrl+click toggle, Shift+click range
- Properties panel showing entry metadata
- SQLCipher-encrypted persistence for disk configs (including credentials) and preferences
- OS keychain integration for database encryption key
- Background FTS5 indexing with incremental updates
- Drag and drop: move/copy within and across disks, sidebar and file list targets
- Settings dialog: shortcut rebinding with capture mode, theme picker
- Tab system: multiple browser tabs per disk, preview tabs, duplicate, close others/all
- File preview: syntax-highlighted code (Shiki, 60+ languages, ayu-dark theme, line numbers), images, binary fallback
- Job management system: collapsible panel, progress bars, cancellation, human-readable descriptions
- Credential redaction: sensitive fields blanked in IPC responses
- Input validation: names, keys, patterns, file rename targets
- Error sanitization: generic messages to frontend, full details logged server-side
- Structured logging via `tracing-subscriber` with env-filter
- Accessibility: ARIA labels, roles, expanded states, keyboard focus management
- Command palette: Cmd/Ctrl+P fuzzy finder for actions, disks, and recent paths
- Bookmarks / pinned paths: quick-access links to frequently visited directories across any disk
- SSH key authentication for SFTP: password or private key file, with auth mode toggle in the UI
- Saved connections / profiles: export/import disk configs (credentials redacted) for team sharing via Settings dialog
- Displaying size for folders: recursive directory size calculation with on-demand "Calculate Size" button in properties panel
- Batch rename: find/replace across multiple selected files with live preview dialog (Cmd/Ctrl+Shift+R)
- Compression support: browse into .zip/.tar.gz archives (read-only listing) and extract archives from context menu
- Sync/diff between disks: compare two directories across backends, show added/removed/changed files with filter and summary
- File size treemap / disk usage: visual breakdown of space consumption with stacked bar chart, sorted list, drill-down navigation, and breadcrumbs (Cmd/Ctrl+Shift+U)
- Webhooks / watch mode: live directory monitoring for local disks via OS filesystem watcher, auto-refresh on file changes
- Google Cloud Storage backend with service account auth, folder emulation, and server-side copy
- Resizable, configurable columns: Finder-like column picker (right-click header), drag-to-resize borders, additional columns (Kind, Created, Permissions), localStorage persistence


### Planned

Items roughly grouped by impact. Priority TBD — to be tackled one by one.

**Features:**

| # | Feature | Description | Complexity |
|---|---------|-------------|------------|
| 10 | More backends | Dropbox (requires OAuth) | Medium |
| 15 | Google Drive, OneDrive, Yandex Disk | OAuth backends (planned) | Medium |
| 14 | MEGA.nz backend | Pending upstream library fix (the `mega` crate is incompatible with MEGA's current API) | Medium |

**Infrastructure:**

| # | Item | Description | Complexity |
|---|------|-------------|------------|
| 12 | Integration tests | Testcontainers-based tests for S3 (MinIO), Azure (Azurite), SFTP (openssh-server), FTP (vsftpd); Podman-compatible | Medium |
| 13 | CI pipeline | GitHub Actions workflows: unit tests on every push, integration tests with containerized services | Small |

## License

This project is licensed under the [MIT License](LICENSE).

By contributing to this repository, you agree that your contributions will be licensed under the MIT License.
