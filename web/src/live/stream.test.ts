import assert from "node:assert/strict";
import { test } from "node:test";

import { FIRST_RETRY_MS, MAX_RETRY_MS, nextRetryMs } from "./backoff.ts";
import { LiveStream, streamUrl, type SourceLike, type StreamMessage } from "./stream.ts";

class FakeSource implements SourceLike {
  readyState = 0;
  closed = false;
  handlers = new Map<string, (ev: { data?: unknown }) => void>();
  url: string;
  constructor(url: string) {
    this.url = url;
  }
  addEventListener(type: string, fn: (ev: { data?: unknown }) => void): void {
    this.handlers.set(type, fn);
  }
  close(): void {
    this.closed = true;
    this.readyState = 2;
  }
  open(): void {
    this.readyState = 1;
    this.handlers.get("open")?.({});
  }
  emit(type: string, data: string): void {
    this.handlers.get(type)?.({ data });
  }
  fail(closed: boolean): void {
    this.readyState = closed ? 2 : 0;
    this.handlers.get("error")?.({});
  }
}

function harness(supported = true) {
  const sources: FakeSource[] = [];
  const timers = new Map<number, { fn: () => void; ms: number }>();
  let id = 0;
  let now = 1000;
  const got: StreamMessage[] = [];
  const s = new LiveStream(
    {
      open: (url) => {
        if (!supported) return null;
        const f = new FakeSource(url);
        sources.push(f);
        return f;
      },
      setTimeout: (fn, ms) => {
        timers.set(++id, { fn, ms });
        return id;
      },
      clearTimeout: (t) => {
        timers.delete(t as number);
      },
      now: () => now,
    },
    (m) => got.push(m),
  );
  /** Run timers due within `ms` (all zero-delay ones at least). */
  const run = (ms = 0) => {
    for (const [k, t] of Array.from(timers)) {
      if (t.ms <= ms) {
        timers.delete(k);
        t.fn();
      }
    }
  };
  return {
    s,
    sources,
    timers,
    got,
    run,
    last: () => sources[sources.length - 1],
    advance: (ms: number) => {
      now += ms;
    },
  };
}

test("backoff: 5 s, doubling, a minute at most", () => {
  const seen = [FIRST_RETRY_MS];
  for (let i = 0; i < 6; i++) seen.push(nextRetryMs(seen[seen.length - 1] ?? 0));
  assert.deepEqual(seen, [5000, 10000, 20000, 40000, 60000, 60000, 60000]);
  assert.equal(MAX_RETRY_MS, 60000);
});

test("url lists the wanted topics in a fixed order", () => {
  assert.equal(streamUrl(["system", "containers"]), "/api/v1/stream?topics=containers,system");
  assert.equal(streamUrl([]), null);
});

test("topics follow subscribers; changes are coalesced into one reconnect", () => {
  const h = harness();
  const a = h.s.want("system");
  const b = h.s.want("containers");
  assert.equal(h.sources.length, 0, "nothing opens before the tick");
  h.run();
  assert.equal(h.sources.length, 1);
  assert.equal(h.last()?.url, "/api/v1/stream?topics=containers,system");
  const c = h.s.want("system"); // already wanted: no reconnect
  h.run();
  assert.equal(h.sources.length, 1);
  c();
  h.run();
  assert.equal(h.sources.length, 1);
  b();
  h.run();
  assert.equal(h.sources.length, 2);
  assert.equal(h.sources[0]?.closed, true);
  assert.equal(h.last()?.url, "/api/v1/stream?topics=system");
  a();
  h.run();
  assert.equal(h.last()?.closed, true);
  assert.equal(h.s.getInfo().status, "stopped");
});

test("status: connecting, live on open and hello, messages parsed and passed on", () => {
  const h = harness();
  h.s.want("containers");
  h.run();
  assert.equal(h.s.getInfo().status, "connecting");
  h.last()?.open();
  assert.equal(h.s.getInfo().status, "live");
  h.last()?.emit("stats", '{"at":5,"containers":{}}');
  h.last()?.emit("stats", "not json");
  h.last()?.emit("config", "");
  assert.deepEqual(h.got, [
    { type: "stats", data: { at: 5, containers: {} } },
    { type: "config", data: {} },
  ]);
});

test("a dropped connection the browser retries: reconnecting, no new source", () => {
  const h = harness();
  h.s.want("system");
  h.run();
  h.last()?.open();
  h.advance(10);
  h.last()?.fail(false);
  assert.equal(h.s.getInfo().status, "reconnecting");
  assert.equal(h.s.getInfo().troubleSince, 1010);
  assert.equal(h.sources.length, 1);
  h.last()?.open();
  assert.equal(h.s.getInfo().status, "live");
  assert.equal(h.s.getInfo().troubleSince, 0);
});

test("a stream closed for good is opened again with backoff; an open resets it", () => {
  const h = harness();
  h.s.want("system");
  h.run();
  h.last()?.fail(true);
  assert.equal(h.sources.length, 1);
  assert.deepEqual([...h.timers.values()].map((t) => t.ms), [5000]);
  h.run(5000);
  assert.equal(h.sources.length, 2);
  h.last()?.fail(true);
  assert.deepEqual([...h.timers.values()].map((t) => t.ms), [10000]);
  // A topic change while a retry waits does not open early.
  h.s.want("stats");
  h.run(0);
  assert.equal(h.sources.length, 2);
  h.run(10000);
  assert.equal(h.sources.length, 3);
  assert.equal(h.last()?.url, "/api/v1/stream?topics=stats,system");
  h.last()?.open();
  assert.equal(h.s.nextRetry(), 5000);
});

test("pause closes and stays closed; resume opens at once", () => {
  const h = harness();
  h.s.want("containers");
  h.run();
  h.last()?.open();
  h.s.pause();
  assert.equal(h.last()?.closed, true);
  assert.equal(h.s.getInfo().status, "stopped");
  h.s.want("system");
  h.run();
  assert.equal(h.sources.length, 1, "no reconnect while paused");
  h.s.resume();
  assert.equal(h.sources.length, 2);
  assert.equal(h.s.getInfo().status, "connecting");
  assert.equal(h.last()?.url, "/api/v1/stream?topics=containers,system");
});

test("pause cancels a waiting retry", () => {
  const h = harness();
  h.s.want("containers");
  h.run();
  h.last()?.fail(true);
  assert.equal(h.timers.size, 1);
  h.s.pause();
  assert.equal(h.timers.size, 0);
});

test("no EventSource: stopped, never retried", () => {
  const h = harness(false);
  h.s.want("containers");
  h.run();
  assert.equal(h.s.getInfo().status, "stopped");
  assert.equal(h.timers.size, 0);
});

test("events of a closed source are ignored", () => {
  const h = harness();
  const off = h.s.want("containers");
  h.run();
  const first = h.last();
  off();
  h.run();
  first?.emit("stats", "{}");
  assert.equal(h.got.length, 0);
});
