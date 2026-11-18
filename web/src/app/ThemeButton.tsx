// The theme toggle in the top bar: one click per step through the same
// themes, in the same order, as the server-rendered UI (static/app.js), and
// a toast that says which theme it switched to (NAV-41, A11Y-11; a phone
// shows it too). A new click replaces the previous theme toast.
import { ContrastIcon, FlameIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { useSyncExternalStore, type ReactNode } from "react";

import { toast } from "../ui/toast.ts";
import { currentTheme, nextTheme, setTheme, subscribeTheme, THEME_LABEL, THEME_NAME, type Theme } from "./theme.ts";

const ICON: Record<Theme, ReactNode> = {
  auto: <MonitorIcon aria-hidden="true" />,
  dark: <MoonIcon aria-hidden="true" />,
  light: <SunIcon aria-hidden="true" />,
  ember: <FlameIcon aria-hidden="true" />,
  contrast: <ContrastIcon aria-hidden="true" />,
};

let gone: (() => void) | null = null;

/** Apply a theme, remember it and say so. */
export function chooseTheme(t: Theme): void {
  setTheme(t);
  gone?.();
  gone = toast(`Theme: ${THEME_LABEL[t]}`, { kind: "info", ms: 2500 });
}

export function ThemeButton() {
  const theme = useSyncExternalStore(subscribeTheme, currentTheme, currentTheme);
  const next = nextTheme(theme);
  return (
    <button
      type="button"
      className="icon-btn"
      data-testid="theme"
      aria-label={`Theme: ${THEME_NAME[theme]}. Click for ${THEME_NAME[next]}`}
      title={`Theme: ${THEME_NAME[theme]} (click for ${THEME_NAME[next]})`}
      onClick={() => chooseTheme(next)}
    >
      {ICON[theme]}
    </button>
  );
}
