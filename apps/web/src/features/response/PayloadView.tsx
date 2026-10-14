import type { Payload, ViewerInfo } from "@post-tester/bindings";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import { formatSize } from "../../lib/format";
import { isJsonContentType, matchesContentType, mediaType, parseJson } from "../../lib/text";
import { appStore } from "../../state/app";
import { useStore } from "../../state/store";
import { CodeEditor } from "../../ui/CodeEditor";
import { JsonTree } from "../../ui/JsonTree";
import { CopyButton, Spinner } from "../../ui/widgets";
import "./response.css";

export type ViewerContext = { status: number; statusText: string; headers: [string, string][]; timeMs: number };

type Props = {
  payload: Payload;
  /** Content type when the payload itself does not carry one. */
  contentType?: string | null;
  /** Enables plugin viewers (responses only). */
  viewer?: ViewerContext;
};

type Mode = "pretty" | "raw" | `viewer:${string}`;

/**
 * Shows a body: pretty (JSON tree or highlighted text), raw, or a plugin viewer.
 * Truncated payloads offer "Load full body" from the blob store.
 */
export function PayloadView({ payload, contentType, viewer }: Props) {
  const [full, setFull] = useState<{ blob: string; text: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>("pretty");
  const plugins = useStore(appStore, (s) => s.plugins);

  const type = payload.content_type ?? contentType ?? null;
  const text = full && full.blob === payload.blob ? full.text : payload.preview;
  const isComplete = !payload.truncated || (full !== null && full.blob === payload.blob);

  const viewers = useMemo<ViewerInfo[]>(
    () => (viewer ? plugins.flatMap((p) => p.capabilities.viewers.filter((v) => matchesContentType(type, v.contentTypes)).map((v) => ({ ...v, name: `${p.name}/${v.name}` }))) : []),
    [plugins, type, viewer],
  );

  useEffect(() => {
    if (mode.startsWith("viewer:") && !viewers.some((v) => `viewer:${v.name}` === mode)) setMode("pretty");
  }, [mode, viewers]);

  const loadFull = async () => {
    if (!payload.blob) return;
    setLoading(true);
    setLoadError(null);
    try {
      setFull({ blob: payload.blob, text: await api.blobText(payload.blob) });
    } catch (e) {
      setLoadError(errorText(e));
    } finally {
      setLoading(false);
    }
  };

  if (payload.size === 0 && payload.preview === "") return <p className="muted pad">Empty body.</p>;

  return (
    <div className="payload">
      <div className="payload-toolbar">
        <div className="segmented" role="group" aria-label="Body view">
          <button type="button" aria-pressed={mode === "pretty"} onClick={() => setMode("pretty")}>
            Pretty
          </button>
          <button type="button" aria-pressed={mode === "raw"} onClick={() => setMode("raw")}>
            Raw
          </button>
          {viewers.map((v) => (
            <button key={v.name} type="button" aria-pressed={mode === `viewer:${v.name}`} onClick={() => setMode(`viewer:${v.name}`)} title={`Plugin viewer ${v.name}`}>
              {v.displayName}
            </button>
          ))}
        </div>
        <span className="faint">
          {mediaType(type) || "unknown type"} · {formatSize(payload.size)}
        </span>
        <span className="spacer" />
        <CopyButton text={text} />
      </div>
      {!isComplete && (
        <div className="callout warning payload-truncated">
          Showing the first {formatSize(payload.preview.length)} of {formatSize(payload.size)}.{" "}
          {payload.blob ? (
            <button type="button" className="btn btn-sm" onClick={loadFull} disabled={loading}>
              {loading ? "Loading…" : "Load full body"}
            </button>
          ) : (
            "The full body was not stored."
          )}
          {loadError && <span className="tone-danger"> {loadError} (the body may have been evicted from memory; send the request again).</span>}
        </div>
      )}
      <div className="payload-content">
        {mode === "pretty" && <PrettyBody text={text} contentType={type} />}
        {mode === "raw" && <pre className="payload-raw mono">{text}</pre>}
        {mode.startsWith("viewer:") && viewer && (
          <ViewerFrame
            viewer={mode.slice("viewer:".length)}
            context={viewer}
            body={text}
            complete={isComplete}
            onNeedFull={payload.blob ? loadFull : undefined}
          />
        )}
      </div>
    </div>
  );
}

function PrettyBody({ text, contentType }: { text: string; contentType: string | null }) {
  const parsed = useMemo(() => (isJsonContentType(contentType) || /^\s*[[{]/.test(text) ? parseJson(text) : null), [text, contentType]);
  if (parsed?.ok && typeof parsed.value === "object" && parsed.value !== null) return <JsonTree value={parsed.value} />;
  const language = parsed?.ok ? "json" : /javascript/.test(contentType ?? "") ? "javascript" : "text";
  return <CodeEditor value={text} readOnly language={language} ariaLabel="Body" className="flush" full />;
}

function ViewerFrame({
  viewer,
  context,
  body,
  complete,
  onNeedFull,
}: {
  viewer: string;
  context: ViewerContext;
  body: string;
  complete: boolean;
  onNeedFull: (() => void) | undefined;
}) {
  const [state, setState] = useState<{ html: string } | { error: string } | null>(null);
  // Callers often build `context` inline; depend on its content, not its identity.
  const contextKey = JSON.stringify(context);
  const needFull = useRef(onNeedFull);
  needFull.current = onNeedFull;

  useEffect(() => {
    if (!complete && needFull.current) {
      needFull.current();
      return;
    }
    const ctx = JSON.parse(contextKey) as ViewerContext;
    let active = true;
    setState(null);
    api
      .renderViewer(viewer, { status: ctx.status, status_text: ctx.statusText, headers: ctx.headers, body, time_ms: ctx.timeMs })
      .then((r) => active && setState({ html: r.html }))
      .catch((e: unknown) => active && setState({ error: errorText(e) }));
    return () => {
      active = false;
    };
  }, [viewer, body, complete, contextKey]);

  if (!state) return <div className="pad"><Spinner label={`Rendering with ${viewer}…`} /></div>;
  if ("error" in state) return <p className="callout danger">The viewer “{viewer}” failed: {state.error}</p>;
  // No `allow-scripts`/`allow-same-origin`: plugin HTML is shown, never executed with our origin.
  return <iframe className="viewer-frame" title={`Viewer ${viewer}`} sandbox="" srcDoc={state.html} />;
}
