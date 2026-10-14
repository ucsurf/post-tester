import { useCallback, useSyncExternalStore } from "react";
import { api } from "../api/client";
import { liveSocketUrl } from "../api/http";
import { useStore } from "../state/store";
import { LiveConnection } from "./connection";
import { withGapFilling } from "./gaps";
import type { LiveState, SpanState } from "./spans";
import { LiveStore } from "./store";

/** The app-wide live event store and its WebSocket connection, plus React hooks. */

export const liveStore = new LiveStore();

export const liveConnection = new LiveConnection({
  url: liveSocketUrl,
  onEvents: withGapFilling((events, info) => liveStore.push(events, info), api.eventsSince),
});

/** The whole live state; re-renders on every applied batch. Prefer `useLiveSelector`. */
export function useLiveState(): LiveState {
  return useSyncExternalStore(liveStore.subscribe, liveStore.getState, liveStore.getState);
}

/** A slice of the live state; re-renders only when the slice changes (by `equal`). */
export function useLiveSelector<S>(selector: (state: LiveState) => S, equal?: (a: S, b: S) => boolean): S {
  return useStore(liveStore, selector, equal);
}

/** One span, re-rendering only when that span changes. */
export function useSpan(id: string | null | undefined): SpanState | null {
  const get = () => (id ? (liveStore.getState().spans.get(id) ?? null) : null);
  return useSyncExternalStore(liveStore.subscribe, get, get);
}

export function useConnectionStatus() {
  return useSyncExternalStore(liveConnection.subscribe, liveConnection.getStatus, liveConnection.getStatus);
}

// ── Shared clocks ───────────────────────────────────────────────────────────
// Every running timer on screen ticks from one interval per period instead of one
// interval per component (a run can show dozens of running rows).

type Clock = { now: number; listeners: Set<() => void>; timer: ReturnType<typeof setInterval> | null };
const clocks = new Map<number, Clock>();

function clock(intervalMs: number): Clock {
  let c = clocks.get(intervalMs);
  if (!c) {
    c = { now: Date.now(), listeners: new Set(), timer: null };
    clocks.set(intervalMs, c);
  }
  return c;
}

function subscribeClock(intervalMs: number, listener: () => void): () => void {
  const c = clock(intervalMs);
  c.listeners.add(listener);
  if (!c.timer) {
    c.now = Date.now();
    c.timer = setInterval(() => {
      c.now = Date.now();
      c.listeners.forEach((l) => l());
    }, intervalMs);
  }
  return () => {
    c.listeners.delete(listener);
    if (c.listeners.size === 0 && c.timer) {
      clearInterval(c.timer);
      c.timer = null;
    }
  };
}

/** The current time, refreshed every `intervalMs` while `active` (for running timers). */
export function useNow(active: boolean, intervalMs = 100): number {
  const subscribe = useCallback(
    (listener: () => void) => (active ? subscribeClock(intervalMs, listener) : () => undefined),
    [active, intervalMs],
  );
  const get = useCallback(() => (active ? clock(intervalMs).now : 0), [active, intervalMs]);
  const now = useSyncExternalStore(subscribe, get, get);
  return active ? now : Date.now();
}
