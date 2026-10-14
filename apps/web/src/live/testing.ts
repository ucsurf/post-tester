import type { Event, EventKind, Source } from "@post-tester/bindings";

/** Test helpers that build events with realistic defaults. */

let seq = 0;

export function resetSeq(): void {
  seq = 0;
}

/** A UUID v7-like id whose string order follows `n`, like real span ids. */
export function spanId(n: number): string {
  return `0190a000-0000-7000-8000-${String(n).padStart(12, "0")}`;
}

export function event(
  span: string,
  kind: EventKind,
  extra: { parent?: string; source?: Source; seq?: number; at?: string } = {},
): Event {
  const next = extra.seq ?? ++seq;
  return {
    seq: next,
    span_id: span,
    ...(extra.parent ? { parent_id: extra.parent } : {}),
    source: extra.source ?? { type: "user" },
    at: extra.at ?? new Date(Date.UTC(2026, 0, 1, 12, 0, 0, next)).toISOString(),
    kind,
  };
}

export const started = (title: string, kind: "request" | "collection_run" | "web_socket" = "request"): EventKind => ({
  type: "started",
  kind,
  title,
  summary: { protocol: kind === "web_socket" ? "ws" : "http", method: "GET", url: `https://api.test/${title}` },
});

export const finished = (outcome: "ok" | "failed" | "error" | "cancelled" = "ok", status = 200): EventKind => ({
  type: "finished",
  outcome,
  duration_ms: 42,
  status,
  size: 128,
});
