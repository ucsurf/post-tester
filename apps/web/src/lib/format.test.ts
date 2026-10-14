import { describe, expect, it } from "vitest";
import { formatDuration, formatSize, plural, statusTone } from "./format";

describe("formatDuration", () => {
  it.each([
    [null, "—"],
    [undefined, "—"],
    [-1, "—"],
    [0, "0 ms"],
    [3.24, "3.2 ms"],
    [9.99, "10 ms"],
    [182.4, "182 ms"],
    [1420, "1.42 s"],
    [12_345, "12.3 s"],
    [125_000, "2m 05s"],
    [3_720_000, "1h 02m"],
  ])("%s → %s", (input, expected) => {
    expect(formatDuration(input)).toBe(expected);
  });
});

describe("formatSize", () => {
  it.each([
    [null, "—"],
    [0, "0 B"],
    [512, "512 B"],
    [1024, "1 KB"],
    [1229, "1.2 KB"],
    [8600, "8.4 KB"],
    [12_500, "12 KB"],
    [3.4 * 1024 * 1024, "3.4 MB"],
  ])("%s → %s", (input, expected) => {
    expect(formatSize(input)).toBe(expected);
  });
});

describe("statusTone", () => {
  it("groups status codes", () => {
    expect(statusTone(null)).toBe("neutral");
    expect(statusTone(101)).toBe("info");
    expect(statusTone(204)).toBe("success");
    expect(statusTone(301)).toBe("redirect");
    expect(statusTone(404)).toBe("warning");
    expect(statusTone(503)).toBe("danger");
  });
});

describe("plural", () => {
  it("picks the right form", () => {
    expect(plural(1, "request")).toBe("1 request");
    expect(plural(2, "request")).toBe("2 requests");
    expect(plural(0, "entry", "entries")).toBe("0 entries");
  });
});
