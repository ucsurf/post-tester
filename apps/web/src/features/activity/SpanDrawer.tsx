import { useEffect, useMemo, useState } from "react";
import { api } from "../../api/client";
import { formatClock, formatDuration, formatSize, plural } from "../../lib/format";
import { sourceLabel } from "../../lib/source";
import { useLiveState, useSpan } from "../../live/live";
import { childCounts, spanDisplayTitle, spanStatus } from "../../live/selectors";
import type { SpanState } from "../../live/spans";
import { appStore, openRequest, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { showError, showToast } from "../../state/toasts";
import { Icon, IconButton } from "../../ui/Icon";
import { TabPanel, Tabs, type TabItem } from "../../ui/Tabs";
import { HeaderTable, MethodBadge, OutcomeIcon, outcomeLabel, SourceBadge, StatusBadge } from "../../ui/widgets";
import { PayloadView } from "../response/PayloadView";
import { RunningTime } from "./ActivityRow";
import { MessageList } from "./MessageList";
import { TestList } from "./TestList";
import { Waterfall } from "./Waterfall";
import "./activity.css";

type DrawerTab = "overview" | "timeline" | "request" | "response" | "messages" | "logs" | "tests" | "variables" | "raw";

/** Details of the selected span, updating live while it runs. */
export function SpanDrawer() {
  const selected = useStore(appStore, (s) => s.selectedSpan);
  if (!selected) return null;
  return <Drawer key={selected} id={selected} />;
}

function Drawer({ id }: { id: string }) {
  const span = useSpan(id);
  const [tab, setTab] = useState<DrawerTab | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector(".modal-backdrop")) selectSpan(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!span) {
    return (
      <aside className="drawer" aria-label="Operation details">
        <div className="drawer-header">
          <div className="drawer-title">
            <h2>Operation not found</h2>
            <IconButton icon="x" label="Close details" onClick={() => selectSpan(null)} />
          </div>
        </div>
        <p className="pad muted">It may have been cleared from Live Activity or dropped to save memory.</p>
      </aside>
    );
  }

  const tabs = drawerTabs(span);
  const current = tab && tabs.some((t) => t.id === tab && !t.hidden) ? tab : (tabs.find((t) => !t.hidden)?.id ?? "raw");

  return (
    <aside className="drawer" aria-label="Operation details">
      <DrawerHeader span={span} />
      <Tabs tabs={tabs} active={current} onChange={setTab} label="Details" idPrefix="drawer" />
      <TabPanel idPrefix="drawer" active={current} className="drawer-body">
        <DrawerContent span={span} tab={current} />
      </TabPanel>
    </aside>
  );
}

function drawerTabs(span: SpanState): TabItem<DrawerTab>[] {
  const failedTests = span.tests.filter((t) => !t.passed).length;
  const isHttp = span.kind === "request" && span.summary.protocol !== "load";
  return [
    { id: "overview", label: span.kind === "collection_run" ? "Requests" : "Overview", hidden: span.children.length === 0 && span.kind !== "collection_run" && span.summary.protocol !== "load" },
    { id: "timeline", label: "Timeline", hidden: !isHttp && Object.keys(span.phases).length === 0 },
    { id: "request", label: "Request", hidden: !span.request && !isHttp },
    { id: "response", label: "Response", hidden: !span.response && !isHttp },
    { id: "messages", label: "Messages", count: span.messages.length, hidden: span.messages.length === 0 && span.kind !== "web_socket" && span.kind !== "mcp_call" },
    { id: "logs", label: "Logs", count: span.logs.length },
    { id: "tests", label: "Tests", count: span.tests.length ? `${span.tests.length - failedTests}/${span.tests.length}` : 0, tone: failedTests ? "bad" : "good", hidden: span.tests.length === 0 && !isHttp },
    { id: "variables", label: "Variables", count: span.variables.length, hidden: span.variables.length === 0 },
    { id: "raw", label: "Raw", count: span.eventCount },
  ];
}

function DrawerHeader({ span }: { span: SpanState }) {
  const status = spanStatus(span);
  const running = status === "running";
  const method = span.summary.method ?? span.request?.method;

  const cancel = async () => {
    try {
      const result = await api.cancel(span.id);
      if (!result.cancelled) showToast("info", "Already finished", "The operation ended before it could be cancelled.");
    } catch (e) {
      showError("Could not cancel", e);
    }
  };

  return (
    <header className="drawer-header">
      <div className="drawer-title">
        <OutcomeIcon status={status} />
        {method && <MethodBadge method={method} />}
        <h2 className="truncate" title={spanDisplayTitle(span)}>
          {spanDisplayTitle(span)}
        </h2>
        {running && (
          <button type="button" className="btn btn-sm btn-danger" onClick={cancel}>
            <Icon name="stop" size={12} />
            Cancel
          </button>
        )}
        {span.summary.request_id && (
          <button type="button" className="btn btn-sm" onClick={() => openRequest(span.summary.request_id as string)} title="Open the saved request in the editor">
            Open request
          </button>
        )}
        <IconButton icon="x" label="Close details" onClick={() => selectSpan(null)} />
      </div>
      <div className="drawer-meta">
        <SourceBadge source={span.source} />
        <span title={sourceLabel(span.source)}>{outcomeLabel(status)}</span>
        {span.status !== null && <StatusBadge status={span.status} text={span.response?.statusText} />}
        <span className="mono">{running ? <RunningTime startedAt={span.startedAt} /> : formatDuration(span.durationMs)}</span>
        {span.size !== null && <span className="mono">{formatSize(span.size)}</span>}
        {span.startedAt > 0 && <span className="mono faint">{formatClock(span.startedAt)}</span>}
      </div>
      {span.summary.url && <div className="mono truncate faint">{span.summary.url}</div>}
      {span.error && <div className="error-box">{span.error}</div>}
    </header>
  );
}

