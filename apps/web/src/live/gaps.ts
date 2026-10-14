import type { Event } from "@post-tester/bindings";

/**
 * Watches event sequence numbers for holes. The server numbers events 1, 2, 3, … per
 * bus; a jump means the socket skipped some (the server dropped them for a slow
 * client, or they happened while we were disconnected and fell outside the history
 * it replays on connect). The caller then fetches `/api/events?since=<seq>`.
 */
export class SeqTracker {
  private last = 0;

  /** Highest sequence number seen so far. */
  get lastSeq(): number {
    return this.last;
  }

  /**
   * Records `events` and returns the seq to fetch from when they skip numbers after
   * what was seen before, otherwise null. `firstOnSocket` marks the history batch a
   * new connection starts with: if its numbers are all lower than before, the server
   * was replaced (numbering restarted) and tracking starts over.
   */
  observe(events: readonly Event[], firstOnSocket = false): number | null {
    if (events.length === 0) return null;
    const seqs = events.map((e) => e.seq).sort((a, b) => a - b);
    if (firstOnSocket && (seqs[seqs.length - 1] as number) < this.last) this.last = 0;
    let since: number | null = null;
    for (const seq of seqs) {
      if (seq <= this.last) continue;
      if (this.last > 0 && seq > this.last + 1 && since === null) since = this.last;
      this.last = seq;
    }
    return since;
  }
}

/**
 * Wraps an event sink so gaps are filled from the server: events are delivered as
 * they come, and when numbers are skipped the missing range is fetched once (one
 * request at a time; a gap found meanwhile is fetched right after). Delivering
 * events twice is harmless: the span fold ignores ones it has already applied.
 */
export function withGapFilling(
  deliver: (events: Event[], info?: { first: boolean }) => void,
  fetchSince: (seq: number) => Promise<Event[]>,
): (events: Event[], info?: { first: boolean }) => void {
  const tracker = new SeqTracker();
  let inFlight = false;
  let queued: number | null = null;

  const fill = (since: number) => {
    if (inFlight) {
      queued = queued === null ? since : Math.min(queued, since);
      return;
    }
    inFlight = true;
    fetchSince(since)
      .then((missed) => {
        tracker.observe(missed);
        if (missed.length > 0) deliver(missed);
      })
      // Best effort: what cannot be fetched (evicted on the server) stays missing.
      .catch(() => undefined)
      .finally(() => {
        inFlight = false;
        const next = queued;
        queued = null;
        if (next !== null) fill(next);
      });
  };

  return (events, info) => {
    deliver(events, info);
    const since = tracker.observe(events, info?.first ?? false);
    if (since !== null) fill(since);
  };
}
