import assert from "node:assert/strict";
import { test } from "node:test";

import { addToast, dismissToast, MAX_TOASTS, releaseToasts, toastMs, type ToastItem, type ToastState } from "./toast.ts";

const t = (id: number): ToastItem => ({ id, text: `t${id}`, kind: "ok", ms: 4500 });
const empty: ToastState = { shown: [], queued: [] };

test("at most three, the oldest goes", () => {
  let s = empty;
  for (let i = 1; i <= 5; i++) s = addToast(s, t(i), false);
  assert.equal(MAX_TOASTS, 3);
  assert.deepEqual(
    s.shown.map((x) => x.id),
    [3, 4, 5],
  );
});

test("never over a dialog: queued, then shown when it closes", () => {
  let s = addToast(empty, t(1), false);
  s = addToast(s, t(2), true);
  s = addToast(s, t(3), true);
  assert.deepEqual(
    s.shown.map((x) => x.id),
    [1],
  );
  assert.deepEqual(
    s.queued.map((x) => x.id),
    [2, 3],
  );
  s = releaseToasts(s);
  assert.deepEqual(
    s.shown.map((x) => x.id),
    [1, 2, 3],
  );
  assert.equal(s.queued.length, 0);
  assert.equal(releaseToasts(s), s);
});

test("dismiss removes from either list; unknown ids change nothing", () => {
  let s = addToast(empty, t(1), false);
  s = addToast(s, t(2), true);
  assert.equal(dismissToast(s, 9), s);
  s = dismissToast(s, 2);
  assert.equal(s.queued.length, 0);
  s = dismissToast(s, 1);
  assert.equal(s.shown.length, 0);
});

test("durations: errors and sticky ones stay; an action gives more time", () => {
  assert.equal(toastMs({}), 4500);
  assert.equal(toastMs({ action: { label: "Undo", run: () => {} } }), 8000);
  assert.equal(toastMs({ ms: 2500 }), 2500);
  assert.equal(toastMs({ kind: "bad" }), null);
  assert.equal(toastMs({ sticky: true }), null);
});
