/**
 * Exports a collection as Markdown documentation: one section per request
 * with its URL, description, query parameters, headers, body and tests.
 *
 * Auth is described by type only, and templates such as `{{secret:token}}`
 * are kept as written, so no secret values end up in the document.
 */
import { definePlugin } from "@post-tester/plugin-sdk";
import type { ExportCollection, ExportedRequest, KeyValue, RequestAuth, RequestBody } from "@post-tester/plugin-sdk";

/** Makes text safe inside a Markdown table cell. */
export const tableCell = (text: string): string => text.replace(/\\/g, "\\\\").replace(/\|/g, "\\|").replace(/\r?\n/g, " ");

/** A fenced code block whose fence is longer than any backtick run inside the code. */
export function codeBlock(code: string, language = ""): string {
  const longestRun = Math.max(0, ...(code.match(/`+/g) ?? []).map((run) => run.length));
  const fence = "`".repeat(Math.max(3, longestRun + 1));
  return `${fence}${language}\n${code}\n${fence}`;
}

const pairsTable = (title: string, pairs: KeyValue[]): string[] => {
  if (pairs.length === 0) return [];
  const rows = pairs.map((p) => `| ${tableCell(p.key)} | ${tableCell(p.value)}${p.enabled === false ? " (disabled)" : ""} |`);
  return [`**${title}**`, "", "| Name | Value |", "|---|---|", ...rows, ""];
};

const bodySection = (body: RequestBody | undefined): string[] => {
  switch (body?.type) {
    case undefined:
    case "none":
      return [];
    case "json":
      return ["**Body** (JSON)", "", codeBlock(body.content, "json"), ""];
    case "text":
      return [`**Body** (${body.content_type ?? "text/plain"})`, "", codeBlock(body.content), ""];
    case "form":
      return pairsTable("Body (form)", body.fields);
    case "graphql":
      return ["**Body** (GraphQL)", "", codeBlock(body.query, "graphql"), ...(body.variables ? ["", codeBlock(body.variables, "json")] : []), ""];
  }
};

const authText = (auth: RequestAuth | undefined): string | undefined => {
  if (!auth) return undefined;
  switch (auth.type) {
    case "none":
      return "none";
    case "api_key":
      return `API key (${auth.location ?? "header"} \`${auth.key}\`)`;
    case "plugin":
      return `plugin \`${auth.name}\``;
    default:
      return auth.type;
  }
};

export function requestSection(request: ExportedRequest): string {
  const method = (request.method ?? "GET").toUpperCase();
  const title = request.name || `${method} ${request.url}`;
  const auth = authText(request.auth);
  const lines = [
    `### ${title}`,
    "",
    codeBlock(`${method} ${request.url}`, "http"),
    "",
    ...(request.description ? [request.description, ""] : []),
    ...(auth ? [`**Auth:** ${auth}`, ""] : []),
    ...pairsTable("Query parameters", request.query ?? []),
    ...pairsTable("Headers", request.headers ?? []),
    ...bodySection(request.body),
    ...(request.tests?.length ? ["**Tests**", "", ...request.tests.map((t) => `- \`${t}\``), ""] : []),
  ];
  return lines.join("\n");
}

export function exportMarkdown(collection: ExportCollection): string {
  const requests = collection.requests ?? [];
  const header = [`# ${collection.name}`, "", `${requests.length} request(s).`, ""];
  return [...header, ...requests.map(requestSection)].join("\n").trimEnd() + "\n";
}

export default definePlugin({
  exporters: {
    markdown: {
      displayName: "Markdown documentation",
      fileExtension: ".md",
      export: (collection) => exportMarkdown(collection),
    },
  },
});
