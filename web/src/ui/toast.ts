// Toasts (DESIGN.md §11.8; static/ui.js HB.toast): one polite stack in the
// corner, at most three, newest last; an error ("bad") stays until closed,
// others leave after 4.5 s (8 s with an action). Never over an open dialog:
// a toast raised while one is open waits in a queue and shows when the last
// dialog closes. `toast()` can be called from anywhere (no hook); the
// <Toaster/> in the shell renders the stack.
//
// The state transitions are pure functions (toast.test.ts); the module
// keeps one store instance for the app.
import { openDialogs, subscribeDialogs } from "./layers.ts";

export type ToastKind = "ok" | "info" | "warn" | "bad";

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface ToastOptions {
  kind?: ToastKind;
  action?: ToastAction;
  /** How long it stays, in ms (default 4500, 8000 with an action; "bad" stays until closed). */
  ms?: number;
  /** Stays until closed. */
  sticky?: boolean;
}

export interface ToastItem {
  id: number;
  text: string;
  kind: ToastKind;
  action?: ToastAction;
  /** Milliseconds on screen, or null: until closed. */
  ms: number | null;
}

export interface ToastState {
  shown: ToastItem[];
  /** Raised while a dialog was open; shown when none is. */
  queued: ToastItem[];
}

export const MAX_TOASTS = 3;

export function toastMs(o: ToastOptions): number | null {
  if (o.kind === "bad" || o.sticky) return null;
  return o.ms ?? (o.action ? 8000 : 4500);
}

/** Add a toast: shown at once, or queued while `held` (a dialog is open). */
export function addToast(s: ToastState, t: ToastItem, held: boolean): ToastState {
  if (held) return { shown: s.shown, queued: [...s.queued, t] };
  return { shown: [...s.shown, t].slice(-MAX_TOASTS), queued: s.queued };
}

/** No dialog is open any more: the queue joins the stack (still three at most). */
export function releaseToasts(s: ToastState): ToastState {
  if (s.queued.length === 0) return s;
  return { shown: [...s.shown, ...s.queued].slice(-MAX_TOASTS), queued: [] };
}

export function dismissToast(s: ToastState, id: number): ToastState {
  const shown = s.shown.filter((t) => t.id !== id);
  const queued = s.queued.filter((t) => t.id !== id);
  if (shown.length === s.shown.length && queued.length === s.queued.length) return s;
  return { shown, queued };
}

// ---- the app's store ----

let state: ToastState = { shown: [], queued: [] };
let nextId = 1;
const listeners = new Set<() => void>();

function set(next: ToastState): void {
  if (next === state) return;
  state = next;
  for (const l of listeners) l();
}

export function getToasts(): ToastState {
  return state;
}

export function subscribeToasts(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

subscribeDialogs(() => {
  if (openDialogs() === 0) set(releaseToasts(state));
});

/** Show a toast; returns a function that removes it (shown or still queued). */
export function toast(text: string, o: ToastOptions = {}): () => void {
  const item: ToastItem = { id: nextId++, text, kind: o.kind ?? "ok", ms: toastMs(o) };
  if (o.action) item.action = o.action;
  set(addToast(state, item, openDialogs() > 0));
  return () => set(dismissToast(state, item.id));
}

export function dismiss(id: number): void {
  set(dismissToast(state, id));
}
