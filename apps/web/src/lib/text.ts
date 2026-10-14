/** Small text helpers: shell-style argument splitting, JSON and content types. */

/** Splits `--port 3000 "my dir" 'a b'` into arguments, honouring quotes and `\` escapes. */
export function splitArgs(line: string): string[] {
  const args: string[] = [];
  let current = "";
  let inArg = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i] as string;
    if (quote) {
      if (ch === quote) quote = null;
      else if (ch === "\\" && quote === '"' && i + 1 < line.length) current += line[++i];
      else current += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      inArg = true;
    } else if (ch === "\\" && i + 1 < line.length) {
      current += line[++i];
      inArg = true;
    } else if (/\s/.test(ch)) {
      if (inArg) args.push(current);
      current = "";
      inArg = false;
    } else {
      current += ch;
      inArg = true;
    }
  }
  if (inArg) args.push(current);
  return args;
}

export type ParsedJson = { ok: true; value: unknown } | { ok: false; error: string };

export function parseJson(text: string): ParsedJson {
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

/** Pretty-prints JSON text; returns the input unchanged when it is not JSON. */
export function prettyJson(text: string): string {
  const parsed = parseJson(text);
  return parsed.ok ? JSON.stringify(parsed.value, null, 2) : text;
}

/** `application/json; charset=utf-8` → `application/json`. */
export function mediaType(contentType: string | null | undefined): string {
  return (contentType ?? "").split(";")[0]?.trim().toLowerCase() ?? "";
}

export function isJsonContentType(contentType: string | null | undefined): boolean {
  const type = mediaType(contentType);
  return type === "application/json" || type.endsWith("+json") || type === "text/json";
}

/**
 * Whether a content type matches one of a viewer's patterns:
 * - no patterns: every response (the viewer did not restrict itself);
 * - `*` (or `*` slash `*`): every response, even one without a content type;
 * - `*` inside a pattern is a wildcard: `image/*`, `application/*+json`;
 * - a trailing `/` is a prefix: `text/` matches `text/csv`;
 * - otherwise the media type must be equal (parameters such as `charset` are ignored).
 */
export function matchesContentType(contentType: string | null | undefined, patterns: readonly string[]): boolean {
  if (patterns.length === 0) return true;
  const type = mediaType(contentType);
  return patterns.some((pattern) => {
    const p = mediaType(pattern);
    if (p === "*" || p === "*/*") return true;
    if (!type || !p) return false;
    if (p.endsWith("/")) return type.startsWith(p);
    if (!p.includes("*")) return type === p;
    return new RegExp(`^${p.split("*").map(escapeRegex).join(".*")}$`).test(type);
  });
}

/** Looks up a header value case-insensitively in `[name, value]` pairs or `{name, value}` objects. */
export function headerValue(headers: ReadonlyArray<readonly [string, string] | { name: string; value: string }>, name: string): string | undefined {
  const wanted = name.toLowerCase();
  for (const header of headers) {
    const [key, value] = Array.isArray(header) ? header : [(header as { name: string }).name, (header as { value: string }).value];
    if (key.toLowerCase() === wanted) return value;
  }
  return undefined;
}

function escapeRegex(text: string): string {
  return text.replace(/[.+?^${}()|[\]\\/]/g, "\\$&");
}
