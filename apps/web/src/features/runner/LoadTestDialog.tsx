import { useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { LoadReport } from "../../api/types";
import { formatDuration } from "../../lib/format";
import { useLiveState } from "../../live/live";
import { findRecentRoot } from "../../live/selectors";
import { appStore, closeDialog, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { showError } from "../../state/toasts";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";
import { ProgressBar, StatusBadge } from "../../ui/widgets";
import { RunningTime } from "../activity/ActivityRow";
import "./runner.css";

/** Sends the current request many times in parallel and reports latency percentiles. */
export function LoadTestDialog({ requestKey }: { requestKey: string }) {
  const doc = useStore(appStore, (s) => s.requests[requestKey]);
  const [concurrency, setConcurrency] = useState(10);
  const [total, setTotal] = useState(100);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [report, setReport] = useState<LoadReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!doc) return null;
  const running = startedAt !== null && !report && !error;

  const start = async () => {
    setStartedAt(Date.now());
    setReport(null);
    setError(null);
    try {
      setReport(
        await api.loadTest({
          request: doc.draft,
          ...(doc.id ? { requestId: doc.id } : {}),
          environment: appStore.get().environment,
          concurrency,
          requests: total,
        }),
      );
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <Modal
      title={`Load test: ${doc.draft.name || doc.id || doc.draft.url || "request"}`}
      width={640}
      onClose={closeDialog}
      footer={
        <>
          <button type="button" className="btn" onClick={closeDialog}>
            Close
          </button>
          <button type="button" className="btn btn-primary" onClick={start} disabled={running || !doc.draft.url}>
            <Icon name="zap" size={12} /> {report ? "Run again" : "Start"}
          </button>
        </>
      }
    >
      <div className="stack">
        <p className="muted">Sends the request (as currently edited) many times. Individual requests are not listed in Live Activity; the load test appears there as one operation.</p>
        <div className="runner-form">
          <label className="field">
            <span>Concurrent workers</span>
            <input className="input" type="number" min={1} max={1000} value={concurrency} onChange={(e) => setConcurrency(clamp(Number(e.target.value), 1, 1000))} disabled={running} />
          </label>
          <label className="field">
            <span>Total requests</span>
            <input className="input" type="number" min={1} max={1_000_000} value={total} onChange={(e) => setTotal(clamp(Number(e.target.value), 1, 1_000_000))} disabled={running} />
          </label>
        </div>
        {running && <LiveLoad since={startedAt} />}
        {error && <p className="error-box">{error}</p>}
        {report && <LoadSummary report={report} />}
      </div>
    </Modal>
  );
}

function clamp(value: number, min: number, max: number): number {
  return Number.isFinite(value) ? Math.min(max, Math.max(min, Math.round(value))) : min;
}

function LiveLoad({ since }: { since: number }) {
  const state = useLiveState();
  const span = findRecentRoot(state, since - 2000, (s) => s.summary.protocol === "load" && s.source.type === "user");
  if (!span) return <p className="muted">Starting…</p>;
  const progress = span.progress;
  const cancel = async () => {
    try {
      await api.cancel(span.id);
    } catch (e) {
      showError("Could not stop the load test", e);
    }
  };
  return (
    <div className="row">
      <span className="grow">{progress ? <ProgressBar done={progress.done} total={progress.total} /> : <span className="spinner" />}</span>
      {progress && (
        <span className="mono">
          {progress.done}/{progress.total}
        </span>
      )}
      <span className="mono muted">
        <RunningTime startedAt={span.startedAt} />
      </span>
      <button type="button" className="btn btn-sm btn-danger" onClick={cancel}>
        <Icon name="stop" size={12} /> Stop
      </button>
    </div>
  );
}

function LoadSummary({ report }: { report: LoadReport }) {
  const stats: [string, string][] = [
    ["Requests", String(report.requests)],
    ["Succeeded", String(report.succeeded)],
    ["Failed", String(report.failed)],
    ["Duration", formatDuration(report.duration_ms)],
    ["Throughput", `${report.requests_per_second.toFixed(1)} req/s`],
    ["Min", formatDuration(report.min_ms)],
    ["Mean", formatDuration(report.mean_ms)],
    ["p50", formatDuration(report.p50_ms)],
    ["p90", formatDuration(report.p90_ms)],
    ["p99", formatDuration(report.p99_ms)],
    ["Max", formatDuration(report.max_ms)],
  ];
  const codes = Object.entries(report.status_codes).sort(([a], [b]) => Number(a) - Number(b));
  return (
    <div className="stack">
      <div className="stat-grid">
        {stats.map(([label, value]) => (
          <div key={label} className={`stat${label === "Failed" && report.failed > 0 ? " bad" : ""}`}>
            <span className="stat-label">{label}</span>
            <span className="stat-value">{value}</span>
          </div>
        ))}
      </div>
      {codes.length > 0 && (
        <div className="row" style={{ flexWrap: "wrap" }}>
          <span className="label">Status codes</span>
          {codes.map(([code, count]) => (
            <span key={code} className="row" style={{ gap: 4 }}>
              <StatusBadge status={Number(code)} /> × {count}
            </span>
          ))}
        </div>
      )}
      {report.span_id && (
        <div>
          <button type="button" className="btn btn-sm" onClick={() => report.span_id && selectSpan(report.span_id)}>
            Open in Live Activity
          </button>
        </div>
      )}
    </div>
  );
}
