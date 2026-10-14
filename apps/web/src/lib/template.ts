/**
 * Helpers for `{{...}}` templates: `{{baseUrl}}`, `{{secret:apiKey}}`, `{{uuid()}}`,
 * `{{totp(secret:"x")}}`.
 */

export type TemplateKind = "variable" | "secret" | "function";

export type TemplateRange = { from: number; to: number; name: string; kind: TemplateKind };

const TEMPLATE = /\{\{\s*([^{}]*?)\s*\}\}/g;

/** Finds every `{{...}}` in `text` (used to highlight them). */
export function findTemplates(text: string): TemplateRange[] {
  const ranges: TemplateRange[] = [];
  for (const match of text.matchAll(TEMPLATE)) {
    const inner = match[1] ?? "";
    const from = match.index;
    ranges.push({ from, to: from + match[0].length, name: templateName(inner), kind: templateKind(inner) });
  }
  return ranges;
}

export function templateKind(inner: string): TemplateKind {
  if (inner.startsWith("secret:")) return "secret";
  if (/^[\w.-]+\s*\(/.test(inner)) return "function";
  return "variable";
}

function templateName(inner: string): string {
  if (inner.startsWith("secret:")) return inner.slice("secret:".length);
  const paren = inner.indexOf("(");
  return paren === -1 ? inner : inner.slice(0, paren).trim();
}

/**
 * When the cursor is inside an unfinished `{{`, returns where the typed name starts
 * and what has been typed so far, so autocomplete can replace it.
 */
export function completionContext(text: string, cursor: number): { from: number; query: string } | null {
  const before = text.slice(0, cursor);
  const open = before.lastIndexOf("{{");
  if (open === -1) return null;
  const typed = before.slice(open + 2);
  if (typed.includes("}}") || /[{}\s]/.test(typed.trimStart())) return null;
  const leading = typed.length - typed.trimStart().length;
  return { from: open + 2 + leading, query: typed.trimStart() };
}

/** Names of the plain variables used by a template string. */
export function usedVariables(text: string): string[] {
  return findTemplates(text)
    .filter((r) => r.kind === "variable")
    .map((r) => r.name);
}
