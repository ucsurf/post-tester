import { useRef, type KeyboardEvent, type ReactNode } from "react";

export type TabItem<K extends string> = { id: K; label: ReactNode; count?: number | string; tone?: "good" | "bad"; hidden?: boolean };

type Props<K extends string> = {
  tabs: readonly TabItem<K>[];
  active: K;
  onChange: (id: K) => void;
  label: string;
  idPrefix: string;
  trailing?: ReactNode;
};

/** An accessible tab list (arrow keys move between tabs). Pair with `TabPanel`. */
export function Tabs<K extends string>({ tabs, active, onChange, label, idPrefix, trailing }: Props<K>) {
  const list = useRef<HTMLDivElement>(null);
  const visible = tabs.filter((t) => !t.hidden);

  const onKeyDown = (event: KeyboardEvent) => {
    const index = visible.findIndex((t) => t.id === active);
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % visible.length;
    else if (event.key === "ArrowLeft") next = (index - 1 + visible.length) % visible.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = visible.length - 1;
    const tab = visible[next];
    if (!tab) return;
    event.preventDefault();
    onChange(tab.id);
    list.current?.querySelector<HTMLButtonElement>(`#${CSS.escape(`${idPrefix}-tab-${tab.id}`)}`)?.focus();
  };

  return (
    <div className="tabs" role="tablist" aria-label={label} ref={list} onKeyDown={onKeyDown}>
      {visible.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          id={`${idPrefix}-tab-${tab.id}`}
          aria-selected={tab.id === active}
          aria-controls={`${idPrefix}-panel`}
          tabIndex={tab.id === active ? 0 : -1}
          className="tab"
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
          {tab.count !== undefined && tab.count !== 0 && <span className={`count ${tab.tone ?? ""}`}>{tab.count}</span>}
        </button>
      ))}
      {trailing && (
        <>
          <span className="spacer" />
          <div className="row tabs-trailing">{trailing}</div>
        </>
      )}
    </div>
  );
}

export function TabPanel({ idPrefix, active, children, className }: { idPrefix: string; active: string; children: ReactNode; className?: string }) {
  return (
    <div role="tabpanel" id={`${idPrefix}-panel`} aria-labelledby={`${idPrefix}-tab-${active}`} className={className}>
      {children}
    </div>
  );
}
