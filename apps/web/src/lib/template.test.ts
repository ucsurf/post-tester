import { describe, expect, it } from "vitest";
import { completionContext, findTemplates, usedVariables } from "./template";

describe("findTemplates", () => {
  it("finds variables, secrets and functions with their ranges", () => {
    const text = "{{baseUrl}}/x?k={{ secret:apiKey }}&id={{uuid()}}";
    expect(findTemplates(text)).toEqual([
      { from: 0, to: 11, name: "baseUrl", kind: "variable" },
      { from: 16, to: 35, name: "apiKey", kind: "secret" },
      { from: 39, to: 49, name: "uuid", kind: "function" },
    ]);
  });

  it("ignores unfinished templates", () => {
    expect(findTemplates("{{open")).toEqual([]);
  });
});

describe("completionContext", () => {
  it("returns the typed prefix after {{", () => {
    expect(completionContext("{{bas", 5)).toEqual({ from: 2, query: "bas" });
    expect(completionContext("x/{{ to", 7)).toEqual({ from: 5, query: "to" });
    expect(completionContext("{{", 2)).toEqual({ from: 2, query: "" });
  });

  it("returns null outside a template", () => {
    expect(completionContext("{{a}}/b", 7)).toBeNull();
    expect(completionContext("plain", 5)).toBeNull();
    expect(completionContext("{{a b", 5)).toBeNull();
  });
});

describe("usedVariables", () => {
  it("lists plain variables only", () => {
    expect(usedVariables("{{a}}/{{secret:b}}/{{c}}/{{now()}}")).toEqual(["a", "c"]);
  });
});
