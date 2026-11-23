import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_DEFER_MS, quietDelay } from "./quiet.ts";

test("a missing release or repeated input cannot defer a refresh past its deadline", () => {
  const start = 1_000;
  const deadline = start + MAX_DEFER_MS;
  assert.equal(quietDelay(true, 0, deadline, start), 100);
  assert.equal(quietDelay(true, 0, deadline, deadline - 25), 25);
  assert.equal(quietDelay(true, 0, deadline, deadline), 0);
  assert.equal(quietDelay(false, deadline + 80, deadline, deadline), 0);
  assert.equal(quietDelay(false, start + 80, deadline, start), 80);
});
