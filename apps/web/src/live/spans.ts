import type {
  Direction,
  Event,
  Header,
  Level,
  Outcome,
  Payload,
  Phase,
  Source,
  SpanId,
  SpanKind,
  Summary,
  TlsInfo,
} from "@post-tester/bindings";

/**
 * Folds the live event stream into a map of spans.
 *
 * Pure and framework-independent: `applyEvents(state, events)` returns a new state
 * and never mutates the old one. Spans that changed are new objects (so UI code can
 * compare by reference); unchanged spans keep their identity.
 *
 * Robust against duplicates (history replay after a reconnect) and out-of-order
 * delivery (a child's events before its parent's, or a span's events across batches).
 */

export type RequestInfo = { method: string; url: string; httpVersion: string; headers: Header[]; body: Payload };
export type ResponseInfo = { status: number; statusText: string; httpVersion: string; headers: Header[]; body: Payload };
export type ConnectionInfo = { remoteAddr: string; tls: TlsInfo | null };
export type MessageEntry = { seq: number; at: number; direction: Direction; label: string | null; payload: Payload };
export type LogEntry = { seq: number; at: number; level: Level; text: string };
export type TestEntry = { seq: number; name: string; passed: boolean; detail: string | null };
export type VariableEntry = { seq: number; at: number; name: string; value: string };

export type SpanState = {
  id: SpanId;
  parentId: SpanId | null;
  source: Source;
  kind: SpanKind;
  title: string;
  summary: Summary;
  /** False while only later events (or a child's reference) have been seen. */
  started: boolean;
  /** Epoch ms of the `Started` event (or of the earliest event seen so far). */
  startedAt: number;
  /** Epoch ms of the latest event. */
  updatedAt: number;
  finishedAt: number | null;
  /** Child span ids in creation order. */
  children: SpanId[];
  phases: Partial<Record<Phase, number>>;
  connection: ConnectionInfo | null;
  request: RequestInfo | null;
  response: ResponseInfo | null;
  messages: MessageEntry[];
  logs: LogEntry[];
  tests: TestEntry[];
  variables: VariableEntry[];
  progress: { done: number; total: number } | null;
  /** `null` while the span is running. */
  outcome: Outcome | null;
  durationMs: number | null;
  status: number | null;
  size: number | null;
  error: string | null;
  /** The raw events (newest kept when capped), for the Raw view. */
  events: Event[];
  /** Total events received, including ones dropped by the cap. */
  eventCount: number;
  lastSeq: number;
};

export type LiveState = {
  spans: ReadonlyMap<SpanId, SpanState>;
  /** Top-level span ids, newest first. */
  roots: readonly SpanId[];
  /** Increases on every change; handy as a cheap dependency. */
  version: number;
  /**
   * Ids of spans evicted to save memory. Their events are ignored from then on, so a
   * history replay after a reconnect does not bring them back as half-empty spans.
   */
  dropped: ReadonlySet<SpanId>;
};

export type LiveOptions = {
  /** Total spans kept in memory; the oldest finished top-level spans are dropped first. */
  maxSpans: number;
  /** Per-span cap for messages, logs and raw events (the newest are kept). */
  maxEntriesPerSpan: number;
};

export const DEFAULT_LIVE_OPTIONS: LiveOptions = { maxSpans: 5000, maxEntriesPerSpan: 2000 };

export function emptyLiveState(): LiveState {
  return { spans: new Map(), roots: [], version: 0, dropped: new Set() };
}

const PLACEHOLDER_SOURCE: Source = { type: "system" };

function newSpan(id: SpanId): SpanState {
  return {
    id,
    parentId: null,
    source: PLACEHOLDER_SOURCE,
    kind: "request",
    title: "",
    summary: {},
    started: false,
    startedAt: 0,
    updatedAt: 0,
    finishedAt: null,
    children: [],
    phases: {},
    connection: null,
    request: null,
    response: null,
    messages: [],
    logs: [],
    tests: [],
    variables: [],
    progress: null,
    outcome: null,
    durationMs: null,
    status: null,
    size: null,
    error: null,
    events: [],
    eventCount: 0,
    lastSeq: -1,
  };
}

function cloneSpan(span: SpanState): SpanState {
  return {
    ...span,
    children: span.children.slice(),
    phases: { ...span.phases },
    messages: span.messages.slice(),
    logs: span.logs.slice(),
    tests: span.tests.slice(),
    variables: span.variables.slice(),
    events: span.events.slice(),
  };
}

