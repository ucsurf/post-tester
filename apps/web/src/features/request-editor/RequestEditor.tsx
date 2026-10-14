import type { Request } from "@post-tester/bindings";
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import { applyUrlToRequest, composeUrl } from "../../lib/url";
import {
  appStore,
  deleteRequest,
  duplicateRequest,
  isDirty,
  openDialog,
  renameRequest,
  saveRequest,
  sendRequest,
  updateDraft,
  type RequestDoc,
} from "../../state/app";
import { useStore } from "../../state/store";
import { Icon } from "../../ui/Icon";
import { Menu } from "../../ui/Menu";
import { Splitter, usePersistentNumber } from "../../ui/Splitter";
import { TabPanel, Tabs } from "../../ui/Tabs";
import { TemplateInput } from "../../ui/TemplateInput";
import { EmptyState, Spinner } from "../../ui/widgets";
import { ResponsePanel } from "../response/ResponsePanel";
import { CodegenDialog, PreviewDialog } from "./CodeDialogs";
import { AuthTab, BodyTab, ExtractTab, HeadersTab, OptionsTab, ParamsTab, ScriptsTab, TestsTab } from "./tabs";
import { COMMON_METHODS, countEnabled } from "./model";
import "./request-editor.css";

type EditorTab = "params" | "headers" | "body" | "auth" | "scripts" | "tests" | "extract" | "options";

/** Editor for one open request (top) with its response (bottom). */
export function RequestEditor({ docKey }: { docKey: string }) {
  const doc = useStore(appStore, (s) => s.requests[docKey]);
  const [height, setHeight] = usePersistentNumber("post-tester.editorHeight", 300);

  if (!doc) return null;
  if (doc.loading) {
    return (
      <div className="empty">
        <Spinner label={`Loading ${doc.id ?? "request"}…`} />
      </div>
    );
  }
  if (doc.loadError) {
    return (
      <EmptyState icon="⚠️" title={`Could not open ${doc.id ?? "the request"}`}>
        <p>{doc.loadError}</p>
        <p>Fix the YAML file in your workspace, then open it again from the sidebar.</p>
      </EmptyState>
    );
  }

  return (
    <div className="request-editor">
      <div className="editor-top" style={{ height }}>
        <EditorHeader doc={doc} />
        <EditorTabs doc={doc} />
      </div>
      <Splitter orientation="horizontal" size={height} onResize={setHeight} min={140} max={Math.max(240, window.innerHeight - 220)} direction={1} label="Resize request and response" />
      <div className="editor-bottom">
        <ResponsePanel docKey={docKey} />
      </div>
    </div>
  );
}

