import type { Folder, PluginInfo, Request } from "@post-tester/bindings";
import { api } from "../api/client";
import { errorText } from "../api/http";
import type { ExecutionView, Info, TemplateFunction, Variable } from "../api/types";
import { idBaseName, newRequest, normalizeRequest, requestsEqual } from "../features/request-editor/model";
import { confirmDialog, promptDialog, validateRequestId } from "./dialogs";
import { Store } from "./store";
import { showError, showToast } from "./toasts";

/** Workspace data, open tabs and request drafts. Actions below are the only writers. */

export type PanelKind = "environments" | "plugins" | "agent" | "mcp" | "settings";

export type Tab = { key: string; kind: "request" } | { key: string; kind: "websocket" } | { key: string; kind: PanelKind };

export type SendState =
  | { phase: "idle" }
  | { phase: "sending"; startedAt: number; controller: AbortController }
  | { phase: "done"; startedAt: number; result: ExecutionView }
  | { phase: "failed"; startedAt: number; error: string };

export type RequestDoc = {
  key: string;
  /** Saved id (`auth/login`), or null for a new, unsaved request. */
  id: string | null;
  /** Folder suggested when a new request is saved for the first time. */
  folder: string | null;
  saved: Request | null;
  draft: Request;
  loading: boolean;
  loadError: string | null;
  send: SendState;
};

export type Dialog =
  | { type: "import"; into?: string }
  | { type: "runner"; folder: string | null; requests: string[] }
  | { type: "load-test"; requestKey: string };

export type AppState = {
  info: Info | null;
  infoError: string | null;
  tree: Folder | null;
  treeError: string | null;
  environment: string | null;
  environments: string[];
  plugins: PluginInfo[];
  variables: Variable[];
  templateFunctions: TemplateFunction[];
  tabs: Tab[];
  activeTab: string | null;
  requests: Record<string, RequestDoc>;
  dialog: Dialog | null;
  /** Span shown in the detail drawer. */
  selectedSpan: string | null;
};

export const appStore = new Store<AppState>({
  info: null,
  infoError: null,
  tree: null,
  treeError: null,
  environment: null,
  environments: [],
  plugins: [],
  variables: [],
  templateFunctions: [],
  tabs: [],
  activeTab: null,
  requests: {},
  dialog: null,
  selectedSpan: null,
});

const ENV_KEY = "post-tester.environment";
let tabCounter = 0;
const nextKey = (prefix: string) => `${prefix}-${++tabCounter}`;

// ── Workspace ──────────────────────────────────────────────────────────────

export async function loadWorkspace(): Promise<void> {
  try {
    const info = await api.info();
    const stored = readLocal(ENV_KEY);
    const environment = stored && info.environments.includes(stored) ? stored : info.defaultEnvironment;
    appStore.set({ info, infoError: null, environments: info.environments, environment });
  } catch (e) {
    appStore.set({ infoError: errorText(e) });
    return;
  }
  if (!appStore.get().info?.workspace) return;
  await Promise.all([refreshTree(), refreshPlugins(), refreshVariables(), refreshTemplateFunctions()]);
}

export async function refreshTree(): Promise<void> {
  try {
    appStore.set({ tree: await api.tree(), treeError: null });
  } catch (e) {
    appStore.set({ treeError: errorText(e) });
  }
}

export async function refreshEnvironments(): Promise<void> {
  try {
    const environments = await api.environments();
    appStore.set((s) => ({ environments, environment: s.environment && environments.includes(s.environment) ? s.environment : null }));
  } catch (e) {
    showError("Could not load environments", e);
  }
}

export async function refreshPlugins(): Promise<void> {
  try {
    appStore.set({ plugins: await api.plugins() });
  } catch {
    // The plugins panel shows its own error; other screens work without plugins.
  }
}

export async function refreshVariables(): Promise<void> {
  try {
    appStore.set({ variables: await api.variables(appStore.get().environment) });
  } catch {
    appStore.set({ variables: [] });
  }
}

async function refreshTemplateFunctions(): Promise<void> {
  try {
    appStore.set({ templateFunctions: await api.templateFunctions() });
  } catch {
    appStore.set({ templateFunctions: [] });
  }
}

export function selectEnvironment(environment: string | null): void {
  writeLocal(ENV_KEY, environment);
  appStore.set({ environment });
  void refreshVariables();
}

// ── Tabs ───────────────────────────────────────────────────────────────────

