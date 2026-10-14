import type { ButtonHTMLAttributes } from "react";

/** A small inline SVG icon set (stroke icons, 16px grid), so no icon library is needed. */

const PATHS = {
  plus: "M8 3v10M3 8h10",
  x: "M4 4l8 8M12 4l-8 8",
  check: "M3 8.5l3 3 7-7",
  chevronRight: "M6 4l4 4-4 4",
  chevronDown: "M4 6l4 4 4-4",
  search: "M7 12A5 5 0 1 0 7 2a5 5 0 0 0 0 10zM14 14l-3.5-3.5",
  play: "M5 3.5v9l7.5-4.5z",
  pause: "M5.5 3.5v9M10.5 3.5v9",
  stop: "M4 4h8v8H4z",
  send: "M2.5 8h9M8 4l4 4-4 4",
  folder: "M2 4.5A1.5 1.5 0 0 1 3.5 3h3l1.5 1.5h4.5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z",
  import: "M8 2v8M5 7l3 3 3-3M3 12v1.5h10V12",
  trash: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 9h6l.5-9",
  copy: "M5.5 5.5h7v7h-7zM3.5 10.5v-7h7",
  more: "M3.5 8h.01M8 8h.01M12.5 8h.01",
  refresh: "M13 8a5 5 0 1 1-1.5-3.5M13 2.5V5h-2.5",
  settings: "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4",
  sun: "M8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM8 1v1.5M8 13.5V15M1 8h1.5M13.5 8H15M3 3l1 1M12 12l1 1M3 13l1-1M12 4l1-1",
  moon: "M13 9.5A5.5 5.5 0 0 1 6.5 3a5.5 5.5 0 1 0 6.5 6.5z",
  monitor: "M2 3h12v8H2zM6 14h4M8 11v3",
  globe: "M8 14.5A6.5 6.5 0 1 0 8 1.5a6.5 6.5 0 0 0 0 13zM1.5 8h13M8 1.5c2 2 2 11 0 13M8 1.5c-2 2-2 11 0 13",
  plug: "M6 2v3M10 2v3M4.5 5h7v2.5a3.5 3.5 0 0 1-7 0zM8 11v3",
  bot: "M3 6h10v7H3zM8 3v3M6 9.5h.01M10 9.5h.01M1.5 9v2M14.5 9v2",
  terminal: "M2.5 3h11v10h-11zM5 6.5l2 1.5-2 1.5M8.5 10H11",
  key: "M10 9a3 3 0 1 0-3-3c0 .5.1.9.3 1.3L2.5 12v1.5H5V12h1.5v-1.5H8l.7-.7c.4.1.8.2 1.3.2z",
  layers: "M8 2l6 3-6 3-6-3zM2 8l6 3 6-3M2 11l6 3 6-3",
  alert: "M8 2l6.5 11h-13zM8 6.5v3M8 11.5h.01",
  info: "M8 14.5A6.5 6.5 0 1 0 8 1.5a6.5 6.5 0 0 0 0 13zM8 7.5V11M8 5h.01",
  filter: "M2 3h12l-4.5 5.5V13l-3-1.5v-3z",
  panelRight: "M2 3h12v10H2zM10 3v10",
  panelBottom: "M2 3h12v10H2zM2 10h12",
  collapse: "M10 4l4 4-4 4M2 8h12",
  eye: "M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
  download: "M8 2v8M5 7l3 3 3-3M3 13h10",
  code: "M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5",
  zap: "M9 1.5L3 9h4.5L7 14.5 13 7H8.5z",
  link: "M7 9a3 3 0 0 0 4.2 0l2-2a3 3 0 0 0-4.2-4.2l-.7.7M9 7a3 3 0 0 0-4.2 0l-2 2a3 3 0 0 0 4.2 4.2l.7-.7",
  arrowDown: "M8 2.5v10M4 9l4 4 4-4",
  arrowUp: "M8 13.5v-10M4 7l4-4 4 4",
  clear: "M3 13h10M4.5 10.5L11 4M6 4h6v6",
  edit: "M10.5 2.5l3 3L6 13H3v-3z",
  question: "M8 14.5A6.5 6.5 0 1 0 8 1.5a6.5 6.5 0 0 0 0 13zM6.2 6.2a1.9 1.9 0 0 1 3.6.8c0 1.3-1.8 1.6-1.8 2.7M8 11.5h.01",
  list: "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01",
  lock: "M4 7h8v6.5H4zM5.5 7V5a2.5 2.5 0 0 1 5 0v2",
  activity: "M1.5 8.5h3l1.5-4 3 8 1.5-4h4",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 14, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

type IconButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon: IconName;
  label: string;
  small?: boolean;
};

/** An icon-only button. `label` becomes the accessible name and tooltip. */
export function IconButton({ icon, label, small, className, ...rest }: IconButtonProps) {
  return (
    <button type="button" aria-label={label} title={label} className={`icon-btn${small ? " icon-btn-sm" : ""} ${className ?? ""}`} {...rest}>
      <Icon name={icon} size={small ? 12 : 14} />
    </button>
  );
}
