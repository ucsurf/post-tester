/**
 * A tiny stand-in for `pt serve`, for working on the UI without the Rust server.
 *
 *   pnpm --filter web mock     # API on http://127.0.0.1:7777
 *   pnpm --filter web dev      # UI on http://127.0.0.1:5173/?token=dev
 *
 * Implements a subset of crates/server/API.md: info, tree, requests, environments,
 * variables, send (performs the real HTTP request and emits a realistic event
 * sequence on /api/live), run, plugins, template functions, codegen, cookies and
 * WebSocket echo sessions. An "AI agent" sends a request every 20 s so the 🤖 path
 * is visible. Everything is in memory.
 */
import { randomBytes } from "node:crypto";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { WebSocketServer, type WebSocket } from "ws";
import type { Environment, Event, EventKind, Folder, PluginInfo, Request, Source } from "@post-tester/bindings";

const PORT = Number(process.env.PORT ?? 7777);
const TOKEN = process.env.TOKEN ?? "dev";

// ── State ────────────────────────────────────────────────────────────────────

const requests = new Map<string, Partial<Request>>([
  ["users/list", { name: "List users", method: "GET", url: "{{baseUrl}}/users", query: [{ key: "page", value: "2" }], tests: ["status == 200", "json($.data) exists"] }],
  ["users/get", { name: "Get user", method: "GET", url: "{{baseUrl}}/users/2", extract: { userEmail: "json($.data.email)" } }],
  ["auth/login", { name: "Login", method: "POST", url: "{{baseUrl}}/login", body: { type: "json", content: '{\n  "email": "eve.holt@reqres.in",\n  "password": "cityslicka"\n}' }, tests: ["status == 200"] }],
  ["health", { method: "GET", url: "https://httpbin.org/get", tests: ["status == 200", "time < 2000"] }],
]);
const environments = new Map<string, Environment>([
  ["local", { description: "Your machine", variables: { baseUrl: "http://localhost:3000/api" } }],
  ["reqres", { description: "Public demo API", variables: { baseUrl: "https://reqres.in/api" } }],
]);
let defaultEnvironment: string | null = "reqres";
const runtime = new Map<string, string>();
const history: Event[] = [];
const blobs = new Map<string, { type: string; body: string }>();
const clients = new Set<WebSocket>();
let pending: Event[] = [];
let seq = 0;

const plugins: PluginInfo[] = [
  {
    name: "csv-viewer",
    displayName: "CSV viewer",
    description: "Shows text/csv responses as a table.",
    version: "0.1.0",
    dir: "plugins/csv-viewer",
    provides: ["viewers", "templateFunctions"],
    permissions: ["network:api.github.com"],
    pendingPermissions: ["network:api.github.com"],
    capabilities: {
      templateFunctions: [{ name: "slug", description: "Lower-case, dash-separated text", args: [] }],
      auth: [{ name: "signature", displayName: "HMAC signature", fields: [{ name: "keyId", type: "string", label: "Key id" }, { name: "secret", type: "secret", label: "Secret" }] }],
      hooks: [],
      importers: [],
      exporters: [],
      codegen: [],
      viewers: [{ name: "table", displayName: "Table", contentTypes: ["text/csv", "application/json"] }],
      mcpTools: [],
      keys: ["viewers", "templateFunctions", "auth"],
    },
  },
];

// ── Events ───────────────────────────────────────────────────────────────────

