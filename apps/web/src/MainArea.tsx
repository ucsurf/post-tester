import { lazy, Suspense, useRef, type KeyboardEvent, type MouseEvent } from "react";
import { forgetWsTab, useWsTabs, wsTabTitle } from "./features/websocket/wsTabs";
import { modKey } from "./lib/platform";
import { activateTab, appStore, closeTab, isDirty, newRequestTab, openDialog, openPanel, tabTitle, type AppState, type PanelKind, type Tab } from "./state/app";
import { shallowEqualArrays, useStore } from "./state/store";
import { Icon, IconButton, type IconName } from "./ui/Icon";
import { MethodBadge, Spinner } from "./ui/widgets";

// Everything behind a tab loads on demand; the first screen needs none of it.
const RequestEditor = lazy(() => import("./features/request-editor/RequestEditor").then((m) => ({ default: m.RequestEditor })));
const WebSocketTab = lazy(() => import("./features/websocket/WebSocketTab").then((m) => ({ default: m.WebSocketTab })));
const EnvironmentsPanel = lazy(() => import("./features/environments/EnvironmentsPanel").then((m) => ({ default: m.EnvironmentsPanel })));
const PluginsPanel = lazy(() => import("./features/plugins/PluginsPanel").then((m) => ({ default: m.PluginsPanel })));
const McpClientPanel = lazy(() => import("./features/mcp/McpClientPanel").then((m) => ({ default: m.McpClientPanel })));
const ConnectAgentPanel = lazy(() => import("./features/mcp/ConnectAgentPanel").then((m) => ({ default: m.ConnectAgentPanel })));

/** The tab bar and the content of the active tab. */
export function MainArea() {
  const tabs = useStore(appStore, (s) => s.tabs);
  const active = useStore(appStore, (s) => s.activeTab);
  const tab = tabs.find((t) => t.key === active) ?? null;

  return (
    <>
      <TabBar tabs={tabs} active={active} />
      <div className="tab-content">
        {tab ? (
          <Suspense fallback={<TabLoading />}>
            <TabContent tab={tab} />
          </Suspense>
        ) : (
          <Welcome />
        )}
      </div>
    </>
  );
}

function TabLoading() {
  return (
    <div className="empty">
      <Spinner label="Loading…" />
    </div>
  );
}

function TabContent({ tab }: { tab: Tab }) {
  switch (tab.kind) {
    case "request":
      return <RequestEditor key={tab.key} docKey={tab.key} />;
    case "websocket":
      return <WebSocketTab key={tab.key} tabKey={tab.key} />;
    case "environments":
      return <EnvironmentsPanel />;
    case "plugins":
      return <PluginsPanel />;
    case "mcp":
      return <McpClientPanel />;
    case "agent":
      return <ConnectAgentPanel />;
  }
}

const PANEL_TITLES: Record<PanelKind, { title: string; icon: IconName }> = {
  environments: { title: "Environments", icon: "layers" },
  plugins: { title: "Plugins", icon: "plug" },
  mcp: { title: "MCP client", icon: "terminal" },
  agent: { title: "Connect AI agent", icon: "bot" },
};

type TabLabel = { key: string; title: string; method: string | null; icon: IconName | null; dirty: boolean; sending: boolean };

function labelsOf(state: AppState): TabLabel[] {
  return state.tabs.map((tab) => {
    if (tab.kind === "request") {
      const doc = state.requests[tab.key];
      return {
        key: tab.key,
        title: doc ? tabTitle(doc) : "Request",
        method: doc?.draft.method ?? null,
        icon: null,
        dirty: doc ? isDirty(doc) : false,
        sending: doc?.send.phase === "sending",
      };
    }
    if (tab.kind === "websocket") return { key: tab.key, title: "WebSocket", method: "WS", icon: null, dirty: false, sending: false };
    const meta = PANEL_TITLES[tab.kind];
    return { key: tab.key, title: meta.title, method: null, icon: meta.icon, dirty: false, sending: false };
  });
}

