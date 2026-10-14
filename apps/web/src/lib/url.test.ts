import { describe, expect, it } from "vitest";
import { applyUrlToRequest, composeUrl, normalizeUrlAndQuery, parseUrl } from "./url";

describe("parseUrl", () => {
  it("returns the url untouched when there is no query", () => {
    expect(parseUrl("{{baseUrl}}/users")).toEqual({ base: "{{baseUrl}}/users", params: [] });
  });

  it("splits params and decodes values", () => {
    expect(parseUrl("https://x.dev/s?q=a%20b&page=2&flag")).toEqual({
      base: "https://x.dev/s",
      params: [
        { key: "q", value: "a b" },
        { key: "page", value: "2" },
        { key: "flag", value: "" },
      ],
    });
  });

  it("keeps the fragment on the base and tolerates bad escapes", () => {
    expect(parseUrl("/a?x=100%#top")).toEqual({ base: "/a#top", params: [{ key: "x", value: "100%" }] });
  });

  it("keeps template variables intact", () => {
    expect(parseUrl("{{host}}/x?id={{userId}}").params).toEqual([{ key: "id", value: "{{userId}}" }]);
  });
});

describe("composeUrl", () => {
  it("adds only enabled params", () => {
    expect(
      composeUrl("https://x.dev/s", [
        { key: "q", value: "a b" },
        { key: "off", value: "1", enabled: false },
        { key: "flag", value: "" },
      ]),
    ).toBe("https://x.dev/s?q=a b&flag");
  });

  it("escapes characters that would break parsing", () => {
    expect(composeUrl("/s", [{ key: "a=b", value: "x&y#z" }])).toBe("/s?a%3Db=x%26y%23z");
  });

  it("inserts the query before the fragment", () => {
    expect(composeUrl("/s#top", [{ key: "a", value: "1" }])).toBe("/s?a=1#top");
  });

  it("round-trips with parseUrl", () => {
    const query = [
      { key: "q", value: "50% & more" },
      { key: "id", value: "{{id}}" },
    ];
    const parsed = parseUrl(composeUrl("/s", query));
    expect(parsed.params).toEqual(query);
  });
});

describe("applyUrlToRequest", () => {
  it("replaces enabled params and keeps disabled ones in place", () => {
    const result = applyUrlToRequest("/s?a=9&c=3", [
      { key: "a", value: "1" },
      { key: "b", value: "2", enabled: false },
      { key: "old", value: "x" },
    ]);
    expect(result).toEqual({
      url: "/s",
      query: [
        { key: "a", value: "9" },
        { key: "b", value: "2", enabled: false },
        { key: "c", value: "3" },
      ],
    });
  });

  it("drops enabled params removed from the URL and appends new ones", () => {
    expect(applyUrlToRequest("/s", [{ key: "a", value: "1" }])).toEqual({ url: "/s", query: [] });
    expect(applyUrlToRequest("/s?x=1&y=2", [])).toEqual({
      url: "/s",
      query: [
        { key: "x", value: "1" },
        { key: "y", value: "2" },
      ],
    });
  });
});

describe("normalizeUrlAndQuery", () => {
  it("moves a query string from the url into query", () => {
    expect(normalizeUrlAndQuery("/s?a=1", [{ key: "b", value: "2" }])).toEqual({
      url: "/s",
      query: [
        { key: "a", value: "1" },
        { key: "b", value: "2" },
      ],
    });
  });
});
