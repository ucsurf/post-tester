import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { plural } from "../../lib/format";
import { liveConnection, liveStore, useConnectionStatus, useLiveState, useNow } from "../../live/live";
import {
  EMPTY_FILTER,
  filterRoots,
  flattenRows,
  isFilterActive,
  spanProtocol,
  type ActivityFilter,
} from "../../live/selectors";
import type { LiveState } from "../../live/spans";
import { appStore, selectSpan } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon, IconButton } from "../../ui/Icon";
import { ActivityFilters as Filters } from "./ActivityFilters";
import { ActivityRow, activityRowId } from "./ActivityRow";
import { rowData, type RowContext } from "./rows";
import "./activity.css";

const ROW_HEIGHT = 30;

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
  const [activeId, setActiveId] = useState<string | null>(null);
  const selected = useStore(appStore, (s) => s.selectedSpan);
  const listRef = useRef<HTMLDivElement>(null);
  // Rows for operations that start after the panel mounted get a short highlight.
  const mountedAt = useRef(Date.now());

  const roots = useMemo(() => filterRoots(view, filter), [view, filter]);
  const rows = useMemo(() => flattenRows(view, roots, expanded), [view, roots, expanded]);
  // Only needed (and only recomputed per batch) while the filter bar is open.
  const protocols = useMemo(
    () => (showFilters ? [...new Set(view.roots.map((id) => view.spans.get(id)).filter((s) => s !== undefined).map(spanProtocol))].sort() : []),
    [view, showFilters],
  );

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

  // Stable, so memoized rows do not re-render when the list changes.
  const onSelect = useCallback((id: string) => {
    selectSpan(id);
    setActiveId(id);
  }, []);

  // The keyboard cursor follows its operation as new rows arrive above it.
  const foundIndex = activeId === null ? -1 : rows.findIndex((r) => r.id === activeId);
  const activeIndex = foundIndex === -1 ? 0 : foundIndex;
  const activeRow = rows[activeIndex];

  const onKeyDown = (e: KeyboardEvent) => {
    const row = activeRow;
    let next = activeIndex;
    if (e.key === "ArrowDown") next = Math.min(rows.length - 1, activeIndex + 1);
    else if (e.key === "ArrowUp") next = Math.max(0, activeIndex - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = rows.length - 1;
    else if (e.key === "PageDown") next = Math.min(rows.length - 1, activeIndex + pageSize());
    else if (e.key === "PageUp") next = Math.max(0, activeIndex - pageSize());
    else if ((e.key === "Enter" || e.key === " ") && row) {
      e.preventDefault();
      return onSelect(row.id);
    } else if (e.key === "ArrowRight" && row?.hasChildren && !expanded.has(row.id)) {
      e.preventDefault();
      return toggleExpanded(row.id);
    } else if (e.key === "ArrowRight" && row?.hasChildren) next = Math.min(rows.length - 1, activeIndex + 1);
    else if (e.key === "ArrowLeft" && row && expanded.has(row.id)) return toggleExpanded(row.id);
    else if (e.key === "ArrowLeft" && row && row.depth > 0) {
      // Jump to the parent row.
      const parent = view.spans.get(row.id)?.parentId;
      const index = parent ? rows.findIndex((r) => r.id === parent) : -1;
      if (index === -1) return;
      next = index;
    } else return;
    e.preventDefault();
    setActiveId(rows[next]?.id ?? null);
    virtualizer.scrollToIndex(next, { align: "auto" });
  };
  const pageSize = () => Math.max(1, Math.floor((listRef.current?.clientHeight ?? ROW_HEIGHT * 10) / ROW_HEIGHT) - 1);

  const newWhilePaused = frozen ? countNew(frozen, live) : 0;
  const filtered = isFilterActive(filter);
  const activeFilterCount = filter.sources.size + filter.protocols.size + filter.statuses.size + (filter.text.trim() ? 1 : 0);
  const items = virtualizer.getVirtualItems();
  const rowContext: RowContext = { expanded, selected, activeId: activeRow?.id ?? null, mountedAt: mountedAt.current, now: Date.now() };
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
          role="tree"
          aria-label="Operations, newest first"
          tabIndex={0}
          aria-activedescendant={activeRow ? activityRowId(activeRow.id) : undefined}
          onKeyDown={onKeyDown}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((item) => {
              const row = rows[item.index];
              const data = row ? rowData(view, row, rowContext) : null;
              if (!row || !data) return null;
              return <ActivityRow key={row.id} {...data} top={item.start} height={ROW_HEIGHT} onSelect={onSelect} onToggle={toggleExpanded} />;
            })}
          </div>
        </div>
      )}
    </section>
  );
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
