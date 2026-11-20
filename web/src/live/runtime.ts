import type { Alarm } from "../api/gen/Alarm.ts";
// The live layer of the running app: one LiveStream (stream.ts) wired to
// the browser, the data stores the hooks read, and the fetches that fill
// them. Nothing starts at import; the first hook that mounts calls
// `start()`.
//
// - Stream: one EventSource per tab; closed after 10 s hidden and on
//   pagehide (back/forward cache), opened again on return; after more than
//   a minute away everything is fetched again.
// - Messages are applied in animation frames (store.ts), at most one render
//   per frame, held while the user touches or scrolls.
// - `containers` replaces the containers view (and seeds the stats hub),
//   `stats` goes to the stats hub only (stats.ts: the view keeps its
//   identity, so nothing structural re-renders on a stats tick),
//   `system` is the latest host sample (plus the vitals rings), `config`
//   refetches bootstrap, containers and strips and tells onConfigChange
//   listeners (pages refetch their own data; nothing reloads), `resync`
//   (the server dropped messages for this tab) does the same.
// - `hello` carries the server's build: a different sha or content hash
//   than the build this page came from shows the "new version" notice.
import type { Bootstrap } from "../api/gen/Bootstrap.ts";
import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { Hello } from "../api/gen/Hello.ts";
import type { StatsEvent } from "../api/gen/StatsEvent.ts";
import type { Strips } from "../api/gen/Strips.ts";
import type { SystemView } from "../api/gen/SystemView.ts";
import { api, getSession, subscribeSession } from "../api/session.ts";
import { HIDDEN_CLOSE_MS, STALE_AFTER_MS } from "./backoff.ts";
import { EMPTY_RING, newerView, shareStrips, shareView, pushRing, seedRing, sumSeries, vitalsOf, type Ring } from "./reducer.ts";
import { createScheduler, createStore, type Store } from "./store.ts";
import { StatsHub } from "./stats.ts";
import { LiveStream, type SourceLike, type StreamMessage, type Topic } from "./stream.ts";

export interface Loaded<T> {
  data: T | null;
  /** Why the last fetch failed (the data, if any, is older). */
  error: string | null;
}

const empty = <T>(): Loaded<T> => ({ data: null, error: null });

export const bootstrap: Store<Loaded<Bootstrap>> = createStore(empty<Bootstrap>());
export const containers: Store<Loaded<ContainersView>> = createStore(empty<ContainersView>());
/** Live CPU and memory per container (stats.ts). */
export const stats = new StatsHub();
export const strips: Store<Loaded<Strips>> = createStore(empty<Strips>());
export const system: Store<SystemView | null> = createStore<SystemView | null>(null);
export interface VitalsRings {
  cpu: Ring;
  mem: Ring;
  disk: Ring;
  net: Ring;
}
export const rings: Store<VitalsRings> = createStore<VitalsRings>({ cpu: EMPTY_RING, mem: EMPTY_RING, disk: EMPTY_RING, net: EMPTY_RING });
/** True once the server runs another build than this page. */
export const newBuild: Store<boolean> = createStore(false);
/** The problem count from /alarm.json while the stream is paused (hidden tab), else null. */
export const hiddenProblems: Store<number | null> = createStore<number | null>(null);
/** Server time minus browser time, seconds (from bootstrap.now). */
export const skew: Store<number> = createStore(0);

const boardListeners = new Set<(event: { id: string; version: number }) => void>();
export function watchBoards(cb: (event: { id: string; version: number }) => void): () => void {
  boardListeners.add(cb); const off = wantTopic("boards");
  return () => { boardListeners.delete(cb); off(); };
}

export type ReadTopic = "widget" | "alerts" | "host";
const reads = new Map<ReadTopic, Set<(data: unknown) => void>>();
export function watchReadTopic(topic: ReadTopic, cb: (data: unknown) => void): () => void {
  const listeners = reads.get(topic) ?? new Set();
  reads.set(topic, listeners);
  listeners.add(cb);
  const release = wantTopic(topic);
  return () => { listeners.delete(cb); release(); };
}

const configListeners = new Set<() => void>();

/** Called after the server's config changed (and after a resync): refetch what depends on it. */
export function onConfigChange(cb: () => void): () => void {
  configListeners.add(cb);
  return () => {
    configListeners.delete(cb);
  };
}

// ---- fetches ----

function message(r: { kind: string; message?: string; status?: number }): string {
  return r.kind === "network" ? (r.message ?? "network error") : `${r.message ?? "error"} (HTTP ${r.status ?? "?"})`;
}

