import assert from "node:assert/strict";
import { test } from "node:test";

import { apiQuery, historyNote, logsAt, parseWindow, rangeSearch, resetSearch, tabs, withParams, zoomSearch } from "./range.ts";

test("the window from the URL", () => {
  assert.deepEqual(parseWindow(""), { kind: "range", range: "6h" });
  assert.deepEqual(parseWindow("?range=7d"), { kind: "range", range: "7d" });
  assert.deepEqual(parseWindow("?range=9y"), { kind: "range", range: "6h" });
  assert.deepEqual(parseWindow("?from=100&to=200&range=1h"), { kind: "zoom", from: 100, to: 200 });
  assert.deepEqual(parseWindow("?from=200&to=100"), { kind: "range", range: "6h" }, "backwards is no zoom");
  assert.deepEqual(parseWindow("?from=x&to=1"), { kind: "range", range: "6h" });
  assert.equal(apiQuery({ kind: "zoom", from: 1, to: 2 }), "from=1&to=2");
  assert.equal(apiQuery({ kind: "range", range: "24h" }), "range=24h");
});

test("links keep the rest of the query", () => {
  assert.equal(rangeSearch("?from=1&to=2&edit=override", "24h"), "?edit=override&range=24h");
  assert.equal(rangeSearch("?range=24h", "6h"), "?range=6h");
  assert.equal(zoomSearch("?range=24h", 10, 20), "?from=10&to=20");
  assert.equal(resetSearch("?from=10&to=20", "24h"), "?range=24h");
  assert.equal(withParams("?a=1", { edit: "override" }), "?a=1&edit=override");
  assert.equal(withParams("?edit=override", { edit: null }), "");
});

test("tabs keep the range and point at the old UI", () => {
  const t = tabs("my app", "24h");
  assert.deepEqual(
    t.map((x) => x.href),
    [
      "/app/containers/my%20app?range=24h",
      "/containers/my%20app/logs?range=24h",
      "/containers/my%20app?tab=metrics&range=24h",
      "/containers/my%20app?tab=inspect&range=24h",
      "/containers/my%20app?tab=processes&range=24h",
      "/containers/my%20app?tab=events&range=24h",
    ],
  );
  assert.equal(tabs("x", "6h")[0]?.href, "/app/containers/x");
  assert.equal(logsAt("web", 100), "/containers/web/logs#t=100000");
  assert.equal(logsAt("web", null), "/containers/web/logs");
});

test("history note (OV-43)", () => {
  const now = 1_000_000;
  assert.equal(historyNote(null, "6h", now), null);
  assert.equal(historyNote(now - 4 * 3600, "6h", now), null, "more than half the range has data");
  assert.deepEqual(historyNote(now - 1800, "6h", now), { have: 1800, fit: "1h" });
  assert.deepEqual(historyNote(now - 2 * 86400, "30d", now), { have: 172800, fit: "7d" });
  assert.deepEqual(historyNote(now - 600, "1h", now), { have: 600, fit: null });
});
