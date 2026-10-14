import { lazy, Suspense } from "react";
import type { TemplateInputProps } from "./TemplateInputImpl";

export type { TemplateInputProps } from "./TemplateInputImpl";

const Impl = lazy(() => import("./TemplateInputImpl"));

/**
 * A one-line input that highlights `{{variables}}`, loaded on first use (it is built
 * on CodeMirror). Until it is ready the value is shown in the same box.
 */
export function TemplateInput(props: TemplateInputProps) {
  return (
    <Suspense
      fallback={
        <div className={`template-input template-input-loading ${props.className ?? ""}`} aria-busy="true" aria-label={props.ariaLabel}>
          <span className={props.value ? undefined : "faint"}>{props.value || props.placeholder}</span>
        </div>
      }
    >
      <Impl {...props} />
    </Suspense>
  );
}
