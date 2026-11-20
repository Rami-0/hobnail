import assert from "node:assert/strict";
import { test } from "node:test";
import { historyWindow, windowSteps } from "./window.ts";

test("history carries the window, comparison and timeline page in its URL", () => {
  const view = historyWindow(new URLSearchParams("range=7d&from=100&to=900&level=warn&cmp=1&before=3"));
  const params = new URLSearchParams(view.query);
  assert.deepEqual(Object.fromEntries(params), { range: "7d", from: "100", to: "900", level: "warn", cmp: "1", before: "3" });
  assert.equal(view.zoomed, true);
});

test("history discards malformed or incomplete arguments", () => {
  const view = historyWindow(new URLSearchParams("range=2h&from=20.5&to=900&level=x&before=-3"));
  assert.equal(view.query, "range=24h&level=incidents");
  assert.equal(view.zoomed, false);
});

test("window moves preserve the width and stop at the retained boundaries", () => {
  assert.deepEqual(windowSteps(400, 1000, 1000).in, { from: "550", to: "850", before: null });
  assert.equal(windowSteps(400, 1000, 1000).later, null);
  assert.equal(windowSteps(401, 1000, 1000).in, null);
  assert.equal(windowSteps(1000 - 30 * 86400, 1000, 1000).earlier, null);
});
