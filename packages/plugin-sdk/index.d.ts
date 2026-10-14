/**
 * # @post-tester/plugin-sdk
 *
 * Types for writing post-tester plugins. A plugin is a folder with a
 * `plugin.json` and an entry file that default-exports {@link definePlugin}:
 *
 * ```ts
 * import { definePlugin } from "@post-tester/plugin-sdk";
 *
 * export default definePlugin({
 *   templateFunctions: {
 *     shout: {
 *       description: "Returns the text in upper case.",
 *       args: [{ name: "text", type: "string", description: "Text to convert" }],
 *       run: ({ text }) => String(text).toUpperCase(),
 *     },
 *   },
 * });
 * ```
 *
 * Every key of the definition must also be listed in `plugin.json` → `provides`:
 *
 * | `provides`          | `definePlugin` key   | Adds                                             |
 * |---------------------|----------------------|--------------------------------------------------|
 * | `template-function` | `templateFunctions`  | Functions usable in `{{ }}` templates            |
 * | `auth`              | `auth`               | Authentication methods                           |
 * | `hook`              | `hooks`              | Code that runs around every request              |
 * | `importer`          | `importers`          | Importing requests from another format           |
 * | `exporter`          | `exporters`          | Exporting a collection to another format         |
 * | `codegen`           | `codegen`            | "Copy as code" generators                        |
 * | `viewer`            | `viewers`            | Custom response views (HTML)                     |
 * | `mcp-tool`          | `mcpTools`           | Tools AI agents can call through MCP             |
 *
 * Plugins run in a sandbox (QuickJS). There is no `fetch`, `require`, `process`,
 * `URL`, `TextEncoder`, `setTimeout` or `Intl`; everything outside the sandbox
 * goes through {@link PluginContext | ctx}. Standard JavaScript built-ins,
 * `atob`/`btoa`, `queueMicrotask` and `console` are available.
 *
 * Status: early development; the API may change before 1.0.
 *
 * @packageDocumentation
 */

// ── Plugin definition ───────────────────────────────────────────────────────

/**
 * Declares a plugin. Use it as the default export of the entry file named in
 * `plugin.json` → `main`.
 *
 * It returns the definition unchanged after checking that every top-level key
 * is known, so a typo such as `templateFunction` (missing `s`) fails when the
 * plugin loads, not when it is first used.
 *
 * @throws If `definition` is not an object or has an unknown top-level key.
 *
 * @example
 * ```ts
 * export default definePlugin({
 *   hooks: {
 *     beforeRequest: (request, ctx) => {
 *       request.headers.set("X-Request-Id", ctx.random.uuid());
 *       return request;
 *     },
 *   },
 * });
 * ```
 */
export declare function definePlugin<T extends PluginDefinition>(definition: T): T;

/** The SDK version built into post-tester, e.g. `"0.1.0"`. */
export declare const version: string;

/**
 * Everything a plugin can provide. Each key is optional, but each key you use
 * must be listed in `plugin.json` → `provides` (and vice versa); `pt plugin
 * validate` reports any mismatch.
 */
export interface PluginDefinition {
  /**
   * Functions usable in `{{ }}` templates, keyed by function name.
   * `provides`: `"template-function"`.
   *
   * @example
   * ```ts
   * templateFunctions: {
   *   shout: { description: "Upper-cases text.", args: [{ name: "text" }], run: ({ text }) => String(text).toUpperCase() },
   * }
   * // In a request: {{shout("hello")}} or {{shout(text: "hello")}}
   * ```
   */
  templateFunctions?: Record<string, TemplateFunctionDefinition>;

  /**
   * Authentication methods, keyed by method name. A request selects one with
   * `auth: { type: plugin, name: "<plugin-name>/<method>", config: { ... } }`.
   * `provides`: `"auth"`.
   */
  auth?: Record<string, AuthDefinition>;

  /**
   * Code that runs around every request sent while the plugin is installed.
   * `provides`: `"hook"`.
   */
  hooks?: Hooks;

  /**
   * Importers that turn text (a file's content) into requests, keyed by name.
   * `provides`: `"importer"`.
   */
  importers?: Record<string, ImporterDefinition>;

