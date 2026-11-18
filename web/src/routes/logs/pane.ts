import { api } from "../../api/session.ts";
import { filterRows, matcher, Ring, type LogLine, type Row } from "./model.ts";

export interface ViewOptions {
  search: string; regex: boolean; sensitive: boolean; only: boolean; context: number;
  levels: string; wrap: boolean; time: boolean; paused: boolean;
}
export interface PaneStatus { state: string; count: number; matches: number; current: number; unseen: number; following: boolean; waiting: number; message: string; counts: Record<string, number> }
// This class owns only the ref's descendants. Text is always a text node.
// One animation-frame flush and a visible window bound work independently of tail size.
export class LogPane {
  readonly ring: Ring;
  private source: EventSource | null = null;
  private older: EventSource | null = null;
  private pending: LogLine[] = [];
  private pendingOlder: LogLine[] = [];
  private frame = 0;
  private retry = 0;
  private hiddenTimer = 0;
  private delay = 2000;
  private closed = false;
  private restoring = false;
  private openedJson = new Set<number>();
  private suspended = false;
  private heights = new Map<number, number>();
  private visible: Row[] = [];
  private hits: Row[] = [];
  private offsets: number[] = [];
  private following = true;
  private unseen = 0;
  private lastTop = 0;
  private anchor: { id: number; offset: number } | null = null;
  private current: number | null = null;
  private selected: Row | null = null;
  private pinned = false;
  private starts = new Map<string, { t: number; count: number }>();
  private state = "connecting";
  private message = "";
  private counts: Record<string, number> = {};
  private signature = "";
  private top = document.createElement("div");
  private content = document.createElement("div");
  private bottom = document.createElement("div");
  private observer: ResizeObserver;
  private width = 0;
  private done = false;
  private last = new Map<string, number>();
  private names = new Map<number, string>();
  private pickedTarget = false;
  readonly box: HTMLElement;
  readonly query: string;
  readonly cap: number;
  readonly tail: number;
  private options: ViewOptions;
  private range: string;
  private target: number | null;
  private report: (s: PaneStatus) => void;
  private select: (row: Row | null, focus: boolean) => void;
  constructor(box: HTMLElement, query: string, cap: number, tail: number,
    options: ViewOptions, range: string, target: number | null,
    report: (s: PaneStatus) => void, select: (row: Row | null, focus: boolean) => void) {
    this.box = box; this.query = query; this.cap = cap; this.tail = tail;
    this.options = options; this.range = range; this.target = target; this.report = report; this.select = select;
    this.ring = new Ring(cap);
    this.top.setAttribute("aria-hidden", "true"); this.bottom.setAttribute("aria-hidden", "true");
    this.content.className = "log-window";
    box.replaceChildren(this.top, this.content, this.bottom);
    box.addEventListener("scroll", this.scroll, { passive: true });
    box.addEventListener("wheel", this.wheel, { passive: true });
    box.addEventListener("keydown", this.key);
    box.addEventListener("click", this.click);
    box.addEventListener("mouseover", this.hover);
    this.observer = new ResizeObserver(() => {
      if (this.width !== box.clientWidth) { this.width = box.clientWidth; this.heights.clear(); this.signature = ""; }
      this.schedule();
    });
    this.observer.observe(box); this.observer.observe(this.content);
    if (box.closest(".logs-page")) this.observer.observe(box.closest(".logs-page") as Element);
    window.addEventListener("resize", this.resize);
    box.addEventListener("touchstart", this.touch, { passive: true });
    box.addEventListener("touchmove", this.touch, { passive: true });
    window.addEventListener("pagehide", this.pagehide);
    window.addEventListener("pageshow", this.pageshow);
    document.addEventListener("visibilitychange", this.visibility);
    this.connect();
  }
  update(options: ViewOptions): void {
    const changed = options.wrap !== this.options.wrap || options.time !== this.options.time;
    if (!this.following) this.mark();
    this.restoring = true;
    this.options = options;
    if (changed) { this.heights.clear(); this.signature = ""; }
    this.schedule();
  }
  private pagehide = () => { this.suspended = true; this.disconnect(); };
  private pageshow = () => { if (this.suspended && !document.hidden) { this.suspended = false; this.connect(); } };
  private visibility = () => {
    clearTimeout(this.hiddenTimer);
    if (document.hidden) this.hiddenTimer = window.setTimeout(() => { this.suspended = true; this.disconnect(); }, 10000);
    else this.pageshow();
  };
  private disconnect(): void { this.source?.close(); this.older?.close(); clearTimeout(this.retry); this.source = this.older = null; }
  close(): void {
    this.box.dispatchEvent(new CustomEvent("rh:log-counts", { bubbles: true, detail: { query: this.query, counts: null } }));
    this.closed = true; this.disconnect(); clearTimeout(this.hiddenTimer); cancelAnimationFrame(this.frame);
    this.observer.disconnect();
    window.removeEventListener("resize", this.resize);
    this.box.removeEventListener("touchstart", this.touch);
    this.box.removeEventListener("touchmove", this.touch);
    this.box.removeEventListener("scroll", this.scroll); this.box.removeEventListener("wheel", this.wheel);
    this.box.removeEventListener("keydown", this.key); this.box.removeEventListener("click", this.click); this.box.removeEventListener("mouseover", this.hover);
    window.removeEventListener("pagehide", this.pagehide); window.removeEventListener("pageshow", this.pageshow);
    document.removeEventListener("visibilitychange", this.visibility);
  }
  private connect(): void {
    if (this.closed || this.suspended || this.done) return;
    const params = new URLSearchParams(this.query + this.range);
    params.set("tail", String(this.tail));
    const latest = Math.min(...this.last.values());
    if (Number.isFinite(latest)) params.set("since", String(latest / 1000 + 0.000001));
    else if (this.target && this.target <= Date.now() + 5000) params.set("since", String(Math.max(0, this.target / 1000 - 120)));
    else if (this.target) this.message = "That moment is still to come: showing the live tail.";
    this.state = "connecting"; this.schedule();
    const reconnectFloor = new Map(this.last);
    const reconnecting = reconnectFloor.size > 0;
    const es = this.source = new EventSource(`/api/logs/stream?${params}`);
    es.addEventListener("hello", () => { this.state = "live"; this.delay = 2000; this.schedule(); });
    es.addEventListener("meta", (e) => {
      const m = JSON.parse((e as MessageEvent<string>).data) as { c: number; name: string; running: boolean; status: string };
      this.names.set(m.c, m.name);
      if (!m.running) this.message = `This container is stopped. ${m.status}`;
      this.schedule();
    });
    es.addEventListener("l", (e) => {
      const lines = this.decode((e as MessageEvent<string>).data);
      for (const line of lines) {
        const previous = reconnectFloor.get(line.c);
        if (previous !== undefined && line.t <= previous && reconnecting) continue;
        this.pending.push(line);
        this.last.set(line.c, Math.max(this.last.get(line.c) ?? 0, line.t));
      }
      // Paused input stays bounded too. The cursor advances only when flushed.
      if (this.pending.length > this.cap) this.pending.splice(0, this.pending.length - this.cap);
      this.schedule();
    });
    es.addEventListener("err", (e) => {
      const err = JSON.parse((e as MessageEvent<string>).data) as { kind: string; msg: string };
      this.state = err.kind === "notfound" ? "not found" : "error"; this.message = err.msg; this.schedule();
    });
    es.addEventListener("end", (e) => {
      const end = JSON.parse((e as MessageEvent<string>).data) as { cut?: boolean };
      if (end.cut) this.message = "Showing the first lines of the range; the server line limit was reached.";
    });
    es.addEventListener("done", () => { this.done = true; es.close(); if (this.state !== "error" && this.state !== "not found") this.state = "ended"; this.schedule(); });
    es.addEventListener("error", () => {
      es.close(); this.source = null;
      if (this.closed || this.suspended || this.done) return;
      this.state = "disconnected, retrying"; this.schedule();
      void api.get<{ groups: Array<{ name: string; project: boolean; containers: Array<{ name: string }> }> }>("/api/v1/logs/picker").then((result) => {
        if (this.closed || this.suspended) return;
        if (result.kind === "sign_in" || result.kind === "forbidden") { this.state = "no access"; this.message = "Sign in to read these logs."; this.done = true; this.schedule(); return; }
        const query = new URLSearchParams(this.query);
        if (result.kind === "ok" && !(query.has("stack") ? result.data.groups.some((g) => g.project && g.name === query.get("stack")) : result.data.groups.some((g) => g.containers.some((c) => c.name === query.get("c"))))) {
          this.state = "not found"; this.message = "This container is no longer available."; this.done = true; this.schedule(); return;
        }
        this.retry = window.setTimeout(() => this.connect(), this.delay);
        this.delay = Math.min(30000, this.delay * 2);
      });
    });
  }
  private decode(data: string): LogLine[] {
    const lines = JSON.parse(data) as Array<Omit<LogLine, "c"> & { c: number }>;
    return lines.map((line) => ({ c: this.names.get(line.c) ?? String(line.c), t: line.t, m: line.m, s: line.s, l: line.l, x: line.x }));
  }
  loadOlder(): void {
    if (this.older || this.pendingOlder.length || !this.ring.rows.length || this.ring.rows.length >= this.cap) { this.message = "The ring is full; choose an earlier range to read older lines."; this.schedule(); return; }
    const oldest = (this.ring.rows[0]?.t ?? 0);
    const newest = (this.ring.rows.at(-1)?.t ?? 0);
    const span = Math.max(60000, (newest - oldest) * this.tail / this.ring.rows.length);
    const p = new URLSearchParams(this.query);
    p.set("since", String(Math.max(0, oldest - span) / 1000)); p.set("until", String((oldest - 1) / 1000)); p.set("tail", String(Math.min(this.tail, this.cap - this.ring.rows.length)));
    const older: LogLine[] = [];
    const es = this.older = new EventSource(`/api/logs/stream?${p}`);
    es.addEventListener("l", (e) => older.push(...this.decode((e as MessageEvent<string>).data)));
    const finish = () => { es.close(); this.older = null; this.pendingOlder = older; if (!older.length) this.message = "No older lines in this window."; this.schedule(); };
    es.addEventListener("done", finish); es.addEventListener("error", finish);
  }
  latest(): void { this.following = true; this.unseen = 0; this.anchor = null; this.schedule(); }
  step(direction: number): void {
    if (!this.hits.length) return;
    const i = this.hits.findIndex((r) => r.id === this.current);
    const row = this.hits[(i + direction + this.hits.length) % this.hits.length];
    if (!row) return;
    this.current = row.id; this.seek(row.id); this.signature = ""; this.schedule();
  }
  private seek(id: number): void {
    const i = this.visible.findIndex((r) => r.id === id);
    if (i < 0) return;
    this.following = false; this.anchor = { id, offset: 0 };
    this.box.scrollTop = this.offsets[i] ?? 0; this.lastTop = this.box.scrollTop;
  }
  private resize = () => { this.restoring = true; this.schedule(); };
  private touch = () => { this.restoring = false; };
  private wheel = (e: WheelEvent) => { this.restoring = false; if (e.deltaY < 0) { this.following = false; this.mark(); this.schedule(); } };
  private scroll = () => {
    if (this.width !== this.box.clientWidth) { this.restoring = true; this.schedule(); return; }
    if (this.restoring) return;
    const top = this.box.scrollTop;
    if (top < this.lastTop - 0.1) this.following = false;
    else if (top > this.lastTop && this.box.scrollHeight - top - this.box.clientHeight <= 24) { this.following = true; this.unseen = 0; }
    this.lastTop = top; this.mark(); this.schedule();
  };
  private mark(): void {
    if (this.following) { this.anchor = null; return; }
    const edge = this.box.getBoundingClientRect().top;
    let clipped: HTMLElement | null = null;
    for (const child of this.content.children) {
      const element = child as HTMLElement;
      const bounds = element.getBoundingClientRect();
      if (bounds.top >= edge - 0.5 && bounds.top < edge + this.box.clientHeight) {
        this.anchor = { id: Number(element.dataset.logId), offset: edge - bounds.top };
        return;
      }
      if (bounds.bottom > edge && !clipped) clipped = element;
    }
    if (clipped) { this.anchor = { id: Number(clipped.dataset.logId), offset: edge - clipped.getBoundingClientRect().top }; return; }
    let i = this.indexAt(this.box.scrollTop);
    i = Math.min(i, this.visible.length - 1);
    const row = this.visible[i];
    if (row) this.anchor = { id: row.id, offset: this.box.scrollTop - (this.offsets[i] ?? 0) };
  }
  private key = (e: KeyboardEvent) => {
    if (e.key === "End") { e.preventDefault(); this.latest(); }
    if (e.key === "Enter" && e.target === this.box) {
      e.preventDefault();
      const row = this.visible.find((r) => r.id === this.current) ?? this.visible[this.indexAt(this.box.scrollTop)];
      if (row) { this.pinned = true; this.selected = row; this.select(row, true); this.signature = ""; this.schedule(); }
    }
  };
  private click = (e: MouseEvent) => {
    const summary = (e.target as Element).closest("summary");
    if (summary) {
      const details = summary.closest("details"), row = summary.closest<HTMLElement>("[data-log-id]");
      if (details && row) { const id = Number(row.dataset.logId); if (details.open) this.openedJson.delete(id); else this.openedJson.add(id); }
      requestAnimationFrame(() => { this.measure(); this.schedule(); });
      return;
    }
    if ((e.target as Element).closest("a,button,pre")) return;
    const el = (e.target as Element).closest<HTMLElement>("[data-log-id]");
    const row = el && this.visible.find((r) => r.id === Number(el.dataset.logId));
    if (row) { this.pinned = true; this.selected = row; this.select(row, false); this.signature = ""; this.schedule(); }
    if ((e.target as Element).closest("summary")) requestAnimationFrame(() => { this.measure(); this.schedule(); });
  };
  private hover = (e: MouseEvent) => {
    if (this.pinned) return;
    const element = (e.target as Element).closest<HTMLElement>("[data-log-id]");
    const row = element && this.visible.find((line) => line.id === Number(element.dataset.logId));
    if (row && row.id !== this.selected?.id) { this.selected = row; this.select(row, false); this.signature = ""; this.schedule(); }
  };
  clearSelection(): void { this.pinned = false; this.selected = null; this.select(null, false); this.signature = ""; this.schedule(); }
  moveSelected(direction: number): void {
    const i = this.visible.findIndex((r) => r.id === this.selected?.id);
    const next = this.visible[i + direction];
    if (next) { this.selected = next; this.select(next, true); this.seek(next.id); this.signature = ""; this.schedule(); }
  }
  private schedule(): void { if (!this.frame && !this.closed) this.frame = requestAnimationFrame(() => { this.frame = 0; this.flush(); }); }
  private indexAt(y: number): number {
    let lo = 0, hi = this.offsets.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1; if ((this.offsets[mid] ?? 0) <= y) lo = mid + 1; else hi = mid; }
    return Math.max(0, lo - 1);
  }
  private estimate(row: Row): number {
    if (!this.options.wrap) return 28;
    const reserved = (this.options.time ? 116 : 0) + (this.query.startsWith("stack=") ? 100 : 0) + 24;
    const columns = Math.max(15, (this.box.clientWidth - reserved) / 7.5);
    return 8 + 20 * Math.max(1, Math.ceil((row.fold?.summary.length ?? row.plain.length) / columns));
  }
  private layout(): number {
    let height = 0; this.offsets = [];
    for (const row of this.visible) { this.offsets.push(height); height += this.heights.get(row.id) ?? this.estimate(row); }
    return height;
  }
  private measure(): void {
    for (const child of this.content.children) {
      const el = child as HTMLElement; this.heights.set(Number(el.dataset.logId), el.offsetHeight);
    }
  }
  private restore(total: number): void {
    if (this.following) this.box.scrollTop = Math.max(0, total - this.box.clientHeight);
    else if (this.anchor) {
      const index = this.visible.findIndex((r) => r.id === this.anchor?.id);
      if (index >= 0) this.box.scrollTop = (this.offsets[index] ?? 0) + this.anchor.offset;
      else if (this.visible.length) {
        const removed = this.ring.rows.find((row) => row.id === this.anchor?.id);
        const next = this.visible.find((row) => row.t >= (removed?.t ?? 0)) ?? this.visible[0];
        const at = this.visible.findIndex((row) => row.id === next?.id);
        if (next && at >= 0) { this.anchor = { id: next.id, offset: this.anchor.offset }; this.box.scrollTop = (this.offsets[at] ?? 0) + this.anchor.offset; }
      }
    }
    this.lastTop = this.box.scrollTop;
  }
  private fit(): void {
    const panes = this.box.closest(".log-panes");
    if (innerWidth < 700 && (panes?.children.length ?? 0) > 1) { this.box.style.height = "340px"; return; }
    let below = 0;
    for (let sibling = this.box.nextElementSibling; sibling; sibling = sibling.nextElementSibling) {
      if (getComputedStyle(sibling).position !== "absolute") below += (sibling as HTMLElement).offsetHeight;
    }
    if (panes) for (let sibling = panes.nextElementSibling; sibling; sibling = sibling.nextElementSibling) below += (sibling as HTMLElement).offsetHeight + 8;
    const bar = document.querySelector<HTMLElement>(".bbar");
    const bottom = innerWidth < 700 ? bar?.offsetHeight ?? 56 : 0;
    const height = Math.max(240, innerHeight - (this.box.getBoundingClientRect().top + scrollY) - below - bottom - 32);
    this.box.style.height = `${Math.floor(height)}px`;
  }
  private flush(): void {
    this.restoring = true;
    this.fit();
    let accepted = 0;
    if (!this.options.paused && this.pending.length) {
      const batch = this.pending.splice(0, 256); accepted = batch.length;
      if (!this.following) this.unseen += batch.length;
      this.ring.append(batch);
      for (const line of batch) {
        this.last.set(line.c, Math.max(this.last.get(line.c) ?? 0, line.t));
        const start = this.starts.get(line.c);
        this.starts.set(line.c, { t: start?.t ?? line.t, count: (start?.count ?? 0) + 1 });
      }
      const retained = new Set(this.ring.rows.map((r) => r.id));
      for (const id of this.heights.keys()) if (!retained.has(id)) this.heights.delete(id);
      for (const id of this.openedJson) if (!retained.has(id)) this.openedJson.delete(id);
    }
    if (!this.options.paused && !this.pending.length && this.pendingOlder.length && accepted < 256) {
      this.ring.append(this.pendingOlder.splice(-(256 - accepted)), true);
    }
    const search = matcher(this.options.search, this.options.regex, this.options.sensitive);
    const searchStarted = performance.now();
    const filtered = filterRows(this.ring.rows, search.test, this.options.only, this.options.context, this.options.levels);
    this.box.dataset.searchMs = (performance.now() - searchStarted).toFixed(3);
    this.box.dataset.searchLines = String(this.ring.rows.length);
    const changedSearch = `${this.options.search}\n${this.options.regex}\n${this.options.sensitive}`;
    this.visible = filtered.rows; this.hits = filtered.hits; this.counts = filtered.counts;
    const countKey = Object.values(this.counts).join(",");
    if (countKey !== this.countsKey) { this.countsKey = countKey; this.box.dispatchEvent(new CustomEvent("rh:log-counts", { bubbles: true, detail: { query: this.query, counts: this.counts } })); }

    let total = this.layout();
    if (this.searchKey !== changedSearch) {
      this.searchKey = changedSearch;
      const newest = this.hits.at(-1);
      this.current = newest?.id ?? null;
      if (newest && this.options.search) this.seek(newest.id);
    }
    if (this.target && !this.pickedTarget && this.visible.length) {
      const next = this.visible.find((r) => r.t >= (this.target ?? 0));
      if (next) { this.pickedTarget = true; this.current = next.id; this.seek(next.id); if (next.t > this.target + 1000) this.message = `No line at that moment. Next line: ${new Date(next.t).toLocaleString()}`; }
    }
    this.restore(total);
    for (let pass = 0; pass < 2; pass++) {
      const start = Math.max(0, this.indexAt(this.box.scrollTop) - 8);
      const end = Math.min(this.visible.length, this.indexAt(this.box.scrollTop + this.box.clientHeight) + 10);
      const slice = this.visible.slice(start, end);
      const signature = slice.map((r) => `${r.id}:${r.id === this.current}:${r.id === this.selected?.id}`).join(",") + this.options.wrap + this.options.time;
      if (signature !== this.signature) {
        this.signature = signature;
        const fragment = document.createDocumentFragment();
        for (const row of slice) fragment.append(this.row(row));
        this.content.replaceChildren(fragment);
      }
      this.top.style.height = `${this.offsets[start] ?? 0}px`;
      const after = end < this.visible.length ? (this.offsets[end] ?? total) : total;
      this.bottom.style.height = `${Math.max(0, total - after)}px`;
      this.measure(); total = this.layout(); this.restore(total);
    }
    requestAnimationFrame(() => { this.restoring = false; this.lastTop = this.box.scrollTop; });
    if ((this.pending.length || this.pendingOlder.length) && !this.options.paused) this.schedule();
    this.report({ state: this.options.paused && this.state === "live" ? "paused" : this.state, count: this.ring.rows.length, matches: this.hits.length,
      current: this.hits.findIndex((r) => r.id === this.current) + 1, unseen: this.unseen, following: this.following, waiting: this.pending.length,
      message: search.error ?? (this.ring.rows.length && !this.visible.length ? "No lines match. Clear the search or level filters." : this.message), counts: this.counts });
  }
  private searchKey = "";
  private countsKey = "";
  private row(row: Row): HTMLElement {
    const el = document.createElement("div");
    el.className = `log-row ${this.options.wrap ? "" : "log-nowrap"} ${row.id === this.current ? "log-current" : ""} ${row.id === this.selected?.id ? "log-selected" : ""}`;
    el.dataset.logId = String(row.id); el.dataset.level = row.l ?? "o"; el.dataset.timestamp = String(row.t);
    if (this.options.time) {
      const time = document.createElement("time"); time.dateTime = new Date(row.t).toISOString();
      const date = new Date(row.t);
      time.textContent = (date.toDateString() === new Date().toDateString() ? "" : date.toLocaleDateString(undefined, { day: "numeric", month: "short" }) + " ") + date.toLocaleTimeString(undefined, { hour12: false });
      el.append(time);
    }
    if (this.query.startsWith("stack=")) { const chip = document.createElement("span"); chip.className = "log-name"; chip.textContent = row.c; chip.title = row.c;
      const hash = [...new TextEncoder().encode(row.c)].reduce((h, b) => Math.imul(h ^ b, 16777619) >>> 0, 2166136261);
      chip.style.color = `hsl(${hash % 360} 55% 62%)`; el.append(chip); }
    const text = document.createElement("span"); text.className = "log-text";
    const fold = row.fold;
    if (fold) {
      const details = document.createElement("details"), summary = document.createElement("summary"), pre = document.createElement("pre");
      summary.textContent = fold.summary; pre.textContent = fold.pretty;
      details.open = this.openedJson.has(row.id);
      details.addEventListener("toggle", () => {
        if (!details.isConnected) return;
        if (details.open) this.openedJson.add(row.id); else this.openedJson.delete(row.id);
        this.measure(); this.schedule();
      });
      details.append(summary, pre); text.append(details);
    } else {
      for (const segment of row.segments) {
        const span = document.createElement("span"); span.className = segment.classes;
        if (segment.fg) span.style.color = segment.fg; if (segment.bg) span.style.backgroundColor = segment.bg;
        // Link only explicit http(s) URLs, with no interpretation of other text.
        const urls = /https?:\/\/[^\s<>"']+/g; let end = 0;
        for (const url of segment.text.matchAll(urls)) {
          span.append(document.createTextNode(segment.text.slice(end, url.index)));
          const a = document.createElement("a"); a.href = url[0]; a.rel = "noreferrer noopener"; a.target = "_blank"; a.textContent = url[0]; span.append(a); end = url.index + url[0].length;
        }
        span.append(document.createTextNode(segment.text.slice(end))); text.append(span);
      }
    }
    const start = this.starts.get(row.c);
    if (this.query.startsWith("stack=") && start && start.count >= this.tail && row.t === start.t) {
      const marker = document.createElement("span"); marker.className = "log-history-start"; marker.textContent = `Loaded history starts here for ${row.c}`; text.prepend(marker);
    }
    el.append(text); return el;
  }
}

/** Clipboard support also works on an authenticated plain-HTTP LAN page. */
export async function copyLogText(text: string): Promise<void> {
  if (navigator.clipboard && isSecureContext) {
    try { await navigator.clipboard.writeText(text); return; } catch { /* Try the browser's selection command. */ }
  }
  const field = document.createElement("textarea"); field.className = "log-copy";
  field.append(document.createTextNode(text));
  const focused = document.activeElement;
  document.body.append(field); field.select();
  const copied = document.execCommand("copy"); field.remove();
  if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
  if (!copied) throw new Error("Copy is unavailable");
}
