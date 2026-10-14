import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import "./overlays.css";

export type MenuItem = { id: string; label: ReactNode; hint?: ReactNode; danger?: boolean; disabled?: boolean; onSelect: () => void };

type Props = {
  /** Renders the trigger; spread `props` onto a button. */
  trigger: (props: { onClick: () => void; "aria-expanded": boolean; "aria-haspopup": "menu"; "aria-controls": string }) => ReactNode;
  items: readonly MenuItem[] | (() => readonly MenuItem[]);
  align?: "start" | "end";
  emptyText?: string;
  onOpen?: () => void;
};

/** A dropdown menu with keyboard support (arrows, Enter, Escape). */
export function Menu({ trigger, items, align = "start", emptyText = "Nothing here", onOpen }: Props) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const id = useId();
  const list = open ? (typeof items === "function" ? items() : items) : [];

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    root.current?.querySelector<HTMLButtonElement>("[role=menuitem]:not(:disabled)")?.focus();
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const onKeyDown = (e: KeyboardEvent) => {
    if (!open) return;
    const buttons = [...(root.current?.querySelectorAll<HTMLButtonElement>("[role=menuitem]:not(:disabled)") ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.stopPropagation();
      setOpen(false);
      root.current?.querySelector<HTMLButtonElement>("[aria-haspopup]")?.focus();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      buttons[(index + 1) % buttons.length]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      buttons[(index - 1 + buttons.length) % buttons.length]?.focus();
    }
  };

  return (
    <div className="menu-root" ref={root} onKeyDown={onKeyDown}>
      {trigger({
        onClick: () => {
          if (!open) onOpen?.();
          setOpen(!open);
        },
        "aria-expanded": open,
        "aria-haspopup": "menu",
        "aria-controls": id,
      })}
      {open && (
        <div className={`menu menu-${align}`} role="menu" id={id}>
          {list.length === 0 && <div className="menu-empty">{emptyText}</div>}
          {list.map((item) => (
            <button
              key={item.id}
              type="button"
              role="menuitem"
              className={`menu-item${item.danger ? " danger" : ""}`}
              disabled={item.disabled}
              onClick={() => {
                setOpen(false);
                // Back to the trigger first, so a dialog opened by the item returns focus there.
                root.current?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
                item.onSelect();
              }}
            >
              <span className="grow truncate">{item.label}</span>
              {item.hint && <span className="menu-hint">{item.hint}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A small "?" button that toggles a help card. */
export function HelpPopover({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);
  return (
    <div className="menu-root" ref={root} onKeyDown={(e) => e.key === "Escape" && setOpen(false)}>
      <button type="button" className="btn btn-ghost btn-sm" aria-expanded={open} onClick={() => setOpen(!open)}>
        {label}
      </button>
      {open && (
        <div className="menu menu-end popover" role="dialog" aria-label={label}>
          {children}
        </div>
      )}
    </div>
  );
}
