import type { Phase } from "@post-tester/bindings";
import { formatDuration } from "../../lib/format";
import { PHASE_LABELS, PHASES } from "../../live/selectors";
import type { ConnectionInfo } from "../../live/spans";
import "./activity.css";

type Props = {
  phases: Partial<Record<Phase, number>>;
  /** Total duration; defaults to the sum of the phases. */
  totalMs?: number | null;
  running?: boolean;
  connection?: ConnectionInfo | null;
  httpVersion?: string | null;
  remoteAddr?: string | null;
};

/** The timing waterfall: DNS → Connect → TLS → Waiting → Download, plus connection details. */
export function Waterfall({ phases, totalMs, running, connection, httpVersion, remoteAddr }: Props) {
  const measured = PHASES.filter((p) => phases[p] !== undefined);
  const sum = measured.reduce((acc, p) => acc + (phases[p] ?? 0), 0);
  const total = Math.max(totalMs ?? 0, sum, 0.001);
  let offset = 0;

  return (
    <div className="waterfall">
      {measured.length === 0 ? (
        <p className="muted">{running ? "Waiting for the first timing (DNS lookup)…" : "No connection timings were recorded for this operation."}</p>
      ) : (
        <div className="waterfall-rows" role="table" aria-label="Timing breakdown">
          {measured.map((phase) => {
            const value = phases[phase] ?? 0;
            const left = (offset / total) * 100;
            const width = Math.max((value / total) * 100, 0.5);
            offset += value;
            return (
              <div className="waterfall-row" role="row" key={phase}>
                <span role="cell" className="waterfall-label">
                  {PHASE_LABELS[phase]}
                </span>
                <span role="cell" className="waterfall-track">
                  <span className={`waterfall-bar phase-${phase}`} style={{ left: `${left}%`, width: `${width}%` }} />
                </span>
                <span role="cell" className="waterfall-value mono">
                  {formatDuration(value)}
                </span>
              </div>
            );
          })}
          <div className="waterfall-row waterfall-total" role="row">
            <span role="cell" className="waterfall-label">
              Total
            </span>
            <span role="cell" />
            <span role="cell" className="waterfall-value mono">
              {running ? "…" : formatDuration(totalMs ?? sum)}
            </span>
          </div>
        </div>
      )}
      <ConnectionDetails connection={connection ?? null} httpVersion={httpVersion ?? null} remoteAddr={remoteAddr ?? null} />
    </div>
  );
}

function ConnectionDetails({ connection, httpVersion, remoteAddr }: { connection: ConnectionInfo | null; httpVersion: string | null; remoteAddr: string | null }) {
  const address = connection?.remoteAddr || remoteAddr;
  const tls = connection?.tls;
  if (!address && !httpVersion && !tls) return null;
  return (
    <dl className="facts">
      {address && (
        <>
          <dt>Remote address</dt>
          <dd className="mono">{address}</dd>
        </>
      )}
      {httpVersion && (
        <>
          <dt>HTTP version</dt>
          <dd className="mono">{httpVersion}</dd>
        </>
      )}
      {tls && (
        <>
          <dt>TLS</dt>
          <dd className="mono">{tls.version}</dd>
          <dt>Cipher</dt>
          <dd className="mono">{tls.cipher}</dd>
          <dt>ALPN</dt>
          <dd className="mono">{tls.alpn ?? "—"}</dd>
        </>
      )}
    </dl>
  );
}

/** A thin stacked bar of the phases, for activity rows. */
export function MiniTiming({ phases, totalMs, running }: { phases: Partial<Record<Phase, number>>; totalMs: number | null; running: boolean }) {
  const sum = PHASES.reduce((acc, p) => acc + (phases[p] ?? 0), 0);
  const total = Math.max(totalMs ?? 0, sum);
  if (running && sum === 0) return <span className="mini-timing running" aria-hidden="true" />;
  if (total <= 0) return <span className="mini-timing empty" aria-hidden="true" />;
  return (
    <span className={`mini-timing${running ? " running" : ""}`} aria-hidden="true">
      {PHASES.map((p) => {
        const v = phases[p];
        if (!v) return null;
        return <span key={p} className={`phase-${p}`} style={{ width: `${(v / total) * 100}%` }} />;
      })}
    </span>
  );
}
