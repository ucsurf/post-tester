// Built-in module "@post-tester/plugin-sdk".
// The TypeScript types for this module are published in packages/plugin-sdk.

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
