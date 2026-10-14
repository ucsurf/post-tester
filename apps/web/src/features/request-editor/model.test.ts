import { describe, expect, it } from "vitest";
import { addExample, authChoice, authForChoice, convertBody, newRequest, normalizeRequest, requestExamples, requestsEqual } from "./model";

describe("normalizeRequest", () => {
  it("fills the fields the server omits", () => {
    const request = normalizeRequest({ method: "post", url: "{{baseUrl}}/users" });
    expect(request).toEqual({
      name: "",
      description: "",
      method: "POST",
      url: "{{baseUrl}}/users",
      query: [],
      headers: [],
      body: { type: "none" },
      scripts: { before_request: "", after_response: "" },
      tests: [],
      extract: {},
      options: { timeout_ms: 30_000, follow_redirects: true, max_redirects: 10, verify_tls: true, http_version: "auto" },
    });
    expect("auth" in request).toBe(false);
  });

  it("moves a query string from the url into query and keeps partial options", () => {
    const request = normalizeRequest({ url: "/s?a=1", options: { timeout_ms: 5 } as never });
    expect(request.url).toBe("/s");
    expect(request.query).toEqual([{ key: "a", value: "1" }]);
    expect(request.options.timeout_ms).toBe(5);
    expect(request.options.verify_tls).toBe(true);
  });
});

describe("unknown fields", () => {
  it("keeps fields the editor does not know (e.g. examples) through normalize", () => {
    const input = { url: "/a", examples: [{ name: "ok", status: 200, headers: [], body: "{}" }], future: 1 } as never;
    const request = normalizeRequest(input);
    expect(requestExamples(request)).toHaveLength(1);
    expect((request as unknown as { future: number }).future).toBe(1);
  });

  it("appends examples without dropping existing ones", () => {
    const one = addExample(newRequest(), { name: "a", status: 200, headers: [], body: "" });
    const two = addExample(one, { name: "b", status: 404, headers: [{ key: "x", value: "1" }], body: "nope" });
    expect(requestExamples(two).map((e) => e.name)).toEqual(["a", "b"]);
  });
});

describe("requestsEqual", () => {
  it("ignores key order", () => {
    const a = newRequest({ extract: { a: "x", b: "y" } });
    const b = newRequest({ extract: { b: "y", a: "x" } });
    expect(requestsEqual(a, b)).toBe(true);
    expect(requestsEqual(a, { ...a, url: "/other" })).toBe(false);
  });
});

describe("body and auth helpers", () => {
  it("carries text across body types", () => {
    expect(convertBody({ type: "json", content: "{}" }, "text")).toEqual({ type: "text", content: "{}", content_type: "text/plain" });
    expect(convertBody({ type: "text", content: "q", content_type: "x" }, "graphql")).toEqual({ type: "graphql", query: "q", variables: "" });
    expect(convertBody({ type: "json", content: "{}" }, "none")).toEqual({ type: "none" });
  });

  it("maps auth to a choice and back", () => {
    expect(authChoice(undefined)).toBe("inherit");
    expect(authChoice({ type: "plugin", name: "hmac/signature", config: {} })).toBe("plugin:hmac/signature");
    expect(authForChoice("bearer", undefined)).toEqual({ type: "bearer", token: "" });
    expect(authForChoice("plugin:x/y", undefined)).toEqual({ type: "plugin", name: "x/y", config: {} });
    const current = { type: "basic", username: "u", password: "p" } as const;
    expect(authForChoice("basic", current)).toBe(current);
  });
});
