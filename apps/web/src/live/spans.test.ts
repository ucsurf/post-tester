import { beforeEach, describe, expect, it } from "vitest";
import { applyEvents, emptyLiveState, STOPPED_BEFORE_FINISHING, type LiveState } from "./spans";
import { event, finished, resetSeq, spanId, started } from "./testing";

const A = spanId(1);
const B = spanId(2);
const C = spanId(3);

beforeEach(resetSeq);

function span(state: LiveState, id: string) {
  const s = state.spans.get(id);
  if (!s) throw new Error(`span ${id} missing`);
  return s;
}

describe("applyEvents: a single request", () => {
  it("folds the full lifecycle into one span", () => {
    const events = [
      event(A, started("login")),
      event(A, { type: "phase", phase: "dns", duration_ms: 3 }),
      event(A, { type: "phase", phase: "connect", duration_ms: 12 }),
      event(A, { type: "connected", remote_addr: "1.2.3.4:443", tls: { version: "TLSv1.3", cipher: "AES", alpn: "h2" } }),
      event(A, {
        type: "request_sent",
        method: "POST",
        url: "https://api.test/login",
        http_version: "HTTP/2",
        headers: [{ name: "accept", value: "*/*" }],
        body: { preview: "{}", size: 2, truncated: false },
      }),
      event(A, {
        type: "response_received",
        status: 201,
        status_text: "Created",
        http_version: "HTTP/2",
        headers: [],
        body: { preview: "{\"token\":\"x\"}", size: 13, truncated: false },
      }),
      event(A, { type: "log", level: "info", text: "hello" }),
      event(A, { type: "test_result", name: "status == 201", passed: true }),
      event(A, { type: "variable_set", name: "token", value: "x" }),
      event(A, finished("ok", 201)),
    ];
    const state = applyEvents(emptyLiveState(), events);
    const s = span(state, A);

    expect(state.roots).toEqual([A]);
    expect(s.title).toBe("login");
    expect(s.started).toBe(true);
    expect(s.source).toEqual({ type: "user" });
    expect(s.phases).toEqual({ dns: 3, connect: 12 });
    expect(s.connection).toEqual({ remoteAddr: "1.2.3.4:443", tls: { version: "TLSv1.3", cipher: "AES", alpn: "h2" } });
    expect(s.request?.method).toBe("POST");
    expect(s.response?.statusText).toBe("Created");
    expect(s.logs.map((l) => l.text)).toEqual(["hello"]);
    expect(s.tests).toEqual([expect.objectContaining({ name: "status == 201", passed: true, detail: null })]);
    expect(s.variables.map((v) => [v.name, v.value])).toEqual([["token", "x"]]);
    expect(s.outcome).toBe("ok");
    expect(s.status).toBe(201);
    expect(s.size).toBe(128);
    expect(s.durationMs).toBe(42);
    expect(s.events).toHaveLength(10);
    expect(s.finishedAt).toBeGreaterThan(s.startedAt);
  });

  it("leaves the outcome empty while running", () => {
    const state = applyEvents(emptyLiveState(), [event(A, started("slow"))]);
    expect(span(state, A).outcome).toBeNull();
  });

  it("records an error message from Finished", () => {
    const state = applyEvents(emptyLiveState(), [
      event(A, started("x")),
      event(A, { type: "finished", outcome: "error", duration_ms: 5, error: "connection refused" }),
    ]);
    expect(span(state, A)).toMatchObject({ outcome: "error", error: "connection refused", status: null });
  });
});

describe("applyEvents: immutability", () => {
  it("never mutates the previous state and keeps unchanged spans identical", () => {
    const first = applyEvents(emptyLiveState(), [event(A, started("a")), event(B, started("b"))]);
    const firstA = span(first, A);
    const firstB = span(first, B);
    const second = applyEvents(first, [event(A, { type: "log", level: "info", text: "later" })]);

    expect(firstA.logs).toHaveLength(0);
    expect(span(second, A)).not.toBe(firstA);
    expect(span(second, A).logs).toHaveLength(1);
    expect(span(second, B)).toBe(firstB);
    expect(second.version).toBe(first.version + 1);
  });

  it("returns the same state for an empty batch", () => {
    const state = emptyLiveState();
    expect(applyEvents(state, [])).toBe(state);
  });
});

