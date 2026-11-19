import assert from "node:assert/strict";
import { test } from "node:test";

import { THEME_KEY, THEMES, isTheme, nextTheme, readTheme, themeOf, writeTheme, type Theme, type ThemeStorage } from "./theme.ts";

test("cycle order matches the server-rendered UI: auto, dark, light, ember, contrast", () => {
  assert.deepEqual([...THEMES], ["auto", "dark", "light", "ember", "contrast"]);
  const seen: Theme[] = [];
  let t: Theme = "auto";
  for (let i = 0; i < 6; i++) {
    seen.push(t);
    t = nextTheme(t);
  }
  assert.deepEqual(seen, ["auto", "dark", "light", "ember", "contrast", "auto"]);
});

function fake(init: Record<string, string> = {}): ThemeStorage & { m: Map<string, string> } {
  const m = new Map(Object.entries(init));
  return { m, getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
}

test("storage: the theme key stores raw string values", () => {
  assert.equal(THEME_KEY, "rh-theme");
  const s = fake();
  writeTheme(s, "ember");
  assert.equal(s.m.get("rh-theme"), "ember");
  assert.equal(readTheme(s), "ember");
});

test("storage: unknown, JSON-quoted, missing or blocked values read as nothing", () => {
  assert.equal(readTheme(fake()), null);
  assert.equal(readTheme(fake({ "rh-theme": "solarized" })), null);
  assert.equal(readTheme(fake({ "rh-theme": '"dark"' })), null);
  assert.equal(readTheme(null), null);
  const throwing: ThemeStorage = {
    getItem: () => {
      throw new Error("SecurityError");
    },
    setItem: () => {
      throw new Error("QuotaExceededError");
    },
  };
  assert.equal(readTheme(throwing), null);
  writeTheme(throwing, "dark");
});

test("no data-theme attribute means auto", () => {
  assert.equal(themeOf(undefined), "auto");
  assert.equal(themeOf(""), "auto");
  assert.equal(themeOf("contrast"), "contrast");
  assert.ok(isTheme("light"));
  assert.ok(!isTheme("Light"));
});
