import { useEffect, useState, useSyncExternalStore } from "react";
import { liveSocketUrl } from "../api/http";
import { LiveConnection } from "./connection";
import type { LiveState, SpanState } from "./spans";
import { LiveStore } from "./store";

/** The app-wide live event store and its WebSocket connection, plus React hooks. */

export const liveStore = new LiveStore();

export const liveConnection = new LiveConnection({
  url: liveSocketUrl,
  onEvents: (events) => liveStore.push(events),
});

export function useLiveState(): LiveState {
  return useSyncExternalStore(liveStore.subscribe, liveStore.getState, liveStore.getState);
}

/** One span, re-rendering only when that span changes. */
export function useSpan(id: string | null | undefined): SpanState | null {
  const get = () => (id ? (liveStore.getState().spans.get(id) ?? null) : null);
  return useSyncExternalStore(liveStore.subscribe, get, get);
}

export function useConnectionStatus() {
  return useSyncExternalStore(liveConnection.subscribe, liveConnection.getStatus, liveConnection.getStatus);
}

/** The current time, refreshed every `intervalMs` while `active` (for running timers). */
export function useNow(active: boolean, intervalMs = 100): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [active, intervalMs]);
  return now;
}