describe("applyEvents: tree", () => {
  it("links children to their parent and orders roots newest first", () => {
    const state = applyEvents(emptyLiveState(), [
      event(A, started("Run", "collection_run")),
      event(A, { type: "progress", done: 0, total: 2 }),
      event(B, started("first"), { parent: A }),
      event(B, finished(), { parent: A }),
      event(A, { type: "progress", done: 1, total: 2 }),
      event(C, started("second"), { parent: A }),
      event(spanId(9), started("other")),
    ]);
    expect(state.roots).toEqual([spanId(9), A]);
    expect(span(state, A).children).toEqual([B, C]);
    expect(span(state, A).progress).toEqual({ done: 1, total: 2 });
    expect(span(state, B).parentId).toBe(A);
  });

  it("handles a child arriving before its parent", () => {
    const early = applyEvents(emptyLiveState(), [event(B, started("child"), { parent: A, seq: 5 })]);
    expect(early.roots).toEqual([A]);
    expect(span(early, A).started).toBe(false);
    expect(span(early, A).children).toEqual([B]);

    const late = applyEvents(early, [event(A, started("parent", "collection_run"), { seq: 1 })]);
    expect(late.roots).toEqual([A]);
    expect(span(late, A)).toMatchObject({ started: true, title: "parent", kind: "collection_run" });
  });

  it("moves a placeholder root under its parent when its own parent becomes known", () => {
    const s1 = applyEvents(emptyLiveState(), [event(C, started("grandchild"), { parent: B, seq: 10 })]);
    expect(s1.roots).toEqual([B]);
    const s2 = applyEvents(s1, [event(B, started("child"), { parent: A, seq: 2 })]);
    expect(s2.roots).toEqual([A]);
    expect(span(s2, A).children).toEqual([B]);
    expect(span(s2, B).children).toEqual([C]);
  });
});

describe("applyEvents: out-of-order and duplicates", () => {
  it("sorts a batch by seq before applying", () => {
    const state = applyEvents(emptyLiveState(), [
      event(A, finished(), { seq: 3 }),
      event(A, { type: "log", level: "info", text: "two" }, { seq: 2 }),
      event(A, started("a"), { seq: 1 }),
    ]);
    expect(span(state, A)).toMatchObject({ title: "a", outcome: "ok" });
    expect(span(state, A).events.map((e) => e.seq)).toEqual([1, 2, 3]);
  });

  it("keeps the outcome when an older event arrives after Finished", () => {
    const s1 = applyEvents(emptyLiveState(), [event(A, started("a"), { seq: 1 }), event(A, finished("failed", 500), { seq: 4 })]);
    const s2 = applyEvents(s1, [
      event(A, { type: "log", level: "warn", text: "late" }, { seq: 3 }),
      event(A, { type: "message", direction: "received", payload: { preview: "m", size: 1, truncated: false } }, { seq: 2 }),
    ]);
    const s = span(s2, A);
    expect(s.outcome).toBe("failed");
    expect(s.status).toBe(500);
    expect(s.logs.map((l) => l.text)).toEqual(["late"]);
    expect(s.events.map((e) => e.seq)).toEqual([1, 2, 3, 4]);
  });

  it("ignores events it has already applied (history replay after reconnect)", () => {
    const batch = [event(A, started("a")), event(A, { type: "log", level: "info", text: "once" }), event(A, finished())];
    const s1 = applyEvents(emptyLiveState(), batch);
    const s2 = applyEvents(s1, batch);
    expect(s2).toBe(s1);
    expect(span(s2, A).logs).toHaveLength(1);
    expect(span(s2, A).eventCount).toBe(3);
  });

  it("applies a late event whose seq is older than the newest one but new to the span", () => {
    const s1 = applyEvents(emptyLiveState(), [event(A, started("a"), { seq: 1 }), event(A, finished(), { seq: 5 })]);
    const s2 = applyEvents(s1, [event(A, { type: "phase", phase: "dns", duration_ms: 1 }, { seq: 3 })]);
    expect(span(s2, A).phases.dns).toBe(1);
  });
});

