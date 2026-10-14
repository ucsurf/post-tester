import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { Cookie, ExecutionView, ResponseView } from "../../api/types";
import { formatDuration, formatSize, plural } from "../../lib/format";
import { modKey } from "../../lib/platform";
import { headerValue } from "../../lib/text";
import { useLiveSelector, useSpan } from "../../live/live";
import { findUserSpan } from "../../live/selectors";
import type { SpanState } from "../../live/spans";
import { appStore, selectSpan, sendRequest, updateDraft, type RequestDoc } from "../../state/app";
import { promptDialog } from "../../state/dialogs";
import { addExample } from "../request-editor/model";
import { useStore } from "../../state/store";
import { showError, showToast } from "../../state/toasts";
import { Icon } from "../../ui/Icon";
import { TabPanel, Tabs } from "../../ui/Tabs";
import { EmptyState, HeaderTable, Spinner, StatusBadge } from "../../ui/widgets";
import { RunningTime } from "../activity/ActivityRow";
import { TestList } from "../activity/TestList";
import { Waterfall } from "../activity/Waterfall";
import { PayloadView } from "./PayloadView";
import "./response.css";

/** The response of the active request: live while sending, then the full result. */
export function ResponsePanel({ docKey }: { docKey: string }) {
  const doc = useStore(appStore, (s) => s.requests[docKey]);
  if (!doc) return null;
  const send = doc.send;

  switch (send.phase) {
    case "idle":
      return (
        <div className="response">
          <EmptyState icon="↩︎" title="No response yet">
            Press <span className="kbd">Send</span> or <span className="kbd">{modKey()}</span>+<span className="kbd">Enter</span> to send the request. The response,
            timings and test results appear here.
          </EmptyState>
        </div>
      );
    case "sending":
      return <Sending doc={doc} startedAt={send.startedAt} controller={send.controller} />;
    case "failed":
      return (
        <div className="response">
          <div className="response-error">
            <h3>The request could not be sent</h3>
            <div className="error-box">{send.error}</div>
            <div>
              <button type="button" className="btn" onClick={() => sendRequest(docKey)}>
                <Icon name="refresh" /> Try again
              </button>
            </div>
          </div>
        </div>
      );
    case "done":
      return <Result result={send.result} docKey={docKey} />;
  }
}

function Sending({ doc, startedAt, controller }: { doc: RequestDoc; startedAt: number; controller: AbortController }) {
  // Allow for small clock differences between the browser and the server.
  const span = useLiveSelector((state) => findUserSpan(state, { requestId: doc.id, url: doc.draft.url }, startedAt - 2000));

  const cancel = async () => {
    if (span) {
      try {
        await api.cancel(span.id);
        return;
      } catch (e) {
        showError("Could not cancel", e);
      }
    }
    controller.abort();
  };

  return (
    <div className="response">
      <div className="response-summary">
        <Spinner label={liveStage(span)} />
        <span className="metric">
          <strong>
            <RunningTime startedAt={span?.startedAt || startedAt} />
          </strong>
        </span>
        {span?.response && <StatusBadge status={span.response.status} text={span.response.statusText} />}
        <span className="spacer" />
        <button type="button" className="btn btn-sm btn-danger" onClick={cancel}>
          <Icon name="stop" size={12} /> Cancel
        </button>
      </div>
      <div className="live-progress">
        <Waterfall phases={span?.phases ?? {}} running connection={span?.connection ?? null} />
      </div>
    </div>
  );
}

function liveStage(span: SpanState | null): string {
  if (!span) return "Sending…";
  if (span.response) return "Downloading the body…";
  if (span.request) return "Waiting for the response…";
  if (span.connection) return "Connected, sending…";
  if (span.phases.dns !== undefined) return "Connecting…";
  return "Preparing (variables, auth, scripts)…";
}

type ResultTab = "body" | "headers" | "tests" | "timeline" | "cookies";

