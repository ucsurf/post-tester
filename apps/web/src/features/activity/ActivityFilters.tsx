import { SOURCE_TYPES, type SourceType } from "../../lib/source";
import { EMPTY_FILTER, type ActivityFilter, type SpanStatus } from "../../live/selectors";

const STATUSES: { id: SpanStatus; label: string }[] = [
  { id: "running", label: "Running" },
  { id: "ok", label: "OK" },
  { id: "failed", label: "Tests failed" },
  { id: "error", label: "Error" },
  { id: "cancelled", label: "Cancelled" },
];

/** The Live Activity filter bar: text search plus source, status and protocol chips. */
export function ActivityFilters({
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