describe("applyEvents: limits", () => {
  it("caps per-span entries, keeping the newest", () => {
    const events = [event(A, started("ws", "web_socket"))];
    for (let i = 0; i < 10; i++) {
      events.push(event(A, { type: "message", direction: i % 2 ? "sent" : "received", payload: { preview: `m${i}`, size: 2, truncated: false } }));
    }
    const state = applyEvents(emptyLiveState(), events, { maxSpans: 100, maxEntriesPerSpan: 4 });
    const s = span(state, A);
    expect(s.messages.map((m) => m.payload.preview)).toEqual(["m6", "m7", "m8", "m9"]);
    expect(s.events).toHaveLength(4);
    expect(s.eventCount).toBe(11);
  });

  it("evicts the oldest finished trees first and never running ones", () => {
    const options = { maxSpans: 3, maxEntriesPerSpan: 100 };
    const state = applyEvents(
      emptyLiveState(),
      [
        event(spanId(1), started("running-old")),
        event(spanId(2), started("done-old")),
        event(spanId(2), finished()),
        event(spanId(3), started("child"), { parent: spanId(2) }),
        event(spanId(3), finished(), { parent: spanId(2) }),
        event(spanId(4), started("new")),
        event(spanId(4), finished()),
        event(spanId(5), started("newest")),
      ],
      options,
    );
    expect([...state.spans.keys()].sort()).toEqual([spanId(1), spanId(4), spanId(5)]);
    expect(state.roots).toEqual([spanId(5), spanId(4), spanId(1)]);
  });
});

describe("applyEvents: eviction and replays", () => {
  const options = { maxSpans: 2, maxEntriesPerSpan: 100 };

  it("ignores replayed events of evicted spans instead of resurrecting them", () => {
    const history = [
      event(spanId(1), started("old")),
      event(spanId(1), finished()),
      event(spanId(2), started("middle")),
      event(spanId(2), finished()),
      event(spanId(3), started("new")),
      event(spanId(3), finished()),
    ];
    const state = applyEvents(emptyLiveState(), history, options);
    expect([...state.spans.keys()].sort()).toEqual([spanId(2), spanId(3)]);
    expect(state.dropped.has(spanId(1))).toBe(true);
    const replayed = applyEvents(state, history, options);
    expect(replayed).toBe(state);
  });

  it("evicts a placeholder parent (never seen) once its children are finished", () => {
    const state = applyEvents(
      emptyLiveState(),
      [
        event(spanId(2), started("orphan"), { parent: spanId(1) }),
        event(spanId(2), finished(), { parent: spanId(1) }),
        event(spanId(5), started("a")),
        event(spanId(5), finished()),
        event(spanId(6), started("b")),
      ],
      options,
    );
    expect(state.spans.has(spanId(1))).toBe(false);
    expect(state.roots).toEqual([spanId(6), spanId(5)]);
  });
});

describe("applyEvents: operations interrupted by a restart", () => {
  it("shows spans the server closed after a restart as cancelled, with an unknown duration", () => {
    const state = applyEvents(emptyLiveState(), [
      event(A, started("Run", "collection_run")),
      event(B, started("slow"), { parent: A }),
      event(B, { type: "finished", outcome: "cancelled", duration_ms: 0, error: STOPPED_BEFORE_FINISHING }, { parent: A }),
      event(A, { type: "finished", outcome: "cancelled", duration_ms: 0, error: STOPPED_BEFORE_FINISHING }),
    ]);
    for (const id of [A, B]) {
      expect(span(state, id)).toMatchObject({ outcome: "cancelled", durationMs: null, error: STOPPED_BEFORE_FINISHING });
    }
  });

  it("keeps the duration of a normal cancellation", () => {
    const state = applyEvents(emptyLiveState(), [event(A, started("x")), event(A, { type: "finished", outcome: "cancelled", duration_ms: 812 })]);
    expect(span(state, A).durationMs).toBe(812);
  });
});