function Result({ result, docKey }: { result: ExecutionView; docKey: string }) {
  const [tab, setTab] = useState<ResultTab>("body");
  const span = useSpan(result.spanId);
  const response = result.response;
  const failedTests = result.tests.filter((t) => !t.passed).length;

  return (
    <div className="response">
      <div className="response-summary">
        {response ? (
          <>
            <StatusBadge status={response.status} text={response.statusText} />
            <span className="metric">
              Time <strong>{formatDuration(response.timeMs)}</strong>
            </span>
            <span className="metric" title={`${response.size} bytes decoded, ${response.wireSize} bytes on the wire`}>
              Size <strong>{formatSize(response.size)}</strong>
            </span>
            {response.redirects > 0 && <span className="metric">{plural(response.redirects, "redirect")}</span>}
          </>
        ) : (
          <span className="badge tone-danger">{result.outcome === "cancelled" ? "Cancelled" : "No response"}</span>
        )}
        {result.tests.length > 0 && (
          <span className={`badge ${failedTests ? "tone-danger" : "tone-success"}`}>
            {failedTests ? `${failedTests} test${failedTests > 1 ? "s" : ""} failed` : `${result.tests.length} tests passed`}
          </span>
        )}
        {result.extracted.length > 0 && <span className="badge" title={result.extracted.map(([k, v]) => `${k} = ${v}`).join("\n")}>Set {plural(result.extracted.length, "variable")}</span>}
        <span className="spacer" />
        {response && <SaveExampleButton docKey={docKey} response={response} />}
        <button type="button" className="btn btn-sm btn-ghost" onClick={() => selectSpan(result.spanId)} title="Open the full live details, logs and raw events">
          Details
        </button>
      </div>
      {result.error && (
        <div className="pad">
          <div className="error-box">{result.error}</div>
        </div>
      )}
      {response ? (
        <>
          <Tabs
            label="Response"
            idPrefix="response"
            active={tab}
            onChange={setTab}
            tabs={[
              { id: "body", label: "Body" },
              { id: "headers", label: "Headers", count: response.headers.length },
              { id: "tests", label: "Tests", count: result.tests.length ? `${result.tests.length - failedTests}/${result.tests.length}` : 0, tone: failedTests ? "bad" : "good" },
              { id: "timeline", label: "Timeline" },
              { id: "cookies", label: "Cookies" },
            ]}
          />
          <TabPanel idPrefix="response" active={tab} className="response-body">
            {tab === "body" && (
              <PayloadView
                payload={response.body}
                contentType={headerValue(response.headers, "content-type") ?? null}
                viewer={{ status: response.status, statusText: response.statusText, headers: response.headers, timeMs: response.timeMs }}
              />
            )}
            {tab === "headers" && <HeaderTable headers={response.headers} />}
            {tab === "tests" && <TestList tests={result.tests} emptyText="No tests. Add one per line in the Tests tab, e.g. status == 200." />}
            {tab === "timeline" && (
              <div className="pad">
                <Waterfall
                  phases={timingsToPhases(response)}
                  totalMs={response.timeMs}
                  connection={span?.connection ?? null}
                  httpVersion={response.httpVersion}
                  remoteAddr={response.remoteAddr}
                />
              </div>
            )}
            {tab === "cookies" && <Cookies response={response} />}
          </TabPanel>
        </>
      ) : (
        !result.error && <EmptyState title="No response">The request ended without a response.</EmptyState>
      )}
    </div>
  );
}

/** Appends this response to the request's `examples` (used by the mock server); save to keep it. */
function SaveExampleButton({ docKey, response }: { docKey: string; response: ResponseView }) {
  const save = async () => {
    const name = await promptDialog({
      title: "Save as example",
      message: "Adds this response to the request's examples, which the mock server can serve. Save the request afterwards to write it to disk.",
      label: "Example name",
      initial: `${response.status} ${response.statusText}`.trim(),
      confirmLabel: "Add example",
      validate: (v) => (v.trim() ? null : "Enter a name."),
    });
    if (!name) return;
    let body = response.body.preview;
    if (response.body.truncated && response.body.blob) {
      try {
        body = await api.blobText(response.body.blob);
      } catch (e) {
        showError("Could not load the full body; the example would be incomplete", e);
        return;
      }
    }
    updateDraft(docKey, (r) =>
      addExample(r, { name: name.trim(), status: response.status, headers: response.headers.map(([key, value]) => ({ key, value })), body }),
    );
    showToast("success", "Example added", "Save the request (Cmd/Ctrl+S) to keep it.");
  };
  return (
    <button type="button" className="btn btn-sm btn-ghost" onClick={save} title="Save this response as an example of the request">
      Save as example
    </button>
  );
}

function timingsToPhases(response: ResponseView) {
  const t = response.timings;
  return {
    dns: t.dnsMs,
    connect: t.connectMs,
    ...(t.tlsMs !== null ? { tls: t.tlsMs } : {}),
    waiting: t.waitingMs,
    download: t.downloadMs,
  };
}

function Cookies({ response }: { response: ResponseView }) {
  const [jar, setJar] = useState<Cookie[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const setCookies = response.headers.filter(([name]) => name.toLowerCase() === "set-cookie");

  const mounted = useRef(true);
  const load = useCallback(() => {
    api
      .cookies()
      .then((cookies) => mounted.current && setJar(cookies))
      .catch((e: unknown) => mounted.current && setError(`Could not load the cookie jar: ${errorText(e)}`));
  }, []);
  useEffect(() => {
    mounted.current = true;
    load();
    return () => {
      mounted.current = false;
    };
  }, [load]);

  const clear = async () => {
    try {
      await api.clearCookies();
      load();
    } catch (e) {
      showError("Could not clear cookies", e);
    }
  };

  return (
    <div className="stack pad">
      <h3 className="section-title">Set by this response</h3>
      {setCookies.length === 0 ? <p className="muted">This response did not set any cookies.</p> : <HeaderTable headers={setCookies} />}
      <div className="row">
        <h3 className="section-title grow">Cookie jar (sent automatically with later requests)</h3>
        <button type="button" className="btn btn-sm" onClick={clear} disabled={!jar?.length}>
          Clear all
        </button>
      </div>
      {error && <p className="tone-danger">{error}</p>}
      {jar && jar.length === 0 && <p className="muted">The cookie jar is empty.</p>}
      {jar && jar.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>Domain</th>
              <th>Name</th>
              <th>Value</th>
            </tr>
          </thead>
          <tbody>
            {jar.map((c) => (
              <tr key={`${c.domain}-${c.name}`}>
                <td className="mono">{c.domain}</td>
                <td className="mono">{c.name}</td>
                <td className="mono">{c.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
