import { useState } from "react";
import { api } from "../../api/client";
import { appStore, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { showError } from "../../state/toasts";
import { useSpan } from "../../live/live";
import { Icon } from "../../ui/Icon";
import { KeyValueEditor } from "../../ui/KeyValueEditor";
import { TemplateInput } from "../../ui/TemplateInput";
import { MessageList } from "../activity/MessageList";
import { RunningTime } from "../activity/ActivityRow";
import { wsTabs, WS_TAB_INITIAL as initial, type WsTabState } from "./wsTabs";
import "./websocket.css";

/** A WebSocket client: connect, send messages, and watch traffic live. */
export function WebSocketTab({ tabKey }: { tabKey: string }) {
  const state = useStore(wsTabs, (s) => s[tabKey] ?? initial);
  const set = (patch: Partial<WsTabState>) => wsTabs.set((s) => ({ [tabKey]: { ...(s[tabKey] ?? initial), ...patch } }));
  const span = useSpan(state.sessionId);
  const [busy, setBusy] = useState(false);
  const connected = Boolean(state.sessionId && span && span.outcome === null);

  const connect = async () => {
    setBusy(true);
    try {
      const { sessionId } = await api.wsConnect({
        url: state.url,
        headers: state.headers.filter((h) => h.enabled !== false && h.key).map((h) => [h.key, h.value] as [string, string]),
        environment: appStore.get().environment,
      });
      set({ sessionId });
    } catch (e) {
      showError("Could not connect", e);
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    if (!state.sessionId) return;
    try {
      await api.wsClose(state.sessionId);
    } catch (e) {
      showError("Could not disconnect", e);
    }
  };

  const send = async () => {
    if (!state.sessionId || !state.message) return;
    try {
      await api.wsSend(state.sessionId, state.message);
      set({ message: "" });
    } catch (e) {
      showError("Could not send the message", e);
    }
  };

  return (
    <div className="ws-tab">
      <div className="editor-header">
        <div className="url-row">
          <span className="method-input ws-badge" aria-hidden="true">
            WS
          </span>
          <TemplateInput value={state.url} onChange={(url) => set({ url })} onSubmit={connected ? undefined : connect} ariaLabel="WebSocket URL" placeholder="wss://example.com/socket" />
          {connected ? (
            <button type="button" className="btn btn-danger send-btn" onClick={disconnect}>
              Disconnect
            </button>
          ) : (
            <button type="button" className="btn btn-primary send-btn" onClick={connect} disabled={busy || !state.url}>
              {busy ? "Connecting…" : "Connect"}
            </button>
          )}
        </div>
        <details>
          <summary className="hint">Headers ({state.headers.length})</summary>
          <KeyValueEditor label="Headers" items={state.headers} onChange={(headers) => set({ headers })} />
        </details>
      </div>

      <div className="ws-status">
        {!state.sessionId && <span className="muted">Not connected. Messages appear here live once you connect.</span>}
        {state.sessionId && span && (
          <>
            <span className={`live-dot`} data-state={connected ? "open" : "closed"} />
            <span>{connected ? "Connected" : span.outcome === "error" ? "Connection failed" : "Closed"}</span>
            <span className="mono faint">{connected ? <RunningTime startedAt={span.startedAt} /> : null}</span>
            {span.error && <span className="tone-danger">{span.error}</span>}
            <span className="spacer" />
            <span className="faint">{span.messages.length} messages</span>
            <button type="button" className="btn btn-sm btn-ghost" onClick={() => selectSpan(span.id)}>
              Details
            </button>
          </>
        )}
      </div>

      <div className="ws-messages">
        <MessageList messages={span?.messages ?? []} emptyText={connected ? "Connected. Send a message below." : "No messages."} />
      </div>

      <form
        className="ws-composer"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <textarea
          className="textarea grow"
          rows={3}
          placeholder={connected ? "Message text (JSON or anything). Cmd/Ctrl+Enter to send." : "Connect first to send messages."}
          value={state.message}
          disabled={!connected}
          onChange={(e) => set({ message: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void send();
            }
          }}
          aria-label="Message to send"
        />
        <button type="submit" className="btn btn-primary" disabled={!connected || !state.message}>
          <Icon name="send" /> Send
        </button>
      </form>
    </div>
  );
}
