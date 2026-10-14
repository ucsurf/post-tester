# Building Plugins

> **Status: draft.** The plugin API is not stable yet and may change before 1.0.

This guide explains everything needed to build a post-tester plugin. It is
written for **people and AI agents**: if you follow it step by step, your
plugin should work on the first try.

---

## 1. Quick start

```bash
pt plugin new template-function my-plugin   # create from a template
cd my-plugin
pt plugin validate                          # check plugin.json and exports
pt plugin test                              # run the plugin's tests
pt plugin install .                         # install into the current workspace
```

## 2. Plugin structure

```
my-plugin/
├── plugin.json     # required: what the plugin is, provides and needs
├── index.ts        # required: the code (TypeScript or JavaScript)
├── index.test.ts   # recommended: tests
└── README.md       # recommended: what it does and how to use it
```

## 3. `plugin.json`

```json
{
  "$schema": "./node_modules/@post-tester/plugin-sdk/plugin.schema.json",
  "name": "totp-generator",
  "displayName": "TOTP Generator",
  "description": "Generates time-based one-time passwords for 2FA logins.",
  "version": "1.0.0",
  "engine": "^1.0.0",
  "main": "index.ts",
  "provides": ["template-function"],
  "permissions": []
}
```

| Field | Required | Description |
|---|---|---|
| `name` | yes | Unique id: lowercase letters, numbers and `-` |
| `displayName` | no | Name shown in the app |
| `description` | yes | One sentence describing what the plugin does |
| `version` | yes | Semantic version, e.g. `1.2.0` |
| `engine` | yes | Compatible post-tester versions (semver range) |
| `main` | yes | Entry file |
| `provides` | yes | List of [plugin types](#4-plugin-types) this plugin implements |
| `permissions` | yes | List of [permissions](#6-permissions) it needs (can be empty) |

## 4. Plugin types

Every plugin exports `definePlugin({...})`. Each key adds one capability, and
must also be listed in `provides`.

| Type (`provides`) | Key in `definePlugin` | Adds |
|---|---|---|
| `template-function` | `templateFunctions` | Functions usable in `{{ }}` templates |
| `auth` | `auth` | A new authentication method |
| `hook` | `hooks` | Code that runs before a request is sent or after a response arrives |
| `importer` | `importers` | Import collections from another format |
| `exporter` | `exporters` | Export collections to another format |
| `codegen` | `codegen` | "Copy as code" for a language or library |
| `protocol` | `protocols` | A new protocol |
| `viewer` | `viewers` | A custom way to display responses |
| `panel` | `panels` | A custom UI panel |
| `mcp-tool` | `mcpTools` | A tool that AI agents can call through post-tester's MCP server |

### Template function

```ts
import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: {
    totp: {
      description: "Generate a 6-digit TOTP code from a base32 secret.",
      args: [{ name: "secret", type: "string", description: "Base32 secret" }],
      run: ({ secret }, ctx) => {
        ctx.log.info("Generating TOTP code");
        return generateTotp(secret);
      },
    },
  },
});
```

Usage in a request: `{{totp(secret:"authenticator")}}`

### Auth

```ts
export default definePlugin({
  auth: {
    "api-key-signature": {
      displayName: "API Key + Signature",
      fields: [
        { name: "keyId", type: "string", label: "Key ID" },
        { name: "secret", type: "secret", label: "Signing secret" },
      ],
      apply: async (request, { keyId, secret }, ctx) => {
        const signature = await ctx.crypto.hmacSha256(secret, request.body ?? "");
        request.headers.set("X-Key-Id", keyId);
        request.headers.set("X-Signature", signature);
        return request;
      },
    },
  },
});
```

### Hook

```ts
export default definePlugin({
  hooks: {
    beforeRequest: (request, ctx) => {
      request.headers.set("X-Request-Id", ctx.random.uuid());
      return request;
    },
    afterResponse: (response, ctx) => {
      if (response.status >= 500) ctx.log.warn(`Server error: ${response.status}`);
      return response;
    },
  },
});
```

### Importer

```ts
export default definePlugin({
  importers: {
    "my-format": {
      displayName: "My Format",
      fileExtensions: [".myf"],
      detect: (text) => text.startsWith("MYFORMAT"),
      import: (text, ctx) => ({
        name: "Imported collection",
        requests: parseMyFormat(text),   // returns Request[]
      }),
    },
  },
});
```

### Codegen

```ts
export default definePlugin({
  codegen: {
    "python-httpx": {
      displayName: "Python (httpx)",
      language: "python",
      generate: (request) =>
        `import httpx\n\nhttpx.request(${JSON.stringify(request.method)}, ${JSON.stringify(request.url)})`,
    },
  },
});
```

### MCP tool

```ts
export default definePlugin({
  mcpTools: {
    create_ticket: {
      description: "Create a ticket in the issue tracker.",
      inputSchema: {
        type: "object",
        properties: { title: { type: "string" } },
        required: ["title"],
      },
      run: async ({ title }, ctx) => {
        const res = await ctx.http.post("https://tracker.example.com/api/tickets", {
          json: { title },
        });
        return { id: res.json().id };
      },
    },
  },
});
```

Requires the permission `network:tracker.example.com`.

## 5. The `ctx` object

Plugins run in a sandbox. They have no direct access to the network, files or
system. Everything goes through `ctx`:

| API | Purpose | Permission needed |
|---|---|---|
| `ctx.log.info / warn / error` | Write to Live Activity | none |
| `ctx.progress(done, total)` | Show progress in Live Activity | none |
| `ctx.random.uuid()` | Random values | none |
| `ctx.crypto.*` | Hashing, HMAC, encoding | none |
| `ctx.variables.get / set` | Read and write environment variables | none |
| `ctx.http.get / post / request` | Make HTTP requests | `network:<host>` |
| `ctx.secrets.get(name)` | Read a secret | `secrets:read` |
| `ctx.fs.read / write` | Read and write files in the workspace | `fs:read` / `fs:write` |

Everything a plugin does through `ctx` is shown live in Live Activity.

## 6. Permissions

Declare only what you need. Users approve permissions when they install a plugin.

| Permission | Allows |
|---|---|
| `network:<host>` | HTTP requests to one host, e.g. `network:api.github.com` |
| `network:*` | HTTP requests to any host (avoid; users are warned) |
| `secrets:read` | Reading secrets |
| `fs:read` | Reading files inside the workspace |
| `fs:write` | Writing files inside the workspace |

## 7. Testing

```ts
import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index";

test("totp returns 6 digits", async () => {
  const t = testPlugin(plugin);
  const code = await t.templateFunction("totp", { secret: "JBSWY3DPEHPK3PXP" });
  expect(code).toMatch(/^\d{6}$/);
});
```

Run with `pt plugin test`.

## 8. Checklist (for people and AI agents)

Before publishing, confirm:

- [ ] `plugin.json` passes `pt plugin validate`
- [ ] Every type in `provides` has a matching key in `definePlugin`
- [ ] Every `ctx` API used has its permission declared, and nothing more
- [ ] Every function, argument and field has a `description`
- [ ] Errors are thrown with a clear message: what went wrong and how to fix it
- [ ] Tests exist and `pt plugin test` passes
- [ ] `README.md` explains what the plugin does and shows one usage example

## 9. Installing and sharing

```bash
pt plugin install ./my-plugin                      # local folder
pt plugin install git+https://github.com/me/plugin # Git repository
pt plugin install @me/post-tester-plugin           # npm package
```
