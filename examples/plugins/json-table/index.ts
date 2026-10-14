/**
 * A response viewer that renders JSON as an HTML table.
 *
 * - an array of objects → one row per item, one column per key (union of all keys)
 * - an object           → a two-column key/value table
 * - an array of values  → a one-column table
 * Nested values are shown as compact JSON.
 *
 * Every value comes from a server, so all text is HTML-escaped.
 */
import { definePlugin } from "@post-tester/plugin-sdk";

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

/** Escapes text for use in HTML content and attribute values. */
export const escapeHtml = (text: string): string => text.replace(/[&<>"']/g, (c) => ESCAPES[c]);

const isPlainObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** One cell: strings as-is, everything else (numbers, null, nested values) as JSON. */
const cell = (value: unknown): string => {
  if (value === undefined) return "<td></td>";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return `<td>${escapeHtml(text)}</td>`;
};

const table = (columns: string[], rows: unknown[][]): string => {
  const head = columns.map((c) => `<th>${escapeHtml(c)}</th>`).join("");
  const body = rows.map((row) => `<tr>${row.map(cell).join("")}</tr>`).join("");
  return `<table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
};

export function renderJson(data: unknown): string {
  if (Array.isArray(data)) {
    if (data.length === 0) return "<p>The array is empty.</p>";
    if (data.every(isPlainObject)) {
      // Keep the column order of first appearance.
      const columns = [...new Set(data.flatMap((item) => Object.keys(item)))];
      return table(columns, data.map((item) => columns.map((c) => item[c])));
    }
    return table(["value"], data.map((item) => [item]));
  }
  if (isPlainObject(data)) return table(["key", "value"], Object.entries(data));
  return table(["value"], [[data]]);
}

export default definePlugin({
  viewers: {
    table: {
      displayName: "Table",
      contentTypes: ["application/json"],
      render: (response) => {
        let data: unknown;
        try {
          data = response.json();
        } catch (e) {
          return `<p>Cannot show this response as a table: ${escapeHtml((e as Error).message)}</p>`;
        }
        return renderJson(data);
      },
    },
  },
});
