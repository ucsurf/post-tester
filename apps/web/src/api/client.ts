import type { Environment, Event, Folder, FolderSettings, PluginInfo, Request, ScriptResponse } from "@post-tester/bindings";
import { apiRequest, idPath, rawRequest } from "./http";
import type {
  CodegenResult,
  CodegenTarget,
  Cookie,
  ExecutionView,
  ExportTarget,
  ImportResult,
  Info,
  LoadBody,
  LoadReport,
  McpConnectOptions,
  McpSessionInfo,
  PreparedView,
  RunBody,
  RunReport,
  SendBody,
  TemplateFunction,
  Variable,
} from "./types";

/** One function per endpoint in crates/server/API.md. */
export const api = {
  // Workspace
  info: () => apiRequest<Info>("GET", "/api/info"),
  tree: () => apiRequest<Folder>("GET", "/api/tree"),
  getRequest: (id: string) => apiRequest<Partial<Request>>("GET", `/api/requests/${idPath(id)}`),
  saveRequest: (id: string, request: Request) => apiRequest<void>("PUT", `/api/requests/${idPath(id)}`, { body: request }),
  deleteRequest: (id: string) => apiRequest<void>("DELETE", `/api/requests/${idPath(id)}`),
  getFolderSettings: (id: string) => apiRequest<FolderSettings>("GET", `/api/folders/settings/${idPath(id)}`),
  saveFolderSettings: (id: string, settings: FolderSettings) =>
    apiRequest<void>("PUT", `/api/folders/settings/${idPath(id)}`, { body: settings }),
  environments: () => apiRequest<string[]>("GET", "/api/environments"),
  getEnvironment: (name: string) => apiRequest<Environment>("GET", `/api/environments/${encodeURIComponent(name)}`),
  saveEnvironment: (name: string, environment: Environment) =>
    apiRequest<void>("PUT", `/api/environments/${encodeURIComponent(name)}`, { body: environment }),
  deleteEnvironment: (name: string) => apiRequest<void>("DELETE", `/api/environments/${encodeURIComponent(name)}`),
  variables: (environment: string | null) =>
    apiRequest<Variable[]>("GET", `/api/variables${environment ? `?environment=${encodeURIComponent(environment)}` : ""}`),
  clearRuntimeVariables: () => apiRequest<void>("DELETE", "/api/variables/runtime"),
  setDefaultEnvironment: (environment: string | null) =>
    apiRequest<void>("PUT", "/api/settings/default-environment", { body: { environment } }),

  // Running
  send: (body: SendBody, signal?: AbortSignal) => apiRequest<ExecutionView>("POST", "/api/send", { body, signal }),
  run: (body: RunBody) => apiRequest<RunReport>("POST", "/api/run", { body }),
  loadTest: (body: LoadBody) => apiRequest<LoadReport>("POST", "/api/load-test", { body }),
  cancel: (spanId: string) => apiRequest<{ cancelled: boolean }>("POST", `/api/cancel/${encodeURIComponent(spanId)}`),
  preview: (body: SendBody) => apiRequest<PreparedView>("POST", "/api/preview", { body }),

  // Live events
  eventsSince: (seq: number) => apiRequest<Event[]>("GET", `/api/events?since=${seq}`),
  blobText: async (blobId: string) => (await rawRequest("GET", `/api/blobs/${encodeURIComponent(blobId)}`)).text(),

  // Code, import, plugins
  codegenTargets: () => apiRequest<CodegenTarget[]>("GET", "/api/codegen/targets"),
  codegen: (body: SendBody & { target: string }) => apiRequest<CodegenResult>("POST", "/api/codegen", { body }),
  importText: (text: string, into?: string) => apiRequest<ImportResult>("POST", "/api/import", { body: into ? { text, into } : { text } }),
  exportTargets: () => apiRequest<ExportTarget[]>("GET", "/api/export/targets"),
  exportFolder: (folder: string, exporter: string) => apiRequest<{ content: string }>("POST", "/api/export", { body: { folder, exporter } }),
  plugins: () => apiRequest<PluginInfo[]>("GET", "/api/plugins"),
  reloadPlugins: () => apiRequest<PluginInfo[]>("POST", "/api/plugins/reload"),
  approvePlugin: (name: string) => apiRequest<PluginInfo>("POST", `/api/plugins/${encodeURIComponent(name)}/approve`),
  renderViewer: (viewer: string, response: ScriptResponse) =>
    apiRequest<{ html: string }>("POST", "/api/viewers/render", { body: { viewer, response } }),
  templateFunctions: () => apiRequest<TemplateFunction[]>("GET", "/api/template-functions"),

  // Secrets and cookies
  setSecret: (name: string, value: string) => apiRequest<void>("PUT", `/api/secrets/${encodeURIComponent(name)}`, { body: { value } }),
  deleteSecret: (name: string) => apiRequest<void>("DELETE", `/api/secrets/${encodeURIComponent(name)}`),
  cookies: () => apiRequest<Cookie[]>("GET", "/api/cookies"),
  clearCookies: () => apiRequest<void>("DELETE", "/api/cookies"),

  // WebSocket client sessions
  wsConnect: (body: { url: string; headers?: [string, string][]; environment?: string | null }) =>
    apiRequest<{ sessionId: string }>("POST", "/api/ws/connect", { body }),
  wsSend: (sessionId: string, text: string) => apiRequest<void>("POST", `/api/ws/${encodeURIComponent(sessionId)}/send`, { body: { text } }),
  wsClose: (sessionId: string) => apiRequest<void>("POST", `/api/ws/${encodeURIComponent(sessionId)}/close`),

  // MCP client
  mcpConnect: (options: McpConnectOptions) => apiRequest<McpSessionInfo>("POST", "/api/mcp/connect", { body: options }),
  mcpSessions: () => apiRequest<McpSessionInfo[]>("GET", "/api/mcp/sessions"),
  mcpRefresh: (id: string) => apiRequest<McpSessionInfo>("POST", `/api/mcp/${encodeURIComponent(id)}/refresh`),
  mcpCall: (id: string, tool: string, args: unknown) =>
    apiRequest<unknown>("POST", `/api/mcp/${encodeURIComponent(id)}/call`, { body: { tool, arguments: args } }),
  mcpRead: (id: string, uri: string) => apiRequest<unknown>("POST", `/api/mcp/${encodeURIComponent(id)}/read`, { body: { uri } }),
  mcpPrompt: (id: string, name: string, args: Record<string, string>) =>
    apiRequest<unknown>("POST", `/api/mcp/${encodeURIComponent(id)}/prompt`, { body: { name, arguments: args } }),
  mcpDisconnect: (id: string) => apiRequest<void>("DELETE", `/api/mcp/${encodeURIComponent(id)}`),
};
