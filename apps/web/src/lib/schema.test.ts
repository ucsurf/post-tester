import { describe, expect, it } from "vitest";
import { schemaSkeleton } from "./schema";

describe("schemaSkeleton", () => {
  it("fills every property with a neutral value", () => {
    expect(
      schemaSkeleton({
        type: "object",
        properties: {
          name: { type: "string" },
          age: { type: "integer" },
          score: { type: "number", minimum: 1 },
          active: { type: "boolean" },
          tags: { type: "array", items: { type: "string" } },
          address: { type: "object", properties: { city: { type: "string" } } },
        },
      }),
    ).toEqual({ name: "", age: 0, score: 1, active: false, tags: [""], address: { city: "" } });
  });

  it("prefers default, const, examples and enum", () => {
    expect(
      schemaSkeleton({
        type: "object",
        properties: {
          a: { type: "string", default: "x" },
          b: { const: 3 },
          c: { type: "string", examples: ["sample"] },
          d: { type: "string", enum: ["asc", "desc"] },
        },
      }),
    ).toEqual({ a: "x", b: 3, c: "sample", d: "asc" });
  });

  it("handles nullable type arrays, anyOf and allOf", () => {
    expect(schemaSkeleton({ type: ["null", "string"] })).toBe("");
    expect(schemaSkeleton({ anyOf: [{ type: "null" }, { type: "integer" }] })).toBe(0);
    expect(
      schemaSkeleton({ allOf: [{ type: "object", properties: { a: { type: "string" } } }, { type: "object", properties: { b: { type: "boolean" } } }] }),
    ).toEqual({ a: "", b: false });
  });

  it("resolves local $refs", () => {
    const schema = {
      type: "object",
      properties: { user: { $ref: "#/$defs/User" } },
      $defs: { User: { type: "object", properties: { id: { type: "integer" } } } },
    };
    expect(schemaSkeleton(schema)).toEqual({ user: { id: 0 } });
  });

  it("survives recursive schemas and garbage input", () => {
    const schema = { $ref: "#" };
    expect(() => schemaSkeleton(schema)).not.toThrow();
    expect(schemaSkeleton(undefined)).toBeNull();
    expect(schemaSkeleton({ properties: { a: { type: "string" } } })).toEqual({ a: "" });
    expect(schemaSkeleton({ type: "array" })).toEqual([]);
  });
});
