/* eslint-disable no-control-regex -- ANSI escape and terminal control stripping is intentional. */
// Pure log contracts and transformations. Stream timestamps remain milliseconds.
export interface LogLine { c: string; t: number; m: string; s?: number; l?: string; x?: number }
export interface Row extends LogLine { id: number; plain: string; segments: Segment[]; fold: ReturnType<typeof jsonFold> }
export class Ring {
  rows: Row[] = [];
  next = 0;
  readonly cap: number;
  constructor(cap: number) { this.cap = cap; }
  append(lines: LogLine[], older = false): void {
    const incoming = lines.map((line) => {
      const segments = ansi(line.m), plain = segments.map((segment) => segment.text).join("");
      return { ...line, id: this.next++, plain, segments, fold: jsonFold(plain) };
    });
    incoming.sort((a, b) => a.t - b.t || a.id - b.id);
    const room = Math.max(0, this.cap - this.rows.length);
    this.rows = older ? [...(room ? incoming.slice(-room) : []), ...this.rows] : [...this.rows, ...incoming];
    this.rows.sort((a, b) => a.t - b.t || a.id - b.id);
    if (!older && this.rows.length > this.cap) this.rows = this.rows.slice(-this.cap);
  }
}
export interface Segment { text: string; classes: string; fg?: string; bg?: string }
const palette = ["#121212", "#d65c5c", "#6bb576", "#c9ad58", "#729fe0", "#ba83d0", "#64b7bb", "#d0d0d0", "#777777", "#ff8585", "#93dc9b", "#f0d57f", "#9cc3ff", "#e3acfa", "#94e1e5", "#ffffff"];
const cube = (k: number) => k ? 55 + k * 40 : 0;
function color(n: number): string {
  n = Math.max(0, Math.min(255, n));
  if (n < 16) return palette[n] ?? "#ffffff";
  if (n >= 232) { const v = 8 + (n - 232) * 10; return `rgb(${v},${v},${v})`; }

  n -= 16;
  return `rgb(${cube(Math.floor(n / 36))},${cube(Math.floor(n / 6) % 6)},${cube(n % 6)})`;
}
export function ansi(message: string): Segment[] {
  const out: Segment[] = [];
  let fg: string | undefined, bg: string | undefined;
  const flags = new Set<string>();
  const escape = /\x1b\[([0-9;:?]*)([@-~])|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b./g;
  const push = (text: string) => {
    text = text.replace(/[\x00-\x08\x0b-\x1f\x7f]/g, "");
    if (text) out.push({ text, classes: [...flags].join(" "), fg, bg });
  };
  let end = 0;
  for (const m of message.matchAll(escape)) {
    push(message.slice(end, m.index)); end = m.index + m[0].length;
    if (m[2] !== "m") continue;
    const p = (m[1] || "0").split(/[;:]/).map(Number);
    for (let i = 0; i < p.length; i++) {
      const n = p[i] ?? 0;
      if (n === 0) { fg = bg = undefined; flags.clear(); }
      else if (n === 1) flags.add("log-bold");
      else if (n === 2) flags.add("log-dim");
      else if (n === 3) flags.add("log-italic");
      else if (n === 4) flags.add("log-underline");
      else if (n === 22) { flags.delete("log-bold"); flags.delete("log-dim"); }
      else if (n === 23) flags.delete("log-italic");
      else if (n === 24) flags.delete("log-underline");
      else if (n === 39) fg = undefined;
      else if (n === 49) bg = undefined;
      else if (n >= 30 && n <= 37) fg = color(n - 30);
      else if (n >= 90 && n <= 97) fg = color(n - 82);
      else if (n >= 40 && n <= 47) bg = color(n - 40);
      else if (n >= 100 && n <= 107) bg = color(n - 92);
      else if (n === 38 || n === 48) {
        let c: string | undefined;
        if (p[i + 1] === 5) { c = color(p[i + 2] ?? 0); i += 2; }
        else if (p[i + 1] === 2) { c = `rgb(${p.slice(i + 2, i + 5).map((v) => Math.max(0, Math.min(255, v))).join(",")})`; i += 4; }
        if (n === 38) fg = c; else bg = c;
      }
    }
  }
  push(message.slice(end)); return out;
}
export function jsonFold(text: string): { summary: string; pretty: string } | null {
  if (!text.trim().startsWith("{")) return null;
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const obj = value as Record<string, unknown>;
    const key = ["msg", "message", "event"].find((k) => typeof obj[k] === "string");
    const fields = Object.entries(obj).filter(([k, v]) => k !== key && (v === null || ["string", "number", "boolean"].includes(typeof v))).slice(0, 6);
    return { summary: [key ? String(obj[key]) : "JSON", ...fields.map(([k, v]) => `${k}=${String(v)}`)].join(" · "), pretty: JSON.stringify(value, null, 2) };
  } catch { return null; }
}
// Quantified groups containing another repetition or an alternation can
// explore many equivalent partitions of the same text.
function regexProblem(pattern: string): string | null {
  if (pattern.length > 256) return "Use at most 256 characters in a regular expression.";
  const groups: boolean[] = [];
  let inClass = false;
  for (let i = 0; i < pattern.length; i++) {
    const ch = pattern[i];
    if (ch === "\\") { i++; continue; }
    if (ch === "]" && inClass) { inClass = false; continue; }
    if (inClass) continue;
    if (ch === "[") { inClass = true; continue; }
    if (ch === "(") { groups.push(false); continue; }
    if (ch === ")") {
      const risky = groups.pop();
      if (risky && /^[*+?{]/.test(pattern.slice(i + 1))) return "Use a simpler regular expression without nested repetitions or repeated alternatives.";
      if (risky && groups.length) groups[groups.length - 1] = true;
      continue;
    }
    if (groups.length && (ch === "|" || ch === "*" || ch === "+" || ch === "{" || (ch === "?" && pattern[i - 1] !== "("))) {
      groups[groups.length - 1] = true;
    }
  }
  return null;
}

export function matcher(q: string, re: boolean, sensitive: boolean): { test: (s: string) => boolean; error: string | null } {
  if (!q) return { test: () => false, error: null };
  if (!re) return { test: sensitive ? (s) => s.includes(q) : (s) => s.toLowerCase().includes(q.toLowerCase()), error: null };
  const problem = regexProblem(q);
  if (problem) return { test: () => false, error: problem };
  try { const r = new RegExp(q, sensitive ? "" : "i"); return { test: (s) => r.test(s), error: null }; }
  catch { return { test: () => false, error: "Invalid regular expression" }; }
}
export function filterRows(rows: Row[], test: (s: string) => boolean, only: boolean, context: number, levels: string): { rows: Row[]; hits: Row[]; counts: Record<string, number> } {
  const hits: Row[] = [];
  const indexes = new Set<number>();
  rows.forEach((row, i) => {
    if (!test(row.plain)) return;
    hits.push(row);
    if (only) for (let k = Math.max(0, i - context); k <= Math.min(rows.length - 1, i + context); k++) indexes.add(k);
  });
  const counts: Record<string, number> = { e: 0, w: 0, i: 0, d: 0, o: 0 };
  for (const r of rows) counts[r.l || "o"] = (counts[r.l || "o"] || 0) + 1;
  return { rows: rows.filter((r, i) => levels.includes(r.l || "o") && (!only || indexes.has(i))), hits: hits.filter((r) => levels.includes(r.l || "o")), counts };
}
export function names(q: URLSearchParams, key: string): string[] { return [...new Set(q.getAll(key).flatMap((v) => v.split(",")).map((v) => v.trim()).filter(Boolean))]; }
export function permalink(hash: string): number | null { const m = /^#t=(\d+(?:\.\d+)?)$/.exec(hash); const t = m ? Number(m[1]) : NaN; return Number.isFinite(t) && t >= 0 ? t : null; }
export function searchState(q: URLSearchParams) { return { search: q.get("search") || "", sensitive: q.get("case") === "1", regexFromLink: q.get("re") === "1" }; }

export interface MatchPosition { pane: string; id: number; t: number }
export function nextMatch(hits: MatchPosition[], current: MatchPosition | null, direction: number): MatchPosition | null {
  const sorted = hits.toSorted((a, b) => a.t - b.t || a.pane.localeCompare(b.pane) || a.id - b.id);
  if (!sorted.length) return null;
  const i = sorted.findIndex((h) => h.pane === current?.pane && h.id === current.id);
  return sorted[i < 0 ? (direction < 0 ? sorted.length - 1 : 0) : (i + direction + sorted.length) % sorted.length] ?? null;
}
export function chipName(name: string, stack: string, limit: number): string {
  if (stack && name.startsWith(stack) && /^[-_]/.test(name.slice(stack.length))) name = name.slice(stack.length + 1);
  return name.length <= limit ? name : name.slice(0, Math.ceil(limit / 2) - 1) + "…" + name.slice(-Math.floor(limit / 2));
}

export function matchRanges(text: string, query: string, regex: boolean, sensitive: boolean): Array<[number, number]> {
  if (!query || (regex && regexProblem(query))) return [];
  try {
    const pattern = regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const expression = new RegExp(pattern, sensitive ? "g" : "gi");
    const ranges: Array<[number, number]> = [];
    for (const match of text.matchAll(expression)) {
      if (match[0].length) ranges.push([match.index, match.index + match[0].length]);
      if (ranges.length >= 1000) break;
    }
    return ranges;
  } catch { return []; }
}

export function streamParams(query: string, range: string, tail: number, cap: number, latest: number | null, target: number | null, now: number): URLSearchParams {
  const params = new URLSearchParams(query + range);
  const seeking = target !== null && target <= now + 5000;
  params.set("tail", String(latest !== null || range || seeking ? cap : tail));
  if (latest !== null) params.set("since", String(latest / 1000 + 0.000001));
  else if (seeking) params.set("since", String(Math.max(0, target / 1000 - 120)));
  return params;
}

export function paneHref(query: string, containers: string[], hash = "", picking = false): string {
  const params = new URLSearchParams(query);
  params.delete(picking ? "c" : "sel");
  const key = picking ? "sel" : "c";
  if (containers.length) params.set(key, containers.join(",")); else params.delete(key);
  return "/app/logs" + (params.size ? `?${params}` : "") + hash;
}
