// Theme selection and persistence.
//
// First paint. index.html carries no inline script (CSP `script-src 'self'`),
// so nothing can read localStorage before the browser first paints: with no
// `data-theme` attribute the CSS follows the OS (dark, or light under
// `prefers-color-scheme: light`). The entry module (main.tsx) calls
// `applyStoredTheme()` before the first React render. Limit: a module script
// is deferred, so a browser may paint the (still empty) document once in the
// OS theme before the module runs; with a stored theme that differs from the
// OS one (ember, contrast, or dark/light against the OS), that first frame
// shows the OS background colour. There is no content in it to restyle.
// The owner's default (`[page] theme`) comes with /api/v1/bootstrap and is
// applied then for a browser that has not chosen (applyDefaultTheme); until
// it arrives such a browser shows auto.

export const THEMES = ["auto", "dark", "light", "ember", "contrast"] as const;
export type Theme = (typeof THEMES)[number];

/** The localStorage key for the selected theme. */
export const THEME_KEY = "rh-theme";

/** Words for the theme button's label. */
export const THEME_NAME: Record<Theme, string> = {
  auto: "auto (follows the system)",
  dark: "dark",
  light: "light",
  ember: "ember (warm)",
  contrast: "high contrast",
};

/** Words for the announcement after a change. */
export const THEME_LABEL: Record<Theme, string> = {
  auto: "Auto (follows the system)",
  dark: "Dark",
  light: "Light",
  ember: "Ember",
  contrast: "High contrast",
};

export function isTheme(v: unknown): v is Theme {
  return typeof v === "string" && (THEMES as readonly string[]).includes(v);
}

/** The theme after `t` in the click order (contrast wraps to auto). */
export function nextTheme(t: Theme): Theme {
  const i = THEMES.indexOf(t);
  return THEMES[(i + 1) % THEMES.length] ?? "auto";
}

/** The part of `Storage` used here (tests pass a Map-backed fake). */
export interface ThemeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** The stored theme, or null (nothing stored, unknown value, storage blocked). */
export function readTheme(storage: ThemeStorage | null): Theme | null {
  try {
    const v = storage?.getItem(THEME_KEY) ?? null;
    return isTheme(v) ? v : null;
  } catch {
    return null;
  }
}

export function writeTheme(storage: ThemeStorage | null, t: Theme): void {
  try {
    storage?.setItem(THEME_KEY, t);
  } catch {
    // Private mode or blocked storage: the choice lasts for this page only.
  }
}

/** The theme a `data-theme` value stands for: no attribute means auto. */
export function themeOf(attr: string | null | undefined): Theme {
  return isTheme(attr) ? attr : "auto";
}

function browserStorage(): ThemeStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * Put `t` on `<html data-theme>` and point the browser chrome colour
 * (`<meta name="theme-color">`, phone status bar, installed-app title bar)
 * at the theme's page background.
 */
const themeListeners = new Set<() => void>();

/** Re-render on a theme change (the theme button). */
export function subscribeTheme(fn: () => void): () => void {
  themeListeners.add(fn);
  return () => {
    themeListeners.delete(fn);
  };
}

/** The theme on the page now. */
export function currentTheme(): Theme {
  return themeOf(document.documentElement.dataset.theme);
}

export function applyTheme(t: Theme): void {
  const root = document.documentElement;
  root.dataset.theme = t;
  for (const l of themeListeners) l();
  const meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (meta) meta.content = getComputedStyle(root).getPropertyValue("--bg").trim() || "#1D1210";
}

/** Entry module, before the first render: the stored theme, if any. */
export function applyStoredTheme(): Theme {
  const t = readTheme(browserStorage());
  if (t) applyTheme(t);
  return themeOf(document.documentElement.dataset.theme);
}

/** The theme button: apply and remember. */
export function setTheme(t: Theme): void {
  applyTheme(t);
  writeTheme(browserStorage(), t);
}

/**
 * The owner's default (`[page] theme`, from /api/v1/bootstrap) for a browser
 * that has not chosen one; a stored choice wins (theme per browser over
 * the config default). True when it changed the page.
 */
export function applyDefaultTheme(t: Theme): boolean {
  if (readTheme(browserStorage())) return false;
  if (themeOf(document.documentElement.dataset.theme) === t) return false;
  applyTheme(t);
  return true;
}
