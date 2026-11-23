// Agenda grouping for the `agenda` widget shape: entries by local day, in
// time order, with a relative heading for today and tomorrow.
import type { AgendaEntry } from "../api/gen/AgendaEntry.ts";

export interface AgendaDay { key: string; heading: string; entries: AgendaEntry[] }

const DAY = 86_400_000;

function dayStart(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** Group by the viewer's local day. `now` is milliseconds; `locale` is for tests. */
export function agendaDays(entries: readonly AgendaEntry[], now: number, locale?: string): AgendaDay[] {
  const today = dayStart(now);
  const weekday = new Intl.DateTimeFormat(locale, { weekday: "long", month: "short", day: "numeric" });
  const days = new Map<number, AgendaEntry[]>();
  for (const entry of entries.toSorted((a, b) => a.time - b.time)) {
    const ms = entry.time * 1000;
    if (!Number.isFinite(ms)) continue;
    const start = dayStart(ms);
    days.set(start, [...days.get(start) ?? [], entry]);
  }
  return [...days].map(([start, list]) => {
    const offset = Math.round((start - today) / DAY);
    const heading = offset === 0 ? "Today" : offset === 1 ? "Tomorrow" : offset === -1 ? "Yesterday" : weekday.format(start);
    return { key: String(start), heading, entries: list };
  });
}

/** "21:00", or "All day". */
export function agendaTime(entry: AgendaEntry, locale?: string): string {
  if (entry.all_day) return "All day";
  return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(entry.time * 1000);
}