  /**
   * Exporters that turn a collection into text, keyed by name.
   * `provides`: `"exporter"`.
   */
  exporters?: Record<string, ExporterDefinition>;

  /**
   * Code generators ("copy as code"), keyed by name.
   * `provides`: `"codegen"`.
   */
  codegen?: Record<string, CodegenDefinition>;

  /**
   * Response viewers that render a response as HTML, keyed by name.
   * `provides`: `"viewer"`.
   */
  viewers?: Record<string, ViewerDefinition>;

  /**
   * Tools that AI agents can call through post-tester's MCP server, keyed by
   * tool name. Agents see each tool as `<plugin_name>__<tool>`, with `-` in
   * the plugin name replaced by `_` (e.g. `jwt_tools__decode_jwt`).
   * `provides`: `"mcp-tool"`.
   */
  mcpTools?: Record<string, McpToolDefinition>;
}

/**
 * A value a plugin function may return either directly or as a promise.
 * Every plugin function may be `async`.
 */
export type MaybePromise<T> = T | Promise<T>;

// ── Template functions ──────────────────────────────────────────────────────

/**
 * A function usable in templates: `{{name(arg: "value")}}`.
 *
 * Arguments are JSON values written in the template: `"text"` (strings need
 * double quotes), numbers, `true`/`false`, arrays and objects. They may be
 * named (`{{totp(secret: "JBSW...", digits: 8)}}`) or positional
 * (`{{totp("JBSW...")}}`); positional arguments are matched to {@link args}
 * in order. Templates inside arguments are **not** expanded.
 *
 * @example
 * ```ts
 * templateFunctions: {
 *   repeat: {
 *     description: "Repeats a text a number of times.",
 *     args: [
 *       { name: "text", type: "string", description: "Text to repeat" },
 *       { name: "times", type: "number", description: "How often", default: 2 },
 *     ],
 *     run: ({ text, times }) => String(text).repeat(Number(times)),
 *   },
 * }
 * // {{repeat("ab", 3)}} → "ababab"
 * ```
 */
export interface TemplateFunctionDefinition {
  /**
   * What the function returns, in one sentence. Required: it is shown to
   * users and AI agents, and `pt plugin validate` fails without it.
   */
  description: string;

  /**
   * The arguments, in positional order. Arguments are required unless they
   * have a `default`, `required: false` or `optional: true`; a missing
   * required argument fails with `missing argument "<name>"`.
   */
  args?: TemplateArgument[];

  /**
   * Computes the value inserted into the template.
   *
   * @param args - Arguments by name, after defaults are applied. Values keep
   *   their JSON type (a number stays a number); convert them if needed.
   * @param ctx - The {@link PluginContext}.
   * @returns The text to insert. Strings are inserted as-is, `null`/`undefined`
   *   become an empty string, and anything else is inserted as JSON. The result
   *   may itself contain `{{variables}}`, which are then expanded.
   */
  run(args: TemplateArgs, ctx: PluginContext): MaybePromise<unknown>;
}

/**
 * Arguments passed to {@link TemplateFunctionDefinition.run}, keyed by the
 * declared argument names. Typed loosely so destructuring works; validate and
 * convert values yourself.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type TemplateArgs = Record<string, any>;

/** One argument of a {@link TemplateFunctionDefinition}. */
export interface TemplateArgument {
  /** The argument name, used for named arguments: `{{fn(name: "value")}}`. */
  name: string;
  /**
   * The expected type, shown to users and AI agents. Informational only:
   * values are not converted. Defaults to `"string"`.
   */
  type?: "string" | "number" | "boolean" | "object" | "array" | (string & {});
  /** What the argument means, shown to users and AI agents. */
  description?: string;
  /** Used when the argument is not given. Makes the argument optional. */
  default?: unknown;
  /** Set to `false` to make the argument optional (default: `true`). */
  required?: boolean;
  /** Set to `true` to make the argument optional (same as `required: false`). */
  optional?: boolean;
}

// ── Auth ────────────────────────────────────────────────────────────────────

