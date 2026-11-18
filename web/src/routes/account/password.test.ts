import assert from "node:assert/strict";
import { test } from "node:test";
import { passwordErrors } from "./password.ts";

test("reports independent password problems and uses the server's Unicode limits", () => {
  assert.deepEqual(Object.keys(passwordErrors("", "short", "mismatch")), ["current", "new", "confirm"]);
  assert.deepEqual(passwordErrors("old", "😀".repeat(8), "😀".repeat(8)), {});
  assert.match(passwordErrors("old", "😀".repeat(257), "😀".repeat(257)).new ?? "", /1024/);
  assert.match(passwordErrors("old", "password\n", "password\n").new ?? "", /control/);
});
