/** The modifier key shown in shortcut hints: ⌘ on Apple platforms, Ctrl elsewhere. */
export function modKey(): string {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";
}
