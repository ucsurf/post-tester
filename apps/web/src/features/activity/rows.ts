import { childCounts, type ActivityRow } from "../../live/selectors";
import type { LiveState, SpanState } from "../../live/spans";

/** The data props of one Live Activity row (callbacks and position are added by the list). */
export type RowData = {
  span: SpanState;
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
  selected: boolean;
  active: boolean;
  isNew: boolean;
  failedChildren: number;
};

export type RowContext = {
  expanded: ReadonlySet<string>;
  selected: string | null;
  activeId: string | null;
  /** Rows for operations that started after this moment (and recently) get a highlight. */
  mountedAt: number;
  now: number;
};

/**
 * Builds a row's props from plain values only. Unchanged spans keep their object
 * identity between batches, so a memoized row re-renders only when its own span (or
 * its selection/expansion) changes, not on every batch.
 */
export function rowData(state: LiveState, row: ActivityRow, ctx: RowContext): RowData | null {
  const span = state.spans.get(row.id);
  if (!span) return null;
  return {
    span,
    depth: row.depth,
    hasChildren: row.hasChildren,
    expanded: ctx.expanded.has(row.id),
    selected: ctx.selected === row.id,
    active: ctx.activeId === row.id,
    isNew: span.startedAt > ctx.mountedAt && ctx.now - span.startedAt < 1500,
    failedChildren: span.kind === "collection_run" ? failedCount(state, span) : 0,
  };
}

function failedCount(state: LiveState, span: SpanState): number {
  const counts = childCounts(state, span);
  return counts.failed + counts.error;
}

/** Shallow equality of row props, the same check `React.memo` does. */
export function sameRowData(a: RowData, b: RowData): boolean {
  return (Object.keys(a) as (keyof RowData)[]).every((key) => Object.is(a[key], b[key]));
}
