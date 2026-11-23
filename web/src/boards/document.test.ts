import { test } from "node:test";
import assert from "node:assert/strict";
import { avatar, clampSpans, dirty, freshSuggestions, iconSource, needsSecret, payloadIdentity, rebase, recoveryBase, reduceDocument } from "./document.ts";
import type { BoardDocument } from "../api/gen/BoardDocument.ts";
const base: BoardDocument = { version: 3, sections: [{ id: "s", title: "Services", position: 0, items: [{ id: "i", position: 0, spans: { mobile: 4, tablet: 4, desktop: 4, rows: 1 }, payload: { kind: "link", title: "Site", url: "https://example.com" } }] }] };
test("add entries without IDs, preserve server IDs, never mutate the input", () => {
  const next = reduceDocument(base, { type: "section", title: "Links" });
  assert.equal(next.sections.length, 2); assert.equal(next.sections[1]?.id, undefined); assert.equal(next.sections[0]?.items[0]?.id, "i"); assert.equal(base.sections.length, 1);
  const added = reduceDocument(next, { type: "add", section: 1, payload: { kind: "link", title: "New", url: "https://example.com" } });
  assert.equal(added.sections[1]?.items[0]?.id, undefined);
});
test("move within and between sections, resize and delete retain valid positions", () => {
  let doc = reduceDocument(base, { type: "section", title: "Other" });
  doc = reduceDocument(doc, { type: "move", section: 0, item: 0, toSection: 1, to: 0 });
  assert.equal(doc.sections[1]?.items[0]?.id, "i"); assert.equal(doc.sections[0]?.items.length, 0);
  doc = reduceDocument(doc, { type: "resize", section: 1, item: 0, spans: { mobile: 9, tablet: -1, desktop: 30, rows: 0 } });
  assert.deepEqual(doc.sections[1]?.items[0]?.spans, { mobile: 4, tablet: 1, desktop: 12, rows: 1 });
  doc = reduceDocument(doc, { type: "delete", section: 1, item: 0 }); assert.equal(doc.sections[1]?.items.length, 0);
  doc = reduceDocument(doc, { type: "move-section", section: 1, to: 0 }); assert.equal(doc.sections[0]?.title, "Other");
  doc = reduceDocument(doc, { type: "delete-section", section: 0 }); assert.equal(doc.sections[0]?.id, "s");
});
test("span clamping covers all breakpoints and non-finite values", () => {
  assert.deepEqual(clampSpans({ mobile: NaN, tablet: 99, desktop: 0, rows: 99 }), { mobile: 1, tablet: 8, desktop: 1, rows: 12 });
});
test("dirty tracking ignores version alone and detects layout changes", () => {
  assert.equal(dirty(base, { ...base, version: 9 }), false); assert.equal(dirty(base, reduceDocument(base, { type: "rename", section: 0, title: "Changed" })), true);
});
test("rebase uses current version, keeps local changes and unrelated remote additions", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Mine" });
  const current = structuredClone(base); current.version = 4; current.sections.push({ id: "remote", title: "Remote", position: 1, items: [] });
  const next = rebase(base, mine, current); assert.equal(next.version, 4); assert.equal(next.sections[0]?.title, "Mine"); assert.equal(next.sections[1]?.id, "remote");
});
test("rebase recreates edited deleted entries without stale IDs", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Keep" });
  const next = rebase(base, mine, { version: 4, sections: [] }); assert.equal(next.sections[0]?.id, undefined); assert.equal(next.sections[0]?.items[0]?.id, undefined);
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
  const remote = reduceDocument(original, { type: "move", section: 0, item: 0, toSection: 1, to: 0 }); remote.version++;
  const merged = rebase(original, original, remote);
  assert.equal(merged.sections[0]?.items.length, 0); assert.equal(merged.sections[1]?.items[0]?.id, "i");
});

test("stale recovered drafts preserve server additions and strip deleted item IDs", () => {
  const mine = reduceDocument(base, { type: "rename", section: 0, title: "Recovered" });
  const current = structuredClone(base); current.version = 5;
  const section = current.sections[0];
  if (section) section.items = [{ position: 0, id: "remote-item", spans: { mobile: 4, tablet: 4, desktop: 4, rows: 1 }, payload: { kind: "link", title: "Remote link", url: "https://example.org" } }];
  current.sections.push({ id: "new-remote", title: "New remote section", position: 1, items: [] });
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

test("tile sizes map saved spans to grid columns and rows", async () => {
  const { tileCols, tileSize, stepSize, TILE_SIZES } = await import("./document.ts");
  assert.equal(tileCols({ mobile: 4, tablet: 4, desktop: 4, rows: 1 }), 1);
  assert.equal(tileCols({ mobile: 4, tablet: 8, desktop: 6, rows: 1 }), 2);
  assert.equal(tileCols({ mobile: 4, tablet: 8, desktop: 12, rows: 1 }), 3);
  for (const [name, { spans }] of Object.entries(TILE_SIZES)) assert.equal(tileSize(spans), name);
  assert.deepEqual(stepSize(TILE_SIZES.small.spans, "ArrowRight"), TILE_SIZES.wide.spans);
  assert.deepEqual(stepSize(TILE_SIZES.small.spans, "ArrowDown"), TILE_SIZES.tall.spans);
  assert.deepEqual(stepSize(TILE_SIZES.small.spans, "ArrowLeft"), TILE_SIZES.small.spans);
});
