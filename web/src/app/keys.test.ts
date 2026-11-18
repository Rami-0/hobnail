import assert from "node:assert/strict";
import { test } from "node:test";

import { isTyping, keyAction, modName, moveRow, type KeyPress } from "./keys.ts";

const k = (key: string, over: Partial<KeyPress> = {}): KeyPress => ({ key, ctrlKey: false, metaKey: false, altKey: false, ...over });

test("g then a letter within 1.2 s goes to a page", () => {
  let r = keyAction(k("g"), { gAt: 0 }, 1000, false, false);
  assert.equal(r.action, null);
  assert.equal(r.state.gAt, 1000);
  r = keyAction(k("c"), r.state, 1500, false, false);
  assert.deepEqual(r.action, { kind: "go", page: "containers" });
  assert.equal(r.state.gAt, 0);
  // Too late: "c" alone does nothing.
  r = keyAction(k("g"), { gAt: 0 }, 1000, false, false);
  r = keyAction(k("c"), r.state, 2300, false, false);
  assert.equal(r.action, null);
  // "g l" is Logs, a lone "l" is the logs of the focused container.
  r = keyAction(k("l"), keyAction(k("g"), { gAt: 0 }, 100, false, false).state, 110, false, false);
  assert.deepEqual(r.action, { kind: "go", page: "logs" });
  assert.deepEqual(keyAction(k("l"), { gAt: 0 }, 10, false, false).action, { kind: "logs" });
});

const a = (key: string) => keyAction(k(key), { gAt: 0 }, 0, false, false).action;

test("single keys", () => {
  assert.deepEqual(a("/"), { kind: "slash" });
  assert.deepEqual(a("?"), { kind: "help" });
  assert.deepEqual(a("j"), { kind: "row", dir: 1 });
  assert.deepEqual(a("k"), { kind: "row", dir: -1 });
  assert.equal(a("x"), null);
});

test("off while typing, with modifiers, under a dialog", () => {
  assert.equal(keyAction(k("?"), { gAt: 0 }, 0, true, false).action, null);
  assert.equal(keyAction(k("?"), { gAt: 0 }, 0, false, true).action, null);
  assert.equal(keyAction(k("j", { ctrlKey: true }), { gAt: 0 }, 0, false, false).action, null);
  assert.equal(keyAction(k("j", { metaKey: true }), { gAt: 0 }, 0, false, false).action, null);
  assert.equal(keyAction(k("j", { altKey: true }), { gAt: 0 }, 0, false, false).action, null);
  assert.ok(isTyping("INPUT", "text", false));
  assert.ok(isTyping("INPUT", "search", false));
  assert.ok(isTyping("TEXTAREA", "", false));
  assert.ok(isTyping("SELECT", "", false));
  assert.ok(isTyping("DIV", "", true));
  assert.ok(!isTyping("INPUT", "checkbox", false));
  assert.ok(!isTyping("BUTTON", "submit", false));
});

test("j/k stop at the ends; the first press picks the first or last row", () => {
  assert.equal(moveRow(-1, 1, 5), 0);
  assert.equal(moveRow(-1, -1, 5), 4);
  assert.equal(moveRow(4, 1, 5), 4);
  assert.equal(moveRow(0, -1, 5), 0);
  assert.equal(moveRow(2, 1, 5), 3);
  assert.equal(moveRow(0, 1, 0), -1);
});

test("⌘ on a Mac", () => {
  assert.equal(modName("MacIntel"), "⌘");
  assert.equal(modName("Linux x86_64"), "Ctrl");
  assert.equal(modName("Win32"), "Ctrl");
});

test("the palette chord toggles even in its search field", () => {
  assert.deepEqual(keyAction(k("K", { metaKey: true }), { gAt: 5 }, 100, true, false), {
    action: { kind: "palette" }, state: { gAt: 0 },
  });
  assert.equal(keyAction(k("k", { ctrlKey: true }), { gAt: 0 }, 100, false, true).action, null);
});
