// post-tester sandbox prelude.
//
// Runs once in every sandbox before any plugin or script code. It turns the
// raw host functions (`__host.*`, which pass JSON strings) into the friendly
// APIs plugins and scripts use: `ctx`, `pt`, `pm`, `console`, `expect`.
//
// Host functions return an envelope: {"ok": value} or {"error": "message"}.
//
// Everything is defined inside one function, so plugin and script code can
// only reach what is published on `globalThis` at the end. The raw host
// functions are taken off `globalThis`, and the entry points the host calls
// are read-only, so code cannot intercept later calls.
"use strict";
(() => {

const __host = globalThis.__host;
delete globalThis.__host;

/** Defines a read-only global. */
const __expose = (name, value) =>
  Object.defineProperty(globalThis, name, { value, writable: false, enumerable: false, configurable: false });

const __unwrap = (envelope) => {
  const parsed = JSON.parse(envelope);
  if (parsed && Object.prototype.hasOwnProperty.call(parsed, "error")) throw new Error(parsed.error);
  return parsed.ok;
};

const __text = (value) => {
  if (typeof value === "string") return value;
  if (value === undefined) return "undefined";
  if (value instanceof Error) return value.stack || String(value);
  try { return JSON.stringify(value); } catch { return String(value); }
};

const console = Object.freeze({
  log: (...args) => __host.log("info", args.map(__text).join(" ")),
  info: (...args) => __host.log("info", args.map(__text).join(" ")),
  debug: (...args) => __host.log("debug", args.map(__text).join(" ")),
  warn: (...args) => __host.log("warn", args.map(__text).join(" ")),
  error: (...args) => __host.log("error", args.map(__text).join(" ")),
});
__expose("console", console);

// ── Headers ────────────────────────────────────────────────────────────────

/** Case-insensitive, ordered headers. Accepts [[k, v]], [{key, value}] or {k: v}. */
class Headers {
  constructor(init) {
    this._pairs = [];
    if (Array.isArray(init)) {
      for (const item of init) {
        if (Array.isArray(item)) this._pairs.push([String(item[0]), String(item[1])]);
        else if (item && item.key !== undefined) this._pairs.push([String(item.key), String(item.value ?? "")]);
      }
    } else if (init instanceof Headers) {
      this._pairs = init._pairs.map(([k, v]) => [k, v]);
    } else if (init && typeof init === "object") {
      for (const [k, v] of Object.entries(init)) this._pairs.push([k, String(v)]);
    }
  }
  _find(name) { const n = String(name).toLowerCase(); return this._pairs.findIndex(([k]) => k.toLowerCase() === n); }
  get(name) { const i = this._find(name); return i < 0 ? undefined : this._pairs[i][1]; }
  has(name) { return this._find(name) >= 0; }
  /** Replaces the header, or adds it. */
  set(name, value) { const i = this._find(name); if (i < 0) this._pairs.push([String(name), String(value)]); else this._pairs[i][1] = String(value); }
  /** Adds the header even if one with the same name exists. */
  add(name, value) {
    if (name && typeof name === "object") { this._pairs.push([String(name.key), String(name.value ?? "")]); return; }
    this._pairs.push([String(name), String(value)]);
  }
  /** Postman-compatible alias of `set` that takes {key, value}. */
  upsert(item) { this.set(item.key, item.value ?? ""); }
  delete(name) { const n = String(name).toLowerCase(); this._pairs = this._pairs.filter(([k]) => k.toLowerCase() !== n); }
  remove(name) { this.delete(name); }
  entries() { return this._pairs.map(([k, v]) => [k, v]); }
  toObject() { const o = {}; for (const [k, v] of this._pairs) o[k] = v; return o; }
  toJSON() { return this.entries(); }
}

// ── Requests and responses ─────────────────────────────────────────────────

const __request = (raw) => ({
  method: raw.method ?? "GET",
  url: raw.url ?? "",
  headers: new Headers(raw.headers ?? []),
  body: raw.body ?? "",
});

const __serializeRequest = (req) => ({
  method: String(req.method ?? "GET").toUpperCase(),
  url: String(req.url),
  headers: new Headers(req.headers ?? []).entries(),
  body: req.body === undefined || req.body === null ? "" : typeof req.body === "string" ? req.body : JSON.stringify(req.body),
});

const __response = (raw) => ({
  status: raw.status,
  statusText: raw.status_text ?? raw.statusText ?? "",
  headers: new Headers(raw.headers ?? []),
  body: raw.body ?? "",
  timeMs: raw.time_ms ?? raw.timeMs ?? 0,
  ...(raw.url === undefined ? {} : { url: raw.url }),
  text() { return this.body; },
  json() {
    try { return JSON.parse(this.body); }
    catch (e) { throw new Error(`the response body is not valid JSON: ${e.message}`); }
  },
});

const __serializeResponse = (res) => ({
  status: Number(res.status),
  status_text: String(res.statusText ?? ""),
  headers: new Headers(res.headers ?? []).entries(),
  body: typeof res.body === "string" ? res.body : JSON.stringify(res.body ?? ""),
  time_ms: Number(res.timeMs ?? 0),
});

// ── ctx: the API every plugin function receives ────────────────────────────

const __http = {
  /** Sends a request. Options: { method, url, headers, body, json, timeoutMs, followRedirects }. */
  async request(options) {
    if (!options || !options.url) throw new Error("ctx.http.request needs at least { url }");
    const body = options.json !== undefined ? JSON.stringify(options.json) : options.body;
    const headers = new Headers(options.headers ?? []);
    if (options.json !== undefined && !headers.has("content-type")) headers.set("Content-Type", "application/json");
    const raw = __unwrap(await __host.http(JSON.stringify({
      method: String(options.method ?? "GET").toUpperCase(),
      url: String(options.url),
      headers: headers.entries(),
      body: body === undefined ? "" : String(body),
      timeout_ms: options.timeoutMs ?? 30000,
      follow_redirects: options.followRedirects !== false,
    })));
    return __response(raw);
  },
  get(url, options = {}) { return __http.request({ ...options, method: "GET", url }); },
  post(url, options = {}) { return __http.request({ ...options, method: "POST", url }); },
  put(url, options = {}) { return __http.request({ ...options, method: "PUT", url }); },
  patch(url, options = {}) { return __http.request({ ...options, method: "PATCH", url }); },
  delete(url, options = {}) { return __http.request({ ...options, method: "DELETE", url }); },
};

const ctx = Object.freeze({
  log: Object.freeze({
    debug: (...a) => console.debug(...a),
    info: (...a) => console.info(...a),
    warn: (...a) => console.warn(...a),
    error: (...a) => console.error(...a),
  }),
  progress: (done, total) => __host.progress(Number(done), Number(total)),
  random: Object.freeze({
    uuid: () => __host.uuid(),
    int: (min = 0, max = 1000) => Math.floor(Math.random() * (max - min + 1)) + min,
  }),
  crypto: Object.freeze({
    sha256: (data) => __host.sha256(String(data)),
    hmacSha256: (key, data) => __host.hmacSha256(String(key), String(data)),
    base64Encode: (text) => __host.base64Encode(String(text)),
    base64Decode: (text) => __unwrap(__host.base64Decode(String(text))),
  }),
  variables: Object.freeze({
    get: (name) => __unwrap(__host.varGet(String(name))) ?? undefined,
    set: (name, value) => __unwrap(__host.varSet(String(name), String(value))),
  }),
  http: Object.freeze(__http),
  secrets: Object.freeze({
    get: async (name) => __unwrap(await __host.secretGet(String(name))) ?? undefined,
  }),
  fs: Object.freeze({
    read: (path) => __unwrap(__host.fsRead(String(path))),
    write: (path, content) => __unwrap(__host.fsWrite(String(path), String(content))),
  }),
});
__expose("ctx", ctx);

// ── Calling plugin capabilities ────────────────────────────────────────────

const __section = (plugin, key, name, label) => {
  const table = plugin && plugin[key];
  const entry = table && table[name];
  if (!entry) {
    const available = table ? Object.keys(table).join(", ") || "none" : "none";
    throw new Error(`this plugin has no ${label} named "${name}" (available: ${available})`);
  }
  return entry;
};

const __fn = (entry, method, label) => {
  if (typeof entry[method] !== "function") throw new Error(`${label} must have a ${method}() function`);
  return entry[method];
};

/** Maps positional template arguments ("0", "1") to the declared argument names. */
const __namedArgs = (def, args) => {
  const out = {};
  const declared = Array.isArray(def.args) ? def.args : [];
  for (const [key, value] of Object.entries(args ?? {})) {
    const index = /^\d+$/.test(key) ? Number(key) : -1;
    out[index >= 0 && declared[index] ? declared[index].name : key] = value;
  }
  for (const arg of declared) {
    if (out[arg.name] === undefined && arg.default !== undefined) out[arg.name] = arg.default;
    if (out[arg.name] === undefined && arg.required !== false && arg.optional !== true) {
      throw new Error(`missing argument "${arg.name}"`);
    }
  }
  return out;
};

/** Calls one capability of a plugin and returns plain JSON-compatible data. */
const __pt_dispatch = async (plugin, kind, name, payload) => {
  switch (kind) {
    case "templateFunction": {
      const def = __section(plugin, "templateFunctions", name, "template function");
      const result = await __fn(def, "run", `template function "${name}"`)(__namedArgs(def, payload.args), ctx);
      return result === undefined || result === null ? "" : typeof result === "string" ? result : JSON.stringify(result);
    }
    case "auth": {
      const def = __section(plugin, "auth", name, "auth method");
      const req = __request(payload.request);
      const out = await __fn(def, "apply", `auth method "${name}"`)(req, payload.config ?? {}, ctx);
      return __serializeRequest(out ?? req);
    }
    case "beforeRequest": {
      const req = __request(payload.request);
      const out = plugin.hooks && plugin.hooks.beforeRequest ? await plugin.hooks.beforeRequest(req, ctx) : req;
      return __serializeRequest(out ?? req);
    }
    case "afterResponse": {
      const res = __response(payload.response);
      const out = plugin.hooks && plugin.hooks.afterResponse ? await plugin.hooks.afterResponse(res, ctx) : res;
      return __serializeResponse(out ?? res);
    }
    case "importerDetect": {
      const def = __section(plugin, "importers", name, "importer");
      return typeof def.detect === "function" ? Boolean(await def.detect(payload.text, ctx)) : false;
    }
    case "importer": {
      const def = __section(plugin, "importers", name, "importer");
      const out = await __fn(def, "import", `importer "${name}"`)(payload.text, ctx);
      if (!out || !Array.isArray(out.requests)) throw new Error(`importer "${name}" must return { name, requests: [...] }`);
      return {
        name: String(out.name ?? "Imported"),
        requests: out.requests.map((r) => ({
          ...r,
          headers: r.headers === undefined ? [] : new Headers(r.headers).entries().map(([key, value]) => ({ key, value })),
        })),
      };
    }
    case "exporter": {
      const def = __section(plugin, "exporters", name, "exporter");
      return String(await __fn(def, "export", `exporter "${name}"`)(payload.collection, ctx));
    }
    case "codegen": {
      const def = __section(plugin, "codegen", name, "code generator");
      const req = __request(payload.request);
      return String(await __fn(def, "generate", `code generator "${name}"`)(req, ctx));
    }
    case "viewer": {
      const def = __section(plugin, "viewers", name, "viewer");
      return String(await __fn(def, "render", `viewer "${name}"`)(__response(payload.response), ctx));
    }
    case "mcpTool": {
      const def = __section(plugin, "mcpTools", name, "MCP tool");
      const out = await __fn(def, "run", `MCP tool "${name}"`)(payload.input ?? {}, ctx);
      return out === undefined ? null : out;
    }
    case "describe":
      return __describe(plugin);
    default:
      throw new Error(`unknown call kind "${kind}"`);
  }
};

/** Lists what a plugin provides, with the metadata the app and AI agents need. */
const __describe = (plugin) => {
  const entries = (key) => Object.entries((plugin && plugin[key]) || {});
  return {
    templateFunctions: entries("templateFunctions").map(([name, d]) => ({
      name,
      description: String(d.description ?? ""),
      args: (d.args ?? []).map((a) => ({ name: String(a.name), type: String(a.type ?? "string"), description: String(a.description ?? "") })),
    })),
    auth: entries("auth").map(([name, d]) => ({
      name,
      displayName: String(d.displayName ?? name),
      fields: (d.fields ?? []).map((f) => ({ name: String(f.name), type: String(f.type ?? "string"), label: String(f.label ?? f.name) })),
    })),
    hooks: Object.keys((plugin && plugin.hooks) || {}),
    importers: entries("importers").map(([name, d]) => ({ name, displayName: String(d.displayName ?? name), fileExtensions: d.fileExtensions ?? [] })),
    exporters: entries("exporters").map(([name, d]) => ({ name, displayName: String(d.displayName ?? name), fileExtension: String(d.fileExtension ?? "") })),
    codegen: entries("codegen").map(([name, d]) => ({ name, displayName: String(d.displayName ?? name), language: String(d.language ?? "text") })),
    viewers: entries("viewers").map(([name, d]) => ({ name, displayName: String(d.displayName ?? name), contentTypes: d.contentTypes ?? [] })),
    mcpTools: entries("mcpTools").map(([name, d]) => ({ name, description: String(d.description ?? ""), inputSchema: d.inputSchema ?? { type: "object" } })),
    keys: plugin && typeof plugin === "object" ? Object.keys(plugin) : [],
  };
};

/** Entry point used by the host: calls the loaded plugin and returns a JSON string. */
__expose("__pt_dispatch", __pt_dispatch);
__expose("__pt_call", async (kind, name, payloadJson) => {
  const result = await __pt_dispatch(globalThis.__pt_plugin, kind, name, JSON.parse(payloadJson));
  return JSON.stringify(result === undefined ? null : result);
});

// ── expect(): Jest-style assertions for plugin tests and scripts ───────────

const __show = (v) => {
  if (typeof v === "string") return JSON.stringify(v);
  if (v instanceof RegExp) return String(v);
  try { return JSON.stringify(v) ?? String(v); } catch { return String(v); }
};
const __deepEqual = (a, b) => {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return Number.isNaN(a) && Number.isNaN(b);
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (a instanceof Date || b instanceof Date) return a instanceof Date && b instanceof Date && Object.is(a.getTime(), b.getTime());
  if (a instanceof RegExp || b instanceof RegExp) return a instanceof RegExp && b instanceof RegExp && String(a) === String(b);
  const ka = Object.keys(a), kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => __deepEqual(a[k], b[k]));
};

const expect = (actual) => {
  const make = (negate) => {
    const check = (passed, message) => {
      if (passed === negate) throw new Error(negate ? message.replace("expected", "expected not") : message);
    };
    const api = {
      toBe: (e) => check(Object.is(actual, e), `expected ${__show(actual)} to be ${__show(e)}`),
      toEqual: (e) => check(__deepEqual(actual, e), `expected ${__show(actual)} to equal ${__show(e)}`),
      toMatch: (p) => check(typeof actual === "string" && (p instanceof RegExp ? p.test(actual) : actual.includes(p)), `expected ${__show(actual)} to match ${__show(p)}`),
      toContain: (e) => check(actual != null && (typeof actual === "string" ? actual.includes(e) : Array.from(actual).some((x) => __deepEqual(x, e))), `expected ${__show(actual)} to contain ${__show(e)}`),
      toBeTruthy: () => check(Boolean(actual), `expected ${__show(actual)} to be truthy`),
      toBeFalsy: () => check(!actual, `expected ${__show(actual)} to be falsy`),
      toBeDefined: () => check(actual !== undefined, `expected value to be defined`),
      toBeUndefined: () => check(actual === undefined, `expected ${__show(actual)} to be undefined`),
      toBeNull: () => check(actual === null, `expected ${__show(actual)} to be null`),
      toBeGreaterThan: (n) => check(actual > n, `expected ${__show(actual)} to be greater than ${n}`),
      toBeGreaterThanOrEqual: (n) => check(actual >= n, `expected ${__show(actual)} to be >= ${n}`),
      toBeLessThan: (n) => check(actual < n, `expected ${__show(actual)} to be less than ${n}`),
      toBeLessThanOrEqual: (n) => check(actual <= n, `expected ${__show(actual)} to be <= ${n}`),
      toHaveLength: (n) => check(actual != null && actual.length === n, `expected length ${n}, got ${actual == null ? actual : actual.length}`),
      toHaveProperty: (k, v) => check(actual != null && k in Object(actual) && (v === undefined || __deepEqual(actual[k], v)), `expected ${__show(actual)} to have property ${__show(k)}${v === undefined ? "" : ` = ${__show(v)}`}`),
      toThrow: (p) => {
        let thrown = null;
        try { actual(); } catch (e) { thrown = e; }
        const matches = thrown && (p === undefined || (p instanceof RegExp ? p.test(thrown.message) : String(thrown.message).includes(p)));
        check(Boolean(matches), `expected function to throw${p === undefined ? "" : ` ${__show(p)}`}${thrown ? `, got "${thrown.message}"` : ""}`);
      },
    };
    return api;
  };
  const positive = make(false);
  positive.not = make(true);
  return positive;
};
globalThis.expect = expect;

// ── Test runner for `pt plugin test` ───────────────────────────────────────

const __tests = [];
globalThis.test = (name, fn) => __tests.push({ name: String(name), fn });
globalThis.it = globalThis.test;
globalThis.describe = (name, fn) => fn();
__expose("__pt_run_tests", async () => {
  const results = [];
  for (const { name, fn } of __tests) {
    try {
      await fn();
      results.push({ name, passed: true, detail: null });
    } catch (e) {
      results.push({ name, passed: false, detail: e && e.message ? e.message : String(e) });
    }
  }
  return JSON.stringify(results);
});

// ── Request scripts: the `pt` API and Postman's `pm` API ───────────────────

/** Marks `pm.response`, so `.ok`, `.status()`, `.body()`... check the HTTP response. */
const __isResponse = Symbol("pm.response");

/** Chai-style assertions used by `pm.expect` (a commonly used subset). */
const __chai = (actual) => {
  const state = { negate: false, deep: false };
  /** Throws unless `passed` (or, after `.not`, unless it did not pass). */
  const assert = (passed, message, negated) => {
    if (Boolean(passed) === state.negate) throw new Error(state.negate ? (negated ?? message.replace("expected", "expected not")) : message);
    return chain;
  };
  const typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : v instanceof RegExp ? "regexp" : v instanceof Date ? "date" : typeof v);
  const chain = {};
  const getter = (name, get) => Object.defineProperty(chain, name, { get, configurable: true });
  for (const word of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "does", "but", "still"]) {
    getter(word, () => chain);
  }
  getter("not", () => { state.negate = !state.negate; return chain; });
  getter("deep", () => { state.deep = true; return chain; });

  const response = Boolean(actual && actual[__isResponse]);
  const code = response ? actual.code : undefined;
  /** Response status checks, e.g. `pm.response.to.be.ok` (2xx). */
  const statusIn = (low, high, label) => assert(code >= low && code <= high, `expected response to be ${label}, got status ${code}`, `expected response not to be ${label}, got status ${code}`);
  const statusIs = (expected, label) => assert(code === expected, `expected response to be ${label} (${expected}), got status ${code}`, `expected response not to be ${label} (${expected})`);
  const json = () => { try { return { ok: true, value: actual.json() }; } catch { return { ok: false }; } };

  getter("ok", () => response ? statusIn(200, 299, "ok (2xx)") : assert(Boolean(actual), `expected ${__show(actual)} to be truthy`));
  if (response) {
    getter("success", () => statusIn(200, 299, "successful (2xx)"));
    getter("info", () => statusIn(100, 199, "informational (1xx)"));
    getter("redirection", () => statusIn(300, 399, "a redirection (3xx)"));
    getter("clientError", () => statusIn(400, 499, "a client error (4xx)"));
    getter("serverError", () => statusIn(500, 599, "a server error (5xx)"));
    getter("error", () => statusIn(400, 599, "an error (4xx or 5xx)"));
    getter("accepted", () => statusIs(202, "accepted"));
    getter("badRequest", () => statusIs(400, "a bad request"));
    getter("unauthorized", () => statusIs(401, "unauthorized"));
    getter("forbidden", () => statusIs(403, "forbidden"));
    getter("notFound", () => statusIs(404, "not found"));
    getter("rateLimited", () => statusIs(429, "rate limited"));
    getter("json", () => assert(json().ok, "expected response to have a JSON body", "expected response not to have a JSON body"));
    getter("withBody", () => assert(actual.text().length > 0, "expected response to have a body", "expected response to have no body"));
  }
  getter("true", () => assert(actual === true, `expected ${__show(actual)} to be true`));
  getter("false", () => assert(actual === false, `expected ${__show(actual)} to be false`));
  getter("null", () => assert(actual === null, `expected ${__show(actual)} to be null`));
  getter("undefined", () => assert(actual === undefined, `expected ${__show(actual)} to be undefined`));
  getter("NaN", () => assert(Number.isNaN(actual), `expected ${__show(actual)} to be NaN`));
  getter("exist", () => assert(actual !== null && actual !== undefined, `expected value to exist`, `expected ${__show(actual)} not to exist`));
  getter("empty", () => assert(
    actual != null && (typeof actual === "object" && !Array.isArray(actual) ? Object.keys(actual).length === 0 : actual.length === 0),
    `expected ${__show(actual)} to be empty`,
  ));

  const equal = (e) => assert(state.deep ? __deepEqual(actual, e) : actual === e, `expected ${__show(actual)} to ${state.deep ? "deeply " : ""}equal ${__show(e)}`);
  chain.equal = chain.equals = chain.eq = equal;
  chain.eql = chain.eqls = (e) => assert(__deepEqual(actual, e), `expected ${__show(actual)} to deeply equal ${__show(e)}`);
  const include = (e) => assert(
    actual != null && (
      typeof actual === "string" ? actual.includes(e)
      : Array.isArray(actual) ? actual.some((x) => __deepEqual(x, e))
      : typeof actual !== "object" ? false
      : e !== null && typeof e === "object" ? Object.entries(e).every(([k, v]) => __deepEqual(actual[k], v))
      : e in actual),
    `expected ${__show(actual)} to include ${__show(e)}`,
  );
  chain.include = chain.includes = chain.contain = chain.contains = include;
  chain.a = chain.an = (type) => assert(typeOf(actual) === String(type).toLowerCase(), `expected ${__show(actual)} to be a${/^[aeiou]/i.test(String(type)) ? "n" : ""} ${type}`);
  chain.instanceOf = chain.instanceof = (type) => assert(actual instanceof type, `expected ${__show(actual)} to be an instance of ${type && type.name}`);
  chain.above = chain.gt = chain.greaterThan = (n) => assert(actual > n, `expected ${__show(actual)} to be above ${n}`);
  chain.below = chain.lt = chain.lessThan = (n) => assert(actual < n, `expected ${__show(actual)} to be below ${n}`);
  chain.least = chain.gte = (n) => assert(actual >= n, `expected ${__show(actual)} to be at least ${n}`);
  chain.most = chain.lte = (n) => assert(actual <= n, `expected ${__show(actual)} to be at most ${n}`);
  chain.within = (lo, hi) => assert(actual >= lo && actual <= hi, `expected ${__show(actual)} to be within ${lo}..${hi}`);
  chain.oneOf = (list) => assert(Array.isArray(list) && list.some((x) => __deepEqual(x, actual)), `expected ${__show(actual)} to be one of ${__show(list)}`);
  chain.match = chain.matches = (re) => assert((re instanceof RegExp ? re : new RegExp(re)).test(String(actual)), `expected ${__show(actual)} to match ${re}`);
  chain.string = (text) => assert(typeof actual === "string" && actual.includes(text), `expected ${__show(actual)} to contain ${__show(text)}`);
  chain.property = (k, ...v) => assert(
    actual != null && k in Object(actual) && (v.length === 0 || __deepEqual(actual[k], v[0])),
    `expected ${__show(actual)} to have property ${__show(k)}${v.length === 0 ? "" : ` = ${__show(v[0])}`}`,
  );
  chain.lengthOf = chain.length = (n) => assert(actual != null && actual.length === n, `expected length ${n}, got ${actual == null ? actual : actual.length}`);
  chain.keys = chain.key = (...keys) => assert(actual != null && keys.flat().every((k) => k in Object(actual)), `expected ${__show(actual)} to have keys ${__show(keys.flat())}`);
  chain.satisfy = chain.satisfies = (fn) => assert(fn(actual), `expected ${__show(actual)} to satisfy the condition`);
  // Response assertions: pm.response.to.have.status(200), .header("x"), .body("..."), .jsonBody()
  chain.status = (expected) => typeof expected === "number"
    ? assert(actual.code === expected, `expected response to have status ${expected}, got ${actual.code}`, `expected response not to have status ${expected}`)
    : assert(actual.status === expected, `expected response to have status "${expected}", got "${actual.status}"`);
  chain.header = (name, ...value) => assert(
    actual.headers.has(name) && (value.length === 0 || actual.headers.get(name) === String(value[0])),
    value.length === 0
      ? `expected response to have header ${__show(name)}`
      : `expected response header ${__show(name)} to be ${__show(value[0])}, got ${__show(actual.headers.get(name))}`,
  );
  chain.body = (...expected) => {
    const text = actual.text();
    if (expected.length === 0) return assert(text.length > 0, "expected response to have a body", "expected response to have no body");
    const e = expected[0];
    if (e instanceof RegExp) return assert(e.test(text), `expected response body to match ${e}`);
    if (e !== null && typeof e === "object") {
      const body = json();
      return assert(body.ok && __deepEqual(body.value, e), `expected response body to deeply equal ${__show(e)}`);
    }
    return assert(text === String(e), `expected response body to equal ${__show(e)}`);
  };
  chain.jsonBody = (...args) => {
    const body = json();
    if (!body.ok || args.length === 0) return assert(body.ok && args.length === 0, "expected response to have a JSON body", "expected response not to have a JSON body");
    if (typeof args[0] !== "string") return assert(__deepEqual(body.value, args[0]), `expected the JSON body to deeply equal ${__show(args[0])}`);
    let value = body.value;
    for (const key of args[0].split(".")) value = value == null ? undefined : value[key];
    return args.length === 1
      ? assert(value !== undefined, `expected the JSON body to have ${__show(args[0])}`)
      : assert(__deepEqual(value, args[1]), `expected ${__show(args[0])} in the JSON body to equal ${__show(args[1])}, got ${__show(value)}`);
  };
  return chain;
};

