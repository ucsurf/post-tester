import type { KeyValue } from "@post-tester/bindings";
import { useEffect, useMemo, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { McpConnectOptions, McpPrompt, McpSessionInfo, McpTool } from "../../api/types";
import { schemaSkeleton } from "../../lib/schema";
import { parseJson, splitArgs } from "../../lib/text";
import { useSpan } from "../../live/live";
import { selectSpan } from "../../state/app";
import { showError } from "../../state/toasts";
import { CodeEditor } from "../../ui/CodeEditor";
import { Icon, IconButton } from "../../ui/Icon";
import { JsonTree } from "../../ui/JsonTree";
import { KeyValueEditor, rowsToRecord } from "../../ui/KeyValueEditor";
import { TabPanel, Tabs } from "../../ui/Tabs";
import { EmptyState, Spinner } from "../../ui/widgets";
import { MessageList } from "../activity/MessageList";
import "./mcp.css";

/** "Test an MCP server": connect over stdio or HTTP, explore and call tools, watch JSON-RPC live. */
export function McpClientPanel() {
  const [sessions, setSessions] = useState<McpSessionInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  useEffect(() => {
    api
      .mcpSessions()
      .then((list) => {
        setSessions(list);
        setSelected((s) => s ?? list[0]?.id ?? null);
      })
      .catch(() => setSessions([]));
  }, []);

  const connect = async (options: McpConnectOptions) => {
    setConnecting(true);
    try {
      const session = await api.mcpConnect(options);
      setSessions((list) => [...list.filter((s) => s.id !== session.id), session]);
      setSelected(session.id);
    } catch (e) {
      showError("Could not connect to the MCP server", e);
    } finally {
      setConnecting(false);
    }
  };

  const replace = (session: McpSessionInfo) => setSessions((list) => list.map((s) => (s.id === session.id ? session : s)));
  const remove = (id: string) => {
    setSessions((list) => list.filter((s) => s.id !== id));
    setSelected((s) => (s === id ? null : s));
  };
  const session = sessions.find((s) => s.id === selected) ?? null;

  return (
    <div className="panel-page mcp-page">
      <header className="panel-header">
        <h1>Test an MCP server</h1>
        <p className="muted">Connect to any MCP server, explore its tools, resources and prompts, and call them. Every JSON-RPC message appears live in Traffic and in Live Activity.</p>
      </header>
      <div className="mcp-layout">
        <aside className="stack">
          {sessions.length === 0 ? (
            <ConnectForm onConnect={connect} busy={connecting} />
          ) : (
            // Once connected, the form folds away so the session gets the room.
            <details className="card mcp-new-connection">
              <summary>New connection</summary>
              <ConnectForm onConnect={connect} busy={connecting} />
            </details>
          )}
          {sessions.length > 0 && (
            <div className="card stack">
              <h2 className="section-title">Sessions</h2>
              <ul className="session-list">
                {sessions.map((s) => (
                  <li key={s.id}>
                    <button type="button" aria-current={s.id === selected ? "true" : undefined} onClick={() => setSelected(s.id)}>
                      <span className="truncate grow">{s.name || s.serverName}</span>
                      <span className="faint mono">{s.transport.type}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </aside>
        <section className="mcp-main">
          {session ? (
            <SessionView key={session.id} session={session} onUpdate={replace} onClosed={remove} />
          ) : (
            <EmptyState icon="🧪" title="No MCP server connected">
              Fill in the form to connect, for example <code>npx -y @modelcontextprotocol/server-everything</code> over stdio.
            </EmptyState>
          )}
        </section>
      </div>
    </div>
  );
}

function ConnectForm({ onConnect, busy }: { onConnect: (o: McpConnectOptions) => void; busy: boolean }) {
  const [type, setType] = useState<"stdio" | "http">("stdio");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("npx");
  const [args, setArgs] = useState("-y @modelcontextprotocol/server-everything");
  const [cwd, setCwd] = useState("");
  const [env, setEnv] = useState<KeyValue[]>([]);
  const [url, setUrl] = useState("http://localhost:3001/mcp");
  const [headers, setHeaders] = useState<KeyValue[]>([]);

  const submit = () => {
    const transport: McpConnectOptions["transport"] =
      type === "stdio"
        ? { type: "stdio", command: command.trim(), args: splitArgs(args), ...(env.length ? { env: rowsToRecord(env) } : {}), ...(cwd.trim() ? { cwd: cwd.trim() } : {}) }
        : { type: "http", url: url.trim(), ...(headers.length ? { headers: headers.filter((h) => h.enabled !== false).map((h) => [h.key, h.value] as [string, string]) } : {}) };
    onConnect({ ...(name.trim() ? { name: name.trim() } : {}), transport });
  };

  return (
    <form
      className="card stack"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="row">
        <h2 className="section-title grow">Connect</h2>
        <div className="segmented" role="group" aria-label="Transport">
          <button type="button" aria-pressed={type === "stdio"} onClick={() => setType("stdio")}>
            stdio
          </button>
          <button type="button" aria-pressed={type === "http"} onClick={() => setType("http")}>
            HTTP
          </button>
        </div>
      </div>
      <label className="field">
        <span>Name (optional)</span>
        <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="my-server" />
      </label>
      {type === "stdio" ? (
        <>
          <label className="field">
            <span>Command</span>
            <input className="input input-mono" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="node, npx, uvx, python…" required />
          </label>
          <label className="field">
            <span>Arguments</span>
            <input className="input input-mono" value={args} onChange={(e) => setArgs(e.target.value)} placeholder='server.js --port 3000 "quoted arg"' />
          </label>
          <label className="field">
            <span>Working directory (optional)</span>
            <input className="input input-mono" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="defaults to the workspace" />
          </label>
          <div className="field">
            <span>Environment variables</span>
            <KeyValueEditor label="Environment variables" items={env} onChange={setEnv} toggleable={false} keyPlaceholder="NAME" />
          </div>
        </>
      ) : (
        <>
          <label className="field">
            <span>URL</span>
            <input className="input input-mono" value={url} onChange={(e) => setUrl(e.target.value)} required />
          </label>
          <div className="field">
            <span>Headers</span>
            <KeyValueEditor label="Headers" items={headers} onChange={setHeaders} keyPlaceholder="Authorization" valuePlaceholder="Bearer …" />
          </div>
        </>
      )}
      <button type="submit" className="btn btn-primary" disabled={busy}>
        {busy ? "Connecting…" : "Connect"}
      </button>
    </form>
  );
}

type SessionTab = "tools" | "resources" | "prompts" | "traffic";

function SessionView({ session, onUpdate, onClosed }: { session: McpSessionInfo; onUpdate: (s: McpSessionInfo) => void; onClosed: (id: string) => void }) {
  const [tab, setTab] = useState<SessionTab>("tools");
  const span = useSpan(session.spanId);

  const refresh = async () => {
    try {
      onUpdate(await api.mcpRefresh(session.id));
    } catch (e) {
      showError("Could not refresh", e);
    }
  };
  const disconnect = async () => {
    try {
      await api.mcpDisconnect(session.id);
      onClosed(session.id);
    } catch (e) {
      showError("Could not disconnect", e);
    }
  };

  return (
    <div className="card stack mcp-session">
      <header className="row mcp-session-header">
        <h2 className="grow">
          {session.serverName} <span className="faint mono">v{session.serverVersion}</span>
        </h2>
        <button type="button" className="btn btn-sm btn-ghost" onClick={() => selectSpan(session.spanId)}>
          Details
        </button>
        <IconButton icon="refresh" label="Refresh tools, resources and prompts" onClick={refresh} />
        <button type="button" className="btn btn-sm btn-danger" onClick={disconnect}>
          Disconnect
        </button>
      </header>
      <p className="hint mcp-transport">
        Protocol {session.protocolVersion} · {session.transport.type === "stdio" ? `${session.transport.command} ${session.transport.args.join(" ")}` : session.transport.url}
        {span?.outcome && <span className="tone-danger"> · disconnected{span.error ? `: ${span.error}` : ""}</span>}
      </p>
      {session.instructions && (
        <details className="callout mcp-instructions">
          <summary>Server instructions</summary>
          <p>{session.instructions}</p>
        </details>
      )}
      <Tabs
        label="MCP session"
        idPrefix="mcp"
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "tools", label: "Tools", count: session.tools.length },
          { id: "resources", label: "Resources", count: session.resources.length },
          { id: "prompts", label: "Prompts", count: session.prompts.length },
          { id: "traffic", label: "Traffic", count: span?.messages.length ?? 0 },
        ]}
      />
      <TabPanel idPrefix="mcp" active={tab} className="mcp-tab">
        {tab === "tools" && <ToolsView session={session} />}
        {tab === "resources" && <ResourcesView session={session} />}
        {tab === "prompts" && <PromptsView session={session} />}
        {tab === "traffic" && <MessageList messages={span?.messages ?? []} emptyText="No JSON-RPC messages yet." />}
      </TabPanel>
    </div>
  );
}

function useCall() {
  const [state, setState] = useState<{ busy: boolean; result: unknown; error: string | null }>({ busy: false, result: undefined, error: null });
  const run = async (fn: () => Promise<unknown>) => {
    setState({ busy: true, result: undefined, error: null });
    try {
      setState({ busy: false, result: await fn(), error: null });
    } catch (e) {
      setState({ busy: false, result: undefined, error: errorText(e) });
    }
  };
  return { ...state, run };
}

function ResultView({ busy, result, error }: { busy: boolean; result: unknown; error: string | null }) {
  if (busy) return <Spinner label="Waiting for the server…" />;
  if (error) return <p className="error-box">{error}</p>;
  if (result === undefined) return null;
  const isError = typeof result === "object" && result !== null && (result as { isError?: unknown }).isError === true;
  return (
    <div className={`mcp-result${isError ? " is-error" : ""}`}>
      <h3 className="section-title">{isError ? "Result (the tool reported an error)" : "Result"}</h3>
      <JsonTree value={result} expandDepth={4} />
    </div>
  );
}

function ToolsView({ session }: { session: McpSessionInfo }) {
  const [selected, setSelected] = useState<string | null>(session.tools[0]?.name ?? null);
  const tool = session.tools.find((t) => t.name === selected) ?? null;
  if (session.tools.length === 0) return <p className="muted pad">This server has no tools.</p>;
  return (
    <div className="mcp-split">
      <ItemList items={session.tools.map((t) => ({ id: t.name, label: t.title ?? t.name, hint: t.description }))} selected={selected} onSelect={setSelected} />
      {tool && <ToolCall key={tool.name} sessionId={session.id} tool={tool} />}
    </div>
  );
}

function ToolCall({ sessionId, tool }: { sessionId: string; tool: McpTool }) {
  const skeleton = useMemo(() => JSON.stringify(schemaSkeleton(tool.inputSchema ?? { type: "object" }) ?? {}, null, 2), [tool]);
  const [args, setArgs] = useState(skeleton);
  const call = useCall();
  const parsed = parseJson(args);

  return (
    <div className="stack">
      <h3 className="mono">{tool.name}</h3>
      {tool.description && <p className="muted">{tool.description}</p>}
      <div className="row">
        <span className="label grow">Arguments (JSON, pre-filled from the input schema)</span>
        <button type="button" className="btn btn-sm btn-ghost" onClick={() => setArgs(skeleton)}>
          Reset
        </button>
      </div>
      <CodeEditor value={args} onChange={setArgs} language="json" ariaLabel="Tool arguments" minHeight={120} maxHeight={320} onSubmit={() => parsed.ok && call.run(() => api.mcpCall(sessionId, tool.name, parsed.value))} />
      {!parsed.ok && <p className="hint tone-warning">Not valid JSON: {parsed.error}</p>}
      <div>
        <button type="button" className="btn btn-primary btn-sm" disabled={!parsed.ok || call.busy} onClick={() => parsed.ok && call.run(() => api.mcpCall(sessionId, tool.name, parsed.value))}>
          <Icon name="play" size={12} /> Call tool
        </button>
      </div>
      <ResultView {...call} />
      {tool.inputSchema && (
        <details>
          <summary className="hint">Input schema</summary>
          <JsonTree value={tool.inputSchema} />
        </details>
      )}
    </div>
  );
}

function ResourcesView({ session }: { session: McpSessionInfo }) {
  const [selected, setSelected] = useState<string | null>(session.resources[0]?.uri ?? null);
  const [uri, setUri] = useState(selected ?? "");
  const call = useCall();
  useEffect(() => setUri(selected ?? ""), [selected]);
  return (
    <div className="mcp-split">
      {session.resources.length > 0 ? (
        <ItemList items={session.resources.map((r) => ({ id: r.uri, label: r.title ?? r.name ?? r.uri, hint: r.mimeType ?? r.description }))} selected={selected} onSelect={setSelected} />
      ) : (
        <p className="muted">This server lists no resources. You can still read a URI directly.</p>
      )}
      <div className="stack">
        <label className="field">
          <span>Resource URI</span>
          <input className="input input-mono" value={uri} onChange={(e) => setUri(e.target.value)} />
        </label>
        <div>
          <button type="button" className="btn btn-primary btn-sm" disabled={!uri || call.busy} onClick={() => call.run(() => api.mcpRead(session.id, uri))}>
            Read
          </button>
        </div>
        <ResultView {...call} />
      </div>
    </div>
  );
}

function PromptsView({ session }: { session: McpSessionInfo }) {
  const [selected, setSelected] = useState<string | null>(session.prompts[0]?.name ?? null);
  const prompt = session.prompts.find((p) => p.name === selected) ?? null;
  if (session.prompts.length === 0) return <p className="muted pad">This server has no prompts.</p>;
  return (
    <div className="mcp-split">
      <ItemList items={session.prompts.map((p) => ({ id: p.name, label: p.title ?? p.name, hint: p.description }))} selected={selected} onSelect={setSelected} />
      {prompt && <PromptForm key={prompt.name} sessionId={session.id} prompt={prompt} />}
    </div>
  );
}

function PromptForm({ sessionId, prompt }: { sessionId: string; prompt: McpPrompt }) {
  const [values, setValues] = useState<Record<string, string>>({});
  const call = useCall();
  const missing = (prompt.arguments ?? []).some((a) => a.required && !values[a.name]);
  return (
    <form
      className="stack"
      onSubmit={(e) => {
        e.preventDefault();
        void call.run(() => api.mcpPrompt(sessionId, prompt.name, values));
      }}
    >
      <h3 className="mono">{prompt.name}</h3>
      {prompt.description && <p className="muted">{prompt.description}</p>}
      {(prompt.arguments ?? []).map((arg) => (
        <label key={arg.name} className="field">
          <span>
            {arg.name}
            {arg.required ? " *" : ""}
          </span>
          <input className="input" value={values[arg.name] ?? ""} onChange={(e) => setValues({ ...values, [arg.name]: e.target.value })} placeholder={arg.description} />
        </label>
      ))}
      <div>
        <button type="submit" className="btn btn-primary btn-sm" disabled={missing || call.busy}>
          Get prompt
        </button>
      </div>
      <ResultView {...call} />
    </form>
  );
}

function ItemList({ items, selected, onSelect }: { items: { id: string; label: string; hint?: string | undefined }[]; selected: string | null; onSelect: (id: string) => void }) {
  return (
    <ul className="item-list">
      {items.map((item) => (
        <li key={item.id}>
          <button type="button" aria-current={item.id === selected ? "true" : undefined} onClick={() => onSelect(item.id)} title={item.hint}>
            <span className="mono truncate">{item.label}</span>
            {item.hint && <span className="hint truncate">{item.hint}</span>}
          </button>
        </li>
      ))}
    </ul>
  );
}
