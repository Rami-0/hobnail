// Tiny external stores for useSyncExternalStore, and the frame scheduler
// that applies stream messages: everything that arrived since the last
// frame is applied in one animation frame, so React renders at most once
// per frame however many events came in (plan 4.9). While the user is
// touching or scrolling the page, the frame waits until they stop
// so updates never move what is being read. Injected timers,
// no DOM (store.test.ts).

export interface Store<T> {
  get(): T;
  set(next: T): void;
  update(fn: (cur: T) => T): void;
  subscribe(fn: () => void): () => void;
}

export function createStore<T>(initial: T): Store<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next) {
      if (Object.is(next, value)) return;
      value = next;
      for (const l of listeners) l();
    },
    update(fn) {
      this.set(fn(value));
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };
}

export interface SchedulerDeps {
  /** requestAnimationFrame. */
  frame: (fn: () => void) => unknown;
  setTimeout: (fn: () => void, ms: number) => unknown;
  /** Milliseconds the user is still busy (touching, scrolling); 0 when idle. */
  busyFor: () => number;
}

export interface Scheduler {
  /** Run `fn` in the next frame (in arrival order with everything else queued). */
  queue(fn: () => void): void;
  /** Queued and not yet applied. */
  pending(): number;
}

export function createScheduler(deps: SchedulerDeps): Scheduler {
  const jobs: (() => void)[] = [];
  let armed = false;
  const arm = () => {
    if (armed) return;
    armed = true;
    deps.frame(run);
  };
  const run = () => {
    const wait = deps.busyFor();
    if (wait > 0) {
      deps.setTimeout(() => {
        armed = false;
        arm();
      }, wait + 50);
      return;
    }
    armed = false;
    const now = jobs.splice(0, jobs.length);
    for (const j of now) j();
  };
  return {
    queue(fn) {
      jobs.push(fn);
      arm();
    },
    pending: () => jobs.length,
  };
}
