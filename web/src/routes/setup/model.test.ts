import assert from "node:assert/strict";
import { test } from "node:test";
import type { SetupDocument } from "../../api/gen/SetupDocument.ts";
import { payloadIdentity } from "../../boards/document.ts";
import { move, selectedGroups } from "./model.ts";

test("back, continue and skip preserve edits and stop at the wizard boundaries", () => {
  const before: SetupDocument = { step: "appearance", theme: "ember", items: [{
    id: "saved-link", included: true, name: "My service", group: " Tools ",
    target: { kind: "board", payload: { kind: "link", title: "Discovered title", url: "http://localhost/" } },
  }] };
  assert.equal(move(before, "back"), before);
  const discover = move(before, "continue");
  assert.equal(discover.step, "discover");
  const select = move(discover, "skip");
  assert.equal(select.step, "select");
  const connections = move(select, "continue");
  assert.equal(connections.step, "connections");
  const review = move(connections, "skip");
  assert.equal(review.step, "review");
  assert.equal(move(review, "continue"), review);
  assert.deepEqual(move(move(move(move(review, "back"), "back"), "back"), "back"), before);
  assert.deepEqual([...selectedGroups(review)], [["Tools", before.items]]);
});

test("selection excludes unselected services and discovery identity survives renames", () => {
  const payload = { kind: "container", target: { by: "compose", project: "my-stack", service: "web" } } as const;
  assert.equal(payloadIdentity({ ...payload, title: "New name", icon: "web" }), payloadIdentity(payload));
  const document: SetupDocument = { step: "review", theme: null, items: [
    { id: "discovered", included: false, name: "Excluded", group: "Tools", target: { kind: "board", payload } },
  ] };
  assert.deepEqual([...selectedGroups(document)], []);
});


test("link identity normalizes authority while preserving the exact suffix", () => {
  const key = (url: string) => payloadIdentity({ kind: "link", title: "Fixture", url });
  assert.equal(key("HTTPS://Example.TEST:443"), key("https://example.test/"));
  assert.equal(key("http://example.TEST:080?q=x#top"), key("http://example.test/?q=x#top"));
  for (const path of ["/Path", "/path", "/?a=1&b=2", "/?b=2&a=1", "/#first", "/#second"]) {
    assert.equal(key(`https://example.test${path}`), `link:https://example.test${path}`);
  }
});