/**
 * An authentication method. It receives the fully prepared request (templates
 * expanded, built-in headers and body set) and adds whatever it needs.
 *
 * A request uses it with:
 *
 * ```yaml
 * auth:
 *   type: plugin
 *   name: hmac-signature/hmac   # "<plugin-name>/<method>", or just "<method>" if unique
 *   config:
 *     keyId: my-key
 *     secret: "{{secret:signing-key}}"   # templates in config values are expanded
 * ```
 *
 * @example
 * ```ts
 * auth: {
 *   "custom-token": {
 *     displayName: "Custom token",
 *     fields: [{ name: "token", type: "secret", label: "Token" }],
 *     apply: (request, { token }) => {
 *       request.headers.set("X-Custom-Token", String(token));
 *       return request;
 *     },
 *   },
 * }
 * ```
 */
export interface AuthDefinition {
  /** Name shown in the app's auth picker. Defaults to the method's key. */
  displayName?: string;
  /** The settings users fill in. Their values arrive in `config`. */
  fields?: AuthField[];
  /**
   * Adds authentication to the request.
   *
   * @param request - The request about to be sent. Change it in place and
   *   return it, or return a new {@link RequestInit}.
   * @param config - The request's `auth.config`, with templates such as
   *   `{{secret:name}}` already expanded.
   * @param ctx - The {@link PluginContext}.
   * @returns The request to send. Returning nothing sends `request` as changed in place.
   */
  apply(request: PluginRequest, config: AuthConfig, ctx: PluginContext): MaybePromise<PluginRequest | RequestInit | void>;
}

/**
 * The `config` of an auth method: the values of its {@link AuthField}s by
 * name. String values have their templates expanded; other JSON values are
 * passed as written.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type AuthConfig = Record<string, any>;

/** One setting of an {@link AuthDefinition}, shown as a form field in the app. */
export interface AuthField {
  /** Key of the value in `config`. */
  name: string;
  /**
   * Input type. Use `"secret"` for values that must be masked; users should
   * reference a stored secret, e.g. `{{secret:api-key}}`. Defaults to `"string"`.
   */
  type?: "string" | "secret" | "number" | "boolean" | (string & {});
  /** Label shown next to the field. Defaults to `name`. */
  label?: string;
}

// ── Hooks ───────────────────────────────────────────────────────────────────

/**
 * Hooks run for every request sent while the plugin is installed, in plugin
 * load order, after auth has been applied. Only `beforeRequest` and
 * `afterResponse` exist; any other key is reported by `pt plugin validate`.
 *
 * @example
 * ```ts
 * hooks: {
 *   beforeRequest: (request, ctx) => {
 *     if (!request.headers.has("X-Request-Id")) request.headers.set("X-Request-Id", ctx.random.uuid());
 *     return request;
 *   },
 *   afterResponse: (response, ctx) => {
 *     if (response.timeMs > 1000) ctx.log.warn(`slow response: ${response.timeMs} ms`);
 *     return response;
 *   },
 * }
 * ```
 */
export interface Hooks {
  /**
   * Runs just before the request is sent. Change the request in place and
   * return it (or return nothing), or return a new {@link RequestInit}.
   * Throwing an error stops the request and shows the message.
   */
  beforeRequest?(request: PluginRequest, ctx: PluginContext): MaybePromise<PluginRequest | RequestInit | void>;

  /**
   * Runs when the response has arrived, before tests and after-response
   * scripts. Change the response in place and return it (or return nothing),
   * or return a new {@link ResponseInit}. Changes are what tests and the app see.
   */
  afterResponse?(response: PluginResponse, ctx: PluginContext): MaybePromise<PluginResponse | ResponseInit | void>;
}

// ── Importers ───────────────────────────────────────────────────────────────

/**
 * Turns text in another format into requests. When importing, post-tester
 * asks each importer's {@link detect} in turn and uses the first one that
 * returns `true`.
 *
 * @example
 * ```ts
 * importers: {
 *   "url-list": {
 *     displayName: "URL list",
 *     fileExtensions: [".txt"],
 *     detect: (text) => text.trim().split("\n").every((l) => l.startsWith("http")),
 *     import: (text) => ({
 *       name: "URL list",
 *       requests: text.trim().split("\n").map((url) => ({ method: "GET", url: url.trim() })),
 *     }),
 *   },
 * }
 * ```
 */
