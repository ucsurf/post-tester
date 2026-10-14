/**
 * Imports HAR 1.2 files (HTTP Archive), which browsers' developer tools and
 * most proxies can export. Each `log.entries[].request` becomes one request,
 * grouped into one folder per host.
 *
 * Spec: HAR 1.2, http://www.softwareishard.com/blog/har-12-spec/
 */
import { definePlugin } from "@post-tester/plugin-sdk";
import type { ImportedRequest, KeyValue, PluginContext, RequestBody } from "@post-tester/plugin-sdk";

/** The parts of a HAR file this importer reads. */
interface Har {
  log: {
    creator?: { name?: string };
    entries: Array<{ request: HarRequest }>;
  };
}

interface HarRequest {
  method: string;
  url: string;
  headers?: Array<{ name: string; value: string }>;
  postData?: { mimeType?: string; text?: string; params?: Array<{ name: string; value?: string }> };
}

/**
 * Headers not worth importing: HTTP/2 pseudo-headers (`:authority`), headers
 * the HTTP client computes itself, and cookies (post-tester keeps its own
 * cookie jar, and browser cookies are often sensitive and short-lived).
 */
const SKIPPED_HEADERS = new Set(["host", "content-length", "connection", "cookie"]);

const parseHar = (text: string): Har | undefined => {
  try {
    const data = JSON.parse(text);
    return Array.isArray(data?.log?.entries) ? (data as Har) : undefined;
  } catch {
    return undefined;
  }
};

/** Decodes one query component; keeps the raw text if it is not valid percent-encoding. */
const decode = (text: string): string => {
  try {
    return decodeURIComponent(text.replace(/\+/g, " "));
  } catch {
    return text;
  }
};

/**
 * Splits a URL into the part before `?` and its query parameters. The query is
 * read from the URL itself, which is exactly what was sent (tools differ in
 * whether HAR's `queryString` values are encoded). The sandbox has no `URL`
 * class, so this is done by hand.
 */
export function splitUrl(url: string): { base: string; query: KeyValue[] } {
  const withoutFragment = url.split("#")[0];
  const index = withoutFragment.indexOf("?");
  if (index < 0) return { base: withoutFragment, query: [] };
  const query = withoutFragment
    .slice(index + 1)
    .split("&")
    .filter((pair) => pair.length > 0)
    .map((pair) => {
      const eq = pair.indexOf("=");
      return eq < 0 ? { key: decode(pair), value: "" } : { key: decode(pair.slice(0, eq)), value: decode(pair.slice(eq + 1)) };
    });
  return { base: withoutFragment.slice(0, index), query };
}

/** Folder for a URL: its host (and port), made safe for a folder name. */
const folderFor = (url: string): string => {
  const host = url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "").split(/[/?#]/)[0];
  return host.replace(/[^A-Za-z0-9._-]/g, "-").replace(/^\.+/, "") || "requests";
};

const pathOf = (base: string): string => base.replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, "") || "/";

export function convertBody(postData: HarRequest["postData"]): RequestBody | undefined {
  if (!postData) return undefined;
  const mimeType = (postData.mimeType ?? "").split(";")[0].trim().toLowerCase();
  const text = postData.text ?? "";
  if (mimeType === "application/x-www-form-urlencoded" && postData.params?.length) {
    return { type: "form", fields: postData.params.map((p) => ({ key: p.name, value: p.value ?? "" })) };
  }
  if (!text) return undefined;
  if (mimeType === "application/json" || mimeType.endsWith("+json")) return { type: "json", content: text };
  // Keep the full mime type (including e.g. a multipart boundary).
  return { type: "text", content: text, content_type: postData.mimeType || "text/plain" };
}

export function convertRequest(request: HarRequest, index: number): ImportedRequest {
  if (typeof request?.url !== "string" || !request.url) {
    throw new Error(`log.entries[${index}].request has no "url"; is this a HAR 1.2 file?`);
  }
  const method = String(request.method || "GET").toUpperCase();
  const { base, query } = splitUrl(request.url);
  const headers = (request.headers ?? [])
    .filter((h) => !h.name.startsWith(":") && !SKIPPED_HEADERS.has(h.name.toLowerCase()))
    .map((h) => ({ key: h.name, value: h.value }));

  const converted: ImportedRequest = { folder: folderFor(base), name: `${method} ${pathOf(base)}`, method, url: base };
  if (query.length) converted.query = query;
  if (headers.length) converted.headers = headers;
  const body = convertBody(request.postData);
  if (body) converted.body = body;
  return converted;
}

export default definePlugin({
  importers: {
    har: {
      displayName: "HAR (HTTP Archive)",
      fileExtensions: [".har"],
      // Cheap check first, so large non-HAR files are not parsed.
      detect: (text) => text.includes('"entries"') && parseHar(text) !== undefined,
      import: (text, ctx: PluginContext) => {
        const har = parseHar(text);
        if (!har) throw new Error('not a HAR file: expected JSON with a "log.entries" array');
        const entries = har.log.entries;
        const requests = entries.map((entry, i) => {
          ctx.progress(i + 1, entries.length);
          return convertRequest(entry.request, i);
        });
        ctx.log.info(`Imported ${requests.length} request(s) from HAR`);
        const creator = har.log.creator?.name;
        return { name: creator ? `HAR from ${creator}` : "HAR import", requests };
      },
    },
  },
});
