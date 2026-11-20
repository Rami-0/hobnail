// Browser preferences for the shell and its Settings link.

import { isSettingsTab, type SettingsTab } from "./settings-tabs.ts";

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

/** Folded stacks: names mapped to true. */
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

export function settingsHref(s: KV | null, owner = false, scoped = false): string {
  const tab = read(s, SET_TAB_KEY);
  if (scoped) return tab === "integrations" ? "/settings/integrations" : "/settings/boards";
  return tab && isSettingsTab(tab) && (tab !== "users" || owner) ? `/settings/${tab}` : "/settings";
}

export function rememberSettingsTab(s: KV | null, tab: SettingsTab): void {
  write(s, SET_TAB_KEY, tab);
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
