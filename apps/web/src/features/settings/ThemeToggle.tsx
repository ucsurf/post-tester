import { useEffect, useState } from "react";
import { readLocal, writeLocal } from "../../lib/storage";
import { IconButton } from "../../ui/Icon";

export type ThemeChoice = "system" | "light" | "dark";

const KEY = "post-tester.theme";
const ORDER: ThemeChoice[] = ["system", "light", "dark"];

export function readTheme(): ThemeChoice {
  const value = readLocal(KEY);
  return value === "light" || value === "dark" ? value : "system";
}

/** Applies the theme to `<html data-theme>`; "system" follows the OS setting. */
export function applyTheme(theme: ThemeChoice): void {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

/** Cycles System → Light → Dark; the choice is remembered in localStorage. */
export function ThemeToggle() {
  const [theme, setTheme] = useState<ThemeChoice>(readTheme);

  useEffect(() => {
    applyTheme(theme);
    writeLocal(KEY, theme === "system" ? null : theme);
  }, [theme]);

  const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length] as ThemeChoice;
  const icon = theme === "light" ? "sun" : theme === "dark" ? "moon" : "monitor";
  return <IconButton icon={icon} label={`Theme: ${theme} (switch to ${next})`} onClick={() => setTheme(next)} />;
}
