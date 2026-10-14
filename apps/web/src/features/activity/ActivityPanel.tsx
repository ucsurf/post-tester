import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { plural } from "../../lib/format";
import { SOURCE_TYPES, type SourceType } from "../../lib/source";
import { liveConnection, liveStore, useConnectionStatus, useLiveState, useNow } from "../../live/live";
import {
  EMPTY_FILTER,
  childCounts,
  filterRoots,
  flattenRows,
  isFilterActive,
  spanProtocol,
  type ActivityFilter,
  type SpanStatus,
} from "../../live/selectors";
import type { LiveState } from "../../live/spans";
import { appStore, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon, IconButton } from "../../ui/Icon";
import { ActivityRow } from "./ActivityRow";
import "./activity.css";

const ROW_HEIGHT = 30;
const STATUSES: { id: SpanStatus; label: string }[] = [
  { id: "running", label: "Running" },
  { id: "ok", label: "OK" },
  { id: "failed", label: "Tests failed" },
  { id: "error", label: "Error" },
  { id: "cancelled", label: "Cancelled" },
];

type Props = {
  dock: "right" | "bottom";
  onToggleDock: () => void;
  onCollapse: () => void;
};

/** Everything happening, live: newest first, filterable, expandable into child spans. */
export function ActivityPanel({ dock, onToggleDock, onCollapse }: Props) {
  const live = useLiveState();
  const [frozen, setFrozen] = useState<LiveState | null>(null);
  const view = frozen ?? live;
  const [follow, setFollow] = useState(true);
  const [filter, setFilter] = useState<ActivityFilter>(EMPTY_FILTER);
  const [showFilters, setShowFilters] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [activeIndex, setActiveIndex] = useState(0);
  const selected = useStore(appStore, (s) => s.selectedSpan);
  const listRef = useRef<HTMLDivElement>(null);
  // Rows for operations that start after the panel mounted get a short highlight.
  const mountedAt = useRef(Date.now());

  const roots = useMemo(() => filterRoots(view, filter), [view, filter]);
  const rows = useMemo(() => flattenRows(view, roots, expanded), [view, roots, expanded]);
  const protocols = useMemo(() => [...new Set(view.roots.map((id) => view.spans.get(id)).filter((s) => s !== undefined).map(spanProtocol))].sort(), [view]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
    getItemKey: (index) => rows[index]?.id ?? index,
  });

  // Newest rows are inserted at the top. When not following, keep the rows the user is
  // looking at in place by shifting the scroll position by the number of new rows.
  const previousFirst = useRef<string | undefined>(undefined);
  useLayoutEffect(() => {
    const list = listRef.current;
    const first = rows[0]?.id;
    if (list && previousFirst.current && first !== previousFirst.current) {
      const shift = rows.findIndex((r) => r.id === previousFirst.current);
      if (follow) list.scrollTop = 0;
      else if (shift > 0) list.scrollTop += shift * ROW_HEIGHT;
    }
    previousFirst.current = first;
  }, [rows, follow]);

  const onScroll = () => {
    const top = listRef.current?.scrollTop ?? 0;
    if (follow && top > ROW_HEIGHT * 2) setFollow(false);
  };

  const toggleFollow = () => {
    const next = !follow;
    setFollow(next);
    if (next && listRef.current) listRef.current.scrollTop = 0;
  };

  const togglePause = () => setFrozen(frozen ? null : live);

  const toggleExpanded = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const onSelect = useCallback(
    (id: string) => {
      selectSpan(id);
      const index = rows.findIndex((r) => r.id === id);
      if (index >= 0) setActiveIndex(index);
    },
    [rows],
  );

  const onKeyDown = (e: KeyboardEvent) => {
    const row = rows[activeIndex];
    let next = activeIndex;
    if (e.key === "ArrowDown") next = Math.min(rows.length - 1, activeIndex + 1);
    else if (e.key === "ArrowUp") next = Math.max(0, activeIndex - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = rows.length - 1;
    else if (e.key === "Enter" && row) return selectSpan(row.id);
    else if (e.key === "ArrowRight" && row?.hasChildren && !expanded.has(row.id)) return toggleExpanded(row.id);
    else if (e.key === "ArrowLeft" && row && expanded.has(row.id)) return toggleExpanded(row.id);
    else return;
    e.preventDefault();
    setActiveIndex(next);
    virtualizer.scrollToIndex(next, { align: "auto" });
  };

  const newWhilePaused = frozen ? countNew(frozen, live) : 0;
  const filtered = isFilterActive(filter);
  const activeFilterCount = filter.sources.size + filter.protocols.size + filter.statuses.size + (filter.text.trim() ? 1 : 0);
  const items = virtualizer.getVirtualItems();
  const activeRow = rows[Math.min(activeIndex, rows.length - 1)];

  const now = Date.now();
  return (
    <section className="activity" aria-label="Live Activity">
      <header className="activity-header">
        <ConnectionTitle count={roots.length} />
        <span className="spacer" />
        <IconButton icon="filter" label={showFilters ? "Hide filters" : "Show filters"} aria-pressed={showFilters || filtered} onClick={() => setShowFilters(!showFilters)} />
        <IconButton icon={frozen ? "play" : "pause"} label={frozen ? "Resume live updates" : "Pause (freeze the list; events keep buffering)"} aria-pressed={Boolean(frozen)} onClick={togglePause} />
        <IconButton icon="arrowUp" label={follow ? "Following newest (click to stop)" : "Follow newest"} aria-pressed={follow} onClick={toggleFollow} />
        <IconButton icon="clear" label="Clear the list" onClick={() => liveStore.clear()} />
        <IconButton icon={dock === "right" ? "panelBottom" : "panelRight"} label={dock === "right" ? "Move to the bottom" : "Move to the right"} onClick={onToggleDock} />
        <IconButton icon="x" label="Hide Live Activity" onClick={onCollapse} />
      </header>

      {showFilters && <Filters filter={filter} onChange={setFilter} protocols={protocols} activeCount={activeFilterCount} />}
      <ConnectionBanner />
      {frozen && (
        <div className="activity-banner" role="status">
          <Icon name="pause" />
          <span className="grow">Paused{newWhilePaused > 0 ? ` · ${plural(newWhilePaused, "new operation")} waiting` : ""}. Events keep arriving in the background.</span>
          <button type="button" className="btn btn-sm" onClick={togglePause}>
            Resume
          </button>
        </div>
      )}

      {rows.length === 0 ? (
        filtered ? (
          <div className="empty">
            <h3>No activity matches the filters</h3>
            <button type="button" className="btn" onClick={() => setFilter(EMPTY_FILTER)}>
              Clear filters
            </button>
          </div>
        ) : (
          <div className="empty">
            <div className="empty-icon" aria-hidden="true">
              📡
            </div>
            <h3>Nothing has happened yet</h3>
            <p>Send a request, run a folder, or connect an AI agent. Everything appears here the moment it happens, with who started it.</p>
          </div>
        )
      ) : (
        <div
          className="activity-list"
          ref={listRef}
          onScroll={onScroll}
          role="listbox"
          aria-label="Operations, newest first"
          tabIndex={0}
          aria-activedescendant={activeRow ? `activity-${activeRow.id}` : undefined}
          onKeyDown={onKeyDown}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((item) => {
              const row = rows[item.index];
              const span = row ? view.spans.get(row.id) : undefined;
              if (!row || !span) return null;
              return (
                <ActivityRow
                  key={row.id}
                  domId={`activity-${row.id}`}
                  span={span}
                  depth={row.depth}
                  hasChildren={row.hasChildren}
                  expanded={expanded.has(row.id)}
                  selected={selected === row.id}
                  active={item.index === activeIndex}
                  isNew={span.startedAt > mountedAt.current && now - span.startedAt < 1500}
                  failedChildren={span.kind === "collection_run" ? failedCount(view, row.id) : 0}
                  style={{ top: item.start, height: ROW_HEIGHT }}
                  onSelect={onSelect}
                  onToggle={toggleExpanded}
                />
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
}

function failedCount(state: LiveState, id: string): number {
  const span = state.spans.get(id);
  if (!span) return 0;
  const counts = childCounts(state, span);
  return counts.failed + counts.error;
}

function countNew(frozen: LiveState, live: LiveState): number {
  const newest = frozen.roots[0];
  if (!newest) return live.roots.length;
  const index = live.roots.indexOf(newest);
  return index === -1 ? live.roots.length : index;
}

function ConnectionTitle({ count }: { count: number }) {
  const status = useConnectionStatus();
  const label = status.state === "open" ? "Connected, receiving events live" : status.state === "connecting" ? "Connecting…" : "Disconnected";
  return (
    <h2 className="activity-title">
      <span className="live-dot" data-state={status.state} title={label} />
      <span className="sr-only">{label}. </span>
      Live Activity
      {count > 0 && <span className="badge">{count}</span>}
    </h2>
  );
}

function ConnectionBanner() {
  const status = useConnectionStatus();
  const now = useNow(status.state === "closed", 500);
  if (status.state !== "closed" || status.attempts === 0) return null;
  const seconds = status.retryAt ? Math.max(0, Math.ceil((status.retryAt - now) / 1000)) : 0;
  return (
    <div className="activity-banner danger" role="alert">
      <Icon name="alert" />
      <span className="grow">Lost the live connection to `pt serve`. Retrying{seconds > 0 ? ` in ${seconds}s` : "…"}</span>
      <button type="button" className="btn btn-sm" onClick={() => liveConnection.retryNow()}>
        Retry now
      </button>
    </div>
  );
}

function Filters({
  filter,
  onChange,
  protocols,
  activeCount,
}: {
  filter: ActivityFilter;
  onChange: (f: ActivityFilter) => void;
  protocols: string[];
  activeCount: number;
}) {
  const toggle = <T,>(set: ReadonlySet<T>, value: T): Set<T> => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };
  return (
    <div className="activity-filters" role="group" aria-label="Filters">
      <input
        className="input grow"
        type="search"
        placeholder="Search title, URL, status…"
        aria-label="Search activity"
        value={filter.text}
        onChange={(e) => onChange({ ...filter, text: e.target.value })}
      />
      {activeCount > 0 && (
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => onChange(EMPTY_FILTER)}>
          Reset
        </button>
      )}
      <div className="row" style={{ flexWrap: "wrap", gap: 4, width: "100%" }}>
        {SOURCE_TYPES.map((s) => (
          <button
            key={s.type}
            type="button"
            className="chip"
            aria-pressed={filter.sources.has(s.type)}
            onClick={() => onChange({ ...filter, sources: toggle<SourceType>(filter.sources, s.type) })}
          >
            <span aria-hidden="true">{s.icon}</span>
            {s.label}
          </button>
        ))}
      </div>
      <div className="row" style={{ flexWrap: "wrap", gap: 4, width: "100%" }}>
        {STATUSES.map((s) => (
          <button key={s.id} type="button" className="chip" aria-pressed={filter.statuses.has(s.id)} onClick={() => onChange({ ...filter, statuses: toggle(filter.statuses, s.id) })}>
            {s.label}
          </button>
        ))}
        {protocols.map((p) => (
          <button key={p} type="button" className="chip mono" aria-pressed={filter.protocols.has(p)} onClick={() => onChange({ ...filter, protocols: toggle(filter.protocols, p) })}>
            {p}
          </button>
        ))}
      </div>
    </div>
  );
}