let bootSeq = 0;
export async function fetchBootstrap(): Promise<void> {
  const seq = ++bootSeq;
  const r = await api.get<Bootstrap>("/api/v1/bootstrap");
  if (seq !== bootSeq) return;
  if (r.kind === "ok") {
    bootstrap.set({ data: r.data, error: null });
    skew.set(r.data.now - Date.now() / 1000);
  } else bootstrap.update((c) => ({ data: c.data, error: message(r) }));
}

/** A view from a fetch or an event: the newer Docker read wins; its numbers seed the stats hub. */
function setView(v: ContainersView): void {
  const cur = containers.get().data;
  const newer = newerView(cur, v);
  if (newer === cur) return;
  // The numbers go to the stats store; the view keeps every row (and itself) that shows the same.
  stats.seed(newer);
  const next = shareView(cur, newer);
  if (next !== cur || containers.get().error !== null) containers.set({ data: next, error: null });
}

let ctrSeq = 0;
export async function fetchContainers(): Promise<void> {
  const seq = ++ctrSeq;
  const r = await api.get<ContainersView>("/api/v1/containers");
  if (seq !== ctrSeq) return;
  if (r.kind === "ok") setView(r.data);
  else containers.update((c) => ({ data: c.data, error: message(r) }));
}

let stripSeq = 0;
export async function fetchStrips(): Promise<void> {
  const seq = ++stripSeq;
  const r = await api.get<Strips>("/api/v1/strips");
  if (seq !== stripSeq) return;
  if (r.kind === "ok") {
    const cur = strips.get();
    const next = shareStrips(cur.data, r.data);
    if (next !== cur.data || cur.error !== null) strips.set({ data: next, error: null });
  }
  else strips.update((c) => ({ data: c.data, error: message(r) }));
}

interface HistorySeries {
  range: string;
  step: number;
  points: [number, number][];
}

async function lastHour(metric: string): Promise<[number, number][]> {
  const r = await api.get<HistorySeries>(`/api/history/system?metric=${metric}&range=1h`);
  return r.kind === "ok" && Array.isArray(r.data.points) ? r.data.points : [];
}

let seeded = false;
/** The vitals sparklines start with the last hour from the history database (ui::shell vitals). */
export async function seedVitals(b: Bootstrap): Promise<void> {
  if (seeded || !b.features.history || !b.features.system) return;
  seeded = true;
  const [cpu, mem, disk, rx, tx] = await Promise.all(["cpu", "mem", "disk", "net_rx", "net_tx"].map(lastHour));
  memSeed = mem ?? [];
  rings.update((cur) => ({
    // A live value that came first stays the newest point.
    cpu: cur.cpu.v.length > 1 ? cur.cpu : merge(seedRing(cpu ?? []), cur.cpu),
    mem: cur.mem,
    disk: cur.disk.v.length > 1 ? cur.disk : merge(seedRing(disk ?? []), cur.disk),
    net: cur.net.v.length > 1 ? cur.net : merge(seedRing(sumSeries(rx ?? [], tx ?? [])), cur.net),
  }));
  seedMem();
}

/** Memory history is in bytes and the chip in percent of total: seeded once a sample says the total. */
let memSeed: [number, number][] | null = null;
function seedMem(): void {
  const total = system.get()?.mem_total ?? 0;
  if (!memSeed || total <= 0) return;
  const pts = memSeed;
  memSeed = null;
  rings.update((cur) => (cur.mem.v.length > 1 ? cur : { ...cur, mem: merge(seedRing(pts, (v) => (v * 100) / total), cur.mem) }));
}

function merge(seed: Ring, live: Ring): Ring {
  if (!live.v.length) return seed;
  return { v: [...seed.v, ...live.v].slice(-40), t: live.t };
}

// ---- the stream ----

const scheduler = createScheduler({
  frame: (fn) => window.requestAnimationFrame(fn),
  setTimeout: (fn, ms) => window.setTimeout(fn, ms),
  busyFor: () => Math.max(0, busyUntil - Date.now()),
});

let pageBuild: { sha: string; content: string } | null = null;

function checkBuild(h: Hello): void {
  if (!pageBuild) {
    const s = getSession().session;
    pageBuild = s ? { sha: s.build.sha, content: s.build.content } : { sha: h.sha, content: h.content };
  }
  if (h.sha !== pageBuild.sha || h.content !== pageBuild.content) newBuild.set(true);
}

function refetchAll(): void {
  void fetchBootstrap();
  void fetchContainers();
  if (stripsWanted > 0) void fetchStrips();
  for (const l of configListeners) l();
}

