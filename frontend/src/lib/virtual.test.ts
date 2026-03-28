import { describe, it, expect } from "vitest";
import { computeVirtualWindow, computeGridVirtualWindow } from "./virtual";

const ROW_HEIGHT = 28;

describe("computeVirtualWindow", () => {
  it("returns correct containerHeight for all items", () => {
    const win = computeVirtualWindow(100, 400, ROW_HEIGHT, 0, 0);
    expect(win.containerHeight).toBe(100 * ROW_HEIGHT);
  });

  it("shows enough items to fill the viewport", () => {
    const rootHeight = 400;
    const win = computeVirtualWindow(200, rootHeight, ROW_HEIGHT, 0, 0);
    const visibleCount = win.lastIdx - win.firstIdx;
    expect(visibleCount).toBeGreaterThanOrEqual(Math.ceil(rootHeight / ROW_HEIGHT));
  });

  it("includes overscan items before and after the viewport", () => {
    const overscan = 10;
    // Scroll to the middle so overscan applies on both sides
    const scrollOffset = 1000;
    const win = computeVirtualWindow(200, 400, ROW_HEIGHT, scrollOffset, overscan);
    const winNoOverscan = computeVirtualWindow(200, 400, ROW_HEIGHT, scrollOffset, 0);
    expect(win.firstIdx).toBeLessThan(winNoOverscan.firstIdx);
    expect(win.lastIdx).toBeGreaterThan(winNoOverscan.lastIdx);
  });

  it("clamps firstIdx to 0 at the top", () => {
    const win = computeVirtualWindow(200, 400, ROW_HEIGHT, 0, 10);
    expect(win.firstIdx).toBe(0);
  });

  it("clamps lastIdx to itemCount at the bottom", () => {
    const itemCount = 50;
    // Scroll way past the end
    const scrollOffset = itemCount * ROW_HEIGHT;
    const win = computeVirtualWindow(itemCount, 400, ROW_HEIGHT, scrollOffset, 10);
    expect(win.lastIdx).toBe(itemCount);
  });

  it("viewerTop equals firstIdx * rowHeight", () => {
    const win = computeVirtualWindow(200, 400, ROW_HEIGHT, 500, 5);
    expect(win.viewerTop).toBe(win.firstIdx * ROW_HEIGHT);
  });

  it("handles zero items", () => {
    const win = computeVirtualWindow(0, 400, ROW_HEIGHT, 0, 10);
    expect(win.containerHeight).toBe(0);
    expect(win.firstIdx).toBe(0);
    expect(win.lastIdx).toBe(0);
    expect(win.viewerTop).toBe(0);
  });

  it("handles zero rootHeight", () => {
    const win = computeVirtualWindow(100, 0, ROW_HEIGHT, 0, 10);
    expect(win.firstIdx).toBe(0);
    // With 0 rootHeight, ceil(0/28) = 0, so lastIdx = 0 + 0 + 10 = 10
    expect(win.lastIdx).toBe(10);
  });

  // Core bug test: rootHeight changes should produce different visible ranges
  it("renders more items when rootHeight increases (the resize bug)", () => {
    const smallHeight = 768;
    const largeHeight = 1080;
    const itemCount = 200;

    const winSmall = computeVirtualWindow(itemCount, smallHeight, ROW_HEIGHT, 0, 10);
    const winLarge = computeVirtualWindow(itemCount, largeHeight, ROW_HEIGHT, 0, 10);

    const visibleSmall = winSmall.lastIdx - winSmall.firstIdx;
    const visibleLarge = winLarge.lastIdx - winLarge.firstIdx;

    expect(visibleLarge).toBeGreaterThan(visibleSmall);
    // The difference should be close to the height increase divided by row height
    // (off-by-one possible due to ceil/floor rounding at each viewport size)
    const expectedDiff = Math.ceil(largeHeight / ROW_HEIGHT) - Math.ceil(smallHeight / ROW_HEIGHT);
    expect(visibleLarge - visibleSmall).toBe(expectedDiff);
  });

  it("scrolled window shows correct range", () => {
    // Scroll 10 rows down
    const scrollOffset = 10 * ROW_HEIGHT;
    const win = computeVirtualWindow(200, 400, ROW_HEIGHT, scrollOffset, 0);
    expect(win.firstIdx).toBe(10);
    expect(win.viewerTop).toBe(10 * ROW_HEIGHT);
  });

  it("with single item and large viewport", () => {
    const win = computeVirtualWindow(1, 1080, ROW_HEIGHT, 0, 10);
    expect(win.containerHeight).toBe(ROW_HEIGHT);
    expect(win.firstIdx).toBe(0);
    expect(win.lastIdx).toBe(1);
  });
});

const CELL_WIDTH = 120;
const CELL_HEIGHT = 100;

describe("computeGridVirtualWindow", () => {
  it("falls back to single column when width < cellWidth", () => {
    const win = computeGridVirtualWindow(10, 400, 80, CELL_WIDTH, CELL_HEIGHT, 0, 0);
    expect(win.colCount).toBe(1);
    expect(win.containerHeight).toBe(10 * CELL_HEIGHT);
  });

  it("calculates correct column count", () => {
    // 360px / 120px = 3 columns
    const win = computeGridVirtualWindow(9, 400, 360, CELL_WIDTH, CELL_HEIGHT, 0, 0);
    expect(win.colCount).toBe(3);
  });

  it("calculates correct container height", () => {
    // 10 items, 3 cols → 4 rows (ceil(10/3) = 4)
    const win = computeGridVirtualWindow(10, 400, 360, CELL_WIDTH, CELL_HEIGHT, 0, 0);
    expect(win.colCount).toBe(3);
    expect(win.containerHeight).toBe(4 * CELL_HEIGHT);
  });

  it("returns correct item indices for scrolled position", () => {
    // 30 items, 3 cols → 10 rows. Scroll down 2 rows.
    const scrollOffset = 2 * CELL_HEIGHT;
    const win = computeGridVirtualWindow(30, 400, 360, CELL_WIDTH, CELL_HEIGHT, scrollOffset, 0);
    expect(win.colCount).toBe(3);
    expect(win.firstItemIdx).toBe(2 * 3); // row 2 * 3 cols = item 6
  });

  it("handles last partial row correctly", () => {
    // 10 items, 3 cols → 4 rows, last row has 1 item
    const win = computeGridVirtualWindow(10, 1000, 360, CELL_WIDTH, CELL_HEIGHT, 0, 0);
    expect(win.colCount).toBe(3);
    expect(win.lastItemIdx).toBe(10); // clamped to itemCount, not 4*3=12
  });

  it("handles zero items", () => {
    const win = computeGridVirtualWindow(0, 400, 360, CELL_WIDTH, CELL_HEIGHT, 0, 3);
    expect(win.containerHeight).toBe(0);
    expect(win.firstItemIdx).toBe(0);
    expect(win.lastItemIdx).toBe(0);
    expect(win.colCount).toBe(3);
  });
});
