// post-tester sandbox prelude.
//
// Runs once in every sandbox before any plugin or script code. It turns the
// raw host functions (`__host.*`, which pass JSON strings) into the friendly
// APIs plugins and scripts use: `ctx`, `pt`, `pm`, `console`, `expect`.
//
// Host functions return an envelope: {"ok": value} or {"error": "message"}.
"use strict";

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

globalThis.console = {
  log: (...args) => __host.log("info", args.map(__text).join(" ")),
  info: (...args) => __host.log("info", args.map(__text).join(" ")),
  debug: (...args) => __host.log("debug", args.map(__text).join(" ")),
  warn: (...args) => __host.log("warn", args.map(__text).join(" ")),
  error: (...args) => __host.log("error", args.map(__text).join(" ")),
};

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
  /** Sends a request. Options: { method, url, headers, body, json, timeoutMs }. */
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
globalThis.ctx = ctx;

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
globalThis.__pt_dispatch = async (plugin, kind, name, payload) => {
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
globalThis.__pt_call = async (kind, name, payloadJson) => {
  const result = await __pt_dispatch(globalThis.__pt_plugin, kind, name, JSON.parse(payloadJson));
  return JSON.stringify(result === undefined ? null : result);
};

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
  const ka = Object.keys(a), kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => __deepEqual(a[k], b[k]));
};

globalThis.expect = (actual) => {
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

// ── Test runner for `pt plugin test` ───────────────────────────────────────

const __tests = [];
globalThis.test = (name, fn) => __tests.push({ name: String(name), fn });
globalThis.it = globalThis.test;
globalThis.describe = (name, fn) => fn();
globalThis.__pt_run_tests = async () => {
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
};

// ── Request scripts: the `pt` API and Postman's `pm` API ───────────────────

/** Chai-style assertions used by `pm.expect` (a commonly used subset). */
const __chai = (actual, negate = false) => {
  const state = { negate, deep: false };
  const assert = (passed, message) => {
    if (passed === state.negate) throw new Error(state.negate ? message.replace("expected", "expected not") : message);
    return chain;
  };
  const typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : typeof v);
  const chain = {};
  for (const word of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "does"]) {
    Object.defineProperty(chain, word, { get: () => chain });
  }
  Object.defineProperty(chain, "not", { get: () => { state.negate = !state.negate; return chain; } });
  Object.defineProperty(chain, "deep", { get: () => { state.deep = true; return chain; } });
  Object.defineProperty(chain, "ok", { get: () => assert(Boolean(actual), `expected ${__show(actual)} to be truthy`) });
  Object.defineProperty(chain, "true", { get: () => assert(actual === true, `expected ${__show(actual)} to be true`) });
  Object.defineProperty(chain, "false", { get: () => assert(actual === false, `expected ${__show(actual)} to be false`) });
  Object.defineProperty(chain, "null", { get: () => assert(actual === null, `expected ${__show(actual)} to be null`) });
  Object.defineProperty(chain, "undefined", { get: () => assert(actual === undefined, `expected ${__show(actual)} to be undefined`) });
  Object.defineProperty(chain, "exist", { get: () => assert(actual !== null && actual !== undefined, `expected value to exist`) });
  Object.defineProperty(chain, "empty", {
    get: () => assert(actual != null && (typeof actual === "object" && !Array.isArray(actual) ? Object.keys(actual).length === 0 : actual.length === 0), `expected ${__show(actual)} to be empty`),
  });
  const equal = (e) => assert(state.deep ? __deepEqual(actual, e) : actual === e, `expected ${__show(actual)} to equal ${__show(e)}`);
  chain.equal = chain.equals = chain.eq = equal;
  chain.eql = (e) => assert(__deepEqual(actual, e), `expected ${__show(actual)} to deeply equal ${__show(e)}`);
  const include = (e) => assert(
    actual != null && (typeof actual === "string" ? actual.includes(e) : Array.isArray(actual) ? actual.some((x) => __deepEqual(x, e)) : typeof e === "object" ? Object.entries(e).every(([k, v]) => __deepEqual(actual[k], v)) : e in actual),
    `expected ${__show(actual)} to include ${__show(e)}`,
  );
  chain.include = chain.includes = chain.contain = chain.contains = include;
  chain.a = chain.an = (type) => assert(typeOf(actual) === String(type).toLowerCase(), `expected ${__show(actual)} to be a ${type}`);
  chain.above = chain.gt = chain.greaterThan = (n) => assert(actual > n, `expected ${__show(actual)} to be above ${n}`);
  chain.below = chain.lt = chain.lessThan = (n) => assert(actual < n, `expected ${__show(actual)} to be below ${n}`);
  chain.least = chain.gte = (n) => assert(actual >= n, `expected ${__show(actual)} to be at least ${n}`);
  chain.most = chain.lte = (n) => assert(actual <= n, `expected ${__show(actual)} to be at most ${n}`);
  chain.within = (lo, hi) => assert(actual >= lo && actual <= hi, `expected ${__show(actual)} to be within ${lo}..${hi}`);
  chain.oneOf = (list) => assert(list.some((x) => __deepEqual(x, actual)), `expected ${__show(actual)} to be one of ${__show(list)}`);
  chain.match = (re) => assert(re.test(String(actual)), `expected ${__show(actual)} to match ${re}`);
  chain.property = (k, v) => assert(actual != null && k in Object(actual) && (v === undefined || __deepEqual(actual[k], v)), `expected ${__show(actual)} to have property ${__show(k)}`);
  chain.lengthOf = chain.length = (n) => assert(actual != null && actual.length === n, `expected length ${n}, got ${actual == null ? actual : actual.length}`);
  chain.keys = (...keys) => assert(actual != null && keys.flat().every((k) => k in actual), `expected ${__show(actual)} to have keys ${__show(keys.flat())}`);
  // Response-specific: pm.response.to.have.status(200), .header("x"), .jsonBody()
  chain.status = (code) => typeof code === "number"
    ? assert(actual.code === code, `expected response to have status ${code}, got ${actual.code}`)
    : assert(actual.status === code, `expected response to have status "${code}", got "${actual.status}"`);
  chain.header = (name, value) => assert(actual.headers.has(name) && (value === undefined || actual.headers.get(name) === value), `expected response to have header ${__show(name)}`);
  chain.jsonBody = () => { try { actual.json(); return assert(true, ""); } catch { return assert(false, "expected response to have a JSON body"); } };
  chain.body = (text) => assert(actual.text().includes(text), `expected response body to include ${__show(text)}`);
  return chain;
};

