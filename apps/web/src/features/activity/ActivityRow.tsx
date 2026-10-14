import { memo, type CSSProperties } from "react";
import { formatDuration, formatSize, statusTone } from "../../lib/format";
import { spanDisplayTitle, spanStatus } from "../../live/selectors";
import type { SpanState } from "../../live/spans";
import { useNow } from "../../live/live";
import { Icon } from "../../ui/Icon";
import { MethodBadge, OutcomeIcon, ProgressBar, SourceBadge } from "../../ui/widgets";
import { MiniTiming } from "./Waterfall";

type Props = {
  span: SpanState;
  domId: string;
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
  selected: boolean;
  active: boolean;
  isNew: boolean;
  failedChildren: number;
  style: CSSProperties;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
};

/** One line of Live Activity: who, what, status, timing, size, outcome. */
export const ActivityRow = memo(function ActivityRow({
  span,
  domId,
  depth,
  hasChildren,
  expanded,
  selected,
  active,
  isNew,
  failedChildren,
  style,
  onSelect,
  onToggle,
}: Props) {
  const status = spanStatus(span);
  const running = status === "running";
  const method = span.summary.method ?? span.request?.method;
  const title = spanDisplayTitle(span);
  const progress = span.progress;

  return (
    <div
      id={domId}
      role="option"
      aria-selected={selected}
      aria-expanded={hasChildren ? expanded : undefined}
      aria-level={depth + 1}
      className={`activity-row${running ? " running" : ""}${isNew ? " is-new" : ""}${active ? " active" : ""}`}
      style={{ ...style, ["--depth" as string]: depth }}
      onClick={() => onSelect(span.id)}
      title={span.summary.url && span.summary.url !== title ? `${title}\n${span.summary.url}` : title}
    >
      {hasChildren ? (
        <button
          type="button"
          className="expander"
          tabIndex={-1}
          aria-label={expanded ? "Collapse" : "Expand"}
          onClick={(e) => {
            e.stopPropagation();
            onToggle(span.id);
          }}
        >
          <Icon name={expanded ? "chevronDown" : "chevronRight"} size={12} />
        </button>
      ) : (
        <span className="expander" aria-hidden="true" />
      )}
      {depth === 0 ? <SourceBadge source={span.source} /> : null}
      {span.kind === "collection_run" ? <Icon name="play" size={12} className="tone-info" /> : method ? <MethodBadge method={method} /> : null}
      <span className="row-title truncate">{title}</span>
      {progress && progress.total > 0 && (span.kind === "collection_run" || running) ? (
        <span className="row-progress">
          <ProgressBar done={progress.done} total={progress.total} failed={failedChildren} />
          <span>
            {progress.done}/{progress.total}
          </span>
        </span>
      ) : (
        <>
          <span className={`row-status tone-${statusTone(span.status)}`}>{span.status ?? (running ? "" : "—")}</span>
          <MiniTiming phases={span.phases} totalMs={span.durationMs} running={running} />
        </>
      )}
      <span className="row-duration">{running ? <RunningTime startedAt={span.startedAt} /> : formatDuration(span.durationMs)}</span>
      <span className="row-size">{span.size !== null ? formatSize(span.size) : ""}</span>
      <OutcomeIcon status={status} />
    </div>
  );
});

/** Ticks while the span runs; isolated so only this text re-renders. */
export function RunningTime({ startedAt }: { startedAt: number }) {
  const now = useNow(true, 100);
  return <>{startedAt > 0 ? formatDuration(Math.max(0, now - startedAt)) : "…"}</>;
}
