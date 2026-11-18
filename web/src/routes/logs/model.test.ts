import { test } from "node:test";
import assert from "node:assert/strict";
import { Ring, ansi, jsonFold, matcher, filterRows, names, permalink, cursor, searchState, retryDelay } from "./model.ts";
test("ring caps newest lines and older history cannot evict current text", () => {
  const r = new Ring(3);
  r.append([1, 2, 3, 4].map((t) => ({ c: "a", t, m: String(t) })));
  assert.deepEqual(r.rows.map((r) => r.t), [2, 3, 4]);
  r.append([{ c: "a", t: 1, m: "old" }], true);
  assert.deepEqual(r.rows.map((r) => r.t), [2, 3, 4]);
});
test("ANSI strips controls, preserves hostile text and emits SGR spans", () => {
  const s = ansi("\x1b[1;31m<script>bad</script>\x1b[0m\x1b]8;;javascript:bad\x07hello\x07");
  assert.equal(s.map((v) => v.text).join(""), "<script>bad</script>hello");
  assert.equal(s[0]?.classes, "log-bold"); assert.ok(s[0]?.fg);
  assert.equal(ansi("\x1b[38;2;1;2;3mRGB")[0]?.fg, "rgb(1,2,3)");
  assert.equal(ansi("\x1b[48;5;232mBG")[0]?.bg, "rgb(8,8,8)");
});
test("JSON folds objects, message plus six scalars, never arrays or malformed JSON", () => {
  assert.equal(jsonFold('[1,2]'), null); assert.equal(jsonFold('{bad}'), null);
  assert.equal(jsonFold('{"msg":"hi","a":1,"nested":{}}')?.summary, "hi · a=1");
});
test("plain, case and regex searches tolerate invalid patterns", () => {
  assert.equal(matcher("ERROR", false, false).test("error"), true);
  assert.equal(matcher("ERROR", false, true).test("error"), false);
  assert.equal(matcher("^e.*r$", true, false).test("Error"), true);
  assert.ok(matcher("[", true, false).error);
});
test("levels and matching context compose", () => {
  const r = new Ring(20); r.append(["before", "hit", "after", "no"].map((m, t) => ({ c: "a", t, m, l: t === 1 ? "e" : "i" })));
  const f = filterRows(r.rows, matcher("hit", false, false).test, true, 1, "i");
  assert.deepEqual(f.rows.map((r) => r.plain), ["before", "after"]); assert.equal(f.counts.e, 1);
});
test("URL state, permalinks and reconnect cursor retain units and disable linked regex", () => {
  const q = new URLSearchParams("c=a,b&c=b,c&search=%5B&re=1&case=1");
  assert.deepEqual(names(q, "c"), ["a", "b", "c"]);
  assert.deepEqual(searchState(q), { search: "[", sensitive: true, regexFromLink: true });
  assert.equal(permalink("#t=123.5"), 123.5); assert.equal(permalink("#t=Infinity"), null);
  assert.equal(cursor([{ c: "a", m: "", t: 123000 }]), 123.000001);
  assert.equal(cursor([]), null); assert.equal(retryDelay(6), 30000);
});
