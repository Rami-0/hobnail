import { test } from "node:test";
import assert from "node:assert/strict";
import { avatar, DEFAULT_APPEARANCE, dirty, freshSuggestions, iconSource, minSize, needsSecret, payloadIdentity, rebase, recoveryBase, reduceDocument } from "./document.ts";
import type { BoardDocument } from "../api/gen/BoardDocument.ts";
const base: BoardDocument = { version: 3, name: "Home", appearance: DEFAULT_APPEARANCE, sections: [{ id: "s", title: "Services", position: 0, collapsed: false, items: [{ id: "i", x: 0, y: 0, w: 4, h: 1, payload: { kind: "link", title: "Site", url: "https://example.com" } }] }] };
test("add entries without IDs, preserve server IDs, never mutate the input", () => {
  const next = reduceDocument(base, { type: "section", title: "Links" });
  assert.equal(next.sections.length, 2); assert.equal(next.sections[1]?.id, undefined); assert.equal(next.sections[0]?.items[0]?.id, "i"); assert.equal(base.sections.length, 1);
  const added = reduceDocument(next, { type: "add", section: 1, payload: { kind: "link", title: "New", url: "https://example.com" } });
  assert.equal(added.sections[1]?.items[0]?.id, undefined);
});
test("add into the first free slot; move, resize, transfer and delete keep a compact layout", () => {
  let doc = reduceDocument(base, { type: "add", section: 0, payload: { kind: "widget", widget_id: "w" } });
  assert.deepEqual(doc.sections[0]?.items[1], { x: 4, y: 0, w: 4, h: 3, payload: { kind: "widget", widget_id: "w" } });
  doc = reduceDocument(doc, { type: "layout", section: 0, rects: [{ x: 0, y: 5, w: 4, h: 1 }, { x: 4, y: 0, w: 8, h: 3 }] });
  assert.deepEqual(doc.sections[0]?.items.map((i) => [i.x, i.y, i.w, i.h]), [[0, 0, 4, 1], [4, 0, 8, 3]]);
  doc = reduceDocument(doc, { type: "section", title: "Other" });
  doc = reduceDocument(doc, { type: "transfer", section: 0, item: 0, toSection: 1, x: 6, y: 4 });
  assert.equal(doc.sections[1]?.items[0]?.id, "i"); assert.deepEqual([doc.sections[1]?.items[0]?.x, doc.sections[1]?.items[0]?.y], [6, 0]);
  assert.equal(doc.sections[0]?.items.length, 1);
  doc = reduceDocument(doc, { type: "collapse", section: 1, collapsed: true }); assert.equal(doc.sections[1]?.collapsed, true);
  doc = reduceDocument(doc, { type: "look", section: 1, item: 0, look: { accent: "rose", hide_status: false, hide_metrics: false } });
  assert.equal(doc.sections[1]?.items[0]?.look?.accent, "rose");
  doc = reduceDocument(doc, { type: "look", section: 1, item: 0, look: { hide_status: false, hide_metrics: false } });
  assert.equal("look" in (doc.sections[1]?.items[0] ?? {}), false);
  doc = reduceDocument(doc, { type: "delete", section: 1, item: 0 }); assert.equal(doc.sections[1]?.items.length, 0);
  doc = reduceDocument(doc, { type: "move-section", section: 1, to: 0 }); assert.equal(doc.sections[0]?.title, "Other");
  doc = reduceDocument(doc, { type: "delete-section", section: 0 }); assert.equal(doc.sections[0]?.id, "s");
  doc = reduceDocument(doc, { type: "board", name: "Media", icon: null, appearance: { ...DEFAULT_APPEARANCE, surface: "glass" } });
  assert.equal(doc.name, "Media"); assert.equal(doc.appearance.surface, "glass"); assert.equal("icon" in doc, false);
});
test("minimum sizes come from the widget when it declares one", () => {
  assert.deepEqual(minSize("widget"), { w: 2, h: 2 });
  assert.deepEqual(minSize("widget", { min_size: { w: 6, h: 3 } }), { w: 6, h: 3 });
  assert.deepEqual(minSize("link", { min_size: null }), { w: 2, h: 1 });
});
test("dirty tracking ignores version alone and detects layout changes", () => {
  assert.equal(dirty(base, { ...base, version: 9 }), false); assert.equal(dirty(base, reduceDocument(base, { type: "rename", section: 0, title: "Changed" })), true);
  assert.equal(dirty(base, reduceDocument(base, { type: "board", appearance: { ...DEFAULT_APPEARANCE, density: "compact" } })), true);
});
test("rebase uses current version, keeps local changes and unrelated remote additions", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Mine" });
  const current = structuredClone(base); current.version = 4; current.sections.push({ id: "remote", title: "Remote", position: 1, collapsed: false, items: [] }); current.name = "Theirs";
  const next = rebase(base, mine, current); assert.equal(next.version, 4); assert.equal(next.sections[0]?.title, "Mine"); assert.equal(next.sections[1]?.id, "remote");
  assert.equal(next.name, "Theirs");
  const renamed = rebase(base, reduceDocument(base, { type: "board", name: "Mine" }), current); assert.equal(renamed.name, "Mine");
});
test("rebase recreates edited deleted entries without stale IDs", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Keep" });
  const next = rebase(base, mine, { version: 4, appearance: DEFAULT_APPEARANCE, sections: [] }); assert.equal(next.sections[0]?.id, undefined); assert.equal(next.sections[0]?.items[0]?.id, undefined);
});
test("icons are opt-in same-origin requests with letter fallback", () => {
  assert.equal(iconSource("grafana", false), null); assert.equal(iconSource("grafana", true), "/api/v1/icons/grafana"); assert.equal(iconSource("https://evil", true), null); assert.equal(avatar("  service"), "S"); assert.equal(avatar(""), "?");
});
test("a credential follows its origin, including scheme and effective port", () => {
  assert.equal(needsSecret("https://example.com", "https://example.com:443/path"), false);
  assert.equal(needsSecret("https://example.com", "http://example.com"), true); assert.equal(needsSecret("https://example.com", "https://example.com:444"), true); assert.equal(needsSecret(null, "https://example.com"), false);
});

