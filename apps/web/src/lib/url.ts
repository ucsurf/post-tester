import type { KeyValue } from "@post-tester/bindings";

/**
 * The request model keeps the URL and its query parameters apart (`url` + `query`);
 * the engine appends enabled `query` items when sending. The URL bar shows both
 * together, so these helpers convert between the two forms.
 *
 * Values are kept readable: they are percent-decoded when parsed (if valid) and only
 * the characters that would break parsing (`&`, `#`, and `=` in keys) are encoded
 * when composed. `{{variables}}` pass through untouched.
 */

export type QueryParam = { key: string; value: string };

export type ParsedUrl = { base: string; params: QueryParam[] };

/** Splits `https://x/y?a=1&b#top` into `{ base: "https://x/y#top", params: [a=1, b=] }`. */
export function parseUrl(full: string): ParsedUrl {
  const hashIndex = full.indexOf("#");
  const beforeHash = hashIndex === -1 ? full : full.slice(0, hashIndex);
  const hash = hashIndex === -1 ? "" : full.slice(hashIndex);
  const queryIndex = beforeHash.indexOf("?");
  if (queryIndex === -1) return { base: full, params: [] };
  const base = beforeHash.slice(0, queryIndex) + hash;
  const query = beforeHash.slice(queryIndex + 1);
  const params = query
    .split("&")
    .filter((part) => part !== "")
    .map((part) => {
      const eq = part.indexOf("=");
      const key = eq === -1 ? part : part.slice(0, eq);
      const value = eq === -1 ? "" : part.slice(eq + 1);
      return { key: safeDecode(key), value: safeDecode(value) };
    });
  return { base, params };
}

/** Joins a base URL and the enabled params back into one string for display. */
export function composeUrl(base: string, query: readonly KeyValue[]): string {
  const enabled = query.filter((p) => p.enabled !== false && (p.key !== "" || p.value !== ""));
  if (enabled.length === 0) return base;
  const hashIndex = base.indexOf("#");
  const path = hashIndex === -1 ? base : base.slice(0, hashIndex);
  const hash = hashIndex === -1 ? "" : base.slice(hashIndex);
  const text = enabled.map((p) => `${encodePart(p.key, true)}${p.value === "" ? "" : `=${encodePart(p.value, false)}`}`).join("&");
  return `${path}${path.includes("?") ? "&" : "?"}${text}${hash}`;
}

/**
 * Applies a URL typed in the URL bar to the model. Enabled params are replaced by the
 * ones in the URL (keeping their positions); disabled params are kept as they were.
 */
export function applyUrlToRequest(full: string, existing: readonly KeyValue[]): { url: string; query: KeyValue[] } {
  const { base, params } = parseUrl(full);
  const remaining = [...params];
  const query: KeyValue[] = [];
  for (const item of existing) {
    if (item.enabled === false) {
      query.push(item);
      continue;
    }
    const next = remaining.shift();
    if (next) query.push({ key: next.key, value: next.value });
  }
  for (const param of remaining) query.push({ key: param.key, value: param.value });
  return { url: base, query };
}

/** Moves a query string written inside `url` into `query` (e.g. after an import). */
export function normalizeUrlAndQuery(url: string, query: readonly KeyValue[]): { url: string; query: KeyValue[] } {
  const { base, params } = parseUrl(url);
  if (params.length === 0) return { url, query: [...query] };
  return { url: base, query: [...params.map((p) => ({ key: p.key, value: p.value })), ...query] };
}

/** Decodes valid escape sequences and leaves anything else (e.g. a lone `%`) as written. */
function safeDecode(text: string): string {
  return text.replace(/\+/g, " ").replace(/(%[0-9a-fA-F]{2})+/g, (run) => {
    try {
      return decodeURIComponent(run);
    } catch {
      return run;
    }
  });
}

function encodePart(text: string, isKey: boolean): string {
  // `%` is encoded only when it would otherwise be read back as an escape sequence.
  let out = text.replace(/%(?=[0-9a-fA-F]{2})/g, "%25").replace(/&/g, "%26").replace(/#/g, "%23").replace(/\+/g, "%2B");
  if (isKey) out = out.replace(/=/g, "%3D");
  return out;
}
