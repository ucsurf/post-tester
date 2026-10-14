import type { Outcome, Phase, SpanId } from "@post-tester/bindings";
import type { SourceType } from "../lib/source";
import type { LiveState, SpanState } from "./spans";

/** Pure read helpers over `LiveState`, shared by the activity list and the drawer. */

export type SpanStatus = "running" | Outcome;

export function spanStatus(span: SpanState): SpanStatus {
  return span.outcome ?? "running";
}

/** The protocol label used for filtering, e.g. `http`, `ws`, `mcp`, `run`, `load`. */
export function spanProtocol(span: SpanState): string {
  if (span.summary.protocol) return span.summary.protocol.toLowerCase();
  switch (span.kind) {
    case "request":
      return "http";
    case "web_socket":
      return "ws";
    case "mcp_call":
      return "mcp";
    case "collection_run":
      return "run";
    case "script":
      return "script";
    case "plugin_call":
      return "plugin";
    case "system":
      return "system";
  }
}

/** Elapsed time: the final duration when finished, otherwise time since start. */
export function spanElapsed(span: SpanState, now: number): number | null {
  if (span.durationMs !== null) return span.durationMs;
  if (span.startedAt <= 0) return null;
  return Math.max(0, now - span.startedAt);
}

export function spanDisplayTitle(span: SpanState): string {
  if (span.title) return span.title;
  if (span.summary.url) return span.summary.url;
  return span.started ? "(untitled)" : "Waiting for details…";
}

export const PHASES: readonly Phase[] = ["dns", "connect", "tls", "waiting", "download"];

export const PHASE_LABELS: Record<Phase, string> = {
  dns: "DNS lookup",
  connect: "TCP connect",
  tls: "TLS handshake",
  waiting: "Waiting (TTFB)",
  download: "Download",
};

export type ActivityFilter = {
  sources: ReadonlySet<SourceType>;
  protocols: ReadonlySet<string>;
  statuses: ReadonlySet<SpanStatus>;
  text: string;
};

export const EMPTY_FILTER: ActivityFilter = { sources: new Set(), protocols: new Set(), statuses: new Set(), text: "" };

export function isFilterActive(filter: ActivityFilter): boolean {
  return filter.sources.size > 0 || filter.protocols.size > 0 || filter.statuses.size > 0 || filter.text.trim() !== "";
}

function matchesSpan(span: SpanState, filter: ActivityFilter, needle: string): boolean {
  if (filter.sources.size > 0 && !filter.sources.has(span.source.type)) return false;
  if (filter.protocols.size > 0 && !filter.protocols.has(spanProtocol(span))) return false;
  if (filter.statuses.size > 0 && !filter.statuses.has(spanStatus(span))) return false;
  if (needle) {
    const haystack = [span.title, span.summary.url, span.summary.method, span.summary.request_id, span.status, span.error, span.request?.url]
      .filter((v) => v !== null && v !== undefined)
      .join(" ")
      .toLowerCase();
    if (!haystack.includes(needle)) return false;
  }
  return true;
}

/** Top-level spans that match, or that contain a matching descendant (newest first). */
export function filterRoots(state: LiveState, filter: ActivityFilter): SpanId[] {
  if (!isFilterActive(filter)) return state.roots.slice();
  const needle = filter.text.trim().toLowerCase();
  const visit = (id: SpanId, depth: number): boolean => {
    const span = state.spans.get(id);
    if (!span) return false;
    if (matchesSpan(span, filter, needle)) return true;
    return depth < 4 && span.children.some((child) => visit(child, depth + 1));
  };
  return state.roots.filter((id) => visit(id, 0));
}

export type ActivityRow = { id: SpanId; depth: number; hasChildren: boolean };

/** Flattens the visible tree: roots in the given order, children of expanded spans below them. */
export function flattenRows(state: LiveState, rootIds: readonly SpanId[], expanded: ReadonlySet<SpanId>): ActivityRow[] {
  const rows: ActivityRow[] = [];
  const add = (id: SpanId, depth: number) => {
    const span = state.spans.get(id);
    if (!span) return;
    rows.push({ id, depth, hasChildren: span.children.length > 0 });
    if (expanded.has(id)) span.children.forEach((child) => add(child, depth + 1));
  };
  rootIds.forEach((id) => add(id, 0));
  return rows;
}

/** Counts spans by status. */
export function countByStatus(spans: Iterable<SpanState>): Record<SpanStatus, number> {
  const counts: Record<SpanStatus, number> = { running: 0, ok: 0, failed: 0, error: 0, cancelled: 0 };
  for (const span of spans) counts[spanStatus(span)]++;
  return counts;
}

/** Counts children by outcome, for a run's summary line. */
export function childCounts(state: LiveState, span: SpanState): Record<SpanStatus, number> {
  const counts: Record<SpanStatus, number> = { running: 0, ok: 0, failed: 0, error: 0, cancelled: 0 };
  for (const id of span.children) {
    const child = state.spans.get(id);
    if (child) counts[spanStatus(child)]++;
  }
  return counts;
}

/**
 * The newest top-level span the user started for a given saved request (or URL)
 * at or after `since`. Used to show a request's response live while it is sending.
 */
export function findUserSpan(state: LiveState, match: { requestId: string | null; url: string }, since: number): SpanState | null {
  for (const id of state.roots) {
    const span = state.spans.get(id);
    if (!span) continue;
    if (span.startedAt > 0 && span.startedAt < since) return null;
    if (span.source.type !== "user" || span.kind !== "request") continue;
    if (match.requestId ? span.summary.request_id === match.requestId : span.summary.url === match.url) return span;
  }
  return null;
}

/** The newest top-level span started at or after `since` that matches `predicate`. */
export function findRecentRoot(state: LiveState, since: number, predicate: (span: SpanState) => boolean): SpanState | null {
  for (const id of state.roots) {
    const span = state.spans.get(id);
    if (!span) continue;
    if (span.startedAt > 0 && span.startedAt < since) return null;
    if (predicate(span)) return span;
  }
  return null;
}
