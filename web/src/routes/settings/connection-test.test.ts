import { test } from "node:test";
import assert from "node:assert/strict";
import { matchesConnectionTest } from "./connection-test.ts";

test("connection results and certificate offers expire when the draft changes", () => {
  const tested = { url: "https://service.test/old", pin: "", revision: 1 };
  assert.equal(matchesConnectionTest(tested, { ...tested }), true);
  assert.equal(matchesConnectionTest(tested, { ...tested, url: "https://service.test/new" }), false);
  assert.equal(matchesConnectionTest(tested, { ...tested, pin: "a".repeat(64) }), false);
  // Editing a credential, or changing the URL and then restoring it, still
  // invalidates the in-flight result.
  assert.equal(matchesConnectionTest(tested, { ...tested, revision: 2 }), false);
});
