/**
 * # @post-tester/plugin-sdk/testing
 *
 * Helpers for plugin tests. Test files are named `*.test.ts` or `*.test.js`,
 * sit next to `plugin.json`, and run with `pt plugin test`. `test`, `it`,
 * `describe` and `expect` are globals (see `globals.d.ts`).
 *
 * ```ts
 * import { testPlugin } from "@post-tester/plugin-sdk/testing";
 * import plugin from "./index.ts";
 *
 * test("shout upper-cases text", async () => {
 *   const t = testPlugin(plugin);
 *   expect(await t.templateFunction("shout", { text: "hi" })).toBe("HI");
 * });
 * ```
 *
 * @packageDocumentation
 */

/// <reference path="./globals.d.ts" />

import type { HeadersInit, ImportedRequest, JsonSchema, KeyValue, PluginDefinition } from "./index.js";

/**
 * Wraps a plugin so tests can call it exactly like post-tester does: the same
 * argument handling, defaults, validation and result conversion.
 *
 * Results are the plain data post-tester receives, not live objects: for
 * example, a request's `headers` is an array of `[name, value]` pairs.
 *
 * @example
 * ```ts
 * const t = testPlugin(plugin);
 * const request = await t.beforeRequest({ url: "https://api.example.com/users" });
 * const headers = Object.fromEntries(request.headers);
 * expect(headers["X-Request-Id"]).toMatch(/^[0-9a-f-]{36}$/);
 * ```
 */
export declare function testPlugin(plugin: PluginDefinition): PluginTester;

/** Calls one capability of a plugin. Every method returns a promise. */
export interface PluginTester {
  /**
   * Calls a template function. `args` may use names (`{ secret: "..." }`) or
   * positions (`{ 0: "..." }`), like `{{fn(...)}}` in a template.
   *
   * @returns The text that would be inserted into the template.
   */
  templateFunction(name: string, args?: Record<string, unknown>): Promise<string>;

  /**
   * Applies an auth method to `request` (defaults: `GET https://example.com/`,
   * no headers, empty body).
   *
   * @returns The request that would be sent.
   */
  auth(name: string, request?: TestRequestInput, config?: Record<string, unknown>): Promise<SentRequest>;

  /** Runs the plugin's `beforeRequest` hook (the request is returned unchanged if there is none). */
  beforeRequest(request?: TestRequestInput): Promise<SentRequest>;

  /**
   * Runs the plugin's `afterResponse` hook on `response` (defaults: status
   * `200`, `"OK"`, no headers, empty body, `timeMs: 1`).
   */
  afterResponse(response?: TestResponseInput): Promise<ReceivedResponse>;

  /**
   * Runs an importer.
   *
   * @returns The collection, with each request's `headers` normalized to `{ key, value }` pairs.
   */
  importer(name: string, text: string): Promise<ImportedCollection>;

  /** Runs an importer's `detect` (`false` if it has none). */
  detect(name: string, text: string): Promise<boolean>;

  /** Runs an exporter on a collection. */
  exporter(name: string, collection: { name: string; requests: Array<Record<string, unknown>> }): Promise<string>;

  /** Runs a code generator on `request` (same defaults as {@link auth}). */
  codegen(name: string, request?: TestRequestInput): Promise<string>;

  /** Runs a viewer on `response` (same defaults as {@link afterResponse}). Returns HTML. */
  viewer(name: string, response?: TestResponseInput): Promise<string>;

  /** Calls an MCP tool with `input` (default `{}`). Returns the tool's result. */
  mcpTool(name: string, input?: Record<string, unknown>): Promise<unknown>;

  /** Lists what the plugin provides, as post-tester and AI agents see it. */
  describe(): Promise<PluginDescription>;
}

/** A request to pass into a test call. Missing fields use the defaults. */
export interface TestRequestInput {
  /** Default `"GET"`. */
  method?: string;
  /** Default `"https://example.com/"`. */
  url?: string;
  /** Default: no headers. */
  headers?: HeadersInit;
  /** Default `""`. */
  body?: string;
}

/** A response to pass into a test call. Missing fields use the defaults. */
export interface TestResponseInput {
  /** Default `200`. */
  status?: number;
  /** Default `"OK"`. */
  statusText?: string;
  /** Default: no headers. */
  headers?: HeadersInit;
  /** Default `""`. Must be text; use `JSON.stringify` for JSON bodies. */
  body?: string;
  /** Default `1`. */
  timeMs?: number;
}

/** A request as post-tester receives it from a plugin. */
export interface SentRequest {
  /** Upper-case method. */
  method: string;
  url: string;
  /** Headers as `[name, value]` pairs, in order. Use `Object.fromEntries(headers)` for lookups. */
  headers: Array<[string, string]>;
  body: string;
}

/** A response as post-tester receives it from an `afterResponse` hook. Note the snake_case fields. */
export interface ReceivedResponse {
  status: number;
  status_text: string;
  /** Headers as `[name, value]` pairs, in order. */
  headers: Array<[string, string]>;
  body: string;
  time_ms: number;
}

/** An importer's result as post-tester receives it. */
export interface ImportedCollection {
  name: string;
  requests: Array<Omit<ImportedRequest, "headers"> & { headers: KeyValue[] }>;
}

/** What a plugin provides (the `describe` result). */
export interface PluginDescription {
  templateFunctions: Array<{ name: string; description: string; args: Array<{ name: string; type: string; description: string }> }>;
  auth: Array<{ name: string; displayName: string; fields: Array<{ name: string; type: string; label: string }> }>;
  /** Hook names, e.g. `["beforeRequest", "afterResponse"]`. */
  hooks: string[];
  importers: Array<{ name: string; displayName: string; fileExtensions: string[] }>;
  exporters: Array<{ name: string; displayName: string; fileExtension: string }>;
  codegen: Array<{ name: string; displayName: string; language: string }>;
  viewers: Array<{ name: string; displayName: string; contentTypes: string[] }>;
  mcpTools: Array<{ name: string; description: string; inputSchema: JsonSchema }>;
  /** The top-level keys of the definition. */
  keys: string[];
}