function DrawerContent({ span, tab }: { span: SpanState; tab: DrawerTab }) {
  const running = span.outcome === null;
  switch (tab) {
    case "overview":
      return <ChildrenView span={span} />;
    case "timeline":
      return (
        <div className="drawer-section">
          <Waterfall
            phases={span.phases}
            totalMs={span.durationMs}
            running={running}
            connection={span.connection}
            httpVersion={span.response?.httpVersion ?? span.request?.httpVersion ?? null}
          />
        </div>
      );
    case "request":
      return span.request ? (
        <>
          <div className="drawer-section">
            <div className="row mono">
              <MethodBadge method={span.request.method} />
              <span style={{ wordBreak: "break-all" }}>{span.request.url}</span>
              <span className="faint">{span.request.httpVersion}</span>
            </div>
          </div>
          <div className="drawer-section">
            <h3 className="section-title">Headers</h3>
            <HeaderTable headers={span.request.headers.map((h) => [h.name, h.value] as const)} />
          </div>
          <div className="drawer-section">
            <h3 className="section-title">Body</h3>
            {span.request.body.size === 0 ? <p className="muted">No body.</p> : <PayloadView payload={span.request.body} />}
          </div>
        </>
      ) : (
        <p className="pad muted">{running ? "Preparing the request (variables, auth, scripts)…" : "The request was not sent."}</p>
      );
    case "response":
      return span.response ? (
        <>
          <div className="drawer-section">
            <div className="row">
              <StatusBadge status={span.response.status} text={span.response.statusText} />
              <span className="mono faint">{span.response.httpVersion}</span>
            </div>
          </div>
          <div className="drawer-section">
            <h3 className="section-title">Headers</h3>
            <HeaderTable headers={span.response.headers.map((h) => [h.name, h.value] as const)} />
          </div>
          <div className="drawer-section" style={{ padding: 0 }}>
            <PayloadView
              payload={span.response.body}
              viewer={{
                status: span.response.status,
                statusText: span.response.statusText,
                headers: span.response.headers.map((h) => [h.name, h.value]),
                timeMs: span.durationMs ?? 0,
              }}
            />
          </div>
        </>
      ) : (
        <p className="pad muted">{running ? "Waiting for the response…" : "No response was received."}</p>
      );
    case "messages":
      return <MessageList messages={span.messages} emptyText={running ? "Connected. Messages appear here as they are sent and received." : "No messages."} />;
    case "logs":
      return span.logs.length === 0 ? (
        <p className="pad muted">No log lines. Scripts can write here with console.log().</p>
      ) : (
        <ol className="logs">
          {span.logs.map((log) => (
            <li key={log.seq}>
              <span className="faint">{formatClock(log.at)}</span>
              <span className={`level level-${log.level}`}>{log.level}</span>
              <span className="text">{log.text}</span>
            </li>
          ))}
        </ol>
      );
    case "tests":
      return <TestList tests={span.tests} emptyText={running ? "Tests run after the response arrives." : "No tests. Add expressions such as status == 200 in the request's Tests tab."} />;
    case "variables":
      return (
        <table className="table">
          <thead>
            <tr>
              <th>Variable</th>
              <th>Value</th>
            </tr>
          </thead>
          <tbody>
            {span.variables.map((v) => (
              <tr key={v.seq}>
                <td className="mono">{v.name}</td>
                <td className="mono">{v.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      );
    case "raw":
      return <RawEvents span={span} />;
  }
}

function RawEvents({ span }: { span: SpanState }) {
  const text = useMemo(() => JSON.stringify(span.events, null, 2), [span.events]);
  return (
    <div className="drawer-section">
      {span.eventCount > span.events.length && <p className="hint">Showing the last {span.events.length} of {span.eventCount} events.</p>}
      <pre className="code-block">{text}</pre>
    </div>
  );
}

function ChildrenView({ span }: { span: SpanState }) {
  const state = useLiveState();
  const counts = childCounts(state, span);
  const logs = span.logs.slice(-5);
  return (
    <>
      <div className="drawer-section">
        <p className="muted">
          {plural(span.children.length, "operation")}: {counts.ok} ok · {counts.failed} failed · {counts.error} errors · {counts.running} running
          {span.progress ? ` · progress ${span.progress.done}/${span.progress.total}` : ""}
        </p>
      </div>
      {span.children.length > 0 && (
        <div className="drawer-section">
          <div className="child-list">
            {span.children.map((id) => {
              const child = state.spans.get(id);
              if (!child) return null;
              return (
                <button key={id} type="button" onClick={() => selectSpan(id)}>
                  <OutcomeIcon status={spanStatus(child)} />
                  <MethodBadge method={child.summary.method ?? child.request?.method} />
                  <span className="truncate grow mono">{spanDisplayTitle(child)}</span>
                  {child.status !== null && <StatusBadge status={child.status} />}
                  <span className="mono faint">{formatDuration(child.durationMs)}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
      {logs.length > 0 && (
        <div className="drawer-section">
          <h3 className="section-title">Latest log lines</h3>
          <ol className="logs">
            {logs.map((log) => (
              <li key={log.seq}>
                <span className={`level level-${log.level}`}>{log.level}</span>
                <span className="text">{log.text}</span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </>
  );
}
