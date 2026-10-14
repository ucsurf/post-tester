# @post-tester/plugin-sdk

Types, JSON Schema and test helpers for writing
[post-tester](../../README.md) plugins.

> **Status: early development.** The API may change before 1.0.

post-tester provides `@post-tester/plugin-sdk` and
`@post-tester/plugin-sdk/testing` to plugins at runtime. This package is only
needed for editor completion and type checking.

| File | Contents |
|---|---|
| `index.d.ts` | `definePlugin` and every plugin API type (documented, with examples) |
| `testing.d.ts` | `testPlugin` for `*.test.ts` files |
| `globals.d.ts` | Sandbox globals: `console`, and `test` / `expect` / `ctx` in tests |
| `plugin.schema.json` | JSON Schema for `plugin.json` |
| `tsconfig.json` | Example config for a plugin folder |

```ts
import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: {
    shout: {
      description: "Returns the text in upper case.",
      args: [{ name: "text", type: "string", description: "Text to convert" }],
      run: ({ text }) => String(text).toUpperCase(),
    },
  },
});
```

In `plugin.json`, add
`"$schema": "./node_modules/@post-tester/plugin-sdk/plugin.schema.json"` for
completion and validation.

Read the **[plugin guide](../../guide/plugins/README.md)** for everything else,
and see [`examples/plugins/`](../../examples/plugins/) for tested examples of
every plugin type.

License: MIT OR Apache-2.0
