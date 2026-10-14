import { useState } from "react";
import { api } from "../../api/client";
import { errorText } from "../../api/http";
import type { RunReport } from "../../api/types";
import { formatDuration, plural } from "../../lib/format";
import { useLiveState } from "../../live/live";
import { childCounts, findRecentRoot, spanDisplayTitle, spanStatus } from "../../live/selectors";
import { appStore, closeDialog, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { showError } from "../../state/toasts";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";
import { MethodBadge, OutcomeIcon, ProgressBar, StatusBadge, outcomeLabel } from "../../ui/widgets";
import { RunningTime } from "../activity/ActivityRow";
import "./runner.css";

type Props = { folder: string | null; requests: string[] };

/** Runs a folder (or chosen requests) and shows the run live, then a summary. */
export function RunnerDialog({ folder, requests }: Props) {
  const environments = useStore(appStore, (s) => s.environments);
  const current = useStore(appStore, (s) => s.environment);
  const [environment, setEnvironment] = useState<string | null>(current);
  const [iterations, setIterations] = useState(1);
  const [delayMs, setDelayMs] = useState(0);
  const [bail, setBail] = useState(false);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [report, setReport] = useState<RunReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const targetLabel = folder !== null ? (folder ? `folder ${folder}` : "all requests in the workspace") : plural(requests.length, "request");

  const start = async () => {
    setStartedAt(Date.now());
    setReport(null);
    setError(null);
    try {
      setReport(
        await api.run({
          target: folder !== null ? { folder } : { requests },
          environment,
          bail,
          iterations,
          delayMs,
        }),
      );
    } catch (e) {
      setError(errorText(e));
    }
  };

  const running = startedAt !== null && !report && !error;

  return (
    <Modal
      title={`Run ${targetLabel}`}
      width={760}
      onClose={closeDialog}
      footer={
        <>
          {running && <span className="hint grow">Closing this dialog does not stop the run; it stays visible in Live Activity.</span>}
          <button type="button" className="btn" onClick={closeDialog}>
            Close
          </button>
          <button type="button" className="btn btn-primary" onClick={start} disabled={running}>
            <Icon name="play" size={12} /> {report || error ? "Run again" : "Run"}
          </button>
        </>
      }
    >
      <div className="stack">
        <div className="runner-form">
          <label className="field">
            <span>Environment</span>
            <select className="select" value={environment ?? ""} onChange={(e) => setEnvironment(e.target.value || null)} disabled={running}>
              <option value="">No environment</option>
              {environments.map((name) => (
                <option key={name}>{name}</option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Iterations</span>
            <input className="input" type="number" min={1} max={1000} value={iterations} onChange={(e) => setIterations(Math.max(1, Number(e.target.value) || 1))} disabled={running} />
          </label>
          <label className="field">
            <span>Delay between requests (ms)</span>
            <input className="input" type="number" min={0} value={delayMs} onChange={(e) => setDelayMs(Math.max(0, Number(e.target.value) || 0))} disabled={running} />
          </label>
          <label className="checkbox">
            <input type="checkbox" checked={bail} onChange={(e) => setBail(e.target.checked)} disabled={running} />
            Stop at the first failure
          </label>
        </div>
        {startedAt !== null && !report && <LiveRun since={startedAt} />}
        {error && <p className="error-box">{error}</p>}
        {report && <RunSummary report={report} />}
      </div>
    </Modal>
  );
}

function LiveRun({ since }: { since: number }) {
  const state = useLiveState();
  const span = findRecentRoot(state, since - 2000, (s) => s.kind === "collection_run" && s.source.type === "user");
  if (!span) return <p className="muted">Starting the run…</p>;
  const counts = childCounts(state, span);
  const progress = span.progress ?? { done: 0, total: 0 };

  const cancel = async () => {
    try {
      await api.cancel(span.id);
    } catch (e) {
      showError("Could not cancel the run", e);
    }
  };

  return (
    <div className="stack">
      <div className="row">
        <span className="grow">
          <ProgressBar done={progress.done} total={progress.total} failed={counts.failed + counts.error} />
        </span>
        <span className="mono">
          {progress.done}/{progress.total}
        </span>
        <span className="mono muted">
          <RunningTime startedAt={span.startedAt} />
        </span>
        <button type="button" className="btn btn-sm btn-danger" onClick={cancel}>
          <Icon name="stop" size={12} /> Stop
        </button>
      </div>
      <div className="child-list runner-live">
        {span.children
          .slice()
          .reverse()
          .slice(0, 50)
          .map((id) => {
            const child = state.spans.get(id);
            if (!child) return null;
            return (
              <button key={id} type="button" onClick={() => selectSpan(id)}>
                <OutcomeIcon status={spanStatus(child)} />
                <MethodBadge method={child.summary.method} />
                <span className="truncate grow mono">{spanDisplayTitle(child)}</span>
                {child.status !== null && <StatusBadge status={child.status} />}
                <span className="mono faint">{child.outcome ? formatDuration(child.durationMs) : <RunningTime startedAt={child.startedAt} />}</span>
              </button>
            );
          })}
      </div>
    </div>
  );
}

function RunSummary({ report }: { report: RunReport }) {
  const count = (o: string) => report.items.filter((i) => i.outcome === o).length;
  const tests = report.items.flatMap((i) => i.tests);
  const failedTests = tests.filter((t) => !t.passed).length;
  return (
    <div className="stack">
      <div className="run-totals">
        <span className={`badge ${report.outcome === "ok" ? "tone-success" : "tone-danger"}`}>{outcomeLabel(report.outcome)}</span>
        <span>{plural(report.items.length, "request")}</span>
        <span className="tone-success">{count("ok")} ok</span>
        <span className="tone-warning">{count("failed")} failed</span>
        <span className="tone-danger">{count("error")} errors</span>
        {count("cancelled") > 0 && <span className="muted">{count("cancelled")} cancelled</span>}
        <span>
          Tests: {tests.length - failedTests}/{tests.length} passed
        </span>
        <span className="mono">{formatDuration(report.duration_ms)}</span>
        <span className="spacer" />
        <button type="button" className="btn btn-sm" onClick={() => selectSpan(report.span_id)}>
          Open in Live Activity
        </button>
      </div>
      <div className="runner-table">
        <table className="table">
          <thead>
            <tr>
              <th />
              <th>Request</th>
              <th>Status</th>
              <th>Time</th>
              <th>Tests</th>
            </tr>
          </thead>
          <tbody>
            {report.items.map((item, i) => {
              const failed = item.tests.filter((t) => !t.passed);
              return (
                <tr key={`${item.span_id}-${i}`} onClick={() => selectSpan(item.span_id)} className="clickable">
                  <td>
                    <OutcomeIcon status={item.outcome} />
                  </td>
                  <td>
                    <div>{item.name || item.request_id}</div>
                    {item.error && <div className="tone-danger hint">{item.error}</div>}
                    {failed.map((t) => (
                      <div key={t.name} className="tone-danger hint">
                        ✕ {t.name}
                        {t.detail ? `: ${t.detail}` : ""}
                      </div>
                    ))}
                  </td>
                  <td>{item.status !== null ? <StatusBadge status={item.status} /> : "—"}</td>
                  <td className="mono">{formatDuration(item.duration_ms)}</td>
                  <td className="mono">{item.tests.length ? `${item.tests.length - failed.length}/${item.tests.length}` : "—"}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
