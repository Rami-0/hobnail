// The global keyboard map as a pure
// function of the key press (keys.test.ts). Off while typing in a text
// field and with Ctrl, ⌘ or Alt held; `g` then a letter within 1.2 s jumps
// to a page.
//
// The command palette also toggles while its own input has focus.

export type KeyAction =
  | { kind: "go"; page: GoPage }
  | { kind: "slash" }
  | { kind: "help" }
  | { kind: "palette" }
  | { kind: "row"; dir: 1 | -1 }
  | { kind: "logs" };

export type GoPage = "overview" | "containers" | "host" | "logs" | "history" | "alerts" | "settings";

/** `g` + this key. */
export const GO: Record<string, GoPage> = {
  o: "overview",
  c: "containers",
  h: "host",
  l: "logs",
  y: "history",
  a: "alerts",
  s: "settings",
};

export const G_WINDOW_MS = 1200;

export interface KeyPress {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

export interface KeyState {
  /** When `g` was pressed (ms), 0 for not pending. */
  gAt: number;
}

/**
 * What a key press does, and the next state. `typing`: focus is in a field
 * that takes text; `blocked`: a dialog or the
 * drawer is open (they handle their own keys).
 */
export function keyAction(
  e: KeyPress,
  st: KeyState,
  now: number,
  typing: boolean,
  blocked: boolean,
): { action: KeyAction | null; state: KeyState } {
  if (!blocked && !e.altKey && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    return { action: { kind: "palette" }, state: { gAt: 0 } };
  }
  if (typing || blocked || e.ctrlKey || e.metaKey || e.altKey) return { action: null, state: { gAt: 0 } };
  const k = e.key;
  if (st.gAt && now - st.gAt < G_WINDOW_MS && GO[k]) return { action: { kind: "go", page: GO[k] }, state: { gAt: 0 } };
  if (k === "g") return { action: null, state: { gAt: now } };
  const none = { gAt: 0 };
  switch (k) {
    case "/":
      return { action: { kind: "slash" }, state: none };
    case "?":
      return { action: { kind: "help" }, state: none };
    case "j":
      return { action: { kind: "row", dir: 1 }, state: none };
    case "k":
      return { action: { kind: "row", dir: -1 }, state: none };
    case "l":
      return { action: { kind: "logs" }, state: none };
    default:
      return { action: null, state: none };
  }
}

/** Input types that do not take text: shortcuts stay on there (NAV-64). */
const NON_TEXT = /^(checkbox|radio|button|submit|reset|range|color|file|image)$/;

/** Whether focus is in a field that takes text (an element's tag, type and contenteditable). */
export function isTyping(tag: string, type: string, editable: boolean): boolean {
  if (editable) return true;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  return tag === "INPUT" && !NON_TEXT.test(type);
}

/** Next row index for j/k from `cur` (-1: none yet) among `n` rows. */
export function moveRow(cur: number, dir: 1 | -1, n: number): number {
  if (n <= 0) return -1;
  if (cur < 0) return dir > 0 ? 0 : n - 1;
  return Math.max(0, Math.min(n - 1, cur + dir));
}

/** The modifier's name in the shortcuts sheet: ⌘ on a Mac, Ctrl elsewhere. */
export function modName(platform: string): string {
  return /Mac|iPhone|iPad/.test(platform) ? "⌘" : "Ctrl";
}

/** Page-local log bindings, also rendered in the shortcuts sheet. */
export const LOG_KEYS: ReadonlyArray<{ keys: string[]; description: string }> = [
  { keys: ["↵"], description: "Logs list: open the first match" },
  { keys: ["/"], description: "Logs: focus search" },
  { keys: ["↵"], description: "Log search: older match" },
  { keys: ["Shift", "↵"], description: "Log search: newer match" },
  { keys: ["Esc"], description: "Log search: clear" },
  { keys: ["End"], description: "Logs: jump to latest" },
  { keys: ["↵"], description: "Log pane: select a line and focus Copy" },
  { keys: ["↑"], description: "Log actions: previous line" },
  { keys: ["↓"], description: "Log actions: next line" },
  { keys: ["Esc"], description: "Log actions: return to pane" },
];
