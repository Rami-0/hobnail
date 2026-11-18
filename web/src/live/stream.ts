// The live stream client: one EventSource per tab to
// /api/v1/stream?topics=…, whose topics follow what the mounted components
// asked for (`want`). No DOM here: the EventSource, timers and clock are
// injected (browser.ts passes the real ones, stream.test.ts fakes), so the
// connect / reconnect / pause rules are unit tested.
//
// Status: "connecting" until the first open, "live" while open,
// "reconnecting" after an error (the browser retries a dropped connection
// itself; a stream it closed for good is opened again after FIRST_RETRY_MS,
// doubling to MAX_RETRY_MS), "stopped" while paused (tab hidden for 10 s,
// page hidden or in the back/forward cache), with no topic wanted, or with
// no EventSource at all.
import { FIRST_RETRY_MS, nextRetryMs } from "./backoff.ts";

export type LiveStatus = "connecting" | "live" | "reconnecting" | "stopped";

/** Stream topics, in the order the URL lists them. */
export const TOPICS = ["containers", "stats", "system", "boards", "widget"] as const;
export type Topic = (typeof TOPICS)[number];

/** Event names the server sends (src/api/stream.rs). */
export const EVENTS = ["hello", "containers", "stats", "system", "config", "resync", "boards", "widget"] as const;
export type EventName = (typeof EVENTS)[number];

export interface StreamMessage {
  type: EventName;
  data: unknown;
}

/** The part of EventSource the client uses ("open", "error" and the named events). readyState 2 = CLOSED. */
export interface SourceLike {
  readyState: number;
  addEventListener(type: string, fn: (ev: { data?: unknown }) => void): void;
  close(): void;
}

export interface StreamDeps {
  /** A new EventSource, or null when the browser has none. */
  open: (url: string) => SourceLike | null;
  setTimeout: (fn: () => void, ms: number) => unknown;
  clearTimeout: (id: unknown) => void;
  /** Milliseconds (Date.now). */
  now: () => number;
}

export interface StreamInfo {
  status: LiveStatus;
  /** When the stream last delivered something or opened (ms), 0 before that. */
  lastAt: number;
  /** When the current "reconnecting" spell began (ms), else 0. */
  troubleSince: number;
}

export const STREAM_PATH = "/api/v1/stream";

/** The stream URL for a set of topics (TOPICS order), or null for none. */
export function streamUrl(topics: Iterable<Topic>): string | null {
  const want = new Set(topics);
  const list = TOPICS.filter((t) => want.has(t));
  return list.length ? `${STREAM_PATH}?topics=${list.join(",")}` : null;
}

export class LiveStream {
  private deps: StreamDeps;
  private sink: (m: StreamMessage) => void;
  private counts = new Map<Topic, number>();
  private es: SourceLike | null = null;
  private esUrl: string | null = null;
  private retryT: unknown = null;
  private retryMs = FIRST_RETRY_MS;
  private reconcileT: unknown = null;
  private paused = false;
  private unsupported = false;
  private info: StreamInfo = { status: "connecting", lastAt: 0, troubleSince: 0 };
  private listeners = new Set<() => void>();

  constructor(deps: StreamDeps, sink: (m: StreamMessage) => void) {
    this.deps = deps;
    this.sink = sink;
  }

  getInfo(): StreamInfo {
    return this.info;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  /** The current URL (for tests and the debug line), null when closed. */
  url(): string | null {
    return this.esUrl;
  }

  /** The wait before the next retry of a stream the browser closed. */
  nextRetry(): number {
    return this.retryMs;
  }

  /** Ask for a topic while mounted; the stream reconnects with the new set on the next tick. */
  want(topic: Topic): () => void {
    this.counts.set(topic, (this.counts.get(topic) ?? 0) + 1);
    this.schedule();
    let done = false;
    return () => {
      if (done) return;
      done = true;
      const n = (this.counts.get(topic) ?? 1) - 1;
      if (n <= 0) this.counts.delete(topic);
      else this.counts.set(topic, n);
      this.schedule();
    };
  }

  /** Close and stay closed (hidden tab, pagehide). */
  pause(): void {
    this.paused = true;
    this.cancelRetry();
    this.close();
    this.setStatus("stopped");
  }

  /** Open again at once (tab visible, back from the cache). */
  resume(): void {
    if (!this.paused) return;
    this.paused = false;
    this.reconcile();
  }

  isPaused(): boolean {
    return this.paused;
  }

  /** Apply the wanted topics now (normally one tick after `want`). */
  reconcile(): void {
    if (this.reconcileT !== null) {
      this.deps.clearTimeout(this.reconcileT);
      this.reconcileT = null;
    }
    const url = streamUrl(this.counts.keys());
    if (this.paused || url === null) {
      this.cancelRetry();
      this.close();
      this.setStatus("stopped");
      return;
    }
    if (this.es && this.esUrl === url) return;
    if (this.retryT !== null && !this.es) return; // a retry is already waiting
    this.close();
    this.connect(url);
  }

  private schedule(): void {
    if (this.reconcileT !== null) return;
    this.reconcileT = this.deps.setTimeout(() => {
      this.reconcileT = null;
      this.reconcile();
    }, 0);
  }

  private connect(url: string): void {
    if (this.unsupported) return;
    const s = this.deps.open(url);
    if (!s) {
      this.unsupported = true;
      this.setStatus("stopped");
      return;
    }
    this.es = s;
    this.esUrl = url;
    if (this.info.status === "stopped") this.setStatus("connecting");
    const mark = () => {
      this.info = { ...this.info, lastAt: this.deps.now() };
    };
    for (const type of EVENTS) {
      s.addEventListener(type, (ev) => {
        if (this.es !== s) return;
        let data: unknown = {};
        if (typeof ev.data === "string" && ev.data !== "") {
          try {
            data = JSON.parse(ev.data);
          } catch {
            return;
          }
        }
        mark();
        if (type === "hello") this.setLive();
        this.sink({ type, data });
      });
    }
    s.addEventListener("open", () => {
      if (this.es !== s) return;
      this.retryMs = FIRST_RETRY_MS;
      mark();
      this.setLive();
    });
    s.addEventListener("error", () => {
      if (this.es !== s) return;
      this.setStatus("reconnecting");
      if (s.readyState !== 2) return; // the browser retries this one itself
      s.close();
      this.es = null;
      this.esUrl = null;
      this.cancelRetry();
      const wait = this.retryMs;
      this.retryMs = nextRetryMs(this.retryMs);
      this.retryT = this.deps.setTimeout(() => {
        this.retryT = null;
        this.reconcile();
      }, wait);
    });
  }

  private setLive(): void {
    this.setStatus("live");
  }

  private close(): void {
    // Its listeners check `this.es` and ignore a closed source.
    this.es?.close();
    this.es = null;
    this.esUrl = null;
  }

  private cancelRetry(): void {
    if (this.retryT !== null) this.deps.clearTimeout(this.retryT);
    this.retryT = null;
  }

  private setStatus(status: LiveStatus): void {
    if (status === this.info.status) return;
    const troubleSince = status === "reconnecting" ? this.deps.now() : 0;
    this.info = { ...this.info, status, troubleSince };
    for (const l of this.listeners) l();
  }
}
