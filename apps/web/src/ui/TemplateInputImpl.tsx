import { EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder as placeholderExt } from "@codemirror/view";
import { minimalSetup } from "codemirror";
import { useEffect, useRef } from "react";
import { appStore } from "../state/app";
import { useStore } from "../state/store";
import { editorTheme, refreshTemplates, templateCompletion, templateHighlighter } from "./codemirror";

export type TemplateInputProps = {
  value: string;
  onChange: (value: string) => void;
  /** Called on Enter (and Cmd/Ctrl+Enter). */
  onSubmit?: () => void;
  placeholder?: string;
  ariaLabel: string;
  className?: string;
  /**
   * Decides whether an outside `value` should replace the text being edited. Lets the
   * URL bar keep what the user typed when it means the same thing (e.g. `a%20b` vs `a b`).
   */
  isEquivalent?: (typed: string, value: string) => boolean;
};

/**
 * A one-line input that highlights `{{variables}}` (unknown ones are underlined) and
 * autocompletes variables and template functions after `{{`.
 */
export default function TemplateInput({ value, onChange, onSubmit, placeholder, ariaLabel, className, isEquivalent }: TemplateInputProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const callbacks = useRef({ onChange, onSubmit });
  callbacks.current = { onChange, onSubmit };
  const variables = useStore(appStore, (s) => s.variables);

  useEffect(() => {
    if (!host.current) return;
    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          minimalSetup,
          editorTheme,
          // Single line: newlines (e.g. from paste) become spaces.
          EditorState.transactionFilter.of((tr) => {
            if (!tr.docChanged || tr.newDoc.lines <= 1) return tr;
            const text = tr.newDoc.toString().replace(/\r?\n/g, " ");
            return [{ changes: { from: 0, to: tr.startState.doc.length, insert: text }, selection: { anchor: Math.min(text.length, tr.newSelection.main.head) } }];
          }),
          keymap.of([
            {
              key: "Enter",
              run: () => {
                callbacks.current.onSubmit?.();
                return true;
              },
            },
            {
              key: "Mod-Enter",
              run: () => {
                callbacks.current.onSubmit?.();
                return true;
              },
            },
          ]),
          EditorView.contentAttributes.of({ "aria-label": ariaLabel, "aria-multiline": "false", role: "textbox" }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) callbacks.current.onChange(update.state.doc.toString());
          }),
          templateHighlighter(() => new Set(appStore.get().variables.map((v) => v.name))),
          templateCompletion(() => ({ variables: appStore.get().variables, functions: appStore.get().templateFunctions })),
          ...(placeholder ? [placeholderExt(placeholder)] : []),
        ],
      }),
    });
    view.current = editor;
    return () => {
      editor.destroy();
      view.current = null;
    };
    // Created once; `value` changes are synced below.
  }, []);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current === value || isEquivalent?.(current, value)) return;
    editor.dispatch({ changes: { from: 0, to: current.length, insert: value } });
  }, [value, isEquivalent]);

  // Re-run the highlighter when the known variables change (unknown ones get underlined).
  useEffect(() => {
    view.current?.dispatch({ effects: refreshTemplates.of(null) });
  }, [variables]);

  return <div ref={host} className={`template-input ${className ?? ""}`} />;
}
