import type { Auth, FieldInfo } from "@post-tester/bindings";
import { appStore } from "../../state/app";
import { useStore } from "../../state/store";
import { authChoice, authForChoice, type AuthChoice } from "./model";

type Props = {
  auth: Auth | undefined;
  /** `undefined` means "inherit" (the field is left out of the file). */
  onChange: (auth: Auth | undefined) => void;
  inheritLabel: string;
  inheritHint: string;
};

/** Picks and edits an auth method: built-in ones and those provided by plugins. */
export function AuthEditor({ auth, onChange: setAuth, inheritLabel, inheritHint }: Props) {
  const plugins = useStore(appStore, (s) => s.plugins);
  const pluginMethods = plugins.flatMap((p) => p.capabilities.auth.map((a) => ({ id: `${p.name}/${a.name}`, label: a.displayName, plugin: p.displayName, fields: a.fields })));
  const choice = authChoice(auth);
  const pluginMethod = auth?.type === "plugin" ? pluginMethods.find((m) => m.id === auth.name) : undefined;

  return (
    <>
      <label className="field" style={{ maxWidth: 360 }}>
        <span>Type</span>
        <select className="select" value={choice} onChange={(e) => setAuth(authForChoice(e.target.value as AuthChoice, auth))}>
          <option value="inherit">{inheritLabel}</option>
          <option value="none">No auth</option>
          <option value="basic">Basic</option>
          <option value="bearer">Bearer token</option>
          <option value="api_key">API key</option>
          {pluginMethods.length > 0 && (
            <optgroup label="From plugins">
              {pluginMethods.map((m) => (
                <option key={m.id} value={`plugin:${m.id}`}>
                  {m.label} ({m.plugin})
                </option>
              ))}
            </optgroup>
          )}
          {auth?.type === "plugin" && !pluginMethod && <option value={choice}>{auth.name} (plugin not installed)</option>}
        </select>
      </label>
      {!auth && <p className="hint">{inheritHint}</p>}
      {auth?.type === "none" && <p className="hint">No credentials are sent, even if the folder defines auth.</p>}
      {auth?.type === "basic" && (
        <div className="form-grid">
          <TextField label="Username" value={auth.username} onChange={(username) => setAuth({ ...auth, username })} />
          <TextField label="Password" value={auth.password} secret onChange={(password) => setAuth({ ...auth, password })} />
        </div>
      )}
      {auth?.type === "bearer" && (
        <div className="form-grid">
          <TextField label="Token" value={auth.token} secret onChange={(token) => setAuth({ ...auth, token })} placeholder="{{token}} or {{secret:apiToken}}" />
        </div>
      )}
      {auth?.type === "api_key" && (
        <div className="form-grid">
          <TextField label="Name" value={auth.key} onChange={(key) => setAuth({ ...auth, key })} />
          <TextField label="Value" value={auth.value} secret onChange={(value) => setAuth({ ...auth, value })} />
          <label className="field">
            <span>Add to</span>
            <select className="select" value={auth.location} onChange={(e) => setAuth({ ...auth, location: e.target.value === "query" ? "query" : "header" })}>
              <option value="header">Header</option>
              <option value="query">Query parameter</option>
            </select>
          </label>
        </div>
      )}
      {auth?.type === "plugin" && (
        <div className="form-grid">
          {pluginMethod ? (
            pluginMethod.fields.map((field) => (
              <PluginField key={field.name} field={field} value={auth.config[field.name]} onChange={(v) => setAuth({ ...auth, config: { ...auth.config, [field.name]: v } })} />
            ))
          ) : (
            <p className="callout warning">The plugin providing “{auth.name}” is not installed or failed to load. See the Plugins panel.</p>
          )}
        </div>
      )}
      {auth && auth.type !== "none" && <p className="hint">Tip: keep credentials out of files with {"{{secret:name}}"} (stored in your OS keychain, see Environments → Secrets).</p>}
    </>
  );
}

function TextField({ label, value, onChange, secret, placeholder }: { label: string; value: string; onChange: (v: string) => void; secret?: boolean; placeholder?: string }) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        className="input input-mono"
        type={secret && !value.includes("{{") ? "password" : "text"}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
      />
    </label>
  );
}

function PluginField({ field, value, onChange }: { field: FieldInfo; value: unknown; onChange: (v: unknown) => void }) {
  const type = field.type.toLowerCase();
  if (type === "boolean") {
    return (
      <label className="checkbox">
        <input type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} />
        {field.label}
      </label>
    );
  }
  if (type === "number") {
    return (
      <label className="field">
        <span>{field.label}</span>
        <input className="input" type="number" value={typeof value === "number" ? value : ""} onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))} />
      </label>
    );
  }
  return <TextField label={field.label} value={typeof value === "string" ? value : value === undefined || value === null ? "" : String(value)} secret={type === "secret" || type === "password"} onChange={onChange} />;
}
