export const RANGES = ["1h", "6h", "24h", "7d", "30d"] as const;
export type Range = (typeof RANGES)[number];

export function historyWindow(search: URLSearchParams) {
  const range = RANGES.find((r) => r === search.get("range")) ?? "24h";
  const integer = (key: string) => {
    const s = search.get(key);
    if (!s || !/^-?\d+$/.test(s)) return null;
    const n = Number(s);
    return Number.isSafeInteger(n) ? n : null;
  };
  const from = integer("from");
  const to = integer("to");
  const before = integer("before");
  const level = ["all", "info", "warn", "error"].find((l) => l === search.get("level")) ?? "incidents";
  const params = new URLSearchParams({ range, level });
  const zoomed = from !== null && to !== null && from < to;
  if (zoomed) {
    params.set("from", String(from));
    params.set("to", String(to));
  }
  if (before !== null && before > 0) params.set("before", String(before));
  if (search.get("cmp") === "1") params.set("cmp", "1");
  return { range, level, zoomed, query: params.toString() };
}

const bounds = (lo: number, hi: number) => ({ from: String(lo), to: String(hi), before: null });

export function windowSteps(from: number, to: number, now: number) {
  const span = to - from;
  const half = Math.floor(span / 2);
  const oldest = now - 30 * 86400;
  const earlier = Math.max(oldest, from - half);
  const later = Math.min(now, to + half);
  const center = from + half;
  const inset = center - Math.floor(half / 2);
  const end = Math.min(now, center + span);
  return {
    earlier: earlier < from ? bounds(earlier, earlier + span) : null,
    later: later > to ? bounds(later - span, later) : null,
    in: half >= 300 ? bounds(inset, inset + half) : null,
    out: span < 30 * 86400 ? bounds(Math.max(oldest, end - 2 * span), end) : null,
  };
}
