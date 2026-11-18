import assert from "node:assert/strict";
import { test } from "node:test";

import { createScheduler, createStore } from "./store.ts";

test("store: notifies on change only", () => {
  const s = createStore(1);
  let n = 0;
  const off = s.subscribe(() => n++);
  s.set(1);
  s.set(2);
  s.update((v) => v + 1);
  assert.equal(s.get(), 3);
  assert.equal(n, 2);
  off();
  s.set(4);
  assert.equal(n, 2);
});

function fakeFrames() {
  const frames: (() => void)[] = [];
  const timers: { fn: () => void; ms: number }[] = [];
  let busy = 0;
  const sch = createScheduler({
    frame: (fn) => frames.push(fn),
    setTimeout: (fn, ms) => timers.push({ fn, ms }),
    busyFor: () => busy,
  });
  return {
    sch,
    frames,
    timers,
    setBusy: (ms: number) => {
      busy = ms;
    },
    nextFrame: () => frames.shift()?.(),
  };
}

test("scheduler: everything queued before a frame runs in that one frame, in order", () => {
  const f = fakeFrames();
  const seen: number[] = [];
  f.sch.queue(() => seen.push(1));
  f.sch.queue(() => seen.push(2));
  f.sch.queue(() => seen.push(3));
  assert.equal(f.frames.length, 1, "one frame requested");
  assert.equal(seen.length, 0);
  f.nextFrame();
  assert.deepEqual(seen, [1, 2, 3]);
  f.sch.queue(() => seen.push(4));
  assert.equal(f.frames.length, 1);
  f.nextFrame();
  assert.deepEqual(seen, [1, 2, 3, 4]);
});

test("scheduler: held while the user scrolls, then applied", () => {
  const f = fakeFrames();
  const seen: number[] = [];
  f.setBusy(1200);
  f.sch.queue(() => seen.push(1));
  f.nextFrame();
  assert.equal(seen.length, 0);
  assert.equal(f.timers[0]?.ms, 1250);
  assert.equal(f.sch.pending(), 1);
  f.setBusy(0);
  f.timers.shift()?.fn();
  f.nextFrame();
  assert.deepEqual(seen, [1]);
});
