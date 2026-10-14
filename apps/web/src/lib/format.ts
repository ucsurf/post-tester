/** Formatting helpers for numbers shown across the UI. */

/** `3.2 ms`, `182 ms`, `1.42 s`, `2m 05s`, `1h 02m`. Returns `—` for missing values. */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 10) return `${trimZeros(ms.toFixed(1))} ms`;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const seconds = ms / 1000;
  if (seconds < 10) return `${seconds.toFixed(2)} s`;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const totalSeconds = Math.round(seconds);
  const minutes = Math.floor(totalSeconds / 60);
  if (minutes < 60) return `${minutes}m ${pad2(totalSeconds % 60)}s`;
  return `${Math.floor(minutes / 60)}h ${pad2(minutes % 60)}m`;
}

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** `0 B`, `512 B`, `1.2 KB`, `12 KB`, `3.4 MB` (1 KB = 1024 bytes). */
export function formatSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  const text = value < 10 ? trimZeros(value.toFixed(1)) : String(Math.round(value));
  return `${text} ${UNITS[unit]}`;
}

/** Wall-clock time with milliseconds, e.g. `14:03:27.120`. */
export function formatClock(epochMs: number): string {
  if (!Number.isFinite(epochMs) || epochMs <= 0) return "—";
  const d = new Date(epochMs);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}.${String(d.getMilliseconds()).padStart(3, "0")}`;
}

/** `1 request`, `3 requests`. */
export function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : pluralForm}`;
}

export type StatusTone = "neutral" | "info" | "success" | "redirect" | "warning" | "danger";

/** Color family of an HTTP status code. */
export function statusTone(status: number | null | undefined): StatusTone {
  if (status === null || status === undefined || status === 0) return "neutral";
  if (status < 200) return "info";
  if (status < 300) return "success";
  if (status < 400) return "redirect";
  if (status < 500) return "warning";
  return "danger";
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function trimZeros(text: string): string {
  return text.replace(/\.0$/, "");
}
