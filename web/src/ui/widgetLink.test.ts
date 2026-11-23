import assert from "node:assert/strict";
import { test } from "node:test";
import { widgetLink } from "./widgetLink.ts";

test("provider item links render as navigation only for HTTP(S) without URL credentials", () => {
  assert.equal(widgetLink("https://fixture.test/item?q=foo"), "https://fixture.test/item?q=foo");
  assert.equal(widgetLink("http://fixture.test/item"), "http://fixture.test/item");
  for (const url of [null, "/internal", "//fixture.test/item", "javascript:void(0)", "https://user:token@fixture.test"]) {
    assert.equal(widgetLink(url), null);
  }
});