function EditorHeader({ doc }: { doc: RequestDoc }) {
  const key = doc.key;
  const request = doc.draft;
  const dirty = isDirty(doc);
  const sending = doc.send.phase === "sending";
  const [dialog, setDialog] = useState<null | "preview" | { codegen: string }>(null);
  const update = (fn: (r: Request) => Request) => updateDraft(key, fn);

  // Keyboard shortcuts while this editor is visible.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || document.querySelector(".modal-backdrop")) return;
      if (e.key === "Enter") {
        e.preventDefault();
        void sendRequest(key);
      } else if (e.key.toLowerCase() === "s") {
        e.preventDefault();
        void saveRequest(key);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [key]);

  const url = composeUrl(request.url, request.query);
  const isEquivalent = useCallback(
    (typed: string, value: string) => {
      const parsed = applyUrlToRequest(typed, appStore.get().requests[key]?.draft.query ?? []);
      return composeUrl(parsed.url, parsed.query) === value;
    },
    [key],
  );

  return (
    <div className="editor-header">
      <div className="editor-title">
        <input
          className="name-input"
          value={request.name}
          placeholder={doc.id ?? "Untitled request"}
          aria-label="Request name"
          onChange={(e) => update((r) => ({ ...r, name: e.target.value }))}
        />
        {doc.id && <span className="faint mono truncate" title="Saved as">{doc.id}</span>}
        {dirty && <span className="dirty-dot" title="Unsaved changes" aria-label="Unsaved changes" />}
        <span className="spacer" />
        <button type="button" className="btn btn-sm" onClick={() => saveRequest(key)} title="Save (Cmd/Ctrl+S)" disabled={!dirty && doc.id !== null}>
          Save
        </button>
        <CodegenMenu onPick={(target) => setDialog({ codegen: target })} />
        <Menu
          align="end"
          trigger={(props) => (
            <button type="button" className="icon-btn" aria-label="More actions" title="More actions" {...props}>
              <Icon name="more" />
            </button>
          )}
          items={[
            { id: "preview", label: "Preview what will be sent", onSelect: () => setDialog("preview") },
            { id: "load", label: "Load test…", onSelect: () => openDialog({ type: "load-test", requestKey: key }) },
            { id: "duplicate", label: "Duplicate", onSelect: () => duplicateRequest(key) },
            { id: "rename", label: "Rename…", disabled: !doc.id, onSelect: () => void renameRequest(key) },
            { id: "delete", label: "Delete…", danger: true, disabled: !doc.id, onSelect: () => doc.id && void deleteRequest(doc.id) },
          ]}
        />
      </div>
      <div className="url-row">
        <input
          className="method-input"
          list="http-methods"
          value={request.method}
          data-method={request.method}
          aria-label="HTTP method"
          onChange={(e) => update((r) => ({ ...r, method: e.target.value.toUpperCase().replace(/\s/g, "") }))}
        />
        <datalist id="http-methods">
          {COMMON_METHODS.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        <TemplateInput
          value={url}
          isEquivalent={isEquivalent}
          onChange={(text) => update((r) => ({ ...r, ...applyUrlToRequest(text, r.query) }))}
          onSubmit={() => sendRequest(key)}
          placeholder="https://api.example.com/users or {{baseUrl}}/users"
          ariaLabel="URL"
          className="url-input"
        />
        <button type="button" className="btn btn-primary send-btn" onClick={() => sendRequest(key)} disabled={sending} title="Send (Cmd/Ctrl+Enter)">
          {sending ? <span className="spinner" /> : <Icon name="send" />}
          Send
        </button>
      </div>
      {dialog === "preview" && <PreviewDialog request={request} requestId={doc.id} onClose={() => setDialog(null)} />}
      {dialog && typeof dialog === "object" && <CodegenDialog request={request} requestId={doc.id} target={dialog.codegen} onClose={() => setDialog(null)} />}
    </div>
  );
}

function CodegenMenu({ onPick }: { onPick: (target: string) => void }) {
  const [targets, setTargets] = useState<{ id: string; name: string; language: string }[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    if (targets) return;
    api
      .codegenTargets()
      .then(setTargets)
      .catch((e: unknown) => setError(errorText(e)));
  };
  return (
    <Menu
      align="end"
      onOpen={load}
      emptyText={error ?? (targets ? "No code generators" : "Loading…")}
      trigger={(props) => (
        <button type="button" className="btn btn-sm" title="Copy as code" {...props}>
          <Icon name="code" size={12} /> Code
        </button>
      )}
      items={() => (targets ?? []).map((t) => ({ id: t.id, label: t.name, hint: t.language, onSelect: () => onPick(t.id) }))}
    />
  );
}

function EditorTabs({ doc }: { doc: RequestDoc }) {
  const [tab, setTab] = useState<EditorTab>("params");
  const request = doc.draft;
  const update = (fn: (r: Request) => Request) => updateDraft(doc.key, fn);
  const scripts = [request.scripts.before_request, request.scripts.after_response].filter((s) => s.trim()).length;
  const bodyLabel = request.body.type === "none" ? undefined : request.body.type;

  return (
    <>
      <Tabs
        label="Request parts"
        idPrefix="request"
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "params", label: "Params", count: countEnabled(request.query) },
          { id: "headers", label: "Headers", count: countEnabled(request.headers) },
          { id: "body", label: "Body", count: bodyLabel },
          { id: "auth", label: "Auth", count: request.auth ? (request.auth.type === "plugin" ? "plugin" : request.auth.type) : undefined },
          { id: "scripts", label: "Scripts", count: scripts },
          { id: "tests", label: "Tests", count: request.tests.length },
          { id: "extract", label: "Extract", count: Object.keys(request.extract).length },
          { id: "options", label: "Options" },
        ]}
      />
      <TabPanel idPrefix="request" active={tab} className="editor-tab">
        {tab === "params" && <ParamsTab request={request} update={update} />}
        {tab === "headers" && <HeadersTab request={request} update={update} />}
        {tab === "body" && <BodyTab request={request} update={update} onSubmit={() => sendRequest(doc.key)} />}
        {tab === "auth" && <AuthTab request={request} update={update} />}
        {tab === "scripts" && <ScriptsTab request={request} update={update} />}
        {tab === "tests" && <TestsTab request={request} update={update} />}
        {tab === "extract" && <ExtractTab request={request} update={update} />}
        {tab === "options" && <OptionsTab request={request} update={update} />}
      </TabPanel>
    </>
  );
}