/** Runs a request script. `input` is the JSON from the host; returns a JSON string. */
globalThis.__pt_run_script = async (phase, code, inputJson) => {
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
  });

  const pending = [];
  const test = (name, fn) => {
    const record = (passed, detail) => output.tests.push({ name: String(name), passed, detail });
    try {
      const result = fn();
      if (result && typeof result.then === "function") {
        pending.push(result.then(() => record(true, null), (e) => record(false, e && e.message ? e.message : String(e))));
      } else {
        record(true, null);
      }
    } catch (e) {
      record(false, e && e.message ? e.message : String(e));
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
    expect: globalThis.expect,
    log: (...a) => console.log(...a),
    http: __http,
  });

  // Postman compatibility.
  const pmResponse = response && {
    code: response.status,
    status: response.statusText,
    responseTime: response.timeMs,
    headers: response.headers,
    json: () => response.json(),
    text: () => response.text(),
    get to() { return __chai(pmResponse).to; },
  };
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
      const opts = typeof options === "string" ? { url: options } : {
        url: typeof options.url === "string" ? options.url : String(options.url),
        method: options.method,
        headers: options.header ?? options.headers,
        body: options.body && options.body.raw !== undefined ? options.body.raw : options.body,
      };
      const promise = __http.request(opts).then((res) => ({ code: res.status, status: res.statusText, headers: res.headers, json: () => res.json(), text: () => res.text() }));
      if (typeof callback === "function") {
        pending.push(promise.then((res) => callback(null, res), (err) => callback(err, null)));
        return undefined;
      }
      return promise;
    },
  });

  const run = new Function("pt", "pm", "console", "ctx", `return (async () => {\n${code}\n})();`);
  await run(pt, pm, console, ctx);
  await Promise.all(pending);
  if (phase === "before_request") output.request = __serializeRequest(request);
  return JSON.stringify(output);
};
