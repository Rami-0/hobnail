import assert from "node:assert/strict";
import { test } from "node:test";
import { changedFields, conflictingFields, dialogState, fieldErrors, rebaseFields, undoOnce } from "./forms.ts";

test("send changed values and rebase without trampling untouched fields", () => {
  const base = { name: "old", visible: true, list: ["one"] };
  const input = { ...base, name: "draft" };
  assert.deepEqual(changedFields(base, input), { name: "draft" });
  assert.deepEqual(rebaseFields(base, input, { name: "theirs", visible: false, list: ["one", "two"] }),
    { name: "draft", visible: false, list: ["one", "two"] });
});

test("map simultaneous form problems and exact positions from invalid responses", () => {
  assert.deepEqual(fieldErrors([
    { message: "Bad token", field: "text", line: 2, col: 4 },
    { message: "Required", field: "name" },
  ]), { text: "Bad token (line 2, column 4)", name: "Required" });
});

test("open only the active collection and keep colons in an item ID", () => {
  assert.deepEqual(dialogState(new URLSearchParams("edit=override:a%3Ab"), "override"), { id: "a:b" });
  assert.deepEqual(dialogState(new URLSearchParams("add=bookmark"), "bookmark"), { id: null });
  assert.equal(dialogState(new URLSearchParams("add=widget"), "bookmark"), null);
});

test("rebase detects only overlapping unsatisfied changes", () => {
  assert.deepEqual(conflictingFields({ text: "base", name: "base" }, { text: "draft", name: "same" },
    { text: "other", name: "same" }), ["text"]);
});

test("queued undo action can restore only once", () => {
  let count = 0;
  const undo = undoOnce(() => { count++; });
  undo();
  undo();
  assert.equal(count, 1);
});
