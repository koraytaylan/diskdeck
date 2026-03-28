/**
 * @file Virtual Scroll Window Computation
 *
 * Pure mathematical functions for computing which items to render in a
 * virtualized list or grid. The key idea: instead of rendering all N items,
 * only render the items visible in the viewport plus a configurable
 * "overscan" buffer above and below.
 *
 * **Why custom instead of a library?**
 * The `@solid-primitives/virtual` package (v0.2.3) reads the root height
 * once during initialization and never re-tracks it, so window resizes
 * cause stale calculations. These functions are reactive-safe because they
 * are called inside SolidJS `createMemo` / derived signals that pass fresh
 * dimensions on every change.
 *
 * Used by:
 * - `FileList`     -- flat list virtualization via `computeVirtualWindow`.
 * - `FileGrid`     -- grid virtualization via `computeGridVirtualWindow`.
 *
 * @module lib/virtual
 */

/**
 * Output of the virtual list window calculation.
 *
 * @property containerHeight - Total scrollable height in pixels (itemCount * rowHeight).
 *                             Applied as the height of an invisible spacer div.
 * @property viewerTop       - Pixel offset for the "viewer" div that holds visible items.
 *                             Uses `position: absolute; top: viewerTop`.
 * @property firstIdx        - Index of the first item to render (inclusive).
 * @property lastIdx         - Index of the last item to render (exclusive, for use with `slice`).
 */
export interface VirtualWindow {
  containerHeight: number;
  viewerTop: number;
  firstIdx: number;
  lastIdx: number;
}

/**
 * Output of the virtual grid window calculation.
 *
 * @property containerHeight - Total scrollable height in pixels.
 * @property viewerTop       - Pixel offset for the visible items container.
 * @property firstItemIdx    - First item index to render (inclusive), accounting for grid rows.
 * @property lastItemIdx     - Last item index to render (exclusive), clamped to itemCount.
 * @property colCount        - Number of columns that fit in the current viewport width.
 */
export interface GridVirtualWindow {
  containerHeight: number;
  viewerTop: number;
  firstItemIdx: number;
  lastItemIdx: number;
  colCount: number;
}

/**
 * Compute the virtual window for a grid layout.
 *
 * Internally delegates to `computeVirtualWindow` by treating each
 * *grid row* (containing `colCount` items) as a single virtual row.
 * The returned `firstItemIdx` / `lastItemIdx` are flat item indices
 * suitable for `array.slice(firstItemIdx, lastItemIdx)`.
 *
 * @param itemCount    - Total number of items in the flat list.
 * @param rootHeight   - Current viewport height in pixels.
 * @param rootWidth    - Current viewport width in pixels (determines column count).
 * @param cellWidth    - Width of a single grid cell in pixels.
 * @param cellHeight   - Height of a single grid cell (row height) in pixels.
 * @param scrollOffset - Current `scrollTop` of the scroll container.
 * @param overscan     - Number of extra *grid rows* to render above/below the viewport.
 * @returns Grid virtual window parameters.
 */
export function computeGridVirtualWindow(
  itemCount: number,
  rootHeight: number,
  rootWidth: number,
  cellWidth: number,
  cellHeight: number,
  scrollOffset: number,
  overscan: number,
): GridVirtualWindow {
  const colCount = Math.max(1, Math.floor(rootWidth / cellWidth));
  const rowCount = Math.ceil(itemCount / colCount);
  const win = computeVirtualWindow(rowCount, rootHeight, cellHeight, scrollOffset, overscan);
  return {
    containerHeight: win.containerHeight,
    viewerTop: win.viewerTop,
    firstItemIdx: win.firstIdx * colCount,
    lastItemIdx: Math.min(itemCount, win.lastIdx * colCount),
    colCount,
  };
}

/**
 * Compute the virtual window for a flat list.
 *
 * Calculates which range of items [firstIdx, lastIdx) should be rendered
 * given the current scroll position, viewport size, and overscan buffer.
 *
 * @param itemCount    - Total number of items.
 * @param rootHeight   - Viewport height in pixels.
 * @param rowHeight    - Height of each row in pixels (must be uniform).
 * @param scrollOffset - Current `scrollTop` of the scroll container.
 * @param overscan     - Number of extra rows to render above/below the visible area.
 *                        Prevents flicker during fast scrolling.
 * @returns Virtual window parameters for positioning the visible slice.
 */
export function computeVirtualWindow(
  itemCount: number,
  rootHeight: number,
  rowHeight: number,
  scrollOffset: number,
  overscan: number,
): VirtualWindow {
  const firstIdx = Math.max(0, Math.floor(scrollOffset / rowHeight) - overscan);
  const lastIdx = Math.min(
    itemCount,
    Math.floor(scrollOffset / rowHeight) + Math.ceil(rootHeight / rowHeight) + overscan,
  );
  return {
    containerHeight: itemCount * rowHeight,
    viewerTop: firstIdx * rowHeight,
    firstIdx,
    lastIdx,
  };
}
