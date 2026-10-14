import { useMemo, useState } from "react";
import { getToken } from "../../api/http";
import { useLiveState } from "../../live/live";
import { appStore } from "../../state/app";
import { useStore } from "../../state/store";
import { CopyButton } from "../../ui/widgets";
import "./mcp.css";

/** How to let an AI agent use post-tester over MCP (stdio or HTTP). */
export function ConnectAgentPanel() {
  const info = useStore(appStore, (s) => s.info);
  const [reveal, setReveal] = useState(false);
  const token = getToken() ?? "<token>";
  const mcpUrl = info?.mcpUrl ?? `${window.location.origin}/mcp`;
  const shown = reveal ? token : "•".repeat(12);

  const stdio = "claude mcp add post-tester -- pt mcp";
  const http = (t: string) => `claude mcp add --transport http post-tester ${mcpUrl} --header "Authorization: Bearer ${t}"`;
  const jsonStdio = JSON.stringify({ mcpServers: { "post-tester": { command: "pt", args: ["mcp"] } } }, null, 2);
  const jsonHttp = (t: string) => JSON.stringify({ mcpServers: { "post-tester": { type: "http", url: mcpUrl, headers: { Authorization: `Bearer ${t}` } } } }, null, 2);

  return (
    <div className="panel-page">
      <header className="panel-header">
        <h1>Connect an AI agent</h1>
        <p className="muted">
          AI agents (Claude Code, Cursor, any MCP client) can browse your collections, send requests and run tests through post-tester. Everything an agent does
          appears in <strong>Live Activity</strong> with the 🤖 icon and the agent's name, the moment it happens. Secrets are never shown to agents.
        </p>
      </header>

      <AgentsSeen />

      <section className="card stack agent-option">
        <h2>Option 1 · stdio (recommended)</h2>
        <p className="muted">
          The agent starts <code>pt mcp</code> itself. Run this in your workspace folder:
        </p>
        <Command text={stdio} />
        <details>
          <summary className="hint">Other MCP clients (JSON config)</summary>
          <Command text={jsonStdio} />
        </details>
      </section>

      <section className="card stack agent-option">
        <div className="row">
          <h2 className="grow">Option 2 · HTTP (this running server)</h2>
          <label className="checkbox hint">
            <input type="checkbox" checked={reveal} onChange={(e) => setReveal(e.target.checked)} />
            Show token
          </label>
        </div>
        <p className="muted">
          Connects to <code>{mcpUrl}</code> while <code>pt serve</code> runs. The token changes every time the server starts, so update the agent after a restart.
        </p>
        <Command text={http(shown)} copy={http(token)} />
        <details>
          <summary className="hint">Other MCP clients (JSON config)</summary>
          <Command text={jsonHttp(shown)} copy={jsonHttp(token)} />
        </details>
        <p className="hint">Treat the token like a password: anyone with it can use this server while it runs.</p>
      </section>
    </div>
  );
}

function Command({ text, copy }: { text: string; copy?: string }) {
  return (
    <div className="command">
      <pre className="code-block">{text}</pre>
      <CopyButton text={copy ?? text} />
    </div>
  );
}

function AgentsSeen() {
  const state = useLiveState();
  const agents = useMemo(() => {
    const counts = new Map<string, number>();
    for (const id of state.roots) {
      const span = state.spans.get(id);
      if (span?.source.type === "agent") counts.set(span.source.client, (counts.get(span.source.client) ?? 0) + 1);
    }
    return [...counts.entries()];
  }, [state]);
  if (agents.length === 0) return <p className="callout">No agent activity yet. Once an agent connects and calls a tool, it shows up here and in Live Activity.</p>;
  return (
    <p className="callout">
      🤖 Agents seen in this session:{" "}
      {agents.map(([name, count], i) => (
        <span key={name}>
          {i > 0 && ", "}
          <strong>{name}</strong> ({count} operation{count === 1 ? "" : "s"})
        </span>
      ))}
    </p>
  );
}
