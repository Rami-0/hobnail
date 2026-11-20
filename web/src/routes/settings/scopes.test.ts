import assert from "node:assert/strict";
import { test } from "node:test";
import { emptyScopes, toggleScope } from "./scopes.ts";

test("a limited empty scope stays distinct and grants are unique", () => {
  const empty = emptyScopes();
  const board = toggleScope(empty, "boards", "board", true);
  assert.deepEqual(toggleScope(board, "boards", "board", true), board);
  assert.deepEqual(toggleScope(board, "boards", "board", false), empty);
  assert.deepEqual(toggleScope(board, "integrations", "service", true), {
    boards: ["board"], integrations: ["service"],
  });
});
