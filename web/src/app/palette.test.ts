import assert from "node:assert/strict";
import { test } from "node:test";
import { paletteIndex, rankPalette, type PaletteItem } from "./palette.ts";

const items: PaletteItem[] = [
  { kind: "Container", name: "prod-web", children: [{ kind: "Logs", name: "Logs: prod-web" }] },
  { kind: "Bookmark", name: "web", href: "https://example.com" },
  { kind: "Page", name: "Web details" },
];

test("exact names outrank prefixes and word matches, with child actions grouped beneath", () => {
  assert.deepEqual(rankPalette(items, " web ").map((item) => item.name), ["web", "Web details", "prod-web", "Logs: prod-web"]);
  assert.deepEqual(rankPalette(items, "logs: prod").map((item) => item.name), ["Logs: prod-web"]);
  assert.deepEqual(rankPalette(items, ""), [items[2], items[1], items[0]]);
  assert.deepEqual(rankPalette(items, "unknown"), []);
});

test("palette arrows wrap and page moves clamp", () => {
  assert.equal(paletteIndex("ArrowDown", -1, 1), 0);
  assert.equal(paletteIndex("ArrowUp", 0, 4), 3);
  assert.equal(paletteIndex("PageDown", 2, 6), 5);
});