let lastMs = 0;
let counter = 0;
/** Time-ordered ids like UUID v7, so span order follows creation order. */
function uuidv7(): string {
  const now = Date.now();
  counter = now === lastMs ? counter + 1 : 0;
  lastMs = now;
  const hex = now.toString(16).padStart(12, "0") + counter.toString(16).padStart(4, "0").slice(-3);
  const rand = randomBytes(8).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${hex.slice(12, 15)}-8${rand.slice(0, 3)}-${rand.slice(3, 15)}`;
}

class Span {
  readonly id = uuidv7();
  readonly source: Source;
  readonly parent: string | null;
  private readonly started = performance.now();
  constructor(source: Source, parent: string | null) {
    this.source = source;
    this.parent = parent;
  }
  emit(kind: EventKind): void {
    const event: Event = { seq: ++seq, span_id: this.id, ...(this.parent ? { parent_id: this.parent } : {}), source: this.source, at: new Date().toISOString(), kind };
    history.push(event);
    if (history.length > 5000) history.shift();
    pending.push(event);
  }
  elapsed(): number {
    return performance.now() - this.started;
  }
}

setInterval(() => {
  if (pending.length === 0) return;
  const batch = JSON.stringify(pending);
  pending = [];
  for (const client of clients) client.send(batch);
}, 50);

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// ── Sending ──────────────────────────────────────────────────────────────────

function variables(environment: string | null | undefined): Map<string, string> {
  const vars = new Map(Object.entries(environments.get(environment ?? "")?.variables ?? {}));
  for (const [k, v] of runtime) vars.set(k, v);
  return vars;
}

function render(text: string, vars: Map<string, string>): string {
  return text.replace(/\{\{\s*([^{}]*?)\s*\}\}/g, (match, name: string) => {
    if (name.startsWith("secret:")) return "secret-value";
    if (name === "uuid()") return crypto.randomUUID();
    return vars.get(name) ?? match;
  });
}

function preview(text: string) {
  const limit = 64 * 1024;
  const id = uuidv7();
  return { preview: text.slice(0, limit), size: Buffer.byteLength(text), truncated: text.length > limit, ...(text.length > limit ? { blob: id } : {}) };
}

type SendOptions = { requestId?: string | undefined; environment?: string | null | undefined; source?: Source; parent?: string | null };

async function send(request: Partial<Request>, options: SendOptions) {
  const vars = variables(options.environment);
  const method = (request.method ?? "GET").toUpperCase();
  const url = new URL(render(request.url ?? "", vars));
  for (const q of request.query ?? []) if (q.enabled !== false) url.searchParams.append(render(q.key, vars), render(q.value, vars));
  const span = new Span(options.source ?? { type: "user" }, options.parent ?? null);
  span.emit({
    type: "started",
    kind: "request",
    title: request.name || `${method} ${request.url}`,
    summary: { protocol: "http", method, url: request.url ?? "", ...(options.requestId ? { request_id: options.requestId } : {}) },
  });

  const headers: [string, string][] = (request.headers ?? []).filter((h) => h.enabled !== false).map((h) => [render(h.key, vars), render(h.value, vars)]);
  let body = "";
  const b = request.body;
  if (b?.type === "json") {
    body = render(b.content, vars);
    headers.push(["content-type", "application/json"]);
  } else if (b?.type === "text") {
    body = render(b.content, vars);
    headers.push(["content-type", b.content_type]);
  } else if (b?.type === "form") {
    body = new URLSearchParams(b.fields.filter((f) => f.enabled !== false).map((f) => [f.key, render(f.value, vars)])).toString();
    headers.push(["content-type", "application/x-www-form-urlencoded"]);
  } else if (b?.type === "graphql") {
    body = JSON.stringify({ query: b.query, variables: b.variables ? JSON.parse(b.variables) : undefined });
    headers.push(["content-type", "application/json"]);
  }

  const timings = { dnsMs: 2 + Math.random() * 8, connectMs: 8 + Math.random() * 20, tlsMs: url.protocol === "https:" ? 15 + Math.random() * 30 : null, waitingMs: 0, downloadMs: 0 };
  await sleep(timings.dnsMs);
  span.emit({ type: "phase", phase: "dns", duration_ms: timings.dnsMs });
  await sleep(timings.connectMs);
  span.emit({ type: "phase", phase: "connect", duration_ms: timings.connectMs });
  if (timings.tlsMs !== null) {
    await sleep(timings.tlsMs);
    span.emit({ type: "phase", phase: "tls", duration_ms: timings.tlsMs });
  }
  span.emit({ type: "connected", remote_addr: `${url.hostname}:${url.port || (url.protocol === "https:" ? 443 : 80)}`, ...(timings.tlsMs !== null ? { tls: { version: "TLSv1.3", cipher: "TLS13_AES_128_GCM_SHA256", alpn: "h2" } } : {}) });
  const prepared = { method, url: url.toString(), headers, body };
  span.emit({ type: "request_sent", method, url: url.toString(), http_version: "HTTP/1.1", headers: headers.map(([name, value]) => ({ name, value })), body: preview(body) });

  try {
    const sent = performance.now();
    const response = await fetch(url, { method, headers, ...(body && method !== "GET" && method !== "HEAD" ? { body } : {}), signal: AbortSignal.timeout(request.options?.timeout_ms ?? 30_000) });
    timings.waitingMs = performance.now() - sent;
    span.emit({ type: "phase", phase: "waiting", duration_ms: timings.waitingMs });
    const responseHeaders = [...response.headers.entries()];
    const contentType = response.headers.get("content-type") ?? "";
    const started = performance.now();
    const text = await response.text();
    timings.downloadMs = performance.now() - started;
    const payload = { ...preview(text), content_type: contentType };
    if (payload.blob) blobs.set(payload.blob, { type: contentType, body: text });
    span.emit({ type: "response_received", status: response.status, status_text: response.statusText, http_version: "HTTP/1.1", headers: responseHeaders.map(([name, value]) => ({ name, value })), body: payload });
    span.emit({ type: "phase", phase: "download", duration_ms: timings.downloadMs });

    const time = span.elapsed();
    const tests = (request.tests ?? []).map((t) => evaluate(t, response.status, time, text));
    for (const t of tests) span.emit({ type: "test_result", ...t });
    const extracted: [string, string][] = [];
    for (const [name, expr] of Object.entries(request.extract ?? {})) {
      const value = extract(expr, text);
      if (value !== undefined) {
        runtime.set(name, value);
        extracted.push([name, value]);
        span.emit({ type: "variable_set", name, value });
      }
    }
    const outcome = tests.some((t) => !t.passed) ? "failed" : "ok";
    span.emit({ type: "finished", outcome, duration_ms: time, status: response.status, size: Buffer.byteLength(text) });
    return {
      spanId: span.id,
      outcome,
      error: null,
      request: prepared,
      response: {
        status: response.status,
        statusText: response.statusText,
        httpVersion: "HTTP/1.1",
        headers: responseHeaders,
        body: payload,
        size: Buffer.byteLength(text),
        wireSize: Buffer.byteLength(text),
        timeMs: time,
        timings,
        remoteAddr: `${url.hostname}:443`,
        url: response.url,
        redirects: response.redirected ? 1 : 0,
      },
      tests,
      extracted,
    };
  } catch (e) {
    const message = `Could not reach ${url.host}: ${e instanceof Error ? e.message : String(e)}. Check the URL and that the server is running.`;
    span.emit({ type: "log", level: "error", text: message });
    span.emit({ type: "finished", outcome: "error", duration_ms: span.elapsed(), error: message });
    return { spanId: span.id, outcome: "error", error: message, request: prepared, response: null, tests: [], extracted: [] };
  }
}

function evaluate(test: string, status: number, time: number, body: string) {
  const m = /^(status|time)\s*(==|!=|<|>|<=|>=)\s*(\d+)$/.exec(test.trim());
  if (m) {
    const actual = m[1] === "status" ? status : time;
    const expected = Number(m[3]);
    const ops: Record<string, boolean> = { "==": actual === expected, "!=": actual !== expected, "<": actual < expected, ">": actual > expected, "<=": actual <= expected, ">=": actual >= expected };
    const passed = ops[m[2] ?? "=="] ?? false;
    return { name: test, passed, ...(passed ? {} : { detail: `expected ${m[1]} ${m[2]} ${expected}, got ${Math.round(actual)}` }) };
  }
  const j = /^json\((.+)\)\s+exists$/.exec(test.trim());
  if (j) {
    const passed = extract(`json(${j[1]})`, body) !== undefined;
    return { name: test, passed, ...(passed ? {} : { detail: `${j[1]} was not found` }) };
  }
  return { name: test, passed: true, detail: "(the mock server only checks status, time and json(...) exists)" };
}

function extract(expr: string, body: string): string | undefined {
  const m = /^json\(\$\.?(.*)\)$/.exec(expr.trim());
  if (!m) return undefined;
  try {
    let value: unknown = JSON.parse(body);
    for (const part of (m[1] ?? "").split(".").filter(Boolean)) value = (value as Record<string, unknown>)?.[part];
    return value === undefined ? undefined : typeof value === "string" ? value : JSON.stringify(value);
  } catch {
    return undefined;
  }
}

async function run(ids: string[], environment: string | null | undefined, name: string) {
  const span = new Span({ type: "user" }, null);
  span.emit({ type: "started", kind: "collection_run", title: `Run "${name}"`, summary: {} });
  span.emit({ type: "progress", done: 0, total: ids.length });
  const items = [];
  for (const [i, id] of ids.entries()) {
    const request = requests.get(id) ?? {};
    const result = await send(request, { requestId: id, environment, parent: span.id });
    items.push({ request_id: id, name: request.name ?? id, span_id: result.spanId, outcome: result.outcome, status: result.response?.status ?? null, duration_ms: result.response?.timeMs ?? 0, tests: result.tests, error: result.error });
    span.emit({ type: "progress", done: i + 1, total: ids.length });
  }
  const outcome = items.every((i) => i.outcome === "ok") ? "ok" : items.some((i) => i.outcome === "error") ? "error" : "failed";
  span.emit({ type: "finished", outcome, duration_ms: span.elapsed() });
  return { span_id: span.id, name, outcome, items, duration_ms: span.elapsed() };
}

// ── HTTP ─────────────────────────────────────────────────────────────────────

function tree(): Folder {
  const root: Folder = { id: "", name: "", settings: { name: "", description: "", headers: [], variables: {} }, folders: [], requests: [] };
  for (const [id, r] of [...requests].sort(([a], [b]) => a.localeCompare(b))) {
    const parts = id.split("/");
    let folder = root;
    for (const part of parts.slice(0, -1)) {
      const folderId = folder.id ? `${folder.id}/${part}` : part;
      let child = folder.folders.find((f) => f.id === folderId);
      if (!child) {
        child = { id: folderId, name: part, settings: { name: part, description: "", headers: [], variables: {} }, folders: [], requests: [] };
        folder.folders.push(child);
      }
      folder = child;
    }
    folder.requests.push({ id, name: r.name ?? parts.at(-1) ?? id, method: r.method ?? "GET", url: r.url ?? "" });
  }
  return root;
}

async function readJson<T>(req: IncomingMessage): Promise<T> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  return (text ? JSON.parse(text) : {}) as T;
}

function json(res: ServerResponse, status: number, body?: unknown): void {
  res.writeHead(status, body === undefined ? {} : { "content-type": "application/json" });
  res.end(body === undefined ? undefined : JSON.stringify(body));
}

const fail = (res: ServerResponse, status: number, error: string) => json(res, status, { error });

function authorized(req: IncomingMessage, url: URL): boolean {
  return req.headers.authorization === `Bearer ${TOKEN}` || url.searchParams.get("token") === TOKEN;
}

type Body = Record<string, unknown>;
const wsSessions = new Map<string, Span>();

async function route(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
  const path = decodeURIComponent(url.pathname);
  const method = req.method ?? "GET";
  if (!path.startsWith("/api/")) return fail(res, 404, "The mock server only serves /api. Run the UI with `pnpm --filter web dev`.");
  if (!authorized(req, url)) return fail(res, 401, "Missing or wrong token. Open http://127.0.0.1:5173/?token=dev");

  const id = (prefix: string) => path.slice(prefix.length);
  if (path === "/api/info") {
    return json(res, 200, { version: "0.0.0-mock", workspace: { root: "/tmp/mock-workspace", name: "mock-workspace" }, environments: [...environments.keys()], defaultEnvironment, secretStore: "mock keychain", mcpUrl: `http://127.0.0.1:${PORT}/mcp` });
  }
  if (path === "/api/tree") return json(res, 200, tree());
  if (path.startsWith("/api/requests/")) {
    const key = id("/api/requests/");
    if (method === "GET") return requests.has(key) ? json(res, 200, requests.get(key)) : fail(res, 404, `No request ${key}. It may have been renamed or deleted.`);
    if (method === "PUT") return requests.set(key, await readJson<Request>(req)), json(res, 204);
    if (method === "DELETE") return requests.delete(key), json(res, 204);
  }
  if (path === "/api/environments") return json(res, 200, [...environments.keys()]);
  if (path.startsWith("/api/environments/")) {
    const name = id("/api/environments/");
    if (method === "GET") return environments.has(name) ? json(res, 200, environments.get(name)) : fail(res, 404, `No environment ${name}`);
    if (method === "PUT") return environments.set(name, await readJson<Environment>(req)), json(res, 204);
    if (method === "DELETE") return environments.delete(name), json(res, 204);
  }
  if (path === "/api/variables") return json(res, 200, [...variables(url.searchParams.get("environment"))].map(([name, value]) => ({ name, value })));
  if (path === "/api/variables/runtime") return runtime.clear(), json(res, 204);
  if (path === "/api/settings/default-environment") return (defaultEnvironment = (await readJson<{ environment: string | null }>(req)).environment), json(res, 204);
  if (path === "/api/send" && method === "POST") {
    const body = await readJson<{ request: Request; requestId?: string; environment?: string | null }>(req);
    return json(res, 200, await send(body.request, { requestId: body.requestId, environment: body.environment }));
  }
  if (path === "/api/preview" && method === "POST") {
    const body = await readJson<{ request: Request; environment?: string | null }>(req);
    const vars = variables(body.environment);
    return json(res, 200, { method: body.request.method, url: render(body.request.url, vars), headers: body.request.headers.map((h) => [h.key, render(h.value, vars)]), body: body.request.body.type === "json" ? render(body.request.body.content, vars) : "" });
  }
  if (path === "/api/run" && method === "POST") {
    const body = await readJson<{ target: { folder: string } | { requests: string[] }; environment?: string | null }>(req);
    const ids = "folder" in body.target ? [...requests.keys()].filter((k) => k.startsWith((body.target as { folder: string }).folder)) : body.target.requests;
    return json(res, 200, await run(ids, body.environment, "folder" in body.target ? body.target.folder || "workspace" : `${ids.length} request(s)`));
  }
  if (path.startsWith("/api/cancel/")) return json(res, 200, { cancelled: false });
  if (path.startsWith("/api/blobs/")) {
    const blob = blobs.get(id("/api/blobs/"));
    if (!blob) return fail(res, 404, "The body is no longer in memory. Send the request again.");
    res.writeHead(200, { "content-type": blob.type });
    return void res.end(blob.body);
  }
  if (path === "/api/events") return json(res, 200, history.filter((e) => e.seq > Number(url.searchParams.get("since") ?? 0)));
  if (path === "/api/plugins" || path === "/api/plugins/reload") return json(res, 200, plugins);
  if (path.startsWith("/api/plugins/") && path.endsWith("/approve")) {
    const plugin = plugins.find((p) => p.name === path.split("/")[3]);
    if (!plugin) return fail(res, 404, "No such plugin");
    plugin.pendingPermissions = [];
    return json(res, 200, plugin);
  }
  if (path === "/api/viewers/render") {
    const body = await readJson<{ response: { body: string } }>(req);
    return json(res, 200, { html: `<style>body{font:13px system-ui;margin:12px}</style><h3>Mock viewer</h3><pre>${escapeHtml(body.response.body.slice(0, 2000))}</pre>` });
  }
  if (path === "/api/template-functions") return json(res, 200, [{ name: "uuid", description: "A random UUID v4", plugin: null }, { name: "now", description: "Current time (ISO 8601)", plugin: null }, { name: "slug", description: "Lower-case, dash-separated text", plugin: "csv-viewer" }]);
  if (path === "/api/codegen/targets") return json(res, 200, [{ id: "curl", name: "cURL", language: "shell" }, { id: "fetch", name: "JavaScript fetch", language: "javascript" }]);
  if (path === "/api/codegen") {
    const body = await readJson<{ request: Request; environment?: string | null }>(req);
    return json(res, 200, { code: `curl -X ${body.request.method} '${render(body.request.url, variables(body.environment))}'`, language: "shell" });
  }
  if (path === "/api/export/targets") return json(res, 200, []);
  if (path === "/api/cookies") return method === "DELETE" ? json(res, 204) : json(res, 200, [{ domain: "reqres.in", name: "session", value: "abc123" }]);
  if (path.startsWith("/api/secrets/")) return json(res, 204);
  if (path === "/api/import") {
    const body = await readJson<{ text: string; into?: string }>(req);
    const m = /curl\s+(?:-X\s+(\w+)\s+)?['"]?(https?:\/\/[^\s'"]+)/.exec(body.text);
    if (!m) return fail(res, 400, "The mock server only imports simple cURL commands like: curl https://example.com");
    const newId = `${body.into ? `${body.into}/` : "imported/"}request-${requests.size + 1}`;
    requests.set(newId, { method: m[1] ?? "GET", url: m[2] ?? "" });
    return json(res, 200, { folder: body.into ?? "imported", requests: [newId], environments: [] });
  }
  if (path === "/api/ws/connect") {
    const body = await readJson<{ url: string }>(req);
    const span = new Span({ type: "user" }, null);
    span.emit({ type: "started", kind: "web_socket", title: body.url, summary: { protocol: "websocket", method: "WS", url: body.url } });
    span.emit({ type: "log", level: "info", text: "Connected (mock echo server)" });
    wsSessions.set(span.id, span);
    return json(res, 200, { sessionId: span.id });
  }
  const wsMatch = /^\/api\/ws\/([^/]+)\/(send|close)$/.exec(path);
  if (wsMatch) {
    const span = wsSessions.get(wsMatch[1] ?? "");
    if (!span) return fail(res, 404, "This WebSocket session has ended. Connect again.");
    if (wsMatch[2] === "send") {
      const { text } = await readJson<{ text: string }>(req);
      span.emit({ type: "message", direction: "sent", payload: preview(text) });
      setTimeout(() => span.emit({ type: "message", direction: "received", payload: preview(text) }), 150);
    } else {
      span.emit({ type: "finished", outcome: "ok", duration_ms: span.elapsed() });
      wsSessions.delete(span.id);
    }
    return json(res, 204);
  }
  if (path === "/api/mcp/sessions") return json(res, 200, []);
  return fail(res, 501, `The mock server does not implement ${method} ${path}. Use the real server (pt serve) for this feature.`);
}