/** A response as Postman scripts see it: `code`, `status` (the text), `json()`, `to.have...`. */
const __pmResponse = (response) => {
  const res = {
    [__isResponse]: true,
    code: response.status,
    status: response.statusText,
    responseTime: response.timeMs,
    headers: response.headers,
    json: () => response.json(),
    text: () => response.text(),
    get to() { return __chai(res).to; },
  };
  return res;
};

/** URL-encodes Postman `urlencoded` body fields. */
const __formBody = (fields) => (Array.isArray(fields) ? fields : [])
  .filter((f) => f && !f.disabled)
  .map((f) => `${encodeURIComponent(f.key)}=${encodeURIComponent(f.value ?? "")}`)
  .join("&");

/** Converts `pm.sendRequest` options (a URL or a Postman request object) to `ctx.http` options. */
const __postmanRequest = (options) => {
  if (typeof options === "string") return { url: options };
  const headers = new Headers(options.header ?? options.headers ?? []);
  const body = options.body;
  let text;
  if (body === undefined || body === null || typeof body !== "object") {
    text = body;
  } else if (body.mode === "urlencoded") {
    text = __formBody(body.urlencoded);
    if (!headers.has("content-type")) headers.set("Content-Type", "application/x-www-form-urlencoded");
  } else if (body.mode === "graphql") {
    text = JSON.stringify(body.graphql ?? {});
    if (!headers.has("content-type")) headers.set("Content-Type", "application/json");
  } else if (body.mode === "formdata" || body.mode === "file") {
    throw new Error(`pm.sendRequest does not support "${body.mode}" bodies; use "raw" or "urlencoded"`);
  } else if (body.raw !== undefined) {
    text = body.raw;
  } else {
    text = JSON.stringify(body);
  }
  return { url: String(options.url), method: options.method, headers, body: text };
};