export interface ImporterDefinition {
  /** Name shown in the app's import dialog. Defaults to the importer's key. */
  displayName?: string;
  /** File extensions this importer reads, with the dot, e.g. `[".har"]`. */
  fileExtensions?: string[];
  /**
   * Returns `true` if `text` is in this importer's format. Keep it fast and
   * never throw: an error counts as "not detected". Without `detect`, the
   * importer is never chosen automatically.
   */
  detect?(text: string, ctx: PluginContext): MaybePromise<boolean>;
  /**
   * Converts the text. Throw an `Error` with a clear message if the text is
   * invalid (say what is wrong and where).
   */
  import(text: string, ctx: PluginContext): MaybePromise<ImportResult>;
}

/** What an importer returns. */
export interface ImportResult {
  /** Name of the imported collection; used as its folder name. Defaults to `"Imported"`. */
  name?: string;
  /** The imported requests, in order. */
  requests: ImportedRequest[];
}

/**
 * A request returned by an importer. This is the workspace's request format
 * (the same fields as a request YAML file), plus `folder`. Unknown fields are
 * rejected, so only use the fields below. Field names are exactly as shown
 * (note the snake_case in `scripts` and `options`).
 *
 * @example
 * ```ts
 * {
 *   folder: "users",
 *   name: "Create user",
 *   method: "POST",
 *   url: "https://api.example.com/users",
 *   query: [{ key: "notify", value: "true" }],
 *   headers: [{ key: "Accept", value: "application/json" }],
 *   body: { type: "json", content: '{"name": "Ada"}' },
 *   tests: ["status == 201"],
 * }
 * ```
 */
export interface ImportedRequest {
  /**
   * Sub-folder inside the imported collection, e.g. `"users"` or
   * `"users/admin"`. Segments may contain letters, numbers, spaces, `-`, `_`
   * and `.`, separated by `/`. Defaults to the collection's root.
   */
  folder?: string;
  /** Display name. Also used (as a slug) for the file name; defaults to the URL. */
  name?: string;
  /** A longer explanation, shown in the app and to AI agents. */
  description?: string;
  /** HTTP method, e.g. `"GET"`. Defaults to `"GET"`. */
  method?: string;
  /**
   * The URL. May contain `{{variables}}`. Put query parameters in
   * {@link query} instead of the URL so they are easy to edit.
   */
  url: string;
  /** Query parameters, appended to the URL when sent. */
  query?: KeyValue[];
  /**
   * Headers. Any {@link HeadersInit} form is accepted; they are stored as
   * `{ key, value }` pairs.
   */
  headers?: HeadersInit;
  /** The body. Omit for no body. */
  body?: RequestBody;
  /** How the request authenticates. Omit to inherit from the folder. */
  auth?: RequestAuth;
  /** JavaScript run before the request and after the response. */
  scripts?: { before_request?: string; after_response?: string };
  /** Assertions, e.g. `"status == 200"` or `"json($.id) exists"`. */
  tests?: string[];
  /** Variables to capture from the response, e.g. `{ token: "json($.token)" }`. */
  extract?: Record<string, string>;
  /** Transport options. */
  options?: RequestOptions;
}

/** A key/value pair (query parameter, header or form field) that can be switched off. */
export interface KeyValue {
  key: string;
  value: string;
  /** Set to `false` to keep the pair but not send it. Defaults to `true`. */
  enabled?: boolean;
}

/**
 * A request body in the workspace format. `json`, `form` and `graphql` bodies
 * set their `Content-Type` automatically unless a header sets one.
 */
export type RequestBody =
  | { type: "none" }
  /** Raw text with an explicit content type (default `text/plain`). */
  | { type: "text"; content: string; content_type?: string }
  /** JSON text, sent with `Content-Type: application/json`. */
  | { type: "json"; content: string }
  /** `application/x-www-form-urlencoded` fields. */
  | { type: "form"; fields: KeyValue[] }
  /** A GraphQL operation; `variables` is JSON text. */
  | { type: "graphql"; query: string; variables?: string; operation_name?: string };

