import type { Auth, FieldInfo, Request } from "@post-tester/bindings";
import { useMemo } from "react";
import { prettyJson, parseJson } from "../../lib/text";
import { appStore } from "../../state/app";
import { useStore } from "../../state/store";
import { CodeEditor } from "../../ui/CodeEditor";
import { KeyValueEditor, recordToRows, rowsToRecord } from "../../ui/KeyValueEditor";
import { HelpPopover } from "../../ui/Menu";
import { authChoice, authForChoice, convertBody, type AuthChoice, type BodyType } from "./model";

/** The tabs of the request editor. Each receives the draft and an `update` function. */

type TabProps = { request: Request; update: (fn: (r: Request) => Request) => void };

export function ParamsTab({ request, update }: TabProps) {
  return (
    <div className="tab-pad">
      <KeyValueEditor
        label="Query parameters"
        items={request.query}
        onChange={(query) => update((r) => ({ ...r, query }))}
        keyPlaceholder="name"
        valuePlaceholder="value or {{variable}}"
      />
      <p className="hint">Parameters stay in sync with the URL. Untick one to keep it without sending it.</p>
    </div>
  );
}

export function HeadersTab({ request, update }: TabProps) {
  return (
    <div className="tab-pad">
      <KeyValueEditor
        label="Headers"
        items={request.headers}
        onChange={(headers) => update((r) => ({ ...r, headers }))}
        keyPlaceholder="Header-Name"
        valuePlaceholder="value or {{variable}}"
      />
      <p className="hint">Folder headers and the Auth tab are added when the request is sent. Use Preview (⋯ menu) to see the final headers.</p>
    </div>
  );
}

const BODY_TYPES: { id: BodyType; label: string }[] = [
  { id: "none", label: "None" },
  { id: "json", label: "JSON" },
  { id: "text", label: "Text" },
  { id: "form", label: "Form" },
  { id: "graphql", label: "GraphQL" },
];

export function BodyTab({ request, update, onSubmit }: TabProps & { onSubmit: () => void }) {
  const body = request.body;
  const setBody = (next: Request["body"]) => update((r) => ({ ...r, body: next }));
  const jsonError = useMemo(() => {
    if (body.type !== "json" || !body.content.trim()) return null;
    // `{{variables}}` make JSON invalid until they are filled in; check with placeholders.
    const parsed = parseJson(body.content.replace(/"?\{\{[^}]*\}\}"?/g, "0"));
    return parsed.ok ? null : parsed.error;
  }, [body]);

  return (
    <div className="tab-fill">
      <div className="tab-toolbar">
        <div className="segmented" role="group" aria-label="Body type">
          {BODY_TYPES.map((t) => (
            <button key={t.id} type="button" aria-pressed={body.type === t.id} onClick={() => setBody(convertBody(body, t.id))}>
              {t.label}
            </button>
          ))}
        </div>
        {body.type === "json" && (
          <>
            <button type="button" className="btn btn-sm btn-ghost" onClick={() => setBody({ type: "json", content: prettyJson(body.content) })}>
              Format
            </button>
            {jsonError && <span className="hint tone-warning truncate" title={jsonError}>Not valid JSON yet: {jsonError}</span>}
          </>
        )}
        {body.type === "text" && (
          <label className="row">
            <span className="label">Content-Type</span>
            <input
              className="input input-mono"
              value={body.content_type}
              onChange={(e) => setBody({ ...body, content_type: e.target.value })}
              list="content-types"
              aria-label="Content type"
            />
            <datalist id="content-types">
              {["text/plain", "application/xml", "text/html", "text/csv", "application/x-ndjson"].map((t) => (
                <option key={t} value={t} />
              ))}
            </datalist>
          </label>
        )}
      </div>
      {body.type === "none" && <p className="muted tab-pad">This request has no body. Pick a type above to add one.</p>}
      {body.type === "json" && (
        <CodeEditor
          className="tab-editor"
          value={body.content}
          onChange={(content) => setBody({ type: "json", content })}
          language="json"
          templates
          ariaLabel="JSON body"
          placeholder={'{\n  "name": "{{userName}}"\n}'}
          onSubmit={onSubmit}
        />
      )}
      {body.type === "text" && (
        <CodeEditor className="tab-editor" value={body.content} onChange={(content) => setBody({ ...body, content })} templates ariaLabel="Text body" onSubmit={onSubmit} />
      )}
      {body.type === "form" && (
        <div className="tab-pad">
          <KeyValueEditor label="Form fields" items={body.fields} onChange={(fields) => setBody({ type: "form", fields })} keyPlaceholder="field" />
          <p className="hint">Sent as application/x-www-form-urlencoded.</p>
        </div>
      )}
      {body.type === "graphql" && (
        <div className="graphql">
          <div className="graphql-pane">
            <span className="label">Query</span>
            <CodeEditor
              className="tab-editor"
              value={body.query}
              onChange={(query) => setBody({ ...body, query })}
              templates
              ariaLabel="GraphQL query"
              placeholder={"query {\n  me { id name }\n}"}
              onSubmit={onSubmit}
            />
          </div>
          <div className="graphql-pane">
            <label className="row">
              <span className="label">Operation</span>
              <input
                className="input input-mono grow"
                value={body.operation_name ?? ""}
                placeholder="optional"
                onChange={(e) => setBody(withOperation(body, e.target.value))}
                aria-label="Operation name"
              />
            </label>
            <span className="label">Variables (JSON)</span>
            <CodeEditor
              className="tab-editor"
              value={body.variables ?? ""}
              onChange={(variables) => setBody({ ...body, variables })}
              language="json"
              templates
              ariaLabel="GraphQL variables"
              placeholder='{ "id": 1 }'
            />
          </div>
        </div>
      )}
    </div>
  );
}

