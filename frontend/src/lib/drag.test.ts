import { describe, it, expect } from "vitest";
import {
  setDragData,
  getDragData,
  isValidDrop,
  dropEffect,
  isDropAllowed,
  type DragPayload,
} from "./drag";

const MIME = "application/x-diskdeck-paths";

function makeDataTransfer(data?: Record<string, string>): DataTransfer {
  const store: Record<string, string> = { ...data };
  return {
    setData(type: string, value: string) {
      store[type] = value;
    },
    getData(type: string) {
      return store[type] ?? "";
    },
    get types() {
      return Object.keys(store);
    },
    effectAllowed: "uninitialized",
    dropEffect: "none",
  } as unknown as DataTransfer;
}

function makeDragEvent(
  dt?: DataTransfer,
  opts?: { altKey?: boolean },
): DragEvent {
  return {
    dataTransfer: dt ?? null,
    altKey: opts?.altKey ?? false,
  } as unknown as DragEvent;
}

describe("drag utilities", () => {
  describe("setDragData", () => {
    it("stores correct payload and sets effectAllowed", () => {
      const dt = makeDataTransfer();
      const e = makeDragEvent(dt);
      setDragData(e, "disk-1", ["/a.txt", "/b.txt"]);
      const raw = dt.getData(MIME);
      expect(JSON.parse(raw)).toEqual({
        diskId: "disk-1",
        paths: ["/a.txt", "/b.txt"],
      });
      expect(dt.effectAllowed).toBe("copyMove");
    });

    it("does nothing when dataTransfer is null", () => {
      const e = makeDragEvent(undefined);
      // Should not throw
      setDragData(e, "disk-1", ["/a.txt"]);
    });
  });

  describe("getDragData", () => {
    it("returns null for missing MIME type", () => {
      const dt = makeDataTransfer({ "text/plain": "hello" });
      const e = makeDragEvent(dt);
      expect(getDragData(e)).toBeNull();
    });

    it("parses valid payload", () => {
      const payload = JSON.stringify({ diskId: "d1", paths: ["/x"] });
      const dt = makeDataTransfer({ [MIME]: payload });
      const e = makeDragEvent(dt);
      expect(getDragData(e)).toEqual({ diskId: "d1", paths: ["/x"] });
    });

    it("returns null for invalid JSON", () => {
      const dt = makeDataTransfer({ [MIME]: "not-json" });
      const e = makeDragEvent(dt);
      expect(getDragData(e)).toBeNull();
    });

    it("returns null when dataTransfer is null", () => {
      const e = makeDragEvent(undefined);
      expect(getDragData(e)).toBeNull();
    });
  });

  describe("isValidDrop", () => {
    it("returns true when MIME type is present", () => {
      const dt = makeDataTransfer({ [MIME]: "{}" });
      expect(isValidDrop(makeDragEvent(dt))).toBe(true);
    });

    it("returns false when MIME type is missing", () => {
      const dt = makeDataTransfer({});
      expect(isValidDrop(makeDragEvent(dt))).toBe(false);
    });

    it("returns false when dataTransfer is null", () => {
      expect(isValidDrop(makeDragEvent(undefined))).toBe(false);
    });
  });

  describe("dropEffect", () => {
    it("returns move by default", () => {
      expect(dropEffect(makeDragEvent(undefined))).toBe("move");
    });

    it("returns copy when altKey is held", () => {
      expect(dropEffect(makeDragEvent(undefined, { altKey: true }))).toBe(
        "copy",
      );
    });
  });

  describe("isDropAllowed", () => {
    it("allows cross-disk drops", () => {
      expect(
        isDropAllowed({ diskId: "a", paths: ["/x"] }, "b", "/dest"),
      ).toBe(true);
    });

    it("rejects dropping onto self", () => {
      expect(
        isDropAllowed({ diskId: "a", paths: ["/x"] }, "a", "/x"),
      ).toBe(false);
    });

    it("rejects dropping into a child of a dragged folder", () => {
      expect(
        isDropAllowed(
          { diskId: "a", paths: ["/parent"] },
          "a",
          "/parent/child",
        ),
      ).toBe(false);
    });

    it("allows valid drops", () => {
      expect(
        isDropAllowed({ diskId: "a", paths: ["/src/file.txt"] }, "a", "/dest"),
      ).toBe(true);
    });

    it("allows dropping sibling paths", () => {
      expect(
        isDropAllowed(
          { diskId: "a", paths: ["/a", "/b"] },
          "a",
          "/c",
        ),
      ).toBe(true);
    });

    it("allows drop from different disk to target", () => {
      const payload: DragPayload = { diskId: "disk-a", paths: ["/file.txt"] };
      expect(isDropAllowed(payload, "disk-b", "/")).toBe(true);
    });

    it("allows cross-disk drop even to same path", () => {
      const payload: DragPayload = { diskId: "disk-a", paths: ["/docs"] };
      expect(isDropAllowed(payload, "disk-b", "/docs")).toBe(true);
    });
  });
});
