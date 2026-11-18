// The container page's time window and its links, pure (range.test.ts):
// `?range=` (a preset) or `?from=&to=` (a zoom), the API query for it, the
// tab links into the old UI that keep the range, and the "history starts N
// ago" note (src/ui/pages/container.rs `range_picker`, `history_note`).
export const RANGES = ["1h", "6h", "24h", "7d", "30d"] as const;
export type RangeName = (typeof RANGES)[number];

const SECS: Record<RangeName, number> = { "1h": 3600, "6h": 21600, "24h": 86400, "7d": 604800, "30d": 2592000 };

export function rangeSecs(r: string): number {
  return (SECS as Record<string, number>)[r] ?? SECS["6h"];
}

export function isRange(r: string | null): r is RangeName {
  return r !== null && (RANGES as readonly string[]).includes(r);
}

export type Window = { kind: "range"; range: RangeName } | { kind: "zoom"; from: number; to: number };

/** The window the URL asks for: a zoom when `from` < `to` are whole numbers, else the range (default 6h). */
export function parseWindow(search: string): Window {
  const p = new URLSearchParams(search);
  const from = p.get("from");
  const to = p.get("to");
  if (from !== null && to !== null && /^\d+$/.test(from) && /^\d+$/.test(to) && Number(from) < Number(to)) {
    return { kind: "zoom", from: Number(from), to: Number(to) };
  }
  const r = p.get("range");
  return { kind: "range", range: isRange(r) ? r : "6h" };
}

/** The query for GET /api/v1/containers/<name>. */
export function apiQuery(w: Window): string {
  return w.kind === "zoom" ? `from=${w.from}&to=${w.to}` : `range=${w.range}`;
}

/** `search` with `set` applied (null removes a key); "" or "?…". */
export function withParams(search: string, set: Record<string, string | null>): string {
  const p = new URLSearchParams(search);
  for (const [k, v] of Object.entries(set)) {
    if (v === null) p.delete(k);
    else p.set(k, v);
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

/** A preset link: the range replaces any zoom. */
export function rangeSearch(search: string, r: RangeName): string {
  // Always explicit, as the old range picker: every link names its range.
  return withParams(search, { range: r, from: null, to: null });
}

/** A zoom into [from, to] (the URL keeps it, so it can be shared). */
export function zoomSearch(search: string, from: number, to: number): string {
  return withParams(search, { range: null, from: String(from), to: String(to) });
}

/** "Reset zoom": back to the preset that served the zoomed window. */
export function resetSearch(search: string, servedBy: string): string {
  return withParams(search, { from: null, to: null, range: isRange(servedBy) && servedBy !== "6h" ? servedBy : null });
}

export interface Tab {
  id: "overview" | "logs" | "metrics" | "inspect" | "processes" | "events";
  label: string;
  href: string;
}

/** Keep the exact preset or zoom when moving between detail tabs. */
export function tabs(name: string, query: string): Tab[] {
  const root = `/app/containers/${encodeURIComponent(name)}`;
  const win = parseWindow(query);
  const parameters = win.kind === "range" && win.range === "6h" ? "" : apiQuery(win);
  return [
    { id: "overview", label: "Overview", href: root + (parameters ? `?${parameters}` : "") },
    { id: "logs", label: "Logs", href: `${root}/logs${parameters ? `?${parameters}` : ""}` },
    { id: "metrics", label: "Metrics", href: `${root}?tab=metrics${parameters ? `&${parameters}` : ""}` },
    { id: "inspect", label: "Inspect", href: `${root}?tab=inspect${parameters ? `&${parameters}` : ""}` },
    { id: "processes", label: "Processes", href: `${root}?tab=processes${parameters ? `&${parameters}` : ""}` },
    { id: "events", label: "Events", href: `${root}?tab=events${parameters ? `&${parameters}` : ""}` },
  ];
}

/**
 * A range that is mostly empty says how much history there is and offers
 * the shortest range that holds all of it (OV-43). Null: nothing to say.
 */
export function historyNote(firstPoint: number | null, range: string, now: number): { have: number; fit: RangeName | null } | null {
  if (firstPoint === null) return null;
  const have = Math.max(0, now - firstPoint);
  const secs = rangeSecs(range);
  if (have * 2 >= secs) return null;
  const fit = RANGES.find((r) => SECS[r] >= have) ?? null;
  return { have, fit: fit !== null && SECS[fit] < secs ? fit : null };
}

export { logsHref as logsAt } from "../../app/paths.ts";
