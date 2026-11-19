import { test } from "node:test";
import assert from "node:assert/strict";
import { Ring, ansi, jsonFold, matcher, filterRows, names, permalink, cursor, searchState, retryDelay, nextMatch, chipName, matchRanges, streamParams, paneHref } from "./model.ts";
test("ring caps newest lines and older history cannot evict current text", () => {
  const r = new Ring(3);
  r.append([1, 2, 3, 4].map((t) => ({ c: "a", t, m: String(t) })));
  assert.deepEqual(r.rows.map((row) => row.t), [2, 3, 4]);
  r.append([{ c: "a", t: 1, m: "old" }], true);
  assert.deepEqual(r.rows.map((row) => row.t), [2, 3, 4]);
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
  assert.deepEqual(f.rows.map((row) => row.plain), ["before", "after"]); assert.equal(f.counts.e, 1);
});
test("URL state, permalinks and reconnect cursor retain units and disable linked regex", () => {
  const q = new URLSearchParams("c=a,b&c=b,c&search=%5B&re=1&case=1");
  assert.deepEqual(names(q, "c"), ["a", "b", "c"]);
  assert.deepEqual(searchState(q), { search: "[", sensitive: true, regexFromLink: true });
  assert.equal(permalink("#t=123.5"), 123.5); assert.equal(permalink("#t=Infinity"), null);
  assert.equal(cursor([{ c: "a", m: "", t: 123000 }]), 123.000001);
  assert.equal(cursor([]), null); assert.equal(retryDelay(6), 30000);
});

test("match stepping merges panes chronologically and wraps across panes", () => {
  const a = { pane: "a", id: 1, t: 1 }, b = { pane: "b", id: 1, t: 2 }, c = { pane: "a", id: 2, t: 3 };
  assert.deepEqual(nextMatch([c, a, b], null, -1), c);
  assert.deepEqual(nextMatch([c, a, b], c, -1), b);
  assert.deepEqual(nextMatch([c, a, b], c, 1), a);
  assert.equal(nextMatch([], a, 1), null);
});
test("stack chips remove the project prefix and retain both identifying ends", () => {
  assert.equal(chipName("shop_web_1", "shop", 12), "web_1");
  assert.equal(chipName("shop-very-long-service-2", "shop", 12), "very-…vice-2");
  assert.equal(chipName("shopping_web", "shop", 20), "shopping_web");
});

test("highlight ranges preserve literal punctuation and reject invalid or empty matches", () => {
  assert.deepEqual(matchRanges("[a] [A]", "[a]", false, false), [[0,3],[4,7]]);
  assert.deepEqual(matchRanges("red RED", "red", true, true), [[0,3]]);
  assert.deepEqual(matchRanges("text", "[", true, false), []);
  assert.deepEqual(matchRanges("text", "^", true, false), []);
});

test("late stack batches retain the newest timestamps rather than the newest arrivals", () => {
  const ring = new Ring(3);
  ring.append([3,4,5].map((t) => ({c:"fast",t,m:String(t)})));
  ring.append([1,2].map((t) => ({c:"slow",t,m:String(t)})));
  assert.deepEqual(ring.rows.map((r) => r.t), [3,4,5]);
  ring.append([{c:"slow",t:6,m:"new"}]);
  assert.deepEqual(ring.rows.map((r) => r.t), [4,5,6]);
});

test("ranges, seeks and reconnects read a full ring while the initial live tail stays small", () => {
  const live = streamParams("c=web", "", 500, 5000, null, null, 500000);
  assert.equal(live.get("tail"), "500"); assert.equal(live.has("since"), false);
  const range = streamParams("stack=shop", "&since=1h", 500, 5000, null, null, 500000);
  assert.equal(range.get("tail"), "5000"); assert.equal(range.get("since"), "1h");
  const resume = streamParams("c=web", "", 500, 5000, 123000, null, 500000);
  assert.equal(resume.get("tail"), "5000"); assert.equal(resume.get("since"), "123.000001");
  const seek = streamParams("c=web", "", 500, 5000, null, 300000, 500000);
  assert.equal(seek.get("tail"), "5000"); assert.equal(seek.get("since"), "180");
  assert.equal(streamParams("c=web", "", 500, 5000, null, 600000, 500000).get("tail"), "500");
  assert.equal(streamParams("c=web", "", 500, 5000, null, 0, 500000).get("since"), "0");
});

test("pane selection links retain search, levels, range and permalink state", () => {
  const query = "?c=a,b&search=ERROR&case=1&re=1&range=1h&levels=ew&show=3";
  const close = new URL(paneHref(query, ["b"], "#t=123"), "https://test.invalid");
  assert.equal(close.searchParams.get("c"), "b"); assert.equal(close.hash, "#t=123");
  for (const [key,value] of new URLSearchParams(query)) if (key!=="c") assert.equal(close.searchParams.get(key), value);
  const pick = new URL(paneHref(query, ["a","b"], "#t=123", true), "https://test.invalid");
  assert.equal(pick.searchParams.has("c"),false); assert.equal(pick.searchParams.get("sel"), "a,b");
  assert.equal(pick.searchParams.get("range"), "1h"); assert.equal(pick.hash, "#t=123");
});

test("guards log regular expressions before matching or highlighting", () => {
  for (const query of ["(a+)+$", "((a+))+$", "(a|aa)+$", "a".repeat(257)]) {
    const search = matcher(query, true, false);
    assert.ok(search.error, query);
    assert.equal(search.test("a".repeat(1024) + "!"), false);
  }
  assert.equal(matcher("(?:warn|error): [0-9]+", true, false).test("Error: 42"), true);
});