/** How a saved request authenticates. String values may contain templates. */
export type RequestAuth =
  /** Explicitly no auth (stops inheriting the folder's auth). */
  | { type: "none" }
  | { type: "basic"; username: string; password: string }
  | { type: "bearer"; token: string }
  | { type: "api_key"; key: string; value: string; location?: "header" | "query" }
  /** A plugin auth method: `name` is `"<plugin-name>/<method>"`. */
  | { type: "plugin"; name: string; config?: Record<string, unknown> };

/** Per-request transport options. Every field is optional. */
export interface RequestOptions {
  /** Time limit until the response headers arrive. Default `30000`. */
  timeout_ms?: number;
  /** Default `true`. */
  follow_redirects?: boolean;
  /** Default `10`. */
  max_redirects?: number;
  /** Set to `false` to accept invalid or self-signed certificates. Default `true`. */
  verify_tls?: boolean;
  /** Default `"auto"`. */
  http_version?: "auto" | "http1" | "http2";
}

// ── Exporters ───────────────────────────────────────────────────────────────

/**
 * Turns a collection into text, e.g. a document or another tool's format.
 *
 * @example
 * ```ts
 * exporters: {
 *   markdown: {
 *     displayName: "Markdown",
 *     fileExtension: ".md",
 *     export: (collection) => collection.requests.map((r) => `- ${r.method ?? "GET"} ${r.url}`).join("\n"),
 *   },
 * }
 * ```
 */
export interface ExporterDefinition {
  /** Name shown in the app's export menu. Defaults to the exporter's key. */
  displayName?: string;
  /** Extension of the produced file, with the dot, e.g. `".md"`. */
  fileExtension?: string;
  /** Produces the exported text. */
  export(collection: ExportCollection, ctx: PluginContext): MaybePromise<string>;
}

/** The collection given to an exporter. */
export interface ExportCollection {
  /** The collection's name. */
  name: string;
  /**
   * The saved requests, in the workspace's request format (unexpanded
   * templates such as `{{baseUrl}}` are kept). Treat fields other than `url`
   * as optional.
   */
  requests: ExportedRequest[];
}

/** A saved request given to an exporter. Same fields as {@link ImportedRequest}, with headers as `{ key, value }` pairs. */
export interface ExportedRequest extends Omit<ImportedRequest, "headers"> {
  /** The request's id in the workspace, e.g. `"users/get-user"`, when known. */
  id?: string;
  headers?: KeyValue[];
}

// ── Code generators ─────────────────────────────────────────────────────────

/**
 * Generates code that sends a request ("copy as code").
 *
 * @example
 * ```ts
 * codegen: {
 *   httpie: {
 *     displayName: "HTTPie",
 *     language: "shell",
 *     generate: (request) => `http ${request.method} '${request.url}'`,
 *   },
 * }
 * ```
 */
export interface CodegenDefinition {
  /** Name shown in the app's code menu. Defaults to the generator's key. */
  displayName?: string;
  /** Language used for syntax highlighting, e.g. `"go"`, `"python"`, `"shell"`. Defaults to `"text"`. */
  language?: string;
  /**
   * Returns the code.
   *
   * @param request - The fully prepared request: templates expanded, query
   *   parameters in the URL, auth headers set. Secrets appear in clear text,
   *   so do not log the request.
   */
  generate(request: PluginRequest, ctx: PluginContext): MaybePromise<string>;
}

// ── Viewers ─────────────────────────────────────────────────────────────────

/**
 * Renders a response as HTML, e.g. a table, chart or image.
 *
 * Escape every value taken from the response (`&`, `<`, `>`, `"`, `'`), since
 * responses come from servers you may not control.
 *
 * @example
 * ```ts
 * viewers: {
 *   status: {
 *     displayName: "Status",
 *     contentTypes: ["application/json"],
 *     render: (response) => `<h1>${response.status}</h1>`,
 *   },
 * }
 * ```
 */
export interface ViewerDefinition {
  /** Name shown on the viewer's tab. Defaults to the viewer's key. */
  displayName?: string;
  /** Content types this viewer is meant for, e.g. `["application/json"]`. */
  contentTypes?: string[];
  /** Returns an HTML fragment that displays the response. */
  render(response: PluginResponse, ctx: PluginContext): MaybePromise<string>;
}