const labelsEqual = (a: TabLabel[], b: TabLabel[]) =>
  a.length === b.length && a.every((x, i) => {
    const y = b[i];
    return y !== undefined && x.key === y.key && x.title === y.title && x.method === y.method && x.dirty === y.dirty && x.sending === y.sending;
  });

function TabBar({ tabs, active }: { tabs: readonly Tab[]; active: string | null }) {
  const labels = useStore(appStore, labelsOf, labelsEqual);
  const keys = useStore(appStore, (s) => s.tabs.map((t) => t.key), shallowEqualArrays);
  const list = useRef<HTMLDivElement>(null);
  useWsTabs(); // WebSocket tab titles follow their URL.

  const close = async (key: string) => {
    const tab = tabs.find((t) => t.key === key);
    await closeTab(key);
    if (tab?.kind === "websocket" && !appStore.get().tabs.some((t) => t.key === key)) forgetWsTab(key);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const index = keys.indexOf(active ?? "");
    let next: string | undefined;
    if (e.key === "ArrowRight") next = keys[(index + 1) % keys.length];
    else if (e.key === "ArrowLeft") next = keys[(index - 1 + keys.length) % keys.length];
    if (!next) return;
    e.preventDefault();
    activateTab(next);
    list.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(next)}"]`)?.focus();
  };

  return (
    <div className="tabbar" role="tablist" aria-label="Open tabs" ref={list} onKeyDown={onKeyDown}>
      {labels.map((tabLabel) => {
        const label = tabLabel.method === "WS" ? { ...tabLabel, title: wsTabTitle(tabLabel.key) } : tabLabel;
        const selected = label.key === active;
        return (
          <div
            key={label.key}
            className="doc-tab"
            role="tab"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            data-key={label.key}
            title={label.title}
            onClick={() => activateTab(label.key)}
            onAuxClick={(e: MouseEvent) => e.button === 1 && void close(label.key)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") activateTab(label.key);
              if (e.key === "Delete") void close(label.key);
            }}
          >
            {label.sending ? <span className="spinner" /> : label.method ? <MethodBadge method={label.method} /> : label.icon ? <Icon name={label.icon} size={12} /> : null}
            <span className="truncate">{label.title}</span>
            {label.dirty && <span className="dirty-dot" aria-label="Unsaved changes" />}
            <IconButton
              icon="x"
              small
              label={`Close ${label.title}`}
              className="doc-tab-close"
              tabIndex={-1}
              onClick={(e) => {
                e.stopPropagation();
                void close(label.key);
              }}
            />
          </div>
        );
      })}
      <IconButton icon="plus" label="New request" onClick={() => newRequestTab()} />
    </div>
  );
}

function Welcome() {
  const mod = modKey();
  return (
    <div className="welcome">
      <div className="welcome-card stack">
        <h1>Build, send and watch your APIs live</h1>
        <p className="muted">Open a request from the sidebar, or start something new. Everything that happens shows up in Live Activity as it happens.</p>
        <div className="welcome-actions">
          <button type="button" className="btn btn-primary" onClick={() => newRequestTab()}>
            <Icon name="plus" /> New request
          </button>
          <button type="button" className="btn" onClick={() => openDialog({ type: "import" })}>
            <Icon name="import" /> Import cURL / Postman / OpenAPI
          </button>
          <button type="button" className="btn" onClick={() => openPanel("agent")}>
            <Icon name="bot" /> Connect an AI agent
          </button>
          <button type="button" className="btn" onClick={() => openPanel("mcp")}>
            <Icon name="terminal" /> Test an MCP server
          </button>
        </div>
        <dl className="shortcuts">
          <dt>
            <span className="kbd">{mod}</span> <span className="kbd">Enter</span>
          </dt>
          <dd>Send the request</dd>
          <dt>
            <span className="kbd">{mod}</span> <span className="kbd">S</span>
          </dt>
          <dd>Save the request</dd>
          <dt>
            <span className="kbd">{"{{"}</span>
          </dt>
          <dd>Insert a variable or template function</dd>
        </dl>
      </div>
    </div>
  );
}
