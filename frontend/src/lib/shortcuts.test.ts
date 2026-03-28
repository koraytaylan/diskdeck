import { describe, it, expect } from "vitest";
import { formatShortcut, matchesShortcut, defaultShortcuts, shortcutLabels, shortcutCategories, serializeKeys } from "./shortcuts";

describe("formatShortcut", () => {
  it("formats Control+C as ⌃C", () => {
    expect(formatShortcut(["Control", "c"])).toBe("\u2303C");
  });

  it("formats Meta+Shift+N", () => {
    expect(formatShortcut(["Control", "Shift", "n"])).toBe("\u2303\u21e7N");
  });

  it("formats single key Delete", () => {
    expect(formatShortcut(["Delete"])).toBe("Del");
  });

  it("formats F2 as-is", () => {
    expect(formatShortcut(["F2"])).toBe("F2");
  });

  it("formats Alt+ArrowLeft", () => {
    expect(formatShortcut(["Alt", "ArrowLeft"])).toBe("\u2325\u2190");
  });
});

describe("matchesShortcut", () => {
  function createKeyEvent(
    key: string,
    opts: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {},
  ): KeyboardEvent {
    return new KeyboardEvent("keydown", {
      key,
      ctrlKey: opts.ctrl ?? false,
      shiftKey: opts.shift ?? false,
      altKey: opts.alt ?? false,
      metaKey: opts.meta ?? false,
    });
  }

  it("matches Ctrl+C", () => {
    const e = createKeyEvent("c", { ctrl: true });
    expect(matchesShortcut(e, ["Control", "c"])).toBe(true);
  });

  it("does not match when modifier is missing", () => {
    const e = createKeyEvent("c");
    expect(matchesShortcut(e, ["Control", "c"])).toBe(false);
  });

  it("does not match when extra modifier is pressed", () => {
    const e = createKeyEvent("c", { ctrl: true, shift: true });
    expect(matchesShortcut(e, ["Control", "c"])).toBe(false);
  });

  it("matches Delete key", () => {
    const e = createKeyEvent("Delete");
    expect(matchesShortcut(e, ["Delete"])).toBe(true);
  });

  it("matches Ctrl+Shift+N", () => {
    const e = createKeyEvent("n", { ctrl: true, shift: true });
    expect(matchesShortcut(e, ["Control", "Shift", "n"])).toBe(true);
  });

  it("matches F2", () => {
    const e = createKeyEvent("F2");
    expect(matchesShortcut(e, ["F2"])).toBe(true);
  });

  it("does not match wrong key", () => {
    const e = createKeyEvent("v", { ctrl: true });
    expect(matchesShortcut(e, ["Control", "c"])).toBe(false);
  });

  it("matches Cmd+C (Meta) for Control binding (macOS cross-platform)", () => {
    const e = createKeyEvent("c", { meta: true });
    expect(matchesShortcut(e, ["Control", "c"])).toBe(true);
  });

  it("matches Ctrl+C for Meta binding (cross-platform)", () => {
    const e = createKeyEvent("c", { ctrl: true });
    expect(matchesShortcut(e, ["Meta", "c"])).toBe(true);
  });

  it("does not match when neither ctrl nor meta is pressed for Control binding", () => {
    const e = createKeyEvent("c", { alt: true });
    expect(matchesShortcut(e, ["Control", "c"])).toBe(false);
  });

  it("matches Space key", () => {
    const e = createKeyEvent(" ");
    expect(matchesShortcut(e, [" "])).toBe(true);
  });

  it("matches Cmd+Shift+N for Control+Shift binding", () => {
    const e = createKeyEvent("n", { meta: true, shift: true });
    expect(matchesShortcut(e, ["Control", "Shift", "n"])).toBe(true);
  });
});

describe("defaultShortcuts", () => {
  it("has all required action IDs", () => {
    const requiredActions = [
      "file.copy",
      "file.cut",
      "file.paste",
      "file.delete",
      "file.rename",
      "file.selectAll",
      "file.newFolder",
      "nav.back",
      "nav.forward",
      "nav.up",
      "search.focus",
      "view.cycleTheme",
    ];
    for (const action of requiredActions) {
      expect(defaultShortcuts).toHaveProperty(action);
      expect(Array.isArray(defaultShortcuts[action])).toBe(true);
      expect(defaultShortcuts[action].length).toBeGreaterThan(0);
    }
  });
});

describe("shortcutLabels", () => {
  it("has a label for every default shortcut", () => {
    for (const actionId of Object.keys(defaultShortcuts)) {
      expect(shortcutLabels).toHaveProperty(actionId);
      expect(shortcutLabels[actionId].length).toBeGreaterThan(0);
    }
  });
});

describe("shortcutCategories", () => {
  it("covers all default shortcut action IDs", () => {
    const allActions = shortcutCategories.flatMap((c) => c.actions);
    for (const actionId of Object.keys(defaultShortcuts)) {
      expect(allActions).toContain(actionId);
    }
  });

  it("contains no duplicate action IDs", () => {
    const allActions = shortcutCategories.flatMap((c) => c.actions);
    const unique = new Set(allActions);
    expect(allActions.length).toBe(unique.size);
  });
});

describe("serializeKeys", () => {
  it("normalizes key combos to canonical form", () => {
    expect(serializeKeys(["Control", "c"])).toBe("Control+c");
    expect(serializeKeys(["Shift", "Control", "n"])).toBe("Control+Shift+n");
  });

  it("sorts modifiers alphabetically", () => {
    expect(serializeKeys(["Shift", "Alt", "Control", "x"])).toBe(
      "Alt+Control+Shift+x",
    );
  });

  it("produces same output regardless of modifier order", () => {
    expect(serializeKeys(["Shift", "Control", "n"])).toBe(
      serializeKeys(["Control", "Shift", "n"]),
    );
  });

  it("handles single non-modifier key", () => {
    expect(serializeKeys(["Delete"])).toBe("delete");
    expect(serializeKeys(["F2"])).toBe("f2");
  });
});