/** The error the server records for operations that were running when it stopped. */
export const STOPPED_BEFORE_FINISHING = "post-tester stopped before this finished";

/** Whether `event` was already applied to `span` (e.g. replayed history). */
function isDuplicate(span: SpanState | undefined, event: Event): boolean {
  if (!span || event.seq > span.lastSeq) return false;
  const first = span.events[0];
  // Older than everything we kept: it was applied and then dropped by the cap.
  if (first && event.seq < first.seq) return true;
  return indexBySeq(span.events, event.seq).found;
}

export function applyEvents(state: LiveState, incoming: readonly Event[], options: LiveOptions = DEFAULT_LIVE_OPTIONS): LiveState {
  if (incoming.length === 0) return state;
  const events = incoming.slice().sort((a, b) => a.seq - b.seq);

  const spans = new Map(state.spans);
  const drafts = new Set<SpanId>();
  let roots: SpanId[] | null = null;
  const mutableRoots = (): SpanId[] => (roots ??= state.roots.slice());

  const draft = (id: SpanId): SpanState => {
    const existing = spans.get(id);
    if (existing && drafts.has(id)) return existing;
    const span = existing ? cloneSpan(existing) : newSpan(id);
    spans.set(id, span);
    drafts.add(id);
    if (!existing) insertSorted(mutableRoots(), id, descending);
    return span;
  };

  let changed = false;
  const dropped = state.dropped;
  for (const event of events) {
    if (dropped.size > 0 && (dropped.has(event.span_id) || (event.parent_id !== undefined && dropped.has(event.parent_id)))) continue;
    if (isDuplicate(spans.get(event.span_id), event)) continue;
    changed = true;
    const span = draft(event.span_id);

    if (event.parent_id && span.parentId === null && event.parent_id !== span.id) {
      span.parentId = event.parent_id;
      removeValue(mutableRoots(), span.id);
      const parent = draft(event.parent_id);
      insertSorted(parent.children, span.id, ascending);
    }

    applyEvent(span, event);
  }
  if (!changed) return state;

  // Entries are appended uncapped while applying (cheap pushes); trim once per span here.
  for (const id of drafts) {
    const span = spans.get(id);
    if (span) trimEntries(span, options.maxEntriesPerSpan);
  }
  const result: LiveState = { spans, roots: roots ?? state.roots, version: state.version + 1, dropped };
  return evict(result, options.maxSpans);
}

function trimEntries(span: SpanState, cap: number): void {
  for (const list of [span.events, span.messages, span.logs, span.tests, span.variables] as { seq: number }[][]) {
    if (list.length > cap) list.splice(0, list.length - cap);
  }
}

function applyEvent(span: SpanState, event: Event): void {
  const at = Date.parse(event.at);
  const time = Number.isFinite(at) ? at : 0;

  if (!span.started) {
    span.source = event.source;
    if (span.startedAt === 0 || (time > 0 && time < span.startedAt)) span.startedAt = time;
  }
  span.updatedAt = Math.max(span.updatedAt, time);
  span.eventCount++;
  span.lastSeq = Math.max(span.lastSeq, event.seq);
  insertBySeq(span.events, event);

  const kind = event.kind;
  switch (kind.type) {
    case "started":
      span.started = true;
      span.source = event.source;
      span.kind = kind.kind;
      span.title = kind.title;
      span.summary = kind.summary;
      if (time > 0) span.startedAt = time;
      break;
    case "phase":
      span.phases[kind.phase] = kind.duration_ms;
      break;
    case "connected":
      span.connection = { remoteAddr: kind.remote_addr, tls: kind.tls ?? null };
      break;
    case "request_sent":
      span.request = { method: kind.method, url: kind.url, httpVersion: kind.http_version, headers: kind.headers, body: kind.body };
      break;
    case "response_received":
      span.response = {
        status: kind.status,
        statusText: kind.status_text,
        httpVersion: kind.http_version,
        headers: kind.headers,
        body: kind.body,
      };
      span.status = kind.status;
      break;
    case "message":
      insertBySeq(span.messages, { seq: event.seq, at: time, direction: kind.direction, label: kind.label ?? null, payload: kind.payload });
      break;
    case "log":
      insertBySeq(span.logs, { seq: event.seq, at: time, level: kind.level, text: kind.text });
      break;
    case "test_result":
      insertBySeq(span.tests, { seq: event.seq, name: kind.name, passed: kind.passed, detail: kind.detail ?? null });
      break;
    case "progress":
      span.progress = { done: kind.done, total: kind.total };
      break;
    case "variable_set":
      insertBySeq(span.variables, { seq: event.seq, at: time, name: kind.name, value: kind.value });
      break;
    case "finished":
      span.outcome = kind.outcome;
      span.finishedAt = time;
      // Closed by the server after a restart: the real duration is unknown (it reports 0).
      span.durationMs = kind.outcome === "cancelled" && kind.error === STOPPED_BEFORE_FINISHING ? null : kind.duration_ms;
      if (kind.status !== undefined) span.status = kind.status;
      if (kind.size !== undefined) span.size = kind.size;
      span.error = kind.error ?? null;
      break;
  }
}

