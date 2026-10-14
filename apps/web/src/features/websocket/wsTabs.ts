import type { KeyValue } from "@post-tester/bindings";
import { Store, useStore } from "../../state/store";

/** WebSocket tab state, kept outside the (lazily loaded) tab so it survives switching tabs. */

export type WsTabState = { url: string; headers: KeyValue[]; sessionId: string | null; message: string };

export const wsTabs = new Store<Record<string, WsTabState>>({});
export const WS_TAB_INITIAL: WsTabState = { url: "wss://echo.websocket.org", headers: [], sessionId: null, message: "" };

export function wsTabTitle(tabKey: string): string {
  const url = wsTabs.get()[tabKey]?.url ?? WS_TAB_INITIAL.url;
  return url.replace(/^wss?:\/\//, "") || "WebSocket";
}

export function forgetWsTab(tabKey: string): void {
  wsTabs.set((s) => {
    const next = { ...s };
    delete next[tabKey];
    return next;
  });
}

/** Re-renders when any WebSocket tab changes (tab titles follow their URL). */
export function useWsTabs() {
  return useStore(wsTabs, (s) => s);
}