// ── MCP tools ───────────────────────────────────────────────────────────────

/**
 * A tool AI agents can call through post-tester's MCP server. Agents choose
 * tools by their {@link description} and fill the input from
 * {@link inputSchema}, so make both precise.
 *
 * The tool is exposed as `<plugin_name>__<tool>` (plugin name with `-`
 * replaced by `_`), and every call appears in Live Activity.
 *
 * @example
 * ```ts
 * mcpTools: {
 *   word_count: {
 *     description: "Counts the words in a text.",
 *     inputSchema: {
 *       type: "object",
 *       properties: { text: { type: "string", description: "The text to count" } },
 *       required: ["text"],
 *     },
 *     run: ({ text }) => ({ words: String(text).split(/\s+/).filter(Boolean).length }),
 *   },
 * }
 * ```
 */
export interface McpToolDefinition {
  /**
   * When and why to call the tool, and what it returns. Required: `pt plugin
   * validate` fails without it.
   */
  description: string;
  /**
   * JSON Schema (an object schema) for the input. Defaults to `{ "type": "object" }`.
   * The input is not validated against it before `run`; check required values yourself.
   */
  inputSchema?: JsonSchema;
  /**
   * Runs the tool.
   *
   * @param input - The arguments the agent sent.
   * @returns Any JSON-compatible value (`undefined` becomes `null`). Throw an
   *   `Error` with an actionable message on failure; the agent sees it.
   */
  run(input: McpToolInput, ctx: PluginContext): MaybePromise<unknown>;
}

/** The input of an MCP tool call. Typed loosely so destructuring works; validate values yourself. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type McpToolInput = Record<string, any>;

/** A JSON Schema object (draft-07 style), e.g. `{ type: "object", properties: { ... }, required: [...] }`. */
export interface JsonSchema {
  type?: "object" | "string" | "number" | "integer" | "boolean" | "array" | "null" | (string & {});
  description?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  items?: JsonSchema;
  enum?: unknown[];
  default?: unknown;
  [keyword: string]: unknown;
}

// ── Requests and responses ──────────────────────────────────────────────────

/**
 * Anything that can initialize {@link Headers}:
 * - pairs: `[["Accept", "application/json"]]`
 * - key/value objects: `[{ key: "Accept", value: "application/json" }]`
 * - a plain object: `{ Accept: "application/json" }`
 * - another `Headers` instance.
 *
 * Values are converted to strings.
 */
export type HeadersInit =
  | Headers
  | Array<[string, string | number | boolean]>
  | Array<{ key: string; value?: string | number | boolean }>
  | Record<string, string | number | boolean>;

/**
 * Ordered, case-insensitive HTTP headers. Names keep the case they were set
 * with; lookups ignore case. The same name may appear more than once (see {@link add}).
 *
 * It is not a WHATWG `Headers`: it has `add`, `upsert`, `remove`, `toObject`,
 * and `get` returns `undefined` (not `null`) for a missing header.
 *
 * @example
 * ```ts
 * request.headers.set("Accept", "application/json");  // replace or add
 * request.headers.add("Cookie", "a=1");               // always add
 * request.headers.get("accept");                      // "application/json"
 * request.headers.delete("X-Debug");
 * for (const [name, value] of request.headers.entries()) { ... }
 * ```
 */
export interface Headers {
  /** The first value of the header (case-insensitive), or `undefined`. */
  get(name: string): string | undefined;
  /** Whether the header exists (case-insensitive). */
  has(name: string): boolean;
  /** Replaces the first header with this name, or adds it. */
  set(name: string, value: string | number | boolean): void;
  /** Adds a header, even if one with the same name exists. */
  add(name: string, value: string | number | boolean): void;
  /** Adds a header given as `{ key, value }` (Postman style). */
  add(item: { key: string; value?: string | number | boolean }): void;
  /** Postman-compatible `set` taking `{ key, value }`. */
  upsert(item: { key: string; value?: string | number | boolean }): void;
  /** Removes every header with this name (case-insensitive). */
  delete(name: string): void;
  /** Alias of {@link delete}. */
  remove(name: string): void;
  /** A copy of all headers as `[name, value]` pairs, in order. */
  entries(): Array<[string, string]>;
  /** All headers as an object. If a name repeats, the last value wins. */
  toObject(): Record<string, string>;
  /** Same as {@link entries}; used by `JSON.stringify`. */
  toJSON(): Array<[string, string]>;
}

