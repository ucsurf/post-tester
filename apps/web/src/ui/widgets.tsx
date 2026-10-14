import type { Outcome, Source } from "@post-tester/bindings";
import { useState, type ReactNode } from "react";
import { statusTone } from "../lib/format";
import { sourceInfo, sourceLabel } from "../lib/source";
import type { SpanStatus } from "../live/selectors";
import { showToast } from "../state/toasts";
import { Icon, type IconName } from "./Icon";
import "./widgets.css";

/** Small display components shared across features. */

export function MethodBadge({ method }: { method: string | undefined | null }) {
  const m = (method ?? "").toUpperCase();
  return (
    <span className="method" data-method={m}>
      {m === "DELETE" ? "DEL" : m === "OPTIONS" ? "OPT" : m || "—"}
    </span>
  );
}

export function StatusBadge({ status, text }: { status: number | null | undefined; text?: string }) {
  if (!status) return null;
  return (
    <span className={`badge tone-${statusTone(status)}`} title={text ? `${status} ${text}` : String(status)}>
      {status}
      {text ? ` ${text}` : ""}
    </span>
  );
}

export function SourceBadge({ source, compact }: { source: Source; compact?: boolean }) {
  const info = sourceInfo(source);
  const label = sourceLabel(source);
  return (
    <span className="source-badge" data-source={info.type} title={`Started by ${label}`}>
      <span aria-hidden="true">{info.icon}</span>
      {!compact && <span className="truncate">{info.detail && info.type === "agent" ? info.detail : info.label}</span>}
      {compact && <span className="sr-only">{label}</span>}
    </span>
  );
}

const OUTCOME_META: Record<SpanStatus, { icon: IconName | null; label: string; tone: string }> = {
  running: { icon: null, label: "Running", tone: "tone-info" },
  ok: { icon: "check", label: "Succeeded", tone: "tone-success" },
  failed: { icon: "x", label: "Tests failed", tone: "tone-warning" },
  error: { icon: "alert", label: "Error", tone: "tone-danger" },
  cancelled: { icon: "stop", label: "Cancelled", tone: "tone-neutral" },
};

export function OutcomeIcon({ status }: { status: SpanStatus }) {
  const meta = OUTCOME_META[status];
  return (
    <span className={`outcome ${meta.tone}`} title={meta.label} role="img" aria-label={meta.label}>
      {meta.icon ? <Icon name={meta.icon} size={13} /> : <span className="spinner" />}
    </span>
  );
}

export function outcomeLabel(outcome: Outcome | "running"): string {
  return OUTCOME_META[outcome].label;
}

export function ProgressBar({ done, total, failed = 0 }: { done: number; total: number; failed?: number }) {
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  const failedPct = total > 0 ? Math.min(pct, (failed / total) * 100) : 0;
  return (
    <span className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done} aria-label={`${done} of ${total}`}>
      <span className="progress-fill" style={{ width: `${pct}%` }} />
      {failedPct > 0 && <span className="progress-failed" style={{ width: `${failedPct}%` }} />}
    </span>
  );
}

export function EmptyState({ icon, title, children, action }: { icon?: string; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      {icon && (
        <div className="empty-icon" aria-hidden="true">
          {icon}
        </div>
      )}
      <h3>{title}</h3>
      {children && <div className="muted">{children}</div>}
      {action}
    </div>
  );
}

export async function copyText(text: string, what = "Copied to clipboard"): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    showToast("success", what);
  } catch {
    showToast("error", "Could not copy", "The browser blocked clipboard access. Select the text and copy it manually.");
  }
}

export function CopyButton({ text, label = "Copy", small = true }: { text: string; label?: string; small?: boolean }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={`btn btn-ghost ${small ? "btn-sm" : ""}`}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        } catch {
          showToast("error", "Could not copy", "The browser blocked clipboard access. Select the text and copy it manually.");
        }
      }}
    >
      <Icon name={copied ? "check" : "copy"} size={12} />
      {copied ? "Copied" : label}
    </button>
  );
}

/** Headers as a two-column table. */
export function HeaderTable({ headers, empty = "No headers" }: { headers: readonly (readonly [string, string])[]; empty?: string }) {
  if (headers.length === 0) return <p className="muted pad">{empty}</p>;
  return (
    <table className="table header-table">
      <tbody>
        {headers.map(([name, value], i) => (
          <tr key={`${name}-${i}`}>
            <td className="mono header-name">{name}</td>
            <td className="mono">{value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <span className="row muted" role="status">
      <span className="spinner" />
      {label}
    </span>
  );
}
