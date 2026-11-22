// React hooks over the live layer (runtime.ts). Each hook asks for what it
// shows while mounted: the stream's topics follow the mounted hooks, and
// the first one to mount fetches the initial data.
import { useCallback, useDeferredValue, useEffect, useSyncExternalStore } from "react";

import type { Bootstrap } from "../api/gen/Bootstrap.ts";
import type { ContainersView } from "../api/gen/ContainersView.ts";
import type { Strips } from "../api/gen/Strips.ts";
import type { SystemView } from "../api/gen/SystemView.ts";
import {
  bootstrap,
  containers,
  hiddenProblems,
  newBuild,
  rings,
  skew,
  stats,
  stream,
  strips,
  system,
  wantBootstrap,
  wantContainers,
  wantStrips,
  wantTopic,
  type Loaded,
  type VitalsRings,
} from "./runtime.ts";
import type { Stat } from "./stats.ts";
import { createStore, type Store } from "./store.ts";
import type { LiveStatus, StreamInfo } from "./stream.ts";

function useStore<T>(s: Store<T>): T {
  return useSyncExternalStore(s.subscribe, s.get, s.get);
}

const subscribeStream = (fn: () => void) => stream.subscribe(fn);
const getInfo = () => stream.getInfo();
const getStatus = () => stream.getInfo().status;

/** "connecting" | "live" | "reconnecting" | "stopped". */
export function useLiveStatus(): LiveStatus {
  return useSyncExternalStore(subscribeStream, getStatus, getStatus);
}

/** The status with its times (the "stopped at HH:MM" pill). */
export function useLiveInfo(): StreamInfo {
  return useSyncExternalStore(subscribeStream, getInfo, getInfo);
}

/** GET /api/v1/bootstrap, refetched after a config change. Null until it answers. */
export function useBootstrap(): Bootstrap | null {
  useEffect(() => wantBootstrap(), []);
  return useStore(bootstrap).data;
}

/** The bootstrap with the last fetch error. */
export function useBootstrapState(): Loaded<Bootstrap> {
  useEffect(() => wantBootstrap(), []);
  return useStore(bootstrap);
}

/**
 * The containers view: GET /api/v1/containers, then the stream's
 * `containers` events (list or state changed). `stats` events do NOT change
 * it (its identity stays): the live CPU and memory are in `useStat` /
 * `stats` (the rows' cpu_pct/mem_bytes are those of the last list).
 * `error` says why the last fetch failed.
 */
export function useContainers(): { data: ContainersView | null; error: string | null } {
  useEffect(() => wantContainers(), []);
  return useStore(containers);
}

/** One container's live CPU and memory; re-renders only when they change. */
export function useStat(name: string): Stat {
  useEffect(() => wantContainers(), []);
  const sub = useCallback((fn: () => void) => stats.subscribe(name, fn), [name]);
  const get = useCallback(() => stats.get(name), [name]);
  return useSyncExternalStore(sub, get, get);
}

/**
 * Every container's live numbers (a new map per change), for a sort by CPU
 * or memory. Disabled, it does not follow the changes (no re-render).
 */
export function useStatsMap(enabled: boolean): ReadonlyMap<string, Stat> {
  const sub = useCallback((fn: () => void) => (enabled ? stats.subscribeAll(fn) : () => {}), [enabled]);
  return useDeferredValue(useSyncExternalStore(sub, stats.snapshot, stats.snapshot));
}

/** The latest host sample from the `system` topic; null until the first one. */
export function useSystem(): SystemView | null {
  useEffect(() => wantTopic("system"), []);
  return useStore(system);
}

/** The vitals sparklines' points (last hour from history, then live). */
export function useVitalsRings(): VitalsRings {
  useEffect(() => wantTopic("system"), []);
  return useStore(rings);
}

/** GET /api/v1/strips (24 h health strips), refreshed every minute while shown. */
export function useStrips(): Strips | null {
  useEffect(() => wantStrips(), []);
  return useStore(strips).data;
}

/** True once the server runs a different build than this page. */
export function useNewBuild(): boolean {
  return useStore(newBuild);
}

/** /alarm.json's count while the stream is paused (hidden tab), else null. */
export function useHiddenProblems(): number | null {
  return useStore(hiddenProblems);
}

// ---- the clock: server-corrected unix seconds, ticking every 30 s ----

const clock = createStore(Math.floor(Date.now() / 1000));
let ticking = 0;
let tickT: number | null = null;
function tick(): void {
  clock.set(Math.floor(Date.now() / 1000 + skew.get()));
}

/**
 * Now in server time (unix seconds), re-rendering every 30 s, for relative
 * times ("5m ago") that stay true while the page is open.
 */
export function useNow(): number {
  useEffect(() => {
    ticking++;
    tick();
    if (ticking === 1) tickT = window.setInterval(tick, 30000);
    const off = skew.subscribe(tick);
    return () => {
      off();
      ticking--;
      if (ticking === 0 && tickT !== null) {
        window.clearInterval(tickT);
        tickT = null;
      }
    };
  }, []);
  return useStore(clock);
}