/** Replaces `{{name}}` with variable values, like `pm.variables.replaceIn`. */
const __replaceIn = (vars, text) =>
  String(text).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (match, name) => (vars.has(name) ? vars.get(name) : match));

const __errorText = (e) => (e && e.message ? e.message : String(e));

const __runScript = async (phase, code, inputJson) => {
  const input = JSON.parse(inputJson);
  const vars = new Map(input.variables);
  const output = { set_variables: [], unset_variables: [], tests: [], request: null };
  const setVar = (k, v) => { vars.set(String(k), String(v)); output.set_variables.push([String(k), String(v)]); };
  const unsetVar = (k) => { vars.delete(String(k)); output.unset_variables.push(String(k)); };
  const variables = Object.freeze({
    get: (k) => vars.get(String(k)),
    set: setVar,
    unset: unsetVar,
    has: (k) => vars.has(String(k)),
    toObject: () => Object.fromEntries(vars),
    replaceIn: (text) => __replaceIn(vars, text),
  });

  const pending = [];
  /** Registers a test. Results keep the order tests were declared in, even for async tests. */
  const test = (name, fn) => {
    const record = { name: String(name), passed: true, detail: null };
    output.tests.push(record);
    const fail = (e) => { record.passed = false; record.detail = __errorText(e); };
    try {
      if (typeof fn !== "function") throw new Error(`test "${record.name}" needs a function`);
      const result = fn();
      if (result && typeof result.then === "function") pending.push(Promise.resolve(result).then(() => {}, fail));
    } catch (e) {
      fail(e);
    }
  };

  const request = __request(input.request);
  const response = input.response ? __response(input.response) : undefined;

  const pt = Object.freeze({
    phase,
    request,
    response,
    environment: input.environment ?? null,
    variables,
    test,
    expect,
    log: (...a) => console.log(...a),
    http: __http,
  });

  // Postman compatibility.
  const pmResponse = response && __pmResponse(response);
  const pmRequest = {
    get url() { return { toString: () => request.url, valueOf: () => request.url }; },
    set url(v) { request.url = String(v); },
    get method() { return request.method; },
    set method(v) { request.method = String(v); },
    headers: request.headers,
    get body() { return { raw: request.body, toString: () => request.body }; },
  };
  const pm = Object.freeze({
    test,
    expect: (v) => __chai(v),
    response: pmResponse,
    request: pmRequest,
    environment: variables,
    variables,
    collectionVariables: variables,
    globals: variables,
    info: Object.freeze({ eventName: phase === "before_request" ? "prerequest" : "test" }),
    sendRequest: (options, callback) => {
      const promise = Promise.resolve()
        .then(() => __http.request(__postmanRequest(options)))
        .then(__pmResponse);
      if (typeof callback === "function") {
        // Like Postman: errors go to the callback; an error thrown by the callback fails the script.
        pending.push(promise.then((res) => callback(null, res), (err) => callback(err, null)));
        return undefined;
      }
      return promise;
    },
  });

  let run;
  try {
    run = new Function("pt", "pm", "console", "ctx", `return (async () => {\n${code}\n})();`);
  } catch (e) {
    // QuickJS messages such as "unexpected token in expression" do not say they are syntax errors.
    throw new Error(`syntax error in the script: ${__errorText(e)}`);
  }
  await run(pt, pm, console, ctx);
  // Callbacks and async tests may start more work; wait until everything has settled.
  while (pending.length > 0) await Promise.all(pending.splice(0));
  if (phase === "before_request") output.request = __serializeRequest(request);
  return JSON.stringify(output);
};

/** Globals that exist before any script runs; see `__resetGlobals`. */
let __baseline = new Set();

/** Removes globals a script created (e.g. `token = 1` without `let`), so the next script starts clean. */
const __resetGlobals = () => {
  for (const key of Reflect.ownKeys(globalThis)) {
    if (!__baseline.has(key)) {
      try { delete globalThis[key]; } catch { /* non-configurable: nothing to do */ }
    }
  }
};

/** Runs a request script. `input` is the JSON from the host; returns a JSON string. */
__expose("__pt_run_script", async (phase, code, inputJson) => {
  try {
    return await __runScript(phase, code, inputJson);
  } finally {
    __resetGlobals();
  }
});

__baseline = new Set(Reflect.ownKeys(globalThis));
})();
