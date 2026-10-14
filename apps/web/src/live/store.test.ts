import { beforeEach, describe, expect, it, vi } from "vitest";
import { backoffDelay, LiveConnection, parseEventMessage } from "./connection";
import { childCounts, EMPTY_FILTER, filterRoots, findRecentRoot, findUserSpan, flattenRows, spanElapsed, spanProtocol } from "./selectors";
import { applyEvents, emptyLiveState } from "./spans";
import { LiveStore } from "./store";
import { event, finished, resetSeq, spanId, started } from "./testing";

beforeEach(resetSeq);

describe("LiveStore", () => {
  it("applies pushed events once per scheduled flush and notifies once", () => {
    const flushes: (() => void)[] = [];
    const store = new LiveStore((flush) => flushes.push(flush));
    const listener = vi.fn();
    store.subscribe(listener);

    store.push([event(spanId(1), started("a"))]);
    store.push([event(spanId(2), started("b"))]);
    expect(flushes).toHaveLength(1);
    expect(store.getState().spans.size).toBe(0);

    flushes[0]?.();
    expect(store.getState().spans.size).toBe(2);
    expect(listener).toHaveBeenCalledTimes(1);

    store.push([event(spanId(1), finished())]);
    expect(flushes).toHaveLength(2);
  });

  it("clear() forgets every span", () => {
    const store = new LiveStore((flush) => flush());
    store.push([event(spanId(1), started("a"))]);
    store.clear();
    expect(store.getState().roots).toEqual([]);
  });
});

describe("selectors", () => {
  const state = applyEvents(emptyLiveState(), [
    event(spanId(1), started("Run", "collection_run"), { source: { type: "cli" } }),
    event(spanId(2), started("users"), { parent: spanId(1), source: { type: "cli" } }),
    event(spanId(2), finished("failed", 500), { parent: spanId(1), source: { type: "cli" } }),
    event(spanId(3), started("socket", "web_socket"), { source: { type: "agent", client: "claude-code" } }),
    event(spanId(4), started("login")),
    event(spanId(4), finished("ok", 200)),
  ]);

  it("filters by source, protocol, status and text, including descendants", () => {
    expect(filterRoots(state, EMPTY_FILTER)).toEqual([spanId(4), spanId(3), spanId(1)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, sources: new Set(["agent"]) })).toEqual([spanId(3)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, protocols: new Set(["ws"]) })).toEqual([spanId(3)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, statuses: new Set(["running"]) })).toEqual([spanId(3), spanId(1)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, statuses: new Set(["failed"]) })).toEqual([spanId(1)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, text: "LOGIN" })).toEqual([spanId(4)]);
    expect(filterRoots(state, { ...EMPTY_FILTER, text: "500" })).toEqual([spanId(1)]);
  });

  it("flattens expanded trees", () => {
    const roots = filterRoots(state, EMPTY_FILTER);
    expect(flattenRows(state, roots, new Set()).map((r) => r.id)).toEqual([spanId(4), spanId(3), spanId(1)]);
    expect(flattenRows(state, roots, new Set([spanId(1)]))).toEqual([
      { id: spanId(4), depth: 0, hasChildren: false },
      { id: spanId(3), depth: 0, hasChildren: false },
      { id: spanId(1), depth: 0, hasChildren: true },
      { id: spanId(2), depth: 1, hasChildren: false },
    ]);
  });

  it("derives protocol, elapsed time and child counts", () => {
    const run = state.spans.get(spanId(1));
    const socket = state.spans.get(spanId(3));
    if (!run || !socket) throw new Error("missing spans");
    expect(spanProtocol(socket)).toBe("ws");
    expect(spanElapsed(run, run.startedAt + 1500)).toBe(1500);
    expect(childCounts(state, run)).toMatchObject({ failed: 1, running: 0 });
  });

  it("finds the span of a request the user just sent", () => {
    const login = state.spans.get(spanId(4));
    if (!login) throw new Error("missing span");
    expect(findUserSpan(state, { requestId: null, url: "https://api.test/login" }, 0)?.id).toBe(spanId(4));
    expect(findUserSpan(state, { requestId: null, url: "https://api.test/login" }, login.startedAt + 1)).toBeNull();
    expect(findUserSpan(state, { requestId: "auth/login", url: "" }, 0)).toBeNull();
  });

  it("finds the newest matching root started after a moment", () => {
    expect(findRecentRoot(state, 0, (s) => s.kind === "collection_run")?.id).toBe(spanId(1));
    const run = state.spans.get(spanId(1));
    expect(findRecentRoot(state, (run?.startedAt ?? 0) + 1, (s) => s.kind === "collection_run")).toBeNull();
  });
});

describe("connection helpers", () => {
  it("backs off exponentially with a cap", () => {
    const fixed = () => 0.5;
    expect(backoffDelay(0, fixed)).toBe(500);
    expect(backoffDelay(1, fixed)).toBe(1000);
    expect(backoffDelay(3, fixed)).toBe(4000);
    expect(backoffDelay(20, fixed)).toBe(15_000);
    expect(backoffDelay(0, () => 0)).toBe(400);
  });

  it("parses batches and single events, ignoring junk", () => {
    const e = event(spanId(1), started("a"));
    expect(parseEventMessage(JSON.stringify([e]))).toEqual([e]);
    expect(parseEventMessage(JSON.stringify(e))).toEqual([e]);
    expect(parseEventMessage("not json")).toEqual([]);
    expect(parseEventMessage(JSON.stringify([{ hello: 1 }]))).toEqual([]);
  });

  it("reconnects after the socket closes", () => {
    vi.useFakeTimers();
    const sockets: { onclose: ((ev: unknown) => void) | null; onopen: ((ev: unknown) => void) | null }[] = [];
    const received: unknown[] = [];
    const connection = new LiveConnection({
      url: () => "ws://x/api/live",
      onEvents: (events) => received.push(...events),
      createSocket: () => {
        const socket = { onopen: null, onclose: null, onerror: null, onmessage: null, close: vi.fn() };
        sockets.push(socket);
        return socket;
      },
    });
    connection.start();
    expect(connection.getStatus().state).toBe("connecting");
    sockets[0]?.onopen?.({});
    expect(connection.getStatus().state).toBe("open");
    sockets[0]?.onclose?.({});
    expect(connection.getStatus()).toMatchObject({ state: "closed", attempts: 1 });
    vi.advanceTimersByTime(1000);
    expect(sockets).toHaveLength(2);
    connection.stop();
    vi.useRealTimers();
  });
});