export function activateTab(key: string): void {
  appStore.set({ activeTab: key });
}

export function openPanel(kind: PanelKind): void {
  const existing = appStore.get().tabs.find((t) => t.kind === kind);
  if (existing) return activateTab(existing.key);
  const tab: Tab = { key: kind, kind };
  appStore.set((s) => ({ tabs: [...s.tabs, tab], activeTab: tab.key }));
}

export function openWebSocketTab(): void {
  const tab: Tab = { key: nextKey("ws"), kind: "websocket" };
  appStore.set((s) => ({ tabs: [...s.tabs, tab], activeTab: tab.key }));
}

export async function closeTab(key: string): Promise<void> {
  const doc = appStore.get().requests[key];
  if (doc && isDirty(doc)) {
    const ok = await confirmDialog({
      title: "Close without saving?",
      message: `“${tabTitle(doc)}” has changes that are not saved. They will be lost.`,
      confirmLabel: "Close without saving",
      danger: true,
    });
    if (!ok) return;
  }
  if (doc?.send.phase === "sending") doc.send.controller.abort();
  appStore.set((s) => {
    const index = s.tabs.findIndex((t) => t.key === key);
    const tabs = s.tabs.filter((t) => t.key !== key);
    const requests = { ...s.requests };
    delete requests[key];
    const activeTab = s.activeTab === key ? (tabs[Math.min(index, tabs.length - 1)]?.key ?? null) : s.activeTab;
    return { tabs, requests, activeTab };
  });
}

// ── Requests ───────────────────────────────────────────────────────────────

export function isDirty(doc: RequestDoc): boolean {
  if (doc.loading || doc.loadError) return false;
  return doc.saved === null ? !requestsEqual(doc.draft, newRequest()) : !requestsEqual(doc.draft, doc.saved);
}

export function tabTitle(doc: RequestDoc): string {
  if (doc.draft.name) return doc.draft.name;
  if (doc.id) return idBaseName(doc.id);
  return doc.draft.url ? doc.draft.url.replace(/^\{\{[^}]+\}\}/, "").replace(/^https?:\/\//, "") || doc.draft.url : "New request";
}

export async function openRequest(id: string): Promise<void> {
  const existing = Object.values(appStore.get().requests).find((d) => d.id === id);
  if (existing) return activateTab(existing.key);

  const key = nextKey("req");
  const doc: RequestDoc = { key, id, folder: null, saved: null, draft: newRequest(), loading: true, loadError: null, send: { phase: "idle" } };
  appStore.set((s) => ({ tabs: [...s.tabs, { key, kind: "request" }], activeTab: key, requests: { ...s.requests, [key]: doc } }));
  try {
    const request = normalizeRequest(await api.getRequest(id));
    patchDoc(key, { saved: request, draft: request, loading: false });
  } catch (e) {
    patchDoc(key, { loading: false, loadError: errorText(e) });
  }
}

export function newRequestTab(initial?: Request, folder: string | null = null): string {
  const key = nextKey("req");
  const draft = initial ?? newRequest();
  const doc: RequestDoc = { key, id: null, folder, saved: null, draft, loading: false, loadError: null, send: { phase: "idle" } };
  appStore.set((s) => ({ tabs: [...s.tabs, { key, kind: "request" }], activeTab: key, requests: { ...s.requests, [key]: doc } }));
  return key;
}

function patchDoc(key: string, patch: Partial<RequestDoc>): void {
  appStore.set((s) => {
    const doc = s.requests[key];
    return doc ? { requests: { ...s.requests, [key]: { ...doc, ...patch } } } : {};
  });
}

export function updateDraft(key: string, update: (request: Request) => Request): void {
  const doc = appStore.get().requests[key];
  if (doc) patchDoc(key, { draft: update(doc.draft) });
}

/** Saves to the current id, or asks for one when the request is new. Returns false if cancelled. */
export async function saveRequest(key: string): Promise<boolean> {
  const doc = appStore.get().requests[key];
  if (!doc) return false;
  let id = doc.id;
  if (!id) {
    id = await promptDialog({
      title: "Save request",
      message: "Requests are saved as YAML files in collections/. Use / to put it in a folder.",
      label: "Request id",
      placeholder: "users/list",
      initial: [doc.folder, suggestId(doc.draft)].filter(Boolean).join("/"),
      confirmLabel: "Save",
      validate: validateRequestId,
    });
    if (!id) return false;
    id = id.trim();
  }
  try {
    await api.saveRequest(id, doc.draft);
    patchDoc(key, { id, saved: doc.draft });
    showToast("success", `Saved ${id}`);
    void refreshTree();
    return true;
  } catch (e) {
    showError("Could not save the request", e);
    return false;
  }
}

