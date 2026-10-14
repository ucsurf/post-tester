# post-tester local server API

The local server (`pt serve`) exposes the engine over HTTP and WebSocket. The web
UI and the desktop app use this API; it is also usable from scripts.

All TypeScript types referenced below are generated from Rust into
`packages/bindings/src/*.ts` (run `cargo test` to regenerate).

## Security

- The server listens on `127.0.0.1` only, unless `pt serve --host` is given.
- Every `/api` and `/mcp` request needs the session token printed at startup:
  - HTTP: header `Authorization: Bearer <token>`, or query `?token=<token>`
  - WebSocket: query `?token=<token>` (browsers cannot set headers on WebSockets)
- The `Host` header must name this server (blocks DNS rebinding):
  `localhost:<port>` or a loopback address such as `127.0.0.1:<port>` / `[::1]:<port>`.
  With `--host <address>`, that address is accepted too; with `--host 0.0.0.0` (or `::`),
  any IP address is. Host names other than `localhost` are always rejected.
- If an `Origin` header is present it must be `http://` plus an accepted host. There is
  no CORS: pages on other origins can neither call the API nor read its responses.
- `/mcp` follows exactly the same rules.
- Every response carries `Referrer-Policy: no-referrer`, `X-Content-Type-Options: nosniff`
  and `X-Frame-Options: DENY`. Blobs (`/api/blobs/{id}`) are served with
  `Content-Security-Policy: sandbox`, so a tested HTML body never runs on this origin.
- The UI is opened as `http://127.0.0.1:<port>/?token=<token>`; it reads the token
  from the URL, keeps it in `sessionStorage`, and removes it from the address bar.

## Conventions

- JSON in, JSON out. Field names are exactly as in the generated types.
- Errors: HTTP 4xx/5xx with body `{ "error": "message that explains how to fix it" }`.
  This includes malformed JSON bodies (`400`/`422`), unknown `/api/…` paths (`404`),
  wrong methods (`405`) and bodies that are too large (`413`).
