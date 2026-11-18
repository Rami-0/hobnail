// What the shell remembers, under the same keys and value formats as the
// old UI (static/app.js, src/ui/shell.rs BOOT_JS / SHELL_JS), so both UIs
// agree while they live side by side:
//   localStorage   rh-side      "rail" | "open"   (raw string)
//   localStorage   rh-tree      {"<stack>": true}  folded stacks (JSON)
//   sessionStorage rh-tree-q    the sidebar filter text (raw string)
//   localStorage   rh-set-tab   the Settings tab used last (raw, read only here)
// Pure functions over a Storage-like object (prefs.test.ts).

export interface KV {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

export const SIDE_KEY = "rh-side";
export const TREE_KEY = "rh-tree";
export const TREE_Q_KEY = "rh-tree-q";
export const SET_TAB_KEY = "rh-set-tab";

function read(s: KV | null, k: string): string | null {
  try {
    return s?.getItem(k) ?? null;
  } catch {
    return null;
  }
}

function write(s: KV | null, k: string, v: string): void {
  try {
    s?.setItem(k, v);
  } catch {
    // Private mode or blocked storage: the choice lasts for this page.
  }
}

/** The wide-screen sidebar was collapsed to the rail. */
export function readRail(s: KV | null): boolean {
  return read(s, SIDE_KEY) === "rail";
}

export function writeRail(s: KV | null, rail: boolean): void {
  write(s, SIDE_KEY, rail ? "rail" : "open");
}

/** Folded stacks: names mapped to true (static/app.js `store.get("rh-tree", {})`). */
export function readFolded(s: KV | null): Record<string, boolean> {
  const v = read(s, TREE_KEY);
  if (!v) return {};
  try {
    const j: unknown = JSON.parse(v);
    if (typeof j !== "object" || j === null || Array.isArray(j)) return {};
    const out: Record<string, boolean> = {};
    for (const [k, x] of Object.entries(j)) if (x === true) out[k] = true;
    return out;
  } catch {
    return {};
  }
}

/** Fold or unfold one stack, keeping the others' state. */
export function writeFolded(s: KV | null, stack: string, folded: boolean): Record<string, boolean> {
  const c = readFolded(s);
  if (folded) c[stack] = true;
  else delete c[stack];
  write(s, TREE_KEY, JSON.stringify(c));
  return c;
}

export function readTreeQuery(s: KV | null): string {
  return read(s, TREE_Q_KEY) ?? "";
}

export function writeTreeQuery(s: KV | null, q: string): void {
  write(s, TREE_Q_KEY, q);
}

/** The old UI's Settings tabs (routes::SETTINGS); the link opens the one used last. */
export const SETTINGS_TABS = [
  "general",
  "containers",
  "bookmarks",
  "widgets",
  "notifications",
  "appearance",
  "history",
  "config",
  "security",
] as const;

export function settingsHref(s: KV | null): string {
  const t = read(s, SET_TAB_KEY);
  return t && (SETTINGS_TABS as readonly string[]).includes(t) ? `/settings/${t}` : "/settings";
}

export function local(): KV | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function session(): KV | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}
