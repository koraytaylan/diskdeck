import { describe, it, expect } from "vitest";
import {
  isPanelCollapsed,
  toggleAction,
  areSizesValid,
  sizesAfterCollapse,
  sizesAfterExpand,
  SIDEBAR_INDEX,
  MAIN_INDEX,
  PROPERTIES_INDEX,
  PANEL_COUNT,
} from "./panels";

describe("isPanelCollapsed", () => {
  it("returns true when panel size is 0", () => {
    expect(isPanelCollapsed([0, 0.75, 0.25], SIDEBAR_INDEX)).toBe(true);
  });

  it("returns false when panel size is non-zero", () => {
    expect(isPanelCollapsed([0.2, 0.55, 0.25], SIDEBAR_INDEX)).toBe(false);
  });

  it("works for properties panel", () => {
    expect(isPanelCollapsed([0.2, 0.8, 0], PROPERTIES_INDEX)).toBe(true);
    expect(isPanelCollapsed([0.2, 0.55, 0.25], PROPERTIES_INDEX)).toBe(false);
  });

  it("works for main panel", () => {
    expect(isPanelCollapsed([0.5, 0, 0.5], MAIN_INDEX)).toBe(true);
    expect(isPanelCollapsed([0.2, 0.55, 0.25], MAIN_INDEX)).toBe(false);
  });
});

describe("toggleAction", () => {
  it("returns 'expand' when panel is collapsed", () => {
    expect(toggleAction([0, 0.75, 0.25], SIDEBAR_INDEX)).toBe("expand");
  });

  it("returns 'collapse' when panel is expanded", () => {
    expect(toggleAction([0.2, 0.55, 0.25], SIDEBAR_INDEX)).toBe("collapse");
  });

  it("returns 'expand' for collapsed properties panel", () => {
    expect(toggleAction([0.2, 0.8, 0], PROPERTIES_INDEX)).toBe("expand");
  });

  it("returns 'collapse' for expanded properties panel", () => {
    expect(toggleAction([0.2, 0.55, 0.25], PROPERTIES_INDEX)).toBe("collapse");
  });

  it("handles both panels collapsed", () => {
    expect(toggleAction([0, 1, 0], SIDEBAR_INDEX)).toBe("expand");
    expect(toggleAction([0, 1, 0], PROPERTIES_INDEX)).toBe("expand");
  });
});

describe("areSizesValid", () => {
  it("accepts sizes that sum to 1", () => {
    expect(areSizesValid([0.2, 0.55, 0.25])).toBe(true);
  });

  it("accepts sizes with one panel collapsed", () => {
    expect(areSizesValid([0, 0.75, 0.25])).toBe(true);
    expect(areSizesValid([0.2, 0.8, 0])).toBe(true);
  });

  it("accepts sizes with both side panels collapsed", () => {
    expect(areSizesValid([0, 1, 0])).toBe(true);
  });

  it("rejects sizes that do not sum to 1", () => {
    expect(areSizesValid([0.2, 0.55, 0.3])).toBe(false);
    expect(areSizesValid([0.1, 0.1, 0.1])).toBe(false);
  });

  it("rejects wrong number of panels", () => {
    expect(areSizesValid([0.5, 0.5])).toBe(false);
    expect(areSizesValid([0.25, 0.25, 0.25, 0.25])).toBe(false);
    expect(areSizesValid([])).toBe(false);
  });

  it("rejects negative sizes", () => {
    expect(areSizesValid([-0.1, 0.8, 0.3])).toBe(false);
  });
});

describe("sizesAfterCollapse", () => {
  it("collapses sidebar and gives space to main", () => {
    const result = sizesAfterCollapse([0.2, 0.55, 0.25], SIDEBAR_INDEX);
    expect(result).toEqual([0, 0.75, 0.25]);
    expect(areSizesValid(result)).toBe(true);
  });

  it("collapses properties and gives space to main", () => {
    const result = sizesAfterCollapse([0.2, 0.55, 0.25], PROPERTIES_INDEX);
    expect(result).toEqual([0.2, 0.8, 0]);
    expect(areSizesValid(result)).toBe(true);
  });

  it("handles collapsing already-collapsed panel", () => {
    const result = sizesAfterCollapse([0, 0.75, 0.25], SIDEBAR_INDEX);
    expect(result).toEqual([0, 0.75, 0.25]);
    expect(areSizesValid(result)).toBe(true);
  });

  it("does nothing when collapsing main panel", () => {
    const sizes = [0.2, 0.55, 0.25];
    const result = sizesAfterCollapse(sizes, MAIN_INDEX);
    expect(result).toEqual(sizes);
  });

  it("preserves total sum after collapse", () => {
    const initial = [0.15, 0.6, 0.25];
    const afterSidebar = sizesAfterCollapse(initial, SIDEBAR_INDEX);
    const afterProps = sizesAfterCollapse(initial, PROPERTIES_INDEX);
    expect(areSizesValid(afterSidebar)).toBe(true);
    expect(areSizesValid(afterProps)).toBe(true);
  });

  it("does not mutate input array", () => {
    const sizes = [0.2, 0.55, 0.25];
    sizesAfterCollapse(sizes, SIDEBAR_INDEX);
    expect(sizes).toEqual([0.2, 0.55, 0.25]);
  });
});

describe("sizesAfterExpand", () => {
  it("expands sidebar by taking space from main", () => {
    const result = sizesAfterExpand([0, 0.75, 0.25], SIDEBAR_INDEX, 0.2);
    expect(result).toEqual([0.2, 0.55, 0.25]);
    expect(areSizesValid(result)).toBe(true);
  });

  it("expands properties by taking space from main", () => {
    const result = sizesAfterExpand([0.2, 0.8, 0], PROPERTIES_INDEX, 0.25);
    expect(result).toEqual([0.2, 0.55, 0.25]);
    expect(areSizesValid(result)).toBe(true);
  });

  it("clamps expansion to available main panel space", () => {
    const result = sizesAfterExpand([0, 0.3, 0.7], SIDEBAR_INDEX, 0.5);
    expect(result[SIDEBAR_INDEX]).toBe(0.3);
    expect(result[MAIN_INDEX]).toBe(0);
    expect(areSizesValid(result)).toBe(true);
  });

  it("preserves total sum after expand", () => {
    const result = sizesAfterExpand([0, 1, 0], SIDEBAR_INDEX, 0.2);
    expect(areSizesValid(result)).toBe(true);
  });

  it("does nothing when expanding main panel", () => {
    const sizes = [0.2, 0.55, 0.25];
    expect(sizesAfterExpand(sizes, MAIN_INDEX, 0.8)).toEqual(sizes);
  });

  it("does not mutate input array", () => {
    const sizes = [0, 0.75, 0.25];
    sizesAfterExpand(sizes, SIDEBAR_INDEX, 0.2);
    expect(sizes).toEqual([0, 0.75, 0.25]);
  });
});

describe("panel constants", () => {
  it("has correct panel indices", () => {
    expect(SIDEBAR_INDEX).toBe(0);
    expect(MAIN_INDEX).toBe(1);
    expect(PROPERTIES_INDEX).toBe(2);
  });

  it("PANEL_COUNT matches the number of panels", () => {
    expect(PANEL_COUNT).toBe(3);
  });
});
