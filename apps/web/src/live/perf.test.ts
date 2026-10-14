import type { Event } from "@post-tester/bindings";
import { beforeEach, describe, expect, it } from "vitest";
import { sameRowData, rowData, type RowContext } from "../features/activity/rows";
import { filterRoots, EMPTY_FILTER, flattenRows } from "./selectors";
import { applyEvents, emptyLiveState, type LiveState } from "./spans";
import { event, finished, resetSeq, spanId, started } from "./testing";

/**
 * Benchmark-style checks: the fold must stay fast under heavy load (a 1,000-request
 * collection run plus an SSE stream with tens of thousands of messages), delivered in
 * the small batches the server sends (~every 50 ms) and applied once per frame.
 * Budgets are generous so slow CI machines pass; locally these take ~10-60 ms.
 */

beforeEach(resetSeq);

const message = (n: number) => ({ type: "message" as const, direction: "received" as const, payload: { preview: `{"n":${n}}`, size: 8, truncated: false } });

/** A run with `requests` children (7 events each) and an SSE span filling up to `total` events. */
function heavyLoad(requests: number, total: number): Event[] {
  const events: Event[] = [];
  const run = spanId(1);
  const sse = spanId(2);
  events.push(event(run, started("Run", "collection_run")));
  events.push(event(sse, started("stream", "web_socket")));
  for (let i = 0; i < requests; i++) {
    const id = spanId(100 + i);
    events.push(event(id, started(`req-${i}`), { parent: run }));
    events.push(event(id, { type: "phase", phase: "dns", duration_ms: 1 }, { parent: run }));
    events.push(event(id, { type: "phase", phase: "connect", duration_ms: 2 }, { parent: run }));
    events.push(event(id, { type: "phase", phase: "waiting", duration_ms: 5 }, { parent: run }));
    events.push(event(id, { type: "test_result", name: "status == 200", passed: i % 10 !== 0 }, { parent: run }));
    events.push(event(id, finished(i % 10 === 0 ? "failed" : "ok"), { parent: run }));
    events.push(event(run, { type: "progress", done: i + 1, total: requests }));
    // Interleave the stream with the run, as on a busy server.
    for (let m = 0; m < 30 && events.length < total; m++) events.push(event(sse, message(events.length)));
  }
  while (events.length < total) events.push(event(sse, message(events.length)));
  return events;
}

function foldInBatches(events: readonly Event[], size: number): { state: LiveState; ms: number } {
  const t0 = performance.now();
  let state = emptyLiveState();
  for (let i = 0; i < events.length; i += size) state = applyEvents(state, events.slice(i, i + size));
  return { state, ms: performance.now() - t0 };
}

