import type { PluginInfo } from "@post-tester/bindings";
import { useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import { appStore } from "../../state/app";
import { useStore } from "../../state/store";
import { showError, showToast } from "../../state/toasts";
import { Icon } from "../../ui/Icon";
import { EmptyState } from "../../ui/widgets";
import { describePermission } from "./permissions";
import "./plugins.css";

/** Installed plugins, what they provide, and permissions waiting for approval. */
export function PluginsPanel() {
  const plugins = useStore(appStore, (s) => s.plugins);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    setBusy(true);
    setError(null);
    try {
      const list = await api.reloadPlugins();
      appStore.set({ plugins: list });
      showToast("success", "Plugins reloaded");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel-page">
      <header className="panel-header">
        <div className="row">
          <h1 className="grow">Plugins</h1>
          <button type="button" className="btn btn-sm" onClick={reload} disabled={busy}>
            <Icon name="refresh" size={12} /> {busy ? "Reloading…" : "Reload"}
          </button>
        </div>
        <p className="muted">
          Plugins add auth methods, template functions, importers, exporters, code generators, response viewers and MCP tools. They run sandboxed and can only use the
          permissions you approve. To build one, see <code>guide/plugins/README.md</code>.
        </p>
      </header>
      {error && <p className="error-box">{error}</p>}
      {plugins.length === 0 ? (
        <EmptyState icon="🔌" title="No plugins installed">
          <p>
            Put a plugin folder (with <code>plugin.json</code>) in your workspace's plugin directory, list it in <code>post-tester.yaml</code>, then press Reload. See{" "}
            <code>guide/plugins/README.md</code>.
          </p>
        </EmptyState>
      ) : (
        <div className="plugin-list">
          {plugins.map((p) => (
            <PluginCard key={p.name} plugin={p} />
          ))}
        </div>
      )}
    </div>
  );
}

function PluginCard({ plugin }: { plugin: PluginInfo }) {
  const [approving, setApproving] = useState(false);
  const c = plugin.capabilities;
  const provided: [string, string[]][] = (
    [
      ["Auth", c.auth.map((a) => a.displayName)],
      ["Template functions", c.templateFunctions.map((f) => `${f.name}()`)],
      ["Importers", c.importers.map((i) => i.displayName)],
      ["Exporters", c.exporters.map((e) => e.displayName)],
      ["Code generators", c.codegen.map((g) => g.displayName)],
      ["Viewers", c.viewers.map((v) => `${v.displayName} (${v.contentTypes.join(", ")})`)],
      ["MCP tools", c.mcpTools.map((t) => t.name)],
      ["Hooks", c.hooks],
    ] as [string, string[]][]
  ).filter(([, items]) => items.length > 0);

  const approve = async () => {
    setApproving(true);
    try {
      const updated = await api.approvePlugin(plugin.name);
      appStore.set((s) => ({ plugins: s.plugins.map((p) => (p.name === updated.name ? updated : p)) }));
      showToast("success", `Approved ${plugin.displayName}`);
    } catch (e) {
      showError("Could not approve the plugin", e);
    } finally {
      setApproving(false);
    }
  };

  return (
    <article className="card plugin-card">
      <header className="row">
        <h2 className="grow">
          {plugin.displayName} <span className="faint mono">v{plugin.version}</span>
        </h2>
        {plugin.pendingPermissions.length === 0 ? <span className="badge tone-success">Active</span> : <span className="badge tone-warning">Needs approval</span>}
      </header>
      <p className="muted">{plugin.description}</p>
      <p className="hint mono truncate" title={plugin.dir}>
        {plugin.dir}
      </p>

      {plugin.pendingPermissions.length > 0 && (
        <div className="callout warning stack">
          <strong>This plugin asks for permission to:</strong>
          <ul className="permission-list">
            {plugin.pendingPermissions.map((p) => {
              const d = describePermission(p);
              return (
                <li key={p} data-risk={d.risk}>
                  {d.text} <code className="faint">{p}</code>
                </li>
              );
            })}
          </ul>
          <p className="hint">Until you approve, the plugin's features that need these permissions are disabled. Only approve plugins you trust.</p>
          <div>
            <button type="button" className="btn btn-primary btn-sm" onClick={approve} disabled={approving}>
              {approving ? "Approving…" : "Approve all"}
            </button>
          </div>
        </div>
      )}

      {plugin.permissions.length > 0 && plugin.pendingPermissions.length === 0 && (
        <p className="hint">Approved permissions: {plugin.permissions.map((p) => describePermission(p).text).join(" · ")}</p>
      )}

      {provided.length > 0 && (
        <dl className="facts">
          {provided.map(([label, items]) => (
            <div key={label} style={{ display: "contents" }}>
              <dt>{label}</dt>
              <dd>{items.join(", ")}</dd>
            </div>
          ))}
        </dl>
      )}
    </article>
  );
}
