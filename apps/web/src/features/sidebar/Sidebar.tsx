import { useState } from "react";
import { appStore, newRequestTab, openDialog, openPanel, openWebSocketTab, selectEnvironment, type PanelKind } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon, IconButton, type IconName } from "../../ui/Icon";
import { Menu } from "../../ui/Menu";
import { ThemeToggle } from "../settings/ThemeToggle";
import { CollectionTree } from "./CollectionTree";
import "./sidebar.css";

/** Workspace, environment, collection tree and links to the other panels. */
export function Sidebar() {
  const info = useStore(appStore, (s) => s.info);
  const environments = useStore(appStore, (s) => s.environments);
  const environment = useStore(appStore, (s) => s.environment);
  const pendingPlugins = useStore(appStore, (s) => s.plugins.filter((p) => p.pendingPermissions.length > 0).length);
  const activeTab = useStore(appStore, (s) => s.activeTab);
  const [search, setSearch] = useState("");

  return (
    <nav className="sidebar" aria-label="Workspace">
      <header className="sidebar-header">
        <img src="./favicon.svg" alt="" width={20} height={20} />
        <div className="grow">
          <div className="workspace-name truncate" title={info?.workspace?.root}>
            {info?.workspace?.name ?? "post-tester"}
          </div>
          <div className="faint version">v{info?.version}</div>
        </div>
        <ThemeToggle />
      </header>

      <div className="sidebar-section">
        <label className="field">
          <span className="label">Environment</span>
          <select className="select" value={environment ?? ""} onChange={(e) => selectEnvironment(e.target.value || null)} aria-label="Environment">
            <option value="">No environment</option>
            {environments.map((name) => (
              <option key={name} value={name}>
                {name}
                {name === info?.defaultEnvironment ? " (default)" : ""}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="sidebar-actions">
        <Menu
          trigger={(props) => (
            <button type="button" className="btn btn-primary btn-sm grow" {...props}>
              <Icon name="plus" size={12} /> New
            </button>
          )}
          items={[
            { id: "http", label: "HTTP request", hint: "REST, GraphQL", onSelect: () => newRequestTab() },
            { id: "ws", label: "WebSocket connection", onSelect: openWebSocketTab },
            { id: "mcp", label: "MCP server test", onSelect: () => openPanel("mcp") },
          ]}
        />
        <button type="button" className="btn btn-sm" onClick={() => openDialog({ type: "import" })} title="Import cURL, Postman, OpenAPI…">
          <Icon name="import" size={12} /> Import
        </button>
        <IconButton icon="play" label="Run all requests" onClick={() => openDialog({ type: "runner", folder: "", requests: [] })} />
      </div>

      <div className="sidebar-search">
        <Icon name="search" size={12} />
        <input type="search" placeholder="Filter requests" aria-label="Filter requests" value={search} onChange={(e) => setSearch(e.target.value)} />
      </div>

      <div className="sidebar-tree">
        <CollectionTree search={search} />
      </div>

      <div className="sidebar-nav">
        <NavItem panel="environments" icon="layers" label="Environments & secrets" active={activeTab === "environments"} />
        <NavItem panel="plugins" icon="plug" label="Plugins" active={activeTab === "plugins"} badge={pendingPlugins || undefined} />
        <NavItem panel="mcp" icon="terminal" label="Test an MCP server" active={activeTab === "mcp"} />
        <NavItem panel="agent" icon="bot" label="Connect an AI agent" active={activeTab === "agent"} />
      </div>
    </nav>
  );
}

function NavItem({ panel, icon, label, active, badge }: { panel: PanelKind; icon: IconName; label: string; active: boolean; badge?: number | undefined }) {
  return (
    <button type="button" className="nav-item" aria-current={active ? "page" : undefined} onClick={() => openPanel(panel)}>
      <Icon name={icon} />
      <span className="grow truncate">{label}</span>
      {badge !== undefined && (
        <span className="badge tone-warning" title="Plugins waiting for your approval">
          {badge}
        </span>
      )}
    </button>
  );
}
