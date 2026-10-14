import { autocompletion, type Completion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { RangeSetBuilder, StateEffect, type Extension } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { completionContext, findTemplates } from "../lib/template";

/** CodeMirror theme and extensions shared by every editor, driven by CSS tokens. */

export const editorTheme = EditorView.theme({
  "&": {
    color: "var(--text)",
    backgroundColor: "var(--bg)",
    fontSize: "var(--text-sm)",
    height: "100%",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
  ".cm-content": { caretColor: "var(--accent)", padding: "6px 0" },
  ".cm-cursor": { borderLeftColor: "var(--accent)" },
  ".cm-gutters": { backgroundColor: "var(--bg-subtle)", color: "var(--text-faint)", border: "none", borderRight: "1px solid var(--border)" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent) 5%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-muted)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "color-mix(in srgb, var(--accent) 25%, transparent) !important",
  },
  ".cm-matchingBracket": { backgroundColor: "color-mix(in srgb, var(--accent-2) 25%, transparent)", outline: "none" },
  ".cm-foldPlaceholder": { backgroundColor: "var(--bg-active)", border: "none", color: "var(--text-muted)" },
  ".cm-tooltip": { backgroundColor: "var(--bg-elevated)", border: "1px solid var(--border-strong)", borderRadius: "6px", boxShadow: "var(--shadow)" },
  ".cm-tooltip-autocomplete > ul": { fontFamily: "var(--font-mono)", fontSize: "var(--text-sm)", maxHeight: "16em" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--accent-soft)", color: "var(--text)" },
  ".cm-completionDetail": { color: "var(--text-faint)", fontStyle: "normal", marginLeft: "1em" },
  ".cm-completionInfo": { maxWidth: "320px", padding: "6px 8px", fontFamily: "var(--font-sans)" },
  ".cm-placeholder": { color: "var(--text-faint)" },
  ".cm-panels": { backgroundColor: "var(--bg-subtle)", color: "var(--text)" },
  ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--warning) 30%, transparent)" },
  ".cm-tpl": { borderRadius: "3px", padding: "0 1px" },
  ".cm-tpl-variable": { color: "var(--tpl-variable)", backgroundColor: "var(--tpl-variable-bg)" },
  ".cm-tpl-secret": { color: "var(--tpl-secret)", backgroundColor: "var(--tpl-secret-bg)" },
  ".cm-tpl-function": { color: "var(--tpl-function)", backgroundColor: "var(--tpl-function-bg)" },
  ".cm-tpl-unknown": { textDecoration: "underline wavy var(--danger)", textUnderlineOffset: "3px" },
});

export const highlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: [tags.string, tags.special(tags.string)], color: "var(--success)" },
    { tag: [tags.number, tags.bool, tags.null, tags.atom], color: "var(--warning)" },
    { tag: [tags.propertyName, tags.attributeName], color: "var(--accent)" },
    { tag: [tags.keyword, tags.controlKeyword, tags.moduleKeyword, tags.operatorKeyword], color: "var(--redirect)" },
    { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--info)" },
    { tag: [tags.comment, tags.lineComment, tags.blockComment], color: "var(--text-faint)", fontStyle: "italic" },
    { tag: [tags.definition(tags.variableName), tags.typeName], color: "var(--accent-2)" },
    { tag: tags.invalid, color: "var(--danger)" },
  ]),
);

export type TemplateSuggestions = {
  variables: readonly { name: string; value: string }[];
  functions: readonly { name: string; description: string; plugin: string | null }[];
};

/** Dispatch this effect to re-check variable names after the environment changes. */
export const refreshTemplates = StateEffect.define<null>();

/** Highlights `{{...}}` and marks variables that are not defined in the current environment. */
export function templateHighlighter(known: () => ReadonlySet<string> | null): Extension {
  const build = (view: EditorView): DecorationSet => {
    const builder = new RangeSetBuilder<Decoration>();
    const names = known();
    for (const { from, to } of view.visibleRanges) {
      const text = view.state.doc.sliceString(from, to);
      for (const range of findTemplates(text)) {
        const unknown = range.kind === "variable" && names !== null && !names.has(range.name);
        const cls = `cm-tpl cm-tpl-${range.kind}${unknown ? " cm-tpl-unknown" : ""}`;
        const title = unknown ? `“${range.name}” is not defined in the selected environment` : range.kind;
        builder.add(from + range.from, from + range.to, Decoration.mark({ class: cls, attributes: { title } }));
      }
    }
    return builder.finish();
  };
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view);
      }
      update(update: ViewUpdate) {
        const refresh = update.transactions.some((t) => t.effects.some((e) => e.is(refreshTemplates)));
        if (refresh || update.docChanged || update.viewportChanged) this.decorations = build(update.view);
      }
    },
    { decorations: (v) => v.decorations },
  );
}

/** Autocomplete for `{{` → variables, `secret:` and template functions. */
export function templateCompletion(source: () => TemplateSuggestions): Extension {
  return autocompletion({
    activateOnTyping: true,
    override: [
      (context: CompletionContext): CompletionResult | null => {
        const line = context.state.doc.lineAt(context.pos);
        const ctx = completionContext(line.text, context.pos - line.from);
        if (!ctx) return null;
        const { variables, functions } = source();
        const close = (text: string) => (line.text.slice(context.pos - line.from).startsWith("}}") ? text : `${text}}}`);
        const options: Completion[] = [
          ...variables.map((v) => ({
            label: v.name,
            type: "variable",
            detail: truncate(v.value, 30),
            apply: close(v.name),
          })),
          ...functions.map((f) => ({
            label: `${f.name}()`,
            type: "function",
            detail: f.plugin ? `plugin ${f.plugin}` : "built-in",
            info: f.description,
            apply: close(`${f.name}()`),
          })),
          { label: "secret:", type: "keyword", detail: "from the OS keychain", info: "{{secret:name}} reads a secret you stored in Environments → Secrets." },
        ];
        return { from: line.from + ctx.from, options, validFor: /^[\w.:-]*$/ };
      },
    ],
  });
}

function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}
