import type { KeyValue } from "@post-tester/bindings";
import { IconButton } from "./Icon";
import "./widgets.css";

type Props = {
  items: readonly KeyValue[];
  onChange: (items: KeyValue[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /** Show the enable/disable checkbox column. */
  toggleable?: boolean;
  label: string;
  /** Mask values (e.g. secrets). */
  secretValues?: boolean;
};

/**
 * Editable key/value rows. An empty row is always shown at the end, so typing into it
 * adds an item; rows with an empty key and value are not stored.
 */
export function KeyValueEditor({ items, onChange, keyPlaceholder = "Key", valuePlaceholder = "Value", toggleable = true, label, secretValues }: Props) {
  const rows = [...items, { key: "", value: "" }];

  const update = (index: number, patch: Partial<KeyValue>) => {
    const next = rows.map((row, i) => (i === index ? { ...row, ...patch } : row));
    onChange(next.filter((row, i) => i < items.length || row.key !== "" || row.value !== "").map(clean));
  };

  const remove = (index: number) => onChange(items.filter((_, i) => i !== index).map(clean));

  return (
    <table className="kv" aria-label={label}>
      <tbody>
        {rows.map((row, index) => {
          const isNew = index === items.length;
          const enabled = row.enabled !== false;
          return (
            <tr key={index} className={!enabled ? "kv-disabled" : undefined}>
              {toggleable && (
                <td className="kv-toggle">
                  {!isNew && (
                    <input type="checkbox" checked={enabled} aria-label={`Enable ${row.key || "row"}`} onChange={(e) => update(index, { enabled: e.target.checked })} />
                  )}
                </td>
              )}
              <td>
                <input
                  className="kv-input"
                  value={row.key}
                  placeholder={keyPlaceholder}
                  aria-label={`${label} key ${index + 1}`}
                  onChange={(e) => update(index, { key: e.target.value })}
                />
              </td>
              <td>
                <input
                  className="kv-input"
                  type={secretValues ? "password" : "text"}
                  value={row.value}
                  placeholder={valuePlaceholder}
                  aria-label={`${label} value ${index + 1}`}
                  onChange={(e) => update(index, { value: e.target.value })}
                />
              </td>
              <td className="kv-actions">
                {!isNew && <IconButton icon="x" label={`Remove ${row.key || "row"}`} small onClick={() => remove(index)} />}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Stores `enabled` only when false, like the YAML files do. */
function clean(row: KeyValue): KeyValue {
  return row.enabled === false ? { key: row.key, value: row.value, enabled: false } : { key: row.key, value: row.value };
}

/** Converts between `Record<string, string>` maps and key/value rows. */
export function recordToRows(record: Record<string, string>): KeyValue[] {
  return Object.entries(record).map(([key, value]) => ({ key, value }));
}

export function rowsToRecord(rows: readonly KeyValue[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of rows) if (row.key !== "" || row.value !== "") out[row.key] = row.value;
  return out;
}
