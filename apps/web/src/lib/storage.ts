/**
 * localStorage for per-browser preferences (theme, panel sizes, environment). Storage
 * can be unavailable (private mode, blocked site data); preferences then last for the
 * session only, so failures are ignored.
 */

export function readLocal(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Stores `value`, or removes the key when it is null. */
export function writeLocal(key: string, value: string | null): void {
  try {
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, value);
  } catch {
    // See above: not persisted.
  }
}
