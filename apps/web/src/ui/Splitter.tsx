import { useCallback, useEffect, useState, type KeyboardEvent, type PointerEvent } from "react";
import { readLocal, writeLocal } from "../lib/storage";
import "./widgets.css";

type Props = {
  /** `vertical` = a vertical bar that resizes widths; `horizontal` resizes heights. */
  orientation: "vertical" | "horizontal";
  size: number;
  onResize: (size: number) => void;
  min: number;
  max: number;
  /** +1 when dragging right/down grows the panel, -1 when it shrinks it. */
  direction: 1 | -1;
  label: string;
};

/** A draggable, keyboard-accessible separator between two panes. */
export function Splitter({ orientation, size, onResize, min, max, direction, label }: Props) {
  const [drag, setDrag] = useState<{ start: number; size: number } | null>(null);
  const clamp = useCallback((v: number) => Math.round(Math.min(max, Math.max(min, v))), [min, max]);

  useEffect(() => {
    if (!drag) return;
    const move = (e: globalThis.PointerEvent) => {
      const pos = orientation === "vertical" ? e.clientX : e.clientY;
      onResize(clamp(drag.size + (pos - drag.start) * direction));
    };
    const up = () => setDrag(null);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    document.body.style.cursor = orientation === "vertical" ? "col-resize" : "row-resize";
    document.body.style.userSelect = "none";
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [drag, orientation, direction, onResize, clamp]);

  const onPointerDown = (e: PointerEvent) => {
    e.preventDefault();
    setDrag({ start: orientation === "vertical" ? e.clientX : e.clientY, size });
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 64 : 16;
    const grow = orientation === "vertical" ? { ArrowRight: 1, ArrowLeft: -1 } : { ArrowDown: 1, ArrowUp: -1 };
    const sign = grow[e.key as keyof typeof grow];
    if (sign === undefined) return;
    e.preventDefault();
    onResize(clamp(size + sign * step * direction));
  };

  return (
    <div
      className={`splitter splitter-${orientation}${drag ? " dragging" : ""}`}
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={size}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  );
}

/** A number persisted in localStorage (panel sizes). */
export function usePersistentNumber(key: string, initial: number): [number, (value: number) => void] {
  const [value, setValue] = useState(() => {
    const stored = Number(readLocal(key));
    return Number.isFinite(stored) && stored > 0 ? stored : initial;
  });
  const set = useCallback(
    (next: number) => {
      setValue(next);
      writeLocal(key, String(next));
    },
    [key],
  );
  return [value, set];
}
