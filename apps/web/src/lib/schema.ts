/**
 * Builds an example value from a JSON Schema, used to pre-fill the arguments of an
 * MCP tool call. It favours `default`, `const`, `examples` and the first `enum`
 * value, and fills every property so the user only has to edit values.
 */
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

type Schema = Record<string, unknown>;

const MAX_DEPTH = 8;

export function schemaSkeleton(schema: unknown, root: unknown = schema, depth = 0): JsonValue {
  if (!isObject(schema) || depth > MAX_DEPTH) return null;

  if ("default" in schema) return toJson(schema.default);
  if ("const" in schema) return toJson(schema.const);
  if (Array.isArray(schema.examples) && schema.examples.length > 0) return toJson(schema.examples[0]);
  if (Array.isArray(schema.enum) && schema.enum.length > 0) return toJson(schema.enum[0]);

  if (typeof schema.$ref === "string") {
    const target = resolveRef(root, schema.$ref);
    return target ? schemaSkeleton(target, root, depth + 1) : {};
  }

  for (const key of ["oneOf", "anyOf"] as const) {
    const options = schema[key];
    if (Array.isArray(options) && options.length > 0) {
      const preferred = options.find((o) => !(isObject(o) && o.type === "null")) ?? options[0];
      return schemaSkeleton(preferred, root, depth + 1);
    }
  }
  if (Array.isArray(schema.allOf)) {
    const merged: Record<string, JsonValue> = {};
    for (const part of schema.allOf) {
      const value = schemaSkeleton(part, root, depth + 1);
      if (isObject(value)) Object.assign(merged, value);
    }
    return merged;
  }

  switch (pickType(schema)) {
    case "object": {
      const result: Record<string, JsonValue> = {};
      const properties = isObject(schema.properties) ? schema.properties : {};
      for (const [name, property] of Object.entries(properties)) {
        result[name] = schemaSkeleton(property, root, depth + 1);
      }
      return result;
    }
    case "array": {
      if (schema.items === undefined) return [];
      const item = schemaSkeleton(schema.items, root, depth + 1);
      return [item];
    }
    case "string":
      return "";
    case "integer":
    case "number":
      return typeof schema.minimum === "number" ? schema.minimum : 0;
    case "boolean":
      return false;
    case "null":
      return null;
    default:
      return isObject(schema.properties) ? schemaSkeleton({ ...schema, type: "object" }, root, depth + 1) : null;
  }
}

function pickType(schema: Schema): string | undefined {
  const type = schema.type;
  if (typeof type === "string") return type;
  if (Array.isArray(type)) return type.find((t): t is string => typeof t === "string" && t !== "null") ?? "null";
  return undefined;
}

/** Resolves local references such as `#/$defs/User` or `#/definitions/User`. */
function resolveRef(root: unknown, ref: string): unknown {
  if (!ref.startsWith("#")) return undefined;
  let node: unknown = root;
  for (const part of ref.slice(1).split("/").filter(Boolean)) {
    if (!isObject(node)) return undefined;
    node = node[part.replace(/~1/g, "/").replace(/~0/g, "~")];
  }
  return node;
}

function toJson(value: unknown): JsonValue {
  return value === undefined ? null : (JSON.parse(JSON.stringify(value)) as JsonValue);
}

function isObject(value: unknown): value is Schema {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
