import { describe, it, expect } from "vitest";
import { fuzzyMatch, fuzzyScore } from "./fuzzy";

describe("fuzzyMatch", () => {
  it("returns true for substring match", () => {
    expect(fuzzyMatch("cop", "Copy")).toBe(true);
  });

  it("is case-insensitive", () => {
    expect(fuzzyMatch("COPY", "copy")).toBe(true);
    expect(fuzzyMatch("copy", "COPY")).toBe(true);
    expect(fuzzyMatch("Copy", "copy files")).toBe(true);
  });

  it("returns false for no match", () => {
    expect(fuzzyMatch("xyz", "Copy")).toBe(false);
    expect(fuzzyMatch("paste", "Copy")).toBe(false);
  });
});

describe("fuzzyScore", () => {
  it("ranks exact prefix higher than mid-string match", () => {
    const prefixScore = fuzzyScore("cop", "Copy");
    const midScore = fuzzyScore("cop", "File Copy");
    expect(prefixScore).toBeLessThan(midScore);
  });

  it("returns 0 for prefix match", () => {
    expect(fuzzyScore("nav", "Navigate Back")).toBe(0);
  });

  it("returns text length for no match", () => {
    expect(fuzzyScore("xyz", "Copy")).toBe("Copy".length);
  });
});
