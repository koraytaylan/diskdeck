/**
 * @file Application Root Component
 *
 * Assembles the full provider tree and renders the main application layout.
 *
 * **Provider nesting order (outermost to innermost):**
 * 1. `ThemeProvider`     -- Theme must be available first (no dependencies).
 * 2. `ShortcutProvider`  -- Shortcuts depend on theme (for the cycle action).
 * 3. `DiskProvider`      -- Disk list is independent of file state.
 * 4. `TabProvider`       -- Tab state must be available before FileProvider.
 * 5. `FileProvider`      -- File browsing depends on disk and tab contexts.
 * 6. `OperationProvider` -- Operations depend on both disk and file contexts.
 *
 * **Window visibility:**
 * The Tauri window starts hidden (configured in tauri.conf.json) to prevent
 * a white flash before styles are loaded. `onMount` calls `getCurrentWindow().show()`
 * to make the window visible once SolidJS has rendered the initial DOM.
 *
 * @module App
 */

import { type Component, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeProvider } from "./contexts/ThemeContext";
import { DiskProvider } from "./contexts/DiskContext";
import { FileProvider } from "./contexts/FileContext";
import { OperationProvider } from "./contexts/OperationContext";
import { ShortcutProvider } from "./contexts/ShortcutContext";
import { JobProvider } from "./contexts/JobContext";
import { TabProvider } from "./contexts/TabContext";
import { AppLayout } from "./components/layout/AppLayout";

/**
 * Root application component. Sets up the provider tree and shows the
 * Tauri window once the DOM is ready.
 */
const App: Component = () => {
  onMount(() => {
    // Show the window after first render to avoid white flash
    getCurrentWindow().show();
  });

  return (
    <ThemeProvider>
      <ShortcutProvider>
        <DiskProvider>
          <TabProvider>
            <FileProvider>
              <OperationProvider>
                <JobProvider>
                  <AppLayout />
                </JobProvider>
              </OperationProvider>
            </FileProvider>
          </TabProvider>
        </DiskProvider>
      </ShortcutProvider>
    </ThemeProvider>
  );
};

export default App;