export function duplicateRequest(key: string): void {
  const doc = appStore.get().requests[key];
  if (!doc) return;
  const name = `${doc.draft.name || (doc.id ? idBaseName(doc.id) : "Request")} copy`;
  newRequestTab({ ...doc.draft, name }, doc.id?.includes("/") ? doc.id.slice(0, doc.id.lastIndexOf("/")) : doc.folder);
}

export async function renameRequest(key: string): Promise<void> {
  const doc = appStore.get().requests[key];
  if (!doc?.id) return;
  const oldId = doc.id;
  const id = await promptDialog({
    title: "Rename request",
    message: "Saves the request under the new id and removes the old file.",
    label: "New id",
    initial: oldId,
    confirmLabel: "Rename",
    validate: (v) => validateRequestId(v) ?? (v.trim() === oldId ? "Enter a different id." : null),
  });
  if (!id) return;
  try {
    await api.saveRequest(id.trim(), doc.draft);
    await api.deleteRequest(oldId);
    patchDoc(key, { id: id.trim(), saved: doc.draft });
    showToast("success", `Renamed to ${id.trim()}`);
  } catch (e) {
    showError("Could not rename the request", e);
  }
  void refreshTree();
}

export async function deleteRequest(id: string): Promise<void> {
  const ok = await confirmDialog({
    title: "Delete request?",
    message: `This deletes collections/${id}.yaml from your workspace.`,
    confirmLabel: "Delete",
    danger: true,
  });
  if (!ok) return;
  try {
    await api.deleteRequest(id);
    const doc = Object.values(appStore.get().requests).find((d) => d.id === id);
    if (doc) {
      patchDoc(doc.key, { saved: null, id: null });
      appStore.set((s) => {
        const requests = { ...s.requests };
        delete requests[doc.key];
        const tabs = s.tabs.filter((t) => t.key !== doc.key);
        return { requests, tabs, activeTab: s.activeTab === doc.key ? (tabs[0]?.key ?? null) : s.activeTab };
      });
    }
    showToast("success", `Deleted ${id}`);
  } catch (e) {
    showError("Could not delete the request", e);
  }
  void refreshTree();
}

export async function sendRequest(key: string): Promise<void> {
  const doc = appStore.get().requests[key];
  if (!doc || doc.send.phase === "sending") return;
  if (!doc.draft.url.trim()) {
    showToast("info", "Enter a URL first", "For example https://httpbin.org/get or {{baseUrl}}/users.");
    return;
  }
  const controller = new AbortController();
  const startedAt = Date.now();
  patchDoc(key, { send: { phase: "sending", startedAt, controller } });
  try {
    const result = await api.send(
      { request: doc.draft, ...(doc.id ? { requestId: doc.id } : {}), environment: appStore.get().environment },
      controller.signal,
    );
    patchDoc(key, { send: { phase: "done", startedAt, result } });
    if (result.extracted.length > 0) void refreshVariables();
  } catch (e) {
    if (e instanceof DOMException && e.name === "AbortError") {
      patchDoc(key, { send: { phase: "idle" } });
      return;
    }
    patchDoc(key, { send: { phase: "failed", startedAt, error: errorText(e) } });
  }
}

// ── Dialogs and drawer ─────────────────────────────────────────────────────

export function openDialog(dialog: Dialog): void {
  appStore.set({ dialog });
}

export function closeDialog(): void {
  appStore.set({ dialog: null });
}

export function selectSpan(id: string | null): void {
  appStore.set({ selectedSpan: id });
}

function suggestId(request: Request): string {
  const base = request.name || request.url.replace(/\{\{[^}]*\}\}/g, "").replace(/^https?:\/\/[^/]+/, "").replace(/\?.*$/, "");
  return base
    .toLowerCase()
    .replace(/[^a-z0-9/]+/g, "-")
    .replace(/^[-/]+|[-/]+$/g, "")
    .replace(/\/+/g, "/");
}

function readLocal(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLocal(key: string, value: string | null): void {
  try {
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode); the choice then lasts for this session only.
  }
}