test("rebase preserves remote item moves when local layout is unchanged", () => {
  const original = reduceDocument(base, { type: "section", title: "Other" }); if (original.sections[1]) original.sections[1].id = "other";
  const remote = reduceDocument(original, { type: "transfer", section: 0, item: 0, toSection: 1, x: 0, y: 0 }); remote.version++;
  const merged = rebase(original, original, remote);
  assert.equal(merged.sections[0]?.items.length, 0); assert.equal(merged.sections[1]?.items[0]?.id, "i");
});

test("stale recovered drafts preserve server additions and strip deleted item IDs", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Recovered" });
  const current = structuredClone(base); current.version = 5;
  const section = current.sections[0];
  if (section) section.items = [{ x: 0, y: 0, w: 4, h: 1, id: "remote-item", payload: { kind: "link", title: "Remote link", url: "https://example.org" } }];
  current.sections.push({ id: "new-remote", title: "New remote section", position: 1, collapsed: false, items: [] });
  const merged = rebase(recoveryBase(mine, current), mine, current);
  assert.equal(merged.sections[0]?.title, "Recovered");
  assert.equal(merged.sections[0]?.items[0]?.id, undefined);
  assert.equal(merged.sections[0]?.items[1]?.id, "remote-item");
  assert.equal(merged.sections[1]?.id, "new-remote");
});
const suggestion = (title: string, url: string) => ({ title, payload: { kind: "link" as const, title, url } });
test("fresh suggestions skip taken targets and repeat identities, keeping order", () => {
  const list = [suggestion("A", "https://a.example/"), suggestion("A again", "HTTPS://A.EXAMPLE:443/"), suggestion("B", "https://b.example/x"), suggestion("C", "https://c.example/")];
  const fresh = freshSuggestions(list, [payloadIdentity({ kind: "link", title: "", url: "https://c.example/" })]);
  assert.deepEqual(fresh.map((s) => s.title), ["A", "B"]);
  const keys = fresh.map((s) => payloadIdentity(s.payload));
  assert.equal(new Set(keys).size, keys.length);
  assert.deepEqual(freshSuggestions([], []), []);
});

test("concurrent moves into the same cell are compacted apart", () => {
  const two = structuredClone(base); two.sections[0]?.items.push({ id: "j", x: 4, y: 0, w: 4, h: 1, payload: { kind: "link", title: "Two", url: "https://two.example" } });
  const mine = reduceDocument(two, { type: "layout", section: 0, rects: [{ x: 8, y: 0, w: 4, h: 1 }, { x: 4, y: 0, w: 4, h: 1 }] });
  const current = reduceDocument(two, { type: "layout", section: 0, rects: [{ x: 0, y: 0, w: 4, h: 1 }, { x: 8, y: 0, w: 4, h: 1 }] }); current.version = 4;
  const merged = rebase(two, mine, current).sections[0]?.items ?? [];
  assert.deepEqual(merged.map((i) => [i.id, i.x, i.y]), [["i", 8, 0], ["j", 8, 1]]);
});
