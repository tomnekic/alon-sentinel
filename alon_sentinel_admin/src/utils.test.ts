import { describe, expect, it } from "vitest";
import { buildQuery, formatTimestamp, formatRefreshTimestamp } from "./utils";

describe("buildQuery", () => {
  it("returns empty string when all values are null, undefined, or empty", () => {
    expect(buildQuery({ a: null, b: undefined, c: "" })).toBe("");
  });

  it("includes only defined, non-empty values and percent-encodes them", () => {
    expect(buildQuery({ cursor: "abc==", limit: 10, filter: null })).toBe(
      "?cursor=abc%3D%3D&limit=10"
    );
  });

  it("includes numeric zero as a valid value", () => {
    expect(buildQuery({ offset: 0 })).toBe("?offset=0");
  });

  it("omits keys whose values are the empty string", () => {
    expect(buildQuery({ outcome: "", limit: 25 })).toBe("?limit=25");
  });
});

describe("formatTimestamp", () => {
  it("returns 'Never' for null, undefined, and empty string", () => {
    expect(formatTimestamp(null)).toBe("Never");
    expect(formatTimestamp(undefined)).toBe("Never");
    expect(formatTimestamp("")).toBe("Never");
  });

  it("passes through strings that are not valid dates unchanged", () => {
    expect(formatTimestamp("not-a-date")).toBe("not-a-date");
  });

  it("formats a valid ISO timestamp as a locale string", () => {
    const result = formatTimestamp("2026-05-13T12:00:00Z");
    // Locale output varies by environment; just confirm it's neither the sentinel
    // nor the raw ISO string — meaning the date parsed and was formatted.
    expect(result).not.toBe("Never");
    expect(result).not.toBe("2026-05-13T12:00:00Z");
  });
});

describe("formatRefreshTimestamp", () => {
  it("returns 'Never' for null", () => {
    expect(formatRefreshTimestamp(null)).toBe("Never");
  });

  it("returns 'Never' for 0 (falsy epoch)", () => {
    expect(formatRefreshTimestamp(0)).toBe("Never");
  });

  it("formats a valid epoch millisecond value as a time string", () => {
    const result = formatRefreshTimestamp(new Date("2026-05-13T12:00:00Z").getTime());
    expect(result).not.toBe("Never");
  });
});
