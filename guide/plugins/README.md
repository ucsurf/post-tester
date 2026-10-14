# Building Plugins

> **Status: early development.** The plugin API may change before 1.0.

This guide explains everything needed to build a post-tester plugin. It is
written for **people and AI agents**: if you follow it step by step, your
plugin should work on the first try.

Reference material:

- [`packages/plugin-sdk/index.d.ts`](../../packages/plugin-sdk/index.d.ts): the complete, documented API types
- [`packages/plugin-sdk/plugin.schema.json`](../../packages/plugin-sdk/plugin.schema.json): JSON Schema for `plugin.json`
- [`examples/plugins/`](../../examples/plugins/): one small, tested plugin for every plugin type

---

## 1. Quick start

```bash
pt plugin new template-function my-plugin   # create a working plugin from a template
cd my-plugin
pt plugin validate                          # check plugin.json and the code
pt plugin test                              # run the plugin's tests
pt plugin install .                         # install into the current workspace
```

`pt plugin new` accepts every [plugin type](#5-plugin-types). The generated
plugin validates and its tests pass, so you can start from a green state.

## 2. Plugin structure

```
my-plugin/
├── plugin.json     # required: what the plugin is, provides and needs
├── index.ts        # required: the code (TypeScript or JavaScript)
├── index.test.ts   # recommended: tests, run by `pt plugin test`
└── README.md       # recommended: what it does and how to use it
```

The code may be split into several files and imported with relative paths
(see [Limits](#10-limits)).

## 3. `plugin.json`

```json
{
  "$schema": "./node_modules/@post-tester/plugin-sdk/plugin.schema.json",
  "name": "totp-generator",
  "displayName": "TOTP Generator",
  "description": "Generates time-based one-time passwords for 2FA logins.",
  "version": "1.0.0",
  "engine": "^0.1.0",
  "main": "index.ts",
  "provides": ["template-function"],
  "permissions": []
}
```

| Field | Required | Rules |
|---|---|---|
| `name` | yes | Unique id: lowercase letters, numbers and `-`, not starting with `-` |
| `displayName` | no | Name shown in the app (defaults to `name`) |
| `description` | yes | One sentence describing what the plugin does; must not be empty |
| `version` | yes | `MAJOR.MINOR.PATCH`, e.g. `1.2.0` (a `-beta.1` or `+build` suffix is allowed) |
| `engine` | yes | post-tester versions the plugin supports, as an npm-style range: `^0.1.0`, `~0.1.0`, `>=0.1.0`, `>=0.1.0 <1.0.0` (all must match), `^0.1.0 \|\| ^1.0.0` (either), `0.1.0` or `*`. For `0.x`, `^` only matches the same minor version. Hyphen ranges (`1.0.0 - 2.0.0`) are not supported |
| `main` | yes | Entry file, relative to the plugin folder (no absolute paths, no `..`) |
| `provides` | yes | At least one [plugin type](#5-plugin-types) |
| `permissions` | yes | [Permissions](#8-permissions) the plugin needs; `[]` for none |
| `$schema` | no | Path to the schema, for editor completion; ignored by post-tester |

Any other field is an error. The JSON Schema above gives editors completion
and validation; `pt plugin validate` performs the same checks and more.

## 4. The entry file

The `main` file must **default-export** the result of `definePlugin`:

```ts
import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: { /* ... */ },
});
```

- `definePlugin` checks the top-level keys when the plugin loads, so a typo
  such as `templateFunction` fails immediately with a list of valid keys.
- Every key must be listed in `provides`, and every `provides` entry must have
  its key. `pt plugin validate` reports any mismatch.
- Every plugin function may be `async`.
- **TypeScript** (`.ts`) is supported: types are removed when the plugin loads,
  but **not checked**. Run `npx tsc` yourself (see [Types](#12-types-and-editor-support)).
- Throw an `Error` with a clear message when something is wrong (say what
  happened and how to fix it). The message is shown in Live Activity together
  with the plugin name and the capability that failed.

## 5. Plugin types

| Type (`provides`) | Key in `definePlugin` | Adds |
|---|---|---|
| `template-function` | `templateFunctions` | Functions usable in `{{ }}` templates |
| `auth` | `auth` | Authentication methods |
| `hook` | `hooks` | Code that runs before every request is sent and after every response arrives |
| `importer` | `importers` | Importing requests from another format |
| `exporter` | `exporters` | Exporting a collection to another format |
| `codegen` | `codegen` | "Copy as code" for a language or library |
| `viewer` | `viewers` | A custom view of a response, as HTML |
| `mcp-tool` | `mcpTools` | Tools that AI agents can call through post-tester's MCP server |

Each section below shows the shape; the linked example is a complete, tested plugin.

### Template function

Example: [`totp-generator`](../../examples/plugins/totp-generator/)

```ts
export default definePlugin({
  templateFunctions: {
    repeat: {
      description: "Repeats a text a number of times.",          // required
      args: [
        { name: "text", type: "string", description: "Text to repeat" },
        { name: "times", type: "number", description: "How often", default: 2 },
      ],
      run: ({ text, times }, ctx) => String(text).repeat(Number(times)),
    },
  },
});
```

Usage in any request field: `{{repeat("ab", 3)}}` or `{{repeat(text: "ab", times: 3)}}`.

- Arguments are JSON values: `"text"` (double quotes), numbers, `true`/`false`,
  arrays and objects. They keep their type (`3` arrives as a number).
- Positional arguments are matched to `args` in order; named arguments by name.
- An argument is required unless it has a `default`, `required: false` or
  `optional: true`. A missing one fails with `missing argument "<name>"`.
- Templates inside arguments are not expanded: `{{fn("{{var}}")}}` passes the
  text `{{var}}`.
- The return value is inserted as-is if it is a string, as an empty string if
  it is `null`/`undefined`, and as JSON otherwise.
- Function names are global: use a distinctive name made of letters, numbers
  and `_`. If two plugins define the same name, the first loaded wins.

### Auth

Example: [`hmac-signature`](../../examples/plugins/hmac-signature/)

```ts
export default definePlugin({
  auth: {
    "custom-token": {
      displayName: "Custom token",
      fields: [{ name: "token", type: "secret", label: "Token" }],
      apply: (request, config, ctx) => {
        request.headers.set("X-Custom-Token", String(config.token));
        return request;
      },
    },
  },
});
```

A request (or a folder's `_folder.yaml`) selects it with:

```yaml
auth:
  type: plugin
  name: my-plugin/custom-token        # "<plugin name>/<method>"; just "custom-token" if no other plugin has it
  config:
    token: "{{secret:my-token}}"      # string values are expanded before apply() runs
```

`apply` receives the fully prepared [request](#6-the-request-object) (URL with
query parameters, headers, body) after variables and built-in settings have
been applied. Change it in place and return it.

### Hook

Example: [`request-id`](../../examples/plugins/request-id/)

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

- Only `beforeRequest` and `afterResponse` exist; any other name is reported by `pt plugin validate`.
- Hooks run for **every** request, in plugin load order. `beforeRequest` runs
  after auth is applied; `afterResponse` runs before tests and after-response
  scripts, so they see its changes.
- Return the (changed) object, or nothing to keep the changes made in place.
  Throwing stops the request and shows the error.

### Importer

Example: [`har-importer`](../../examples/plugins/har-importer/)

```ts
export default definePlugin({
  importers: {
    "url-list": {
      displayName: "URL list",
      fileExtensions: [".txt"],
      detect: (text) => text.trim().split("\n").every((line) => line.startsWith("http")),
      import: (text, ctx) => ({
        name: "URL list",
        requests: text.trim().split("\n").map((url) => ({ method: "GET", url: url.trim() })),
      }),
    },
  },
});
```

- When importing, post-tester calls each importer's `detect(text)` and uses
  the first that returns `true`. `detect` should be fast and never throw (an
  error counts as "no"). An importer without `detect` is never picked automatically.
- `import(text)` returns `{ name, requests }`. Each request uses the
  workspace's request format (the same fields as a request YAML file), plus an
  optional `folder` such as `"users/admin"`:

  ```ts
  {
    folder: "users",
    name: "Create user",
    method: "POST",
    url: "https://api.example.com/users",
    query: [{ key: "notify", value: "true" }],
    headers: [{ key: "Accept", value: "application/json" }],
    body: { type: "json", content: "{\"name\": \"Ada\"}" },
    tests: ["status == 201"],
  }
  ```

  Body types: `{ type: "json", content }`, `{ type: "text", content, content_type }`,
  `{ type: "form", fields: [{ key, value }] }`,
  `{ type: "graphql", query, variables?, operation_name? }`.
  **Unknown fields are rejected**, so check names against `ImportedRequest` in
  `index.d.ts` (note the snake_case in `content_type`, `scripts` and `options`).

### Exporter

Example: [`markdown-exporter`](../../examples/plugins/markdown-exporter/)

```ts
export default definePlugin({
  exporters: {
    markdown: {
      displayName: "Markdown",
      fileExtension: ".md",
      export: (collection, ctx) =>
        `# ${collection.name}\n\n` + collection.requests.map((r) => `- ${r.method ?? "GET"} ${r.url}`).join("\n"),
    },
  },
});
```

`collection` is `{ name, requests }`, where each request is in the
workspace's request format with templates such as `{{baseUrl}}` kept as
written, plus its `id` (e.g. `auth/login`). Treat fields other than `url` as
optional. Run an exporter with `pt export [folder] --to <plugin>/<exporter>`.

### Codegen

Example: [`go-net-http`](../../examples/plugins/go-net-http/)

```ts
export default definePlugin({
  codegen: {
    httpie: {
      displayName: "HTTPie",
      language: "shell",
      generate: (request, ctx) => {
        const headers = request.headers.entries().map(([k, v]) => ` '${k}:${v}'`).join("");
        return `http ${request.method} '${request.url}'${headers}`;
      },
    },
  },
});
```

`request` is the fully prepared request, so it may contain secrets (for
example an `Authorization` header). Do not log it.

### Viewer

Example: [`json-table`](../../examples/plugins/json-table/)

```ts
const escape = (text: string) =>
  text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

export default definePlugin({
  viewers: {
    status: {
      displayName: "Status",
      contentTypes: ["application/json"],
      render: (response, ctx) => `<h1>${response.status}</h1><pre>${escape(response.body)}</pre>`,
    },
  },
});
```

`render` returns an HTML fragment. **Escape every value taken from the
response**: it comes from a server you may not control.

### MCP tool

Example: [`jwt-tools`](../../examples/plugins/jwt-tools/)

```ts
export default definePlugin({
  mcpTools: {
    create_ticket: {
      description: "Creates a ticket in the issue tracker. Returns the new ticket's id.",   // required
      inputSchema: {
        type: "object",
        properties: { title: { type: "string", description: "One-line summary" } },
        required: ["title"],
      },
      run: async ({ title }, ctx) => {
        if (typeof title !== "string" || !title) throw new Error('"title" is required');
        const res = await ctx.http.post("https://tracker.example.com/api/tickets", { json: { title } });
        if (res.status !== 201) throw new Error(`the tracker answered ${res.status}: ${res.body}`);
        return { id: res.json().id };
      },
    },
  },
});
```

This one needs the permission `network:tracker.example.com`.

How agents see plugin tools:

- Each tool is exposed by post-tester's MCP server as
  **`<plugin_name>__<tool>`**: the plugin name with `-` replaced by `_`, two
  underscores, then the tool's key. `create_ticket` in plugin `issue-tracker`
  becomes `issue_tracker__create_ticket`. Tool keys may only contain letters,
  digits, `_` and `-`, and the full name must be at most 64 characters.
- Results longer than 256 KB are cut before they reach the agent.
- When post-tester's MCP server runs read-only, tools of plugins that declare
  `fs:write` are hidden from agents.
- Agents decide when to call a tool from its `description`, and fill the input
  from `inputSchema` (default `{ "type": "object" }`). Make both precise.
- The input is **not** validated against the schema before `run`; check
  required values yourself and throw a clear error.
- `run` may return any JSON-compatible value (`undefined` becomes `null`).
- Every call, and everything the tool does through `ctx`, appears in Live Activity.

## 6. The request object

Auth methods, `beforeRequest` hooks and code generators receive a request:

| Property | Type | Description |
|---|---|---|
| `method` | `string` | Upper-case method, e.g. `"POST"` |
| `url` | `string` | Full URL including the query string, variables expanded |
| `headers` | `Headers` | The headers (see below) |
| `body` | `string` | The body as text (`""` if none) |

Change it in place (`request.url = ...`, `request.headers.set(...)`) and return
it. You may instead return a new object `{ method, url, headers, body }`, where
`headers` can be any of the forms accepted by `Headers` and a non-string
`body` is sent as JSON.

`Headers` is ordered and case-insensitive. It is not the browser's `Headers` class:

| Method | Description |
|---|---|
| `get(name)` | First value of the header, or `undefined` |
| `has(name)` | Whether the header exists |
| `set(name, value)` | Replace the header, or add it |
| `add(name, value)` / `add({ key, value })` | Add a header, even if one with that name exists |
| `upsert({ key, value })` | Same as `set` (Postman style) |
| `delete(name)` / `remove(name)` | Remove every header with that name |
| `entries()` | All headers as `[name, value]` pairs |
| `toObject()` | All headers as an object (last value wins) |

Headers can be created from `[[name, value]]`, `[{ key, value }]` or `{ name: value }`.

## 7. The response object and `ctx`

### Response

`afterResponse` hooks, viewers and `ctx.http` receive a response:

| Property | Type | Description |
|---|---|---|
| `status` | `number` | Status code, e.g. `200` |
| `statusText` | `string` | Reason phrase, e.g. `"OK"` (may be empty) |
| `headers` | `Headers` | Response headers |
| `body` | `string` | Body as text (decompressed) |
| `timeMs` | `number` | Total time in milliseconds |
| `url` | `string` | The final URL, after redirects (`ctx.http` responses only) |
| `text()` | `string` | The body |
| `json()` | `any` | The body parsed as JSON; throws `the response body is not valid JSON: ...` |

### `ctx`

Every plugin function receives `ctx` as its last argument. Plugins have no
direct access to the network, files or system; everything goes through `ctx`
and appears in Live Activity.

| API | Purpose | Permission |
|---|---|---|
| `ctx.log.debug / info / warn / error(...args)` | Write to Live Activity | none |
| `ctx.progress(done, total)` | Show progress of a long operation | none |
| `ctx.random.uuid()` | A random UUID v4 | none |
| `ctx.random.int(min = 0, max = 1000)` | A random integer, both ends inclusive | none |
| `ctx.crypto.sha256(text)` | SHA-256 as hex | none |
| `ctx.crypto.hmacSha256(key, text)` | HMAC-SHA256 as hex | none |
| `ctx.crypto.base64Encode(text)` / `base64Decode(text)` | Standard base64 of UTF-8 text | none |
| `ctx.variables.get(name)` | A variable's value, or `undefined` | none |
| `ctx.variables.set(name, value)` | Set a session variable, usable as `{{name}}` by later requests | none |
| `ctx.http.request(options)` / `get / post / put / patch / delete(url, options?)` | Send an HTTP request; returns a response | `network:<host>` |
| `await ctx.secrets.get(name)` | A secret's value, or `undefined` | `secrets:read` |
| `ctx.fs.read(path)` | Read a text file in the workspace | `fs:read` |
| `ctx.fs.write(path, content)` | Create or replace a text file in the workspace | `fs:write` |

Details:

- `ctx.http` options: `{ method, url, headers, body, json, timeoutMs, followRedirects }`.
  `json` sends a JSON body and sets `Content-Type: application/json`. Up to 10
  redirects are followed, and each redirect target needs a `network:`
  permission too; pass `followRedirects: false` to get the 3xx response
  itself. Cookies are shared with the app. A non-2xx status does **not**
  throw; check `response.status`.
- `ctx.variables.get` sees session variables, then the default environment,
  then workspace variables. Session variables last until post-tester restarts
  and are not written to files.
- `ctx.fs` paths are relative to the workspace folder; absolute paths, `..`
  and symbolic links leading outside it are refused. Both functions are synchronous.
- Log messages longer than 16 KB are cut.
- `ctx.crypto` works on text (UTF-8). For other algorithms, such as
  HMAC-SHA1, implement them in the plugin (see `totp-generator`).

## 8. Permissions

Declare only what the plugin uses. Every permission must be **declared** in
`plugin.json` and **approved** by the user before it takes effect.

| Permission | Allows |
|---|---|
| `network:<host>` | HTTP requests to one host, e.g. `network:api.github.com` |
| `network:*.<domain>` | Requests to every sub-domain, e.g. `network:*.example.com` (does **not** include `example.com` itself; declare both if needed) |
| `network:*` | Requests to any host (avoid; users are warned) |
| `secrets:read` | `ctx.secrets.get` |
| `fs:read` | `ctx.fs.read` |
| `fs:write` | `ctx.fs.write` |

- A network permission is a **host name only**: no scheme, port or path.
  `network:https://api.github.com` or `network:localhost:8080` are rejected by
  `pt plugin validate`. A permission for a host covers every port.
- Approve declared permissions with `pt plugin approve <name>` (or in the
  app's Plugins panel). Until then they are listed as pending, and the plugin
  still loads.
- Using an API without permission throws an error that says what to do.
  Not declared, then declared but not approved:

  ```
  permission denied: my-plugin did not declare `network:api.github.com`. Add "network:api.github.com" to "permissions" in plugin.json
  permission `network:api.github.com` was not approved for my-plugin. Approve it with `pt plugin approve my-plugin` or in the app's Plugins panel
  ```

## 9. Testing

Put tests in `*.test.ts` or `*.test.js` files next to `plugin.json` and run
`pt plugin test`. `testPlugin` calls the plugin exactly like post-tester does
(same argument handling, defaults and validation):

```ts
import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("repeat uses the default count", async () => {
  const t = testPlugin(plugin);
  expect(await t.templateFunction("repeat", { text: "ab" })).toBe("abab");
});

test("a missing argument is reported", async () => {
  let error: Error | undefined;
  try { await testPlugin(plugin).templateFunction("repeat", {}); } catch (e) { error = e as Error; }
  expect(error?.message).toContain('missing argument "text"');
});
```

`testPlugin(plugin)` methods (all return promises):

| Method | Returns |
|---|---|
| `templateFunction(name, args?)` | The inserted text |
| `auth(name, request?, config?)` | The request: `{ method, url, headers: [[name, value]], body }` |
| `beforeRequest(request?)` | The request, as above |
| `afterResponse(response?)` | `{ status, status_text, headers: [[name, value]], body, time_ms }` |
| `detect(name, text)` | `true` / `false` |
| `importer(name, text)` | `{ name, requests }` (headers as `{ key, value }`) |
| `exporter(name, collection)` | The exported text |
| `codegen(name, request?)` | The code |
| `viewer(name, response?)` | The HTML |
| `mcpTool(name, input?)` | The tool's result |
| `describe()` | What the plugin provides, as the app and agents see it |

Missing request fields default to `GET https://example.com/`, no headers,
empty body; missing response fields to `200 OK`, no headers, empty body,
`timeMs: 1`. Results contain plain data: use `Object.fromEntries(request.headers)`
to look up headers.

Globals in test files: `test(name, fn)` (alias `it`), `describe(name, fn)`,
`expect(value)` and `ctx`. Matchers (each also available as `.not.<matcher>`):

`toBe`, `toEqual`, `toMatch(regex | string)`, `toContain`, `toBeTruthy`,
`toBeFalsy`, `toBeDefined`, `toBeUndefined`, `toBeNull`, `toBeGreaterThan`,
`toBeGreaterThanOrEqual`, `toBeLessThan`, `toBeLessThanOrEqual`,
`toHaveLength`, `toHaveProperty(key, value?)`, `toThrow(regex | string?)`.

- `toThrow` only works with synchronous functions; for async code use
  `try`/`catch` as above. There is no `.resolves` / `.rejects`.
- Tests run one after another. Each test file runs in a fresh sandbox.
- During tests every declared permission is granted, and `ctx.fs` uses the
  plugin folder as its root. Avoid real network calls in tests.

## 10. Limits

Plugins run in a sandboxed JavaScript engine (QuickJS), separately for each plugin:

| Limit | Value |
|---|---|
| Memory | 64 MB per plugin |
| Time | 30 s per call (template function, hook, import, tool call, ...); longer calls are stopped |
| Concurrency | One call at a time per plugin |
| Data | `ctx.fs.read` files and `ctx.http` response bodies up to 16 MB |
| Imports | Relative paths inside the plugin folder (`./util`, `./util.ts`, `./lib/index.ts`) and `@post-tester/plugin-sdk` / `@post-tester/plugin-sdk/testing` |

- **npm packages cannot be imported directly.** Bundle them into your plugin
  first (for example with esbuild into a single `.js` file) and point `main`
  at the bundle.
- Available: standard JavaScript built-ins (`JSON`, `Math`, `Date`, `RegExp`,
  `Map`, `Promise`, `BigInt`, ...), `atob` / `btoa`, `queueMicrotask` and `console`.
- Each plugin has its own engine: plugins cannot see or change each other's
  globals. The globals post-tester provides (`ctx`, `console`) are read-only.
- Stack traces in errors point at lines in your `.ts` files, even though they
  are converted to JavaScript before running.
- Not available: `fetch`, `URL`, `URLSearchParams`, `TextEncoder` /
  `TextDecoder`, `setTimeout`, `Intl`, `crypto`, `require`, `process`. Use `ctx` instead.

## 11. CLI commands

| Command | What it does |
|---|---|
| `pt plugin new <type> <name>` | Create a working plugin of a type from a template |
| `pt plugin validate [dir]` | Check `plugin.json` and the code; prints every problem with a fix |
| `pt plugin test [dir]` | Run the plugin's `*.test.ts` / `*.test.js` files |
| `pt plugin install <dir>` | Install a plugin folder into the current workspace |
| `pt plugin approve <name>` | Approve the permissions a plugin declares |
| `pt plugin list` | List installed plugins, what they provide and pending permissions |

`[dir]` defaults to the current folder. `pt plugin validate` reports problems as
`location: message` plus `fix: ...`, for example:

```
plugin.json: provides: definePlugin has `hooks`, but provides does not list "hook"
  fix: add "hook" to provides
```

It checks: every `plugin.json` rule above, that `main` exists and loads
(syntax errors are reported with file, line and column), the default export,
`provides` against the `definePlugin` keys, hook names, and that every template
function and MCP tool has a `description`.

## 12. Types and editor support

The npm package `@post-tester/plugin-sdk` (in `packages/plugin-sdk` of the
post-tester repository) contains the TypeScript types, the JSON Schema and an
example `tsconfig.json`. It is only needed for your editor and type checker:
at runtime, post-tester provides both SDK modules itself.

- Copy `tsconfig.json` from the package into your plugin folder. It enables
  strict checking and the sandbox globals (`"types": ["@post-tester/plugin-sdk/globals"]`).
- Import types with `import type { PluginRequest } from "@post-tester/plugin-sdk"`.
- Every type and member in `index.d.ts` is documented, with examples.

## 13. Checklist (for people and AI agents)

Before sharing a plugin, confirm:

- [ ] `pt plugin validate` reports no problems
- [ ] Every type in `provides` has its key in `definePlugin`, and vice versa
- [ ] Only permissions that are actually used are declared; network permissions are host names only
- [ ] Every template function and MCP tool has a `description`; arguments, fields and schema properties are described
- [ ] Errors are thrown with a clear message: what went wrong and how to fix it
- [ ] Viewers escape every value taken from the response
- [ ] Nothing imports npm packages directly (bundle them), and no code relies on `fetch`, `URL`, `setTimeout` or Node.js APIs
- [ ] Tests exist, need no network, and `pt plugin test` passes
- [ ] `npx tsc` passes (for TypeScript plugins)
- [ ] `README.md` explains what the plugin does and shows one usage example

## 14. Installing and sharing

```bash
pt plugin install ./my-plugin      # adds the folder to the current workspace
pt plugin approve my-plugin        # approve its declared permissions, if any
pt plugin list                     # check that it loaded
```

Installed plugin folders are listed in the workspace's `post-tester.yaml`
(`plugins`), and approvals are saved there too (`approved_permissions`), so a
team that shares the workspace through Git shares its plugins. To share a
plugin on its own, share its folder (for example as a Git repository); others
install it with `pt plugin install <dir>`.
