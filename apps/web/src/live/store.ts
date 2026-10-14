import type { Event } from "@post-tester/bindings";
import { applyEvents, DEFAULT_LIVE_OPTIONS, emptyLiveState, type LiveOptions, type LiveState } from "./spans";

export type Scheduler = (flush: () => void) => void;

/**
 * Once per animation frame while the page is visible; a slower timer when it is
 * hidden (animation frames pause there, and events must not pile up unapplied).
 */
export const frameScheduler: Scheduler = (flush) => {
  if (typeof document !== "undefined" && document.visibilityState === "visible" && typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => flush());
  } else {
    setTimeout(flush, 250);
  }
};

/**
 * Holds the live state and applies incoming events in batches, at most once per
 * scheduled flush, so a burst of events causes one re-render.
 */
export class LiveStore {
  private state: LiveState = emptyLiveState();
  private pending: Event[] = [];
  private scheduled = false;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly schedule: Scheduler = frameScheduler,
    private readonly options: LiveOptions = DEFAULT_LIVE_OPTIONS,
  ) {}

  getState = (): LiveState => this.state;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  push(events: readonly Event[]): void {
    if (events.length === 0) return;
    for (const event of events) this.pending.push(event);
    if (this.scheduled) return;
    this.scheduled = true;
    this.schedule(() => this.flush());
  }

  /** Applies everything queued so far. Called by the scheduler (or tests). */
  flush(): void {
    this.scheduled = false;
    if (this.pending.length === 0) return;
    const batch = this.pending;
    this.pending = [];
    const next = applyEvents(this.state, batch, this.options);
    if (next === this.state) return;
    this.state = next;
    this.listeners.forEach((listener) => listener());
  }

  /** Forgets every span (the "Clear" button). New events still arrive. */
  clear(): void {
    this.pending = [];
    this.state = emptyLiveState();
    this.listeners.forEach((listener) => listener());
  }
}