/** Drops the oldest finished top-level spans (with their descendants) beyond `maxSpans`. */
function evict(state: LiveState, maxSpans: number): LiveState {
  if (state.spans.size <= maxSpans) return state;
  // `state.spans` is the fresh map built by this batch, so it can be changed in place.
  const spans = state.spans as Map<SpanId, SpanState>;
  const roots = state.roots.slice();
  const removed: SpanId[] = [];
  let kept = roots.length;
  for (let i = roots.length - 1; i >= 0 && spans.size > maxSpans; i--) {
    const id = roots[i] as SpanId;
    if (!isTreeFinished(spans, id)) continue;
    removeTree(spans, id, removed);
    roots[i] = "";
    kept--;
  }
  if (removed.length === 0) return state;
  return { ...state, spans, roots: kept === roots.length ? roots : roots.filter((id) => id !== ""), dropped: remember(state.dropped, removed, maxSpans * 4) };
}

/** Adds ids to the dropped set, keeping only the newest `limit`. */
function remember(dropped: ReadonlySet<SpanId>, ids: readonly SpanId[], limit: number): ReadonlySet<SpanId> {
  const next = new Set(dropped);
  ids.forEach((id) => next.add(id));
  return next.size > limit ? new Set([...next].slice(next.size - limit)) : next;
}

/**
 * Finished, including all descendants. A placeholder (a parent whose own events were
 * never seen, e.g. older than the history) counts as finished once its children are.
 */
function isTreeFinished(spans: ReadonlyMap<SpanId, SpanState>, id: SpanId): boolean {
  const span = spans.get(id);
  if (!span) return true;
  const finished = span.outcome !== null || (!span.started && span.eventCount === 0 && span.children.length > 0);
  return finished && span.children.every((child) => isTreeFinished(spans, child));
}

function removeTree(spans: Map<SpanId, SpanState>, id: SpanId, removed: SpanId[]): void {
  const span = spans.get(id);
  spans.delete(id);
  removed.push(id);
  span?.children.forEach((child) => removeTree(spans, child, removed));
}

// ── Sorted-array helpers ─────────────────────────────────────────────────────
// Span ids are UUID v7, so comparing them as strings orders spans by creation time.

const ascending = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
const descending = (a: string, b: string) => ascending(b, a);

function insertSorted(list: SpanId[], id: SpanId, compare: (a: string, b: string) => number): void {
  let lo = 0;
  let hi = list.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const c = compare(list[mid] as string, id);
    if (c === 0) return;
    if (c < 0) lo = mid + 1;
    else hi = mid;
  }
  list.splice(lo, 0, id);
}

function removeValue(list: SpanId[], id: SpanId): void {
  const index = list.indexOf(id);
  if (index !== -1) list.splice(index, 1);
}

function indexBySeq(list: readonly { seq: number }[], seq: number): { index: number; found: boolean } {
  let lo = 0;
  let hi = list.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    const value = (list[mid] as { seq: number }).seq;
    if (value === seq) return { index: mid, found: true };
    if (value < seq) lo = mid + 1;
    else hi = mid;
  }
  return { index: lo, found: false };
}

/** Inserts by `seq` (usually an append). Capping happens once per batch, in `trimEntries`. */
function insertBySeq<T extends { seq: number }>(list: T[], entry: T): void {
  const last = list[list.length - 1];
  if (!last || last.seq < entry.seq) list.push(entry);
  else list.splice(indexBySeq(list, entry.seq).index, 0, entry);
}
