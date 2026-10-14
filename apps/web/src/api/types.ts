import type { AssertionResult, Outcome, Payload, Request } from "@post-tester/bindings";

/**
 * Types for API shapes that are not generated from Rust (see crates/server/API.md).
 * Report types (`RunReport`, `LoadReport`) are snake_case because they are serialized
 * straight from engine structs.
 */

export type Info = {
  version: string;
  workspace: { root: string; name: string } | null;
  environments: string[];
  defaultEnvironment: string | null;
  secretStore: string;
  mcpUrl: string;
};

export type SendBody = { request: Request; requestId?: string; environment?: string | null };

export type PreparedView = { method: string; url: string; headers: [string, string][]; body: string };

export type Timings = { dnsMs: number; connectMs: number; tlsMs: number | null; waitingMs: number; downloadMs: number };

export type ResponseView = {
  status: number;
  statusText: string;
  httpVersion: string;
  headers: [string, string][];
  body: Payload;
  size: number;
  wireSize: number;
  timeMs: number;
  timings: Timings;
  remoteAddr: string;
  url: string;
  redirects: number;
};

export type ExecutionView = {
  spanId: string;
  outcome: Outcome;
  error: string | null;
  request: PreparedView | null;
  response: ResponseView | null;
  tests: AssertionResult[];
  extracted: [string, string][];
};

export type RunTarget = { folder: string } | { requests: string[] };

export type RunBody = { target: RunTarget; environment?: string | null; bail?: boolean; iterations?: number; delayMs?: number };

export type RunItem = {
  request_id: string;
  name: string;
  span_id: string;
  outcome: Outcome;
  status: number | null;
  duration_ms: number;
  tests: AssertionResult[];
  error: string | null;
};

export type RunReport = { span_id: string; name: string; outcome: Outcome; items: RunItem[]; duration_ms: number };

export type LoadBody = { request: Request; requestId?: string; environment?: string | null; concurrency: number; requests: number };

export type LoadReport = {
  span_id: string | null;
  requests: number;
  succeeded: number;
  failed: number;
  duration_ms: number;
  requests_per_second: number;
  min_ms: number;
  mean_ms: number;
  p50_ms: number;
  p90_ms: number;
  p99_ms: number;
  max_ms: number;
  status_codes: Record<string, number>;
};

export type Variable = { name: string; value: string };

export type CodegenTarget = { id: string; name: string; language: string };
export type CodegenResult = { code: string; language: string };

export type ExportTarget = { id: string; name: string; fileExtension: string };

export type ImportResult = { folder: string; requests: string[]; environments: string[] };

export type TemplateFunction = { name: string; description: string; plugin: string | null };

export type Cookie = { domain: string; name: string; value: string };

export type McpTransport =
  | { type: "stdio"; command: string; args: string[]; env?: Record<string, string>; cwd?: string }
  | { type: "http"; url: string; headers?: [string, string][] };

export type McpConnectOptions = { name?: string; transport: McpTransport };

/** MCP spec objects; only the fields the UI reads are typed. */
export type McpTool = { name: string; title?: string; description?: string; inputSchema?: Record<string, unknown> };
export type McpResource = { uri: string; name?: string; title?: string; description?: string; mimeType?: string };
export type McpPromptArgument = { name: string; description?: string; required?: boolean };
export type McpPrompt = { name: string; title?: string; description?: string; arguments?: McpPromptArgument[] };

export type McpSessionInfo = {
  id: string;
  name: string;
  spanId: string;
  transport: McpTransport;
  serverName: string;
  serverVersion: string;
  protocolVersion: string;
  instructions: string | null;
  tools: McpTool[];
  resources: McpResource[];
  prompts: McpPrompt[];
};
