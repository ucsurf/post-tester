import type { Environment } from "@post-tester/bindings";
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import { appStore, loadWorkspace, refreshEnvironments, refreshVariables, selectEnvironment } from "../../state/app";
import { confirmDialog, promptDialog } from "../../state/dialogs";
import { useStore } from "../../state/store";
import { showError, showToast } from "../../state/toasts";
import { Icon, IconButton } from "../../ui/Icon";
import { KeyValueEditor, recordToRows, rowsToRecord } from "../../ui/KeyValueEditor";
import { EmptyState, Spinner } from "../../ui/widgets";
import "./environments.css";

/** Environments (variables per stage), secrets (OS keychain) and session variables. */
export function EnvironmentsPanel() {
  const environments = useStore(appStore, (s) => s.environments);
  const defaultEnv = useStore(appStore, (s) => s.info?.defaultEnvironment ?? null);
  const [selected, setSelected] = useState<string | null>(environments[0] ?? null);

  useEffect(() => {
    if (selected && !environments.includes(selected)) setSelected(environments[0] ?? null);
    if (!selected && environments[0]) setSelected(environments[0]);
  }, [environments, selected]);

  const create = async () => {
    const name = await promptDialog({
      title: "New environment",
      message: "Saved as environments/<name>.yaml in your workspace.",
      label: "Name",
      placeholder: "staging",
      confirmLabel: "Create",
      validate: (v) => (!/^[\w.-]+$/.test(v.trim()) ? "Use letters, numbers, dots, dashes or underscores." : environments.includes(v.trim()) ? "That environment already exists." : null),
    });
    if (!name) return;
    try {
      await api.saveEnvironment(name.trim(), { description: "", variables: {} });
      await refreshEnvironments();
      setSelected(name.trim());
    } catch (e) {
      showError("Could not create the environment", e);
    }
  };

  return (
    <div className="panel-page">
      <header className="panel-header">
        <h1>Environments & secrets</h1>
        <p className="muted">Environments hold variables such as {"{{baseUrl}}"} per stage. Secrets live in your OS keychain and are referenced as {"{{secret:name}}"}.</p>
      </header>
      <div className="env-layout">
        <aside className="env-list" aria-label="Environments">
          <div className="row">
            <h2 className="section-title grow">Environments</h2>
            <IconButton icon="plus" label="New environment" small onClick={create} />
          </div>
          {environments.length === 0 ? (
            <p className="hint">No environments yet. Create one to switch variables between local, staging and production.</p>
          ) : (
            <ul>
              {environments.map((name) => (
                <li key={name}>
                  <button type="button" aria-current={selected === name ? "true" : undefined} onClick={() => setSelected(name)}>
                    <span className="truncate grow">{name}</span>
                    {name === defaultEnv && <span className="badge">default</span>}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>
        <section className="env-main">
          {selected ? <EnvironmentEditor key={selected} name={selected} isDefault={selected === defaultEnv} /> : <EmptyState title="No environment selected" />}
        </section>
      </div>
      <div className="env-bottom">
        <SecretsCard />
        <SessionVariablesCard />
      </div>
    </div>
  );
}

function EnvironmentEditor({ name, isDefault }: { name: string; isDefault: boolean }) {
  const [saved, setSaved] = useState<Environment | null>(null);
  const [draft, setDraft] = useState<Environment | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Ignore a slow response for an environment that is no longer selected.
    let active = true;
    api
      .getEnvironment(name)
      .then((env) => {
        if (!active) return;
        const normalized = { description: env.description ?? "", variables: env.variables ?? {} };
        setSaved(normalized);
        setDraft(normalized);
      })
      .catch((e: unknown) => active && setError(`Could not load the environment “${name}”: ${errorText(e)}`));
    return () => {
      active = false;
    };
  }, [name]);

  if (error) return <p className="error-box">{error}</p>;
  if (!draft || !saved) return <Spinner label="Loading…" />;
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  const save = async () => {
    try {
      await api.saveEnvironment(name, draft);
      setSaved(draft);
      showToast("success", `Saved environment ${name}`);
      void refreshVariables();
    } catch (e) {
      showError("Could not save the environment", e);
    }
  };

  const remove = async () => {
    const ok = await confirmDialog({ title: `Delete ${name}?`, message: `This deletes environments/${name}.yaml.`, confirmLabel: "Delete", danger: true });
    if (!ok) return;
    try {
      await api.deleteEnvironment(name);
      if (appStore.get().environment === name) selectEnvironment(null);
      await refreshEnvironments();
    } catch (e) {
      showError("Could not delete the environment", e);
    }
  };

  const makeDefault = async () => {
    try {
      await api.setDefaultEnvironment(isDefault ? null : name);
      await loadWorkspace();
    } catch (e) {
      showError("Could not change the default environment", e);
    }
  };

  return (
    <div className="stack">
      <div className="row">
        <h2 className="grow">{name}</h2>
        {dirty && <span className="dirty-dot" title="Unsaved changes" />}
        <button type="button" className="btn btn-sm" onClick={makeDefault}>
          {isDefault ? "Unset default" : "Set as default"}
        </button>
        <button type="button" className="btn btn-sm btn-danger" onClick={remove}>
          <Icon name="trash" size={12} /> Delete
        </button>
        <button type="button" className="btn btn-sm btn-primary" onClick={save} disabled={!dirty}>
          Save
        </button>
      </div>
      <label className="field">
        <span>Description</span>
        <input className="input" value={draft.description} onChange={(e) => setDraft({ ...draft, description: e.target.value })} placeholder="What this environment is for" />
      </label>
      <KeyValueEditor
        label={`Variables of ${name}`}
        toggleable={false}
        items={recordToRows(draft.variables)}
        onChange={(rows) => setDraft({ ...draft, variables: rowsToRecord(rows) })}
        keyPlaceholder="baseUrl"
        valuePlaceholder="https://staging.example.com"
      />
      <p className="hint">Never put real secrets here (these files are meant for git). Use {"{{secret:name}}"} as the value instead.</p>
    </div>
  );
}

function SecretsCard() {
  const store = useStore(appStore, (s) => s.info?.secretStore ?? "the OS keychain");
  const [name, setName] = useState("");
  const [value, setValue] = useState("");

  const set = async () => {
    try {
      await api.setSecret(name.trim(), value);
      showToast("success", `Secret ${name.trim()} saved`, `Use it as {{secret:${name.trim()}}}.`);
      setValue("");
    } catch (e) {
      showError("Could not save the secret", e);
    }
  };

  const remove = async () => {
    const ok = await confirmDialog({ title: `Delete secret ${name.trim()}?`, message: `It is removed from ${store}. Requests that use it will fail until you set it again.`, confirmLabel: "Delete", danger: true });
    if (!ok) return;
    try {
      await api.deleteSecret(name.trim());
      showToast("success", `Secret ${name.trim()} deleted`);
    } catch (e) {
      showError("Could not delete the secret", e);
    }
  };

  return (
    <section className="card stack">
      <h2 className="section-title">
        <Icon name="lock" size={12} /> Secrets
      </h2>
      <p className="hint">Stored in {store}. Values are never shown again, never written to files and never sent to AI agents.</p>
      <form
        className="row secret-form"
        onSubmit={(e) => {
          e.preventDefault();
          void set();
        }}
      >
        <input className="input input-mono" placeholder="name" aria-label="Secret name" value={name} onChange={(e) => setName(e.target.value)} />
        <input className="input input-mono grow" type="password" placeholder="value" aria-label="Secret value" value={value} onChange={(e) => setValue(e.target.value)} autoComplete="new-password" />
        <button type="submit" className="btn btn-primary btn-sm" disabled={!name.trim() || !value}>
          Set
        </button>
        <button type="button" className="btn btn-sm btn-danger" disabled={!name.trim()} onClick={remove}>
          Delete
        </button>
      </form>
    </section>
  );
}

function SessionVariablesCard() {
  const variables = useStore(appStore, (s) => s.variables);
  const environment = useStore(appStore, (s) => s.environment);

  const clear = async () => {
    try {
      await api.clearRuntimeVariables();
      await refreshVariables();
      showToast("success", "Session variables cleared");
    } catch (e) {
      showError("Could not clear session variables", e);
    }
  };

  return (
    <section className="card stack">
      <div className="row">
        <h2 className="section-title grow">Resolved variables {environment ? `(${environment} + session)` : "(session)"}</h2>
        <IconButton icon="refresh" label="Refresh" small onClick={() => refreshVariables()} />
        <button type="button" className="btn btn-sm" onClick={clear}>
          Clear session variables
        </button>
      </div>
      <p className="hint">Session variables are set by scripts and Extract rules while you work, and are forgotten when `pt serve` stops.</p>
      {variables.length === 0 ? (
        <p className="muted">No variables.</p>
      ) : (
        <div className="vars-table">
          <table className="table">
            <tbody>
              {variables.map((v) => (
                <tr key={v.name}>
                  <td className="mono">{v.name}</td>
                  <td className="mono">{v.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
