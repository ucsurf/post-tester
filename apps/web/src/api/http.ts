/**
 * Low-level HTTP access to the local server: token handling, JSON, and errors that
 * carry the server's own `{ "error": "..." }` message.
 */

const TOKEN_KEY = "post-tester.token";

/** An API failure with a message that is safe to show to the user. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

let token: string | null = null;

/**
 * Reads `?token=` from the address bar on first load, keeps it in sessionStorage and
 * removes it from the URL so it is not left in history or screenshots.
 */
export function initToken(): string | null {
  const url = new URL(window.location.href);
  const fromUrl = url.searchParams.get("token");
  if (fromUrl) {
    safeSession((s) => s.setItem(TOKEN_KEY, fromUrl));
    url.searchParams.delete("token");
    window.history.replaceState(window.history.state, "", url.pathname + url.search + url.hash);
    token = fromUrl;
  } else {
    token = safeSession((s) => s.getItem(TOKEN_KEY)) ?? null;
  }
  return token;
}

export function getToken(): string | null {
  return token;
}

/** `ws://host/api/live?token=…`, relative to the page so the dev proxy works too. */
export function liveSocketUrl(): string {
  const url = new URL("api/live", baseUrl());
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  if (token) url.searchParams.set("token", token);
  return url.toString();
}

/** Encodes ids such as `auth/login` segment by segment (they contain `/` on purpose). */
export function idPath(id: string): string {
  return id.split("/").map(encodeURIComponent).join("/");
}

type Options = { body?: unknown; signal?: AbortSignal };

export async function apiRequest<T>(method: string, path: string, options: Options = {}): Promise<T> {
  const response = await rawRequest(method, path, options);
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  if (text === "") return undefined as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new ApiError(response.status, `The server sent a response that is not JSON (${path}).`);
  }
}

/** Like `apiRequest`, but returns the raw `Response` (for blobs). */
export async function rawRequest(method: string, path: string, options: Options = {}): Promise<Response> {
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  let response: Response;
  try {
    response = await fetch(new URL(path.replace(/^\//, ""), baseUrl()), {
      method,
      headers,
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
      signal: options.signal,
    });
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") throw e;
    throw new ApiError(0, "Cannot reach the post-tester server. Is `pt serve` still running?");
  }
  if (!response.ok) throw new ApiError(response.status, await errorMessage(response));
  return response;
}

async function errorMessage(response: Response): Promise<string> {
  const text = await response.text().catch(() => "");
  try {
    const body = JSON.parse(text) as { error?: unknown };
    if (typeof body.error === "string" && body.error) return body.error;
  } catch {
    // Not JSON; fall through.
  }
  if (response.status === 401) return "The session token is missing or wrong. Open the link printed by `pt serve` again.";
  return text.trim() || `${response.status} ${response.statusText}`;
}

/** The page's directory, so the app works when served from `/` or a sub-path. */
function baseUrl(): string {
  return new URL(".", window.location.href).toString();
}

function safeSession<T>(fn: (storage: Storage) => T): T | undefined {
  try {
    return fn(window.sessionStorage);
  } catch {
    return undefined;
  }
}

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