function withOperation(body: Extract<Request["body"], { type: "graphql" }>, name: string): Request["body"] {
  const { operation_name: _drop, ...rest } = body;
  return name ? { ...rest, operation_name: name } : rest;
}

export function AuthTab({ request, update }: TabProps) {
  const plugins = useStore(appStore, (s) => s.plugins);
  const pluginMethods = plugins.flatMap((p) => p.capabilities.auth.map((a) => ({ id: `${p.name}/${a.name}`, label: a.displayName, plugin: p.displayName, fields: a.fields })));
  const auth = request.auth;
  const choice = authChoice(auth);
  const setAuth = (next: Auth | undefined) =>
    update((r) => {
      const { auth: _old, ...rest } = r;
      return next ? { ...rest, auth: next } : rest;
    });
  const pluginMethod = auth?.type === "plugin" ? pluginMethods.find((m) => m.id === auth.name) : undefined;

  return (
    <div className="tab-pad stack auth-tab">
      <label className="field" style={{ maxWidth: 360 }}>
        <span>Type</span>
        <select className="select" value={choice} onChange={(e) => setAuth(authForChoice(e.target.value as AuthChoice, auth))}>
          <option value="inherit">Inherit from folder</option>
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
      {!auth && <p className="hint">Uses the auth of the folder (set in its _folder.yaml), if any.</p>}
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
    </div>
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

export function ScriptsTab({ request, update }: TabProps) {
  const set = (key: "before_request" | "after_response", code: string) => update((r) => ({ ...r, scripts: { ...r.scripts, [key]: code } }));
  return (
    <div className="scripts">
      <div className="scripts-editors">
        <div className="graphql-pane">
          <span className="label">Before request</span>
          <CodeEditor
            className="tab-editor"
            value={request.scripts.before_request}
            onChange={(code) => set("before_request", code)}
            language="javascript"
            ariaLabel="Script before request"
            placeholder={'pt.request.headers.set("X-Request-Id", crypto.randomUUID?.() ?? "1");'}
          />
        </div>
        <div className="graphql-pane">
          <span className="label">After response</span>
          <CodeEditor
            className="tab-editor"
            value={request.scripts.after_response}
            onChange={(code) => set("after_response", code)}
            language="javascript"
            ariaLabel="Script after response"
            placeholder={'pt.test("status is 200", () => expect(pt.response.status).toBe(200));\npt.variables.set("token", pt.response.json().token);'}
          />
        </div>
      </div>
      <aside className="cheatsheet" aria-label="Script cheat sheet">
        <h3 className="section-title">Cheat sheet</h3>
        <dl>
          <dt>pt.request</dt>
          <dd>method, url, headers (get/set), body. Changes are sent.</dd>
          <dt>pt.response</dt>
          <dd>status, statusText, headers, timeMs, json(), text()</dd>
          <dt>pt.variables</dt>
          <dd>get(name), set(name, value), unset(name), has(name)</dd>
          <dt>pt.test(name, fn)</dt>
          <dd>Records a test; throw (or fail expect) to fail it.</dd>
          <dt>expect(x)</dt>
          <dd>toBe, toEqual, toContain, toMatch, toBeGreaterThan…</dd>
          <dt>pt.http.get(url)</dt>
          <dd>Send another request (await it).</dd>
          <dt>console.log()</dt>
          <dd>Writes to the Logs tab in Live Activity.</dd>
          <dt>pm.*</dt>
          <dd>Postman scripts work: pm.test, pm.expect, pm.response, pm.environment, pm.sendRequest.</dd>
        </dl>
      </aside>
    </div>
  );
}

const linesOf = (text: string) =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

export function TestsTab({ request, update }: TabProps) {
  const value = request.tests.join("\n");
  return (
    <div className="tab-fill">
      <div className="tab-toolbar">
        <span className="hint grow">One assertion per line. They run after the response arrives; results show in the response and in Live Activity.</span>
        <HelpPopover label="Syntax help">
          <ExpressionHelp />
        </HelpPopover>
      </div>
      <CodeEditor
        className="tab-editor"
        value={value}
        onChange={(text) => update((r) => ({ ...r, tests: linesOf(text) }))}
        isEquivalent={(typed, v) => linesOf(typed).join("\n") === v}
        ariaLabel="Tests, one per line"
        placeholder={'status == 200\ntime < 500\njson($.id) exists\nheader(content-type) contains json'}
      />
    </div>
  );
}

function ExpressionHelp() {
  return (
    <div className="stack">
      <div>
        <h4 className="section-title">Subjects</h4>
        <table className="table">
          <tbody>
            <tr><td><code>status</code></td><td>Status code, e.g. 200</td></tr>
            <tr><td><code>time</code></td><td>Total time in ms</td></tr>
            <tr><td><code>size</code></td><td>Body size in bytes</td></tr>
            <tr><td><code>body</code></td><td>Body as text</td></tr>
            <tr><td><code>header(name)</code></td><td>A response header</td></tr>
            <tr><td><code>json($.path)</code></td><td>A JSONPath query</td></tr>
          </tbody>
        </table>
      </div>
      <div>
        <h4 className="section-title">Operators</h4>
        <p className="mono">== != &lt; &lt;= &gt; &gt;= contains !contains exists !exists matches !matches</p>
      </div>
      <div>
        <h4 className="section-title">Examples</h4>
        <pre className="code-block">{'status == 200\njson($.items[0].name) == "Alice"\nbody matches ^\\{.*\\}$'}</pre>
      </div>
    </div>
  );
}

export function ExtractTab({ request, update }: TabProps) {
  return (
    <div className="tab-pad">
      <div className="tab-toolbar flush">
        <span className="hint grow">After the response arrives, each expression is evaluated and stored as a session variable (usable as {"{{name}}"} in later requests).</span>
        <HelpPopover label="Syntax help">
          <ExpressionHelp />
        </HelpPopover>
      </div>
      <KeyValueEditor
        label="Extract variables"
        toggleable={false}
        items={recordToRows(request.extract)}
        onChange={(rows) => update((r) => ({ ...r, extract: rowsToRecord(rows) }))}
        keyPlaceholder="variable name"
        valuePlaceholder="json($.token)"
      />
    </div>
  );
}

export function OptionsTab({ request, update }: TabProps) {
  const o = request.options;
  const set = (patch: Partial<Request["options"]>) => update((r) => ({ ...r, options: { ...r.options, ...patch } }));
  return (
    <div className="tab-pad form-grid options">
      <label className="field">
        <span>Timeout (ms)</span>
        <input className="input" type="number" min={0} value={o.timeout_ms} onChange={(e) => set({ timeout_ms: Math.max(0, Number(e.target.value) || 0) })} />
        <span className="hint">Time limit until the response headers arrive.</span>
      </label>
      <label className="field">
        <span>HTTP version</span>
        <select className="select" value={o.http_version} onChange={(e) => set({ http_version: e.target.value as Request["options"]["http_version"] })}>
          <option value="auto">Auto (HTTP/2 when supported)</option>
          <option value="http1">HTTP/1.1 only</option>
          <option value="http2">HTTP/2 only</option>
        </select>
      </label>
      <label className="checkbox">
        <input type="checkbox" checked={o.follow_redirects} onChange={(e) => set({ follow_redirects: e.target.checked })} />
        Follow redirects
      </label>
      <label className="field">
        <span>Max redirects</span>
        <input className="input" type="number" min={0} value={o.max_redirects} disabled={!o.follow_redirects} onChange={(e) => set({ max_redirects: Math.max(0, Number(e.target.value) || 0) })} />
      </label>
      <label className="checkbox">
        <input type="checkbox" checked={o.verify_tls} onChange={(e) => set({ verify_tls: e.target.checked })} />
        Verify TLS certificates
      </label>
      {!o.verify_tls && <p className="callout warning">Invalid and self-signed certificates are accepted. Only use this for local or test servers.</p>}
    </div>
  );
}
