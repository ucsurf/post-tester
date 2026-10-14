import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, placeholder as placeholderExt } from "@codemirror/view";
import { basicSetup, minimalSetup } from "codemirror";
import { useEffect, useRef } from "react";
import { appStore } from "../state/app";
import { editorTheme, highlight, templateCompletion, templateHighlighter } from "./codemirror";

export type EditorLanguage = "json" | "javascript" | "text";

type Props = {
  value: string;
  onChange?: (value: string) => void;
  language?: EditorLanguage;
  readOnly?: boolean;
  placeholder?: string;
  /** Highlight and autocomplete `{{variables}}`. */
  templates?: boolean;
  /** Line numbers, folding and other heavier features. */
  full?: boolean;
  minHeight?: number;
  maxHeight?: number;
  ariaLabel: string;
  onSubmit?: () => void;
  className?: string;
  /** Keeps the typed text when an outside `value` means the same (e.g. ignoring blank lines). */
  isEquivalent?: (typed: string, value: string) => boolean;
};

function languageExtension(language: EditorLanguage): Extension {
  if (language === "json") return json();
  if (language === "javascript") return javascript();
  return [];
}

/** A CodeMirror 6 editor. The document follows `value` unless the user is mid-edit. */
export function CodeEditor({
  value,
  onChange,
  language = "text",
  readOnly = false,
  placeholder,
  templates = false,
  full = true,
  minHeight,
  maxHeight,
  ariaLabel,
  onSubmit,
  className,
  isEquivalent,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const callbacks = useRef({ onChange, onSubmit });
  callbacks.current = { onChange, onSubmit };
  const languageSlot = useRef(new Compartment());
  const readOnlySlot = useRef(new Compartment());

  useEffect(() => {
    if (!host.current) return;
    const extensions: Extension[] = [
      full ? basicSetup : minimalSetup,
      editorTheme,
      highlight,
      EditorView.lineWrapping,
      languageSlot.current.of(languageExtension(language)),
      readOnlySlot.current.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
      EditorView.contentAttributes.of({ "aria-label": ariaLabel }),
      keymap.of([
        {
          key: "Mod-Enter",
          run: () => {
            callbacks.current.onSubmit?.();
            return Boolean(callbacks.current.onSubmit);
          },
        },
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) callbacks.current.onChange?.(update.state.doc.toString());
      }),
    ];
    if (placeholder) extensions.push(placeholderExt(placeholder));
    if (templates) {
      extensions.push(
        templateHighlighter(() => new Set(appStore.get().variables.map((v) => v.name))),
        templateCompletion(() => ({ variables: appStore.get().variables, functions: appStore.get().templateFunctions })),
      );
    }
    const editor = new EditorView({ parent: host.current, state: EditorState.create({ doc: value, extensions }) });
    view.current = editor;
    return () => {
      editor.destroy();
      view.current = null;
    };
    // The editor is created once; later prop changes are applied by the effects below.
  }, []);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current === value || isEquivalent?.(current, value)) return;
    editor.dispatch({ changes: { from: 0, to: current.length, insert: value } });
  }, [value, isEquivalent]);

  useEffect(() => {
    view.current?.dispatch({ effects: languageSlot.current.reconfigure(languageExtension(language)) });
  }, [language]);

  useEffect(() => {
    view.current?.dispatch({
      effects: readOnlySlot.current.reconfigure([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
    });
  }, [readOnly]);

  return (
    <div
      ref={host}
      className={`code-editor ${className ?? ""}`}
      style={{ minHeight, maxHeight }}
      data-readonly={readOnly || undefined}
    />
  );
}