describe("live fold performance", () => {
  const events = heavyLoad(1000, 50_000);

  it("folds 50,000 events in one batch quickly", () => {
    const t0 = performance.now();
    const state = applyEvents(emptyLiveState(), events);
    const ms = performance.now() - t0;
    console.info(`[perf] 50,000 events, one batch: ${ms.toFixed(1)} ms`);
    expect(ms).toBeLessThan(1500);
    expect(state.spans.get(spanId(1))?.children).toHaveLength(1000);
  });

  it("folds 50,000 events in server-sized batches (250 per batch) quickly", () => {
    const { state, ms } = foldInBatches(events, 250);
    console.info(`[perf] 50,000 events, 200 batches of 250: ${ms.toFixed(1)} ms (${((ms / 200) * 1000).toFixed(0)} µs per batch)`);
    expect(ms).toBeLessThan(2000);
    const run = state.spans.get(spanId(1));
    const sse = state.spans.get(spanId(2));
    expect(run?.progress).toEqual({ done: 1000, total: 1000 });
    expect(sse?.messages).toHaveLength(2000); // capped, newest kept
    expect(sse?.eventCount).toBe(50_000 - 1000 * 7 - 1);
    expect(sse?.messages.at(-1)?.payload.preview).toBe(`{"n":${50_000 - 1}}`);
  });

  it("keeps memory bounded with 10,000 separate operations", () => {
    const many: Event[] = [];
    for (let i = 0; i < 10_000; i++) {
      const id = spanId(10 + i);
      many.push(event(id, started(`r${i}`)), event(id, { type: "phase", phase: "dns", duration_ms: 1 }), event(id, { type: "phase", phase: "waiting", duration_ms: 3 }), event(id, { type: "log", level: "info", text: "x" }), event(id, finished()));
    }
    const t0 = performance.now();
    let state = emptyLiveState();
    for (let i = 0; i < many.length; i += 250) state = applyEvents(state, many.slice(i, i + 250), { maxSpans: 5000, maxEntriesPerSpan: 2000 });
    const ms = performance.now() - t0;
    console.info(`[perf] 50,000 events / 10,000 spans with eviction at 5,000: ${ms.toFixed(1)} ms`);
    expect(ms).toBeLessThan(2000);
    expect(state.spans.size).toBeLessThanOrEqual(5000);
    expect(state.roots[0]).toBe(spanId(10 + 9999));
  });

  it("flattening and row props for 1,000 expanded children stay cheap", () => {
    const { state } = foldInBatches(events, 250);
    const t0 = performance.now();
    for (let i = 0; i < 60; i++) {
      // One second of frames: filter, flatten, and build every row's props.
      const rows = flattenRows(state, filterRoots(state, EMPTY_FILTER), new Set([spanId(1)]));
      const ctx: RowContext = { expanded: new Set([spanId(1)]), selected: null, activeId: null, mountedAt: 0, now: 0 };
      rows.forEach((row) => rowData(state, row, ctx));
    }
    const ms = performance.now() - t0;
    console.info(`[perf] 60 frames of filter + flatten + row props for 1,002 rows: ${ms.toFixed(1)} ms`);
    expect(ms).toBeLessThan(1000);
  });
});

describe("row re-renders", () => {
  it("only the rows whose span changed get new props after a batch", () => {
    const run = spanId(1);
    let state = applyEvents(emptyLiveState(), [event(run, started("Run", "collection_run"))]);
    for (let i = 0; i < 20; i++) {
      const id = spanId(10 + i);
      state = applyEvents(state, [event(id, started(`r${i}`), { parent: run }), event(id, finished(), { parent: run })]);
    }
    const others = Array.from({ length: 20 }, (_, i) => spanId(100 + i));
    state = applyEvents(state, others.flatMap((id, i) => [event(id, started(`o${i}`)), event(id, finished())]));
    const ctx: RowContext = { expanded: new Set([run]), selected: null, activeId: null, mountedAt: 0, now: 0 };
    const props = (s: LiveState) => new Map(flattenRows(s, filterRoots(s, EMPTY_FILTER), ctx.expanded).map((row) => [row.id, rowData(s, row, ctx)]));
    const before = props(state);

    // A new message on one span, and a new running span at the top.
    const next = applyEvents(state, [event(others[5] as string, { type: "log", level: "info", text: "late" }), event(spanId(500), started("new"))]);
    const after = props(next);

    const changed = [...after.keys()].filter((id) => {
      const a = before.get(id);
      const b = after.get(id);
      return !a || !b || !sameRowData(a, b);
    });
    expect(changed.sort()).toEqual([others[5], spanId(500)].sort());
  });

  it("a run's row changes when a child fails, even though the run span itself did not", () => {
    const run = spanId(1);
    const child = spanId(2);
    const s1 = applyEvents(emptyLiveState(), [event(run, started("Run", "collection_run")), event(child, started("c"), { parent: run })]);
    const ctx: RowContext = { expanded: new Set(), selected: null, activeId: null, mountedAt: 0, now: 0 };
    const row = { id: run, depth: 0, hasChildren: true };
    const before = rowData(s1, row, ctx);
    const s2 = applyEvents(s1, [event(child, finished("error"), { parent: run })]);
    const after = rowData(s2, row, ctx);
    expect(s2.spans.get(run)).toBe(s1.spans.get(run));
    expect(before && after && sameRowData(before, after)).toBe(false);
    expect(after?.failedChildren).toBe(1);
  });
});