/**
 * A request as plugins see it. Change it in place: set `method`, `url`,
 * `body`, and use the {@link Headers} methods.
 */
export interface PluginRequest {
  /** Upper-case HTTP method, e.g. `"POST"`. */
  method: string;
  /** The full URL including the query string, with templates expanded. */
  url: string;
  /** The request headers. */
  headers: Headers;
  /** The body as text (`""` when there is none). Binary bodies are not supported. */
  body: string;
}

/**
 * A request returned from `auth.apply` or `beforeRequest` instead of the
 * original object. `method` defaults to `"GET"`; a non-string `body` is
 * sent as JSON.
 */
export interface RequestInit {
  method?: string;
  url: string;
  headers?: HeadersInit;
  body?: string | Record<string, unknown> | unknown[] | null;
}

/**
 * A response as plugins see it (hooks, viewers and {@link HttpClient} results).
 */
export interface PluginResponse {
  /** The status code, e.g. `200`. */
  status: number;
  /** The reason phrase, e.g. `"OK"`. May be empty. */
  statusText: string;
  /** The response headers. */
  headers: Headers;
  /** The body as text (decompressed; invalid UTF-8 is replaced). */
  body: string;
  /** Total time in milliseconds, from sending the request to the last byte. */
  timeMs: number;
  /** Returns {@link body}. */
  text(): string;
  /**
   * Parses {@link body} as JSON.
   *
   * @throws `the response body is not valid JSON: ...` if it is not JSON.
   */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  json<T = any>(): T;
}

/**
 * A response returned from `afterResponse` instead of the original object.
 * A non-string `body` is serialized as JSON.
 */
export interface ResponseInit {
  status: number;
  statusText?: string;
  headers?: HeadersInit;
  body?: string | Record<string, unknown> | unknown[];
  timeMs?: number;
}

// ── ctx ─────────────────────────────────────────────────────────────────────

/**
 * The API every plugin function receives as its last argument. It is the
 * only way out of the sandbox, and everything done through it appears in
 * Live Activity.
 *
 * | API             | Permission (in `plugin.json`)          |
 * |-----------------|-----------------------------------------|
 * | `log`, `progress`, `random`, `crypto`, `variables` | none |
 * | `http`          | `network:<host>` (or `network:*.domain`, `network:*`) |
 * | `secrets`       | `secrets:read`                          |
 * | `fs.read`       | `fs:read`                               |
 * | `fs.write`      | `fs:write`                              |
 *
 * A missing permission throws an error that says which permission to add to
 * `plugin.json`; a declared but not yet approved one throws an error that
 * says how to approve it (`pt plugin approve <name>`).
 */
export interface PluginContext {
  /** Writes to Live Activity, attached to the current request or call. */
  readonly log: PluginLogger;

  /**
   * Reports progress of a long operation, shown in Live Activity.
   *
   * @example
   * ```ts
   * entries.forEach((entry, i) => { convert(entry); ctx.progress(i + 1, entries.length); });
   * ```
   */
  progress(done: number, total: number): void;

  /** Random values. */
  readonly random: {
    /** A random UUID v4, e.g. `"3b241101-e2bb-4255-8caf-4136c566a962"`. */
    uuid(): string;
    /** A random integer between `min` and `max`, both inclusive. Defaults: `0` and `1000`. */
    int(min?: number, max?: number): number;
  };

  /**
   * Hashing and encoding. Text is encoded as UTF-8; hashes are lower-case hex.
   * For other algorithms (e.g. HMAC-SHA1), implement them in your plugin.
   */
  readonly crypto: {
    /**
     * SHA-256 of `data` as hex.
     * @example ctx.crypto.sha256("abc") // "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
     */
    sha256(data: string): string;
    /**
     * HMAC-SHA256 of `data` with `key`, as hex.
     * @example ctx.crypto.hmacSha256("key", "The quick brown fox jumps over the lazy dog")
     * // "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
     */
    hmacSha256(key: string, data: string): string;
    /** Standard base64 (with padding) of the UTF-8 text. */
    base64Encode(text: string): string;
    /**
     * Decodes standard base64 into UTF-8 text (invalid UTF-8 is replaced).
     * For binary data use the global `atob`, which returns one character per
     * byte. For base64url, first replace `-` with `+` and `_` with `/`.
     * @throws `invalid base64: ...`
     */
    base64Decode(text: string): string;
  };

