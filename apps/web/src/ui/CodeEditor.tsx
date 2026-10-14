import { lazy, Suspense } from "react";
import type { CodeEditorProps } from "./CodeEditorImpl";

export type { CodeEditorProps, EditorLanguage } from "./CodeEditorImpl";

const Impl = lazy(() => import("./CodeEditorImpl"));

/**
 * A CodeMirror editor, loaded on first use (CodeMirror is most of the app's code).
 * Until it is ready the text is shown as-is, so nothing jumps.
 */
export function CodeEditor(props: CodeEditorProps) {
  return (
    <Suspense fallback={<EditorFallback {...props} />}>
      <Impl {...props} />
    </Suspense>
  );
}

function EditorFallback({ value, placeholder, minHeight, maxHeight, className, ariaLabel }: CodeEditorProps) {
  return (
    <div className={`code-editor code-editor-loading ${className ?? ""}`} style={{ minHeight, maxHeight }} aria-busy="true" aria-label={ariaLabel}>
      <pre className={value ? undefined : "faint"}>{value || placeholder}</pre>
    </div>
  );
}