- Request bodies are limited to 32 MB (128 MB for `/api/import`).
- `{*id}` path parameters are request/folder ids such as `auth/login` (they contain `/`).
  Encode each segment (`encodeURIComponent`). Ids that could leave the workspace
  (`..`, absolute paths, `\`, hidden segments) are rejected with `400`.

## Endpoints

### Workspace

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `/api/info` | | `Info` (below) |
| GET | `/api/tree` | | `Folder` |
| GET | `/api/requests/{*id}` | | `Request` |
| PUT | `/api/requests/{*id}` | `Request` | `204` |
| DELETE | `/api/requests/{*id}` | | `204` |
| GET | `/api/folders/settings/{*id}` (`id` may be empty: `/api/folders/settings/`) | | `FolderSettings` |
| PUT | `/api/folders/settings/{*id}` | `FolderSettings` | `204` |
| GET | `/api/environments` | | `string[]` |
| GET | `/api/environments/{name}` | | `Environment` |
| PUT | `/api/environments/{name}` | `Environment` | `204` |
| DELETE | `/api/environments/{name}` | | `204` |
| GET | `/api/variables?environment=<name>` | | `{ name: string, value: string }[]` (resolved, runtime included) |
| DELETE | `/api/variables/runtime` | | `204` (clears session variables) |
| PUT | `/api/settings/default-environment` | `{ environment: string \| null }` | `204` |

```ts
type Info = {
  version: string;
  workspace: { root: string; name: string } | null;
  environments: string[];
  defaultEnvironment: string | null;
  secretStore: string;          // e.g. "OS keychain"
  mcpUrl: string;               // e.g. "http://127.0.0.1:7777/mcp", using the host the caller used
};
```

### Running

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/api/send` | `SendBody` | `ExecutionView` |
| POST | `/api/run` | `RunBody` | `RunReport` |
| POST | `/api/load-test` | `LoadBody` | `LoadReport` |
| POST | `/api/cancel/{spanId}` | | `{ cancelled: boolean }` |
| POST | `/api/preview` | `SendBody` | `PreparedView` (what would be sent; secrets shown as `•••`) |

```ts
type SendBody = { request: Request; requestId?: string; environment?: string | null };

type ExecutionView = {
  spanId: string;
  outcome: Outcome;
  error: string | null;
  request: PreparedView | null;
  response: ResponseView | null;
  tests: AssertionResult[];
  extracted: [string, string][];
};
type PreparedView = { method: string; url: string; headers: [string, string][]; body: string };
type ResponseView = {
  status: number; statusText: string; httpVersion: string;
  headers: [string, string][];
  body: Payload;               // preview + blob id for the full body
  size: number;                // decoded body bytes
  wireSize: number;            // bytes on the wire
  timeMs: number;
  timings: { dnsMs: number; connectMs: number; tlsMs: number | null; waitingMs: number; downloadMs: number };
  remoteAddr: string; url: string; redirects: number;
};

type RunBody = {
  target: { folder: string } | { requests: string[] };
  environment?: string | null; bail?: boolean; iterations?: number; delayMs?: number;
};
// RunReport: { span_id, name, outcome, items: RunItem[], duration_ms }  (snake_case, from Rust)

type LoadBody = {
  request: Request; requestId?: string; environment?: string | null; concurrency: number; requests: number;
  connections?: "reuse" | "fresh";   // default "reuse"; "fresh" opens a new connection per request
};
// LoadReport: { span_id, requests, succeeded, failed, duration_ms, requests_per_second, min_ms, mean_ms, p50_ms, p90_ms, p99_ms, max_ms, status_codes }
```

### Live events

| Method | Path | Response |
|---|---|---|
| WS | `/api/live?token=…` | Server sends JSON **arrays** of `Event` (batched about every 50 ms, at most 1000 per message). The first messages contain the recent history (up to 5000 events, `[]` when there is none). |
| GET | `/api/events?since=<seq>` | `Event[]` newer than `seq`, from the in-memory history |
| GET | `/api/blobs/{blobId}` | Raw bytes with the original `Content-Type` (404 if evicted) |

A client that reads too slowly to keep up is sent the events it missed again,
from history. Clients should therefore ignore events whose `seq` they already
have. Events are not guaranteed to arrive in `seq` order across messages; sort
by `seq` when order matters. When the server stops, it closes the stream.

### Code, import, plugins

| Method | Path | Body | Response |
|---|---|---|---|
| GET | `/api/codegen/targets` | | `{ id: string; name: string; language: string }[]` |
| POST | `/api/codegen` | `SendBody & { target: string }` | `{ code: string; language: string }` |
| POST | `/api/import` | `{ text: string; into?: string }` | `{ folder: string; requests: string[]; environments: string[] }` |
| GET | `/api/export/targets` | | `{ id: string; name: string; fileExtension: string }[]` (plugin exporters) |
| POST | `/api/export` | `{ folder: string; exporter: string }` | `{ content: string }` |
| GET | `/api/plugins` | | `PluginInfo[]` |
| POST | `/api/plugins/reload` | | `PluginInfo[]` |
| POST | `/api/plugins/{name}/approve` | | `PluginInfo` (approves all declared permissions) |
| POST | `/api/viewers/render` | `{ viewer: string; response: ScriptResponse }` | `{ html: string }` |
| GET | `/api/template-functions` | | `{ name: string; description: string; plugin: string \| null }[]` (built-ins + plugins) |

### Secrets and cookies

| Method | Path | Body | Response |
|---|---|---|---|
| PUT | `/api/secrets/{name}` | `{ value: string }` | `204` |
| DELETE | `/api/secrets/{name}` | | `204` |
| GET | `/api/cookies` | | `{ domain: string; name: string; value: string }[]` |
| DELETE | `/api/cookies` | | `204` |

### WebSocket client sessions

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/api/ws/connect` | `{ url: string; headers?: [string, string][]; environment?: string \| null }` | `{ sessionId: string }` (the span id) |
| POST | `/api/ws/{sessionId}/send` | `{ text: string }` | `204` |
| POST | `/api/ws/{sessionId}/close` | | `204` |

Messages appear as `Message` events on the session's span.

### MCP client (testing MCP servers)

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/api/mcp/connect` | `ConnectOptions` | `SessionInfo` |
| GET | `/api/mcp/sessions` | | `SessionInfo[]` |
| POST | `/api/mcp/{id}/refresh` | | `SessionInfo` |
| POST | `/api/mcp/{id}/call` | `{ tool: string; arguments: object }` | MCP `CallToolResult` JSON |
| POST | `/api/mcp/{id}/read` | `{ uri: string }` | MCP `ReadResourceResult` JSON |
| POST | `/api/mcp/{id}/prompt` | `{ name: string; arguments: Record<string, string> }` | MCP `GetPromptResult` JSON |
| DELETE | `/api/mcp/{id}` | | `204` |

```ts
type ConnectOptions = {
  name?: string;
  transport:
    | { type: "stdio"; command: string; args: string[]; env?: Record<string, string>; cwd?: string }
    | { type: "http"; url: string; headers?: [string, string][] };
};
type SessionInfo = {
  id: string; name: string; spanId: string; transport: ConnectOptions["transport"];
  serverName: string; serverVersion: string; protocolVersion: string; instructions: string | null;
  tools: McpTool[]; resources: McpResource[]; prompts: McpPrompt[];   // MCP spec JSON objects
};
```

### MCP server (AI agents use post-tester)

`/mcp` is a Streamable HTTP MCP endpoint. Agents send the token as
`Authorization: Bearer <token>`. The same server is available over stdio with `pt mcp`.
MCP sessions end when the server stops.