  /**
   * Variables. `get` sees session variables (set by scripts, extraction and
   * plugins), then the default environment, then workspace variables. Folder
   * variables and a request's selected environment are not visible here.
   */
  readonly variables: {
    /** The value of a variable, or `undefined` if it is not set. */
    get(name: string): string | undefined;
    /**
     * Sets a session variable, visible to later requests as `{{name}}` until
     * post-tester restarts. It is not written to any file.
     */
    set(name: string, value: string | number | boolean): void;
  };

  /**
   * HTTP client. Needs `network:<host>` for each host, e.g.
   * `"network:api.github.com"`, `"network:*.example.com"` or `"network:*"`.
   * Each request appears in Live Activity under the plugin call.
   *
   * @example
   * ```ts
   * const res = await ctx.http.post("https://api.example.com/tokens", { json: { user: "ada" } });
   * if (res.status !== 201) throw new Error(`token request failed: ${res.status} ${res.body}`);
   * const { token } = res.json();
   * ```
   */
  readonly http: HttpClient;

  /** Secrets from the system keychain (or `PT_SECRET_*` environment variables). Needs `secrets:read`. */
  readonly secrets: {
    /** The secret's value, or `undefined` if it is not set. */
    get(name: string): Promise<string | undefined>;
  };

  /**
   * Files inside the workspace folder (during `pt plugin test`: the plugin
   * folder). Paths are relative; absolute paths and `..` are refused.
   * Both functions are synchronous.
   */
  readonly fs: {
    /** Reads a UTF-8 text file. Needs `fs:read`. @throws if it does not exist. */
    read(path: string): string;
    /** Creates or replaces a text file, creating folders as needed. Needs `fs:write`. */
    write(path: string, content: string): void;
  };
}

/** Logging to Live Activity. Arguments are joined with spaces; non-strings are shown as JSON. */
export interface PluginLogger {
  debug(...args: unknown[]): void;
  info(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  error(...args: unknown[]): void;
}

/** Options for {@link HttpClient.request}. */
export interface HttpOptions {
  /** HTTP method. Defaults to `"GET"`. */
  method?: string;
  /** Absolute URL, e.g. `"https://api.example.com/users"`. */
  url: string;
  /** Request headers. */
  headers?: HeadersInit;
  /** Body as text. Ignored when {@link json} is given. */
  body?: string;
  /** A value to send as JSON. Sets `Content-Type: application/json` unless a header sets it. */
  json?: unknown;
  /** Time limit in milliseconds until response headers arrive. Defaults to `30000`. */
  timeoutMs?: number;
}

/** Options for the {@link HttpClient} shortcut methods (everything except `method` and `url`). */
export type HttpShortcutOptions = Omit<HttpOptions, "method" | "url">;

/**
 * Sends HTTP requests. Redirects are followed and cookies are shared with the
 * app's cookie jar. Non-2xx statuses do not throw; check `status`. Network
 * errors and denied permissions do throw.
 */
export interface HttpClient {
  /** Sends a request. */
  request(options: HttpOptions): Promise<PluginResponse>;
  /** Sends a GET request. */
  get(url: string, options?: HttpShortcutOptions): Promise<PluginResponse>;
  /** Sends a POST request. */
  post(url: string, options?: HttpShortcutOptions): Promise<PluginResponse>;
  /** Sends a PUT request. */
  put(url: string, options?: HttpShortcutOptions): Promise<PluginResponse>;
  /** Sends a PATCH request. */
  patch(url: string, options?: HttpShortcutOptions): Promise<PluginResponse>;
  /** Sends a DELETE request. */
  delete(url: string, options?: HttpShortcutOptions): Promise<PluginResponse>;
}
