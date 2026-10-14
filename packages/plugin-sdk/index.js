// @post-tester/plugin-sdk
//
// post-tester has this module built in: when a plugin runs, the import
// "@post-tester/plugin-sdk" is served by post-tester itself, not from
// node_modules. This file exists for editors, bundlers and type checkers, and
// behaves exactly like the built-in module.

const KNOWN_KEYS = ["templateFunctions", "auth", "hooks", "importers", "exporters", "codegen", "viewers", "mcpTools"];

/**
 * Declares a plugin. Returns the definition unchanged after checking its shape,
 * so mistakes are reported when the plugin loads instead of when it is used.
 */
export function definePlugin(definition) {
  if (!definition || typeof definition !== "object") {
    throw new Error("definePlugin() expects an object, e.g. definePlugin({ templateFunctions: { ... } })");
  }
  for (const key of Object.keys(definition)) {
    if (!KNOWN_KEYS.includes(key)) {
      throw new Error(`definePlugin(): unknown key "${key}". Valid keys: ${KNOWN_KEYS.join(", ")}`);
    }
  }
  return definition;
}

export const version = "0.1.0";
