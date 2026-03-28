/**
 * @file Panel Collapse/Expand Logic
 *
 * Pure functions for computing panel sizes in the three-panel layout
 * (Sidebar | Main | Properties). Extracted from `AppLayout` for
 * testability -- see `panels.test.ts` for the full test suite.
 *
 * **Size model:**
 * Panel sizes are represented as fractional values (0..1) that must
 * always sum to exactly 1.0 (100% of horizontal space). A collapsed
 * panel has size 0, and its space is redistributed to the main panel.
 *
 * The actual `@corvu/resizable` context is wired in `AppLayout` via
 * `ctx.collapse()` / `ctx.expand()`, which internally manage the
 * fraction arithmetic. These helpers are used for toggle logic and
 * size validation.
 *
 * @module lib/panels
 */

/** Array index of the sidebar panel in the sizes tuple. */
export const SIDEBAR_INDEX = 0;

/** Array index of the main (center) panel in the sizes tuple. */
export const MAIN_INDEX = 1;

/** Array index of the properties panel in the sizes tuple. */
export const PROPERTIES_INDEX = 2;

/** Total number of panels in the layout. */
export const PANEL_COUNT = 3;

/**
 * Check if a panel is collapsed (size is 0).
 *
 * @param sizes      - Current panel size fractions `[sidebar, main, properties]`.
 * @param panelIndex - Index of the panel to check.
 * @returns True if the panel's size is exactly 0.
 */
export function isPanelCollapsed(sizes: number[], panelIndex: number): boolean {
  return sizes[panelIndex] === 0;
}

/**
 * Determine the appropriate toggle action for a panel.
 * Returns "expand" if the panel is currently collapsed, "collapse" otherwise.
 *
 * @param sizes      - Current panel size fractions.
 * @param panelIndex - Index of the panel to toggle.
 * @returns The action to perform.
 */
export function toggleAction(
  sizes: number[],
  panelIndex: number,
): "collapse" | "expand" {
  return isPanelCollapsed(sizes, panelIndex) ? "expand" : "collapse";
}

/**
 * Validate that a sizes array is well-formed.
 * Checks: correct length, no negative values, and sums to 1.0 (within epsilon).
 *
 * @param sizes - Panel size fractions to validate.
 * @returns True if valid.
 */
export function areSizesValid(sizes: number[]): boolean {
  if (sizes.length !== PANEL_COUNT) return false;
  if (sizes.some((s) => s < 0)) return false;
  const sum = sizes.reduce((a, b) => a + b, 0);
  return Math.abs(sum - 1) < 1e-6;
}

/**
 * Compute new panel sizes after collapsing a side panel.
 * The freed space is given entirely to the main panel.
 * Collapsing the main panel itself is a no-op (returns a copy).
 *
 * @param sizes      - Current panel size fractions.
 * @param panelIndex - Index of the panel to collapse (SIDEBAR_INDEX or PROPERTIES_INDEX).
 * @returns New sizes array (does not mutate the input).
 */
export function sizesAfterCollapse(
  sizes: number[],
  panelIndex: number,
): number[] {
  if (panelIndex === MAIN_INDEX) return [...sizes];
  const freed = sizes[panelIndex];
  const result = [...sizes];
  result[panelIndex] = 0;
  result[MAIN_INDEX] += freed;
  return result;
}

/**
 * Compute new panel sizes after expanding a side panel to a target size.
 * Space is taken from the main panel. If the main panel doesn't have
 * enough room, the side panel gets whatever is available.
 * Expanding the main panel itself is a no-op.
 *
 * @param sizes      - Current panel size fractions.
 * @param panelIndex - Index of the panel to expand.
 * @param targetSize - Desired size fraction for the expanded panel.
 * @returns New sizes array (does not mutate the input).
 */
export function sizesAfterExpand(
  sizes: number[],
  panelIndex: number,
  targetSize: number,
): number[] {
  if (panelIndex === MAIN_INDEX) return [...sizes];
  const result = [...sizes];
  const available = result[MAIN_INDEX];
  const actual = Math.min(targetSize, available);
  result[panelIndex] = actual;
  result[MAIN_INDEX] -= actual;
  return result;
}