function escapeHtml(text: string): string {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}

const server = createServer((req, res) => {
  route(req, res).catch((e: unknown) => fail(res, 500, e instanceof Error ? e.message : String(e)));
});

const wss = new WebSocketServer({ noServer: true });
server.on("upgrade", (req, socket, head) => {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
  if (url.pathname !== "/api/live" || url.searchParams.get("token") !== TOKEN) return socket.destroy();
  wss.handleUpgrade(req, socket, head, (ws) => {
    ws.send(JSON.stringify(history));
    clients.add(ws);
    ws.on("close", () => clients.delete(ws));
  });
});

// A pretend AI agent, so agent activity shows up in Live Activity.
setInterval(() => {
  void send(requests.get("users/get") ?? {}, { requestId: "users/get", environment: "reqres", source: { type: "agent", client: "claude-code" } });
}, 20_000);

server.listen(PORT, "127.0.0.1", () => {
  const boot = new Span({ type: "system" }, null);
  boot.emit({ type: "started", kind: "system", title: "Mock server started", summary: {} });
  boot.emit({ type: "finished", outcome: "ok", duration_ms: 1 });
  console.log(`post-tester mock API on http://127.0.0.1:${PORT}`);
  console.log(`Run \`pnpm --filter web dev\` and open http://127.0.0.1:5173/?token=${TOKEN}`);
});
