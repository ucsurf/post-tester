import { describe, expect, it } from "vitest";
import { headerValue, isJsonContentType, matchesContentType, mediaType, prettyJson, splitArgs } from "./text";

describe("splitArgs", () => {
  it("splits on whitespace and honours quotes", () => {
    expect(splitArgs(`-y @modelcontextprotocol/server-everything`)).toEqual(["-y", "@modelcontextprotocol/server-everything"]);
    expect(splitArgs(`--dir "my folder" 'a b' c\\ d`)).toEqual(["--dir", "my folder", "a b", "c d"]);
    expect(splitArgs(`  `)).toEqual([]);
    expect(splitArgs(`--empty ""`)).toEqual(["--empty", ""]);
  });
});

describe("content types", () => {
  it("extracts the media type", () => {
    expect(mediaType("Application/JSON; charset=utf-8")).toBe("application/json");
    expect(mediaType(undefined)).toBe("");
  });

  it("detects JSON", () => {
    expect(isJsonContentType("application/problem+json")).toBe(true);
    expect(isJsonContentType("text/html")).toBe(false);
  });

  it("matches viewer patterns", () => {
    expect(matchesContentType("image/png", ["image/*"])).toBe(true);
    expect(matchesContentType("application/vnd.api+json", ["application/*+json"])).toBe(true);
    expect(matchesContentType("text/csv; charset=utf-8", ["text/csv"])).toBe(true);
    expect(matchesContentType("text/html", ["text/csv"])).toBe(false);
    expect(matchesContentType(undefined, ["*"])).toBe(false);
  });
});

describe("prettyJson", () => {
  it("formats JSON and leaves other text alone", () => {
    expect(prettyJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyJson("not json")).toBe("not json");
  });
});

describe("headerValue", () => {
  it("finds headers case-insensitively in both shapes", () => {
    expect(headerValue([["Content-Type", "text/plain"]], "content-type")).toBe("text/plain");
    expect(headerValue([{ name: "X-Id", value: "1" }], "x-id")).toBe("1");
    expect(headerValue([], "x")).toBeUndefined();
  });
});
