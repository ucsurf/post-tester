import type { Event } from "@post-tester/bindings";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SeqTracker, withGapFilling } from "./gaps";
import { event, resetSeq, spanId, started } from "./testing";

beforeEach(resetSeq);

const at = (seq: number) => event(spanId(1), { type: "log", level: "info", text: String(seq) }, { seq });

describe("SeqTracker", () => {
  it("reports nothing for contiguous numbers, in any batch order", () => {
    const tracker = new SeqTracker();
    expect(tracker.observe([at(1), at(2), at(3)])).toBeNull();
    expect(tracker.observe([at(5), at(4)])).toBeNull();
    expect(tracker.lastSeq).toBe(5);
  });

  it("reports the last seq before a jump", () => {
    const tracker = new SeqTracker();
    tracker.observe([at(1), at(2)]);
    expect(tracker.observe([at(7), at(8)])).toBe(2);
    expect(tracker.observe([at(9)])).toBeNull();
  });

  it("ignores replayed history after a reconnect", () => {
    const tracker = new SeqTracker();
    tracker.observe([at(1), at(2), at(3)]);
    expect(tracker.observe([at(1), at(2), at(3), at(4)], true)).toBeNull();
  });

  it("detects events missed while disconnected (history starts after what we saw)", () => {
    const tracker = new SeqTracker();
    tracker.observe([at(1), at(2)]);
    expect(tracker.observe([at(50), at(51)], true)).toBe(2);
  });

  it("starts over when a new server numbers from scratch", () => {
    const tracker = new SeqTracker();
    tracker.observe([at(100)]);
    expect(tracker.observe([at(1), at(2)], true)).toBeNull();
    expect(tracker.lastSeq).toBe(2);
  });

  it("does not report a gap for the very first batch", () => {
    expect(new SeqTracker().observe([at(40), at(41)], true)).toBeNull();
  });
});

describe("withGapFilling", () => {
  it("delivers events, fetches a detected gap once, and delivers what was missed", async () => {
    const delivered: number[][] = [];
    const fetchSince = vi.fn(async (since: number): Promise<Event[]> => [at(since + 1), at(since + 2)]);
    const onEvents = withGapFilling((events) => delivered.push(events.map((e) => e.seq)), fetchSince);
    onEvents([at(1), at(2)]);
    onEvents([at(5)]);
    expect(fetchSince).toHaveBeenCalledWith(2);
    await vi.waitFor(() => expect(delivered).toHaveLength(3));
    expect(delivered).toEqual([[1, 2], [5], [3, 4]]);
  });

  it("queues a gap found while a fetch is running, and survives fetch errors", async () => {
    let fail = true;
    const calls: number[] = [];
    const fetchSince = async (since: number): Promise<Event[]> => {
      calls.push(since);
      if (fail) {
        fail = false;
        throw new Error("offline");
      }
      return [];
    };
    const onEvents = withGapFilling(() => undefined, fetchSince);
    onEvents([at(1)]);
    onEvents([at(3)]); // gap after 1 → fetch (fails)
    onEvents([at(6)]); // gap after 3 → queued
    await vi.waitFor(() => expect(calls).toEqual([1, 3]));
  });

  it("fills nothing on the first connection", () => {
    const fetchSince = vi.fn(async () => []);
    withGapFilling(() => undefined, fetchSince)([event(spanId(1), started("a"), { seq: 900 })], { first: true });
    expect(fetchSince).not.toHaveBeenCalled();
  });
});
