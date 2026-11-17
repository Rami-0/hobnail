// The theme toggle in the top bar: one click per step through the same
// themes, in the same order, as the server-rendered UI (static/app.js).
import { ContrastIcon, FlameIcon, MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { useState, type ReactNode } from "react";

import { nextTheme, setTheme, THEME_LABEL, THEME_NAME, themeOf, type Theme } from "./theme.ts";

const ICON: Record<Theme, ReactNode> = {
  auto: <MonitorIcon aria-hidden="true" />,
  dark: <MoonIcon aria-hidden="true" />,
  light: <SunIcon aria-hidden="true" />,
  ember: <FlameIcon aria-hidden="true" />,
  contrast: <ContrastIcon aria-hidden="true" />,
};

export function ThemeButton({ announce }: { announce: (text: string) => void }) {
  const [theme, set] = useState<Theme>(() => themeOf(document.documentElement.dataset.theme));
  const next = nextTheme(theme);
  return (
    <button
      type="button"
      className="icon-btn"
      data-testid="theme"
      aria-label={`Theme: ${THEME_NAME[theme]}. Click for ${THEME_NAME[next]}`}
      title={`Theme: ${THEME_NAME[theme]} (click for ${THEME_NAME[next]})`}
      onClick={() => {
        setTheme(next);
        set(next);
        announce(`Theme: ${THEME_LABEL[next]}`);
      }}
    >
      {ICON[theme]}
    </button>
  );
}