function onMessage(m: StreamMessage): void {
  switch (m.type) {
    case "host":
    case "alerts":
    case "widget": {
      const listeners = reads.get(m.type);
      scheduler.queue(() => { for (const cb of listeners ?? []) cb(m.data); });
      return;
    }
    case "boards":
      void fetchBootstrap();
      if (stripsWanted > 0) void fetchStrips();
      for (const cb of boardListeners) cb(m.data as { id: string; version: number });
      return;
    case "hello":
      checkBuild(m.data as Hello);
      return;
    case "config":
    case "resync":
      refetchAll();
      return;
    case "containers":
      scheduler.queue(() => setView(m.data as ContainersView));
      return;
    case "stats":
      scheduler.queue(() => {
        stats.applyEvent(m.data as StatsEvent);
      });
      return;
    case "system":
      scheduler.queue(() => {
        const s = m.data as SystemView;
        system.set(s);
        const v = vitalsOf(s);
        const now = Date.now();
        rings.update((r) => ({
          cpu: pushRing(r.cpu, v.cpu, now),
          mem: pushRing(r.mem, v.mem, now),
          disk: pushRing(r.disk, v.disk, now),
          net: pushRing(r.net, v.net, now),
        }));
        seedMem();
      });
  }
}

export const stream = new LiveStream(
  {
    open: (url) =>
      typeof window.EventSource === "function" ? (new window.EventSource(url) as unknown as SourceLike) : null,
    setTimeout: (fn, ms) => window.setTimeout(fn, ms),
    clearTimeout: (id) => window.clearTimeout(id as number),
    now: () => Date.now(),
  },
  onMessage,
);

// ---- the page: visibility, pagehide, the user's scrolling ----

let busyUntil = 0;
function busy(): void {
  busyUntil = Date.now() + 1500;
}
let started = false;
let hideT: number | null = null;
let hiddenAt = 0;
let alarmT: number | null = null;

async function pollAlarm(): Promise<void> {
  const r = await api.get<Alarm>("/api/v1/alarm");
  if (r.kind === "ok" && typeof r.data.problems === "number") hiddenProblems.set(r.data.problems);
}

function stopAlarm(): void {
  if (alarmT !== null) window.clearInterval(alarmT);
  alarmT = null;
  hiddenProblems.set(null);
}

function back(): void {
  if (hideT !== null) window.clearTimeout(hideT);
  hideT = null;
  stopAlarm();
  const away = hiddenAt ? Date.now() - hiddenAt : 0;
  hiddenAt = 0;
  if (!stream.isPaused()) return;
  stream.resume();
  if (away > STALE_AFTER_MS) refetchAll();
}

/** Idempotent: wire the page events once. Called by the hooks. */
export function start(): void {
  if (started) return;
  started = true;
  for (const ev of ["touchstart", "touchmove", "wheel"] as const) window.addEventListener(ev, busy, { passive: true });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      if (hideT !== null) window.clearTimeout(hideT);
      hideT = window.setTimeout(() => {
        hideT = null;
        hiddenAt = Date.now();
        stream.pause();
        // A pinned tab still shows its problem count: one tiny poll a minute.
        alarmT = window.setInterval(() => void pollAlarm(), 60000);
      }, HIDDEN_CLOSE_MS);
    } else back();
  });
  window.addEventListener("pagehide", () => {
    if (hideT !== null) window.clearTimeout(hideT);
    hideT = null;
    hiddenAt = Date.now();
    stream.pause();
  });
  window.addEventListener("pageshow", (e) => {
    if (e.persisted) back();
  });
  // The page build is the session's; it may arrive after the first hello.
  const s = getSession().session;
  if (s) pageBuild = { sha: s.build.sha, content: s.build.content };
  else {
    const off = subscribeSession(() => {
      const ss = getSession().session;
      if (!ss) return;
      off();
      pageBuild ??= { sha: ss.build.sha, content: ss.build.content };
    });
  }
}

// ---- what is wanted ----

let bootWanted = false;
export function wantBootstrap(): void {
  start();
  if (bootWanted) return;
  bootWanted = true;
  void fetchBootstrap();
}

let ctrWanted = false;
function wantContainersData(): void {
  if (ctrWanted) return;
  ctrWanted = true;
  void fetchContainers();
}

export function wantContainers(): () => void {
  start();
  const a = stream.want("containers");
  const b = stream.want("stats");
  wantContainersData();
  return () => {
    a();
    b();
  };
}

export function wantTopic(t: Topic): () => void {
  start();
  return stream.want(t);
}

let stripsWanted = 0;
let stripsT: number | null = null;
/** Strips are buckets of 15 min; the current one is refetched every minute while shown and live. */
export function wantStrips(): () => void {
  start();
  stripsWanted++;
  if (stripsWanted === 1) {
    if (!strips.get().data) void fetchStrips();
    stripsT = window.setInterval(() => {
      if (!stream.isPaused()) void fetchStrips();
    }, 60000);
  }
  let done = false;
  return () => {
    if (done) return;
    done = true;
    stripsWanted--;
    if (stripsWanted === 0 && stripsT !== null) {
      window.clearInterval(stripsT);
      stripsT = null;
    }
  };
}
