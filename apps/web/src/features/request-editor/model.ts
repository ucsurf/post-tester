import type { Auth, Body, KeyValue, Request, RequestOptions } from "@post-tester/bindings";
import { normalizeUrlAndQuery } from "../../lib/url";

/**
 * The server omits empty fields when it serializes a request (they are skipped in
 * the YAML files too), so everything read from the API is normalized to a complete
 * `Request` before the editor touches it.
 */

export const DEFAULT_OPTIONS: RequestOptions = {
  timeout_ms: 30_000,
  follow_redirects: true,
  max_redirects: 10,
  verify_tls: true,
  http_version: "auto",
};

export function newRequest(overrides: Partial<Request> = {}): Request {
  return normalizeRequest({ method: "GET", url: "", ...overrides });
}

/**
 * Fields the server sends are spread first, so fields this UI does not edit (e.g.
 * `examples`, or anything added to the Rust model later) survive a load → save cycle.
 */
export function normalizeRequest(input: Partial<Request>): Request {
  const { url, query } = normalizeUrlAndQuery(input.url ?? "", input.query ?? []);
  const request: Request = {
    ...input,
    name: input.name ?? "",
    description: input.description ?? "",
    method: (input.method ?? "GET").toUpperCase(),
    url,
    query,
    headers: input.headers ?? [],
    body: input.body ?? { type: "none" },
    scripts: { before_request: input.scripts?.before_request ?? "", after_response: input.scripts?.after_response ?? "" },
    tests: input.tests ?? [],
    extract: input.extract ?? {},
    options: { ...DEFAULT_OPTIONS, ...input.options },
  };
  if (!input.auth) delete request.auth;
  return request;
}

/** Structural equality used for the "unsaved changes" dot. */
export function requestsEqual(a: Request, b: Request): boolean {
  return stableStringify(a) === stableStringify(b);
}

function stableStringify(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) => {
    if (v && typeof v === "object" && !Array.isArray(v)) {
      return Object.fromEntries(Object.entries(v as Record<string, unknown>).sort(([x], [y]) => (x < y ? -1 : x > y ? 1 : 0)));
    }
    return v;
  });
}

/** A saved example response (served by the mock server). Not yet in the generated bindings. */
export type RequestExample = { name: string; status: number; headers: KeyValue[]; body: string; delay_ms?: number };

type WithExamples = Request & { examples?: RequestExample[] };

export function requestExamples(request: Request): RequestExample[] {
  return (request as WithExamples).examples ?? [];
}

/** Appends an example; the request becomes "unsaved" until the user saves it. */
export function addExample(request: Request, example: RequestExample): Request {
  return { ...request, examples: [...requestExamples(request), example] } as WithExamples;
}

export const COMMON_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;

export type BodyType = Body["type"];

/** Switches the body type, carrying the text over where it makes sense. */
export function convertBody(body: Body, type: BodyType): Body {
  if (body.type === type) return body;
  const text = body.type === "json" || body.type === "text" ? body.content : body.type === "graphql" ? body.query : "";
  switch (type) {
    case "none":
      return { type: "none" };
    case "json":
      return { type: "json", content: text };
    case "text":
      return { type: "text", content: text, content_type: "text/plain" };
    case "form":
      return { type: "form", fields: body.type === "form" ? body.fields : [] };
    case "graphql":
      return { type: "graphql", query: text, variables: "" };
  }
}

export type AuthChoice = "inherit" | "none" | "basic" | "bearer" | "api_key" | `plugin:${string}`;

export function authChoice(auth: Auth | undefined): AuthChoice {
  if (!auth) return "inherit";
  return auth.type === "plugin" ? `plugin:${auth.name}` : auth.type;
}

export function authForChoice(choice: AuthChoice, current: Auth | undefined): Auth | undefined {
  if (choice === authChoice(current)) return current;
  switch (choice) {
    case "inherit":
      return undefined;
    case "none":
      return { type: "none" };
    case "basic":
      return { type: "basic", username: "", password: "" };
    case "bearer":
      return { type: "bearer", token: "" };
    case "api_key":
      return { type: "api_key", key: "X-API-Key", value: "", location: "header" };
    default:
      return { type: "plugin", name: choice.slice("plugin:".length), config: {} };
  }
}

/** The last path segment of an id: `auth/login` → `login`. */
export function idBaseName(id: string): string {
  return id.split("/").pop() ?? id;
}

/** The display name of a request: its `name`, else its id, else its URL. */
export function requestLabel(request: Request, id: string | null): string {
  if (request.name) return request.name;
  if (id) return idBaseName(id);
  return request.url || "New request";
}

export function countEnabled(items: readonly KeyValue[]): number {
  return items.filter((i) => i.enabled !== false && i.key !== "").length;
}
