import { memo, useState } from "react";
import "./widgets.css";

const MAX_CHILDREN = 500;

/** A collapsible JSON viewer. Children render only when expanded, so large bodies stay fast. */
export function JsonTree({ value, expandDepth = 2 }: { value: unknown; expandDepth?: number }) {
  return (
    <div className="json-tree" role="tree" aria-label="JSON body">
      <JsonNode name={null} value={value} depth={0} expandDepth={expandDepth} last />
    </div>
  );
}

type NodeProps = { name: string | null; value: unknown; depth: number; expandDepth: number; last: boolean };

const JsonNode = memo(function JsonNode({ name, value, depth, expandDepth, last }: NodeProps) {
  const [open, setOpen] = useState(depth < expandDepth);
  const comma = last ? "" : ",";
  const label = name !== null && <span className="json-key">{JSON.stringify(name)}: </span>;

  if (value === null || typeof value !== "object") {
    return (
      <div className="json-line" role="treeitem" aria-selected={false} style={{ paddingLeft: depth * 14 + 14 }}>
        {label}
        <Primitive value={value} />
        {comma}
      </div>
    );
  }

  const isArray = Array.isArray(value);
  const entries: [string, unknown][] = isArray ? value.map((v, i) => [String(i), v]) : Object.entries(value as Record<string, unknown>);
  const [openBracket, closeBracket] = isArray ? ["[", "]"] : ["{", "}"];

  if (entries.length === 0) {
    return (
      <div className="json-line" role="treeitem" aria-selected={false} style={{ paddingLeft: depth * 14 + 14 }}>
        {label}
        <span className="json-punct">
          {openBracket}
          {closeBracket}
        </span>
        {comma}
      </div>
    );
  }

  const shown = entries.slice(0, MAX_CHILDREN);
  return (
    <div role="treeitem" aria-expanded={open} aria-selected={false}>
      <button type="button" className="json-line json-toggle" style={{ paddingLeft: depth * 14 }} onClick={() => setOpen(!open)}>
        <span className="json-caret" aria-hidden="true">
          {open ? "▾" : "▸"}
        </span>
        {label}
        <span className="json-punct">{openBracket}</span>
        {!open && (
          <>
            <span className="json-summary">{isArray ? `${entries.length} items` : `${entries.length} keys`}</span>
            <span className="json-punct">{closeBracket}</span>
            {comma}
          </>
        )}
      </button>
      {open && (
        <div role="group">
          {shown.map(([key, child], i) => (
            <JsonNode key={key} name={isArray ? null : key} value={child} depth={depth + 1} expandDepth={expandDepth} last={i === entries.length - 1} />
          ))}
          {entries.length > MAX_CHILDREN && (
            <div className="json-line faint" style={{ paddingLeft: (depth + 1) * 14 + 14 }}>
              … {entries.length - MAX_CHILDREN} more (use Raw to see everything)
            </div>
          )}
          <div className="json-line" style={{ paddingLeft: depth * 14 + 14 }}>
            <span className="json-punct">{closeBracket}</span>
            {comma}
          </div>
        </div>
      )}
    </div>
  );
});

function Primitive({ value }: { value: unknown }) {
  if (typeof value === "string") return <span className="json-string">{JSON.stringify(value)}</span>;
  if (typeof value === "number") return <span className="json-number">{String(value)}</span>;
  if (typeof value === "boolean") return <span className="json-bool">{String(value)}</span>;
  return <span className="json-null">null</span>;
}
