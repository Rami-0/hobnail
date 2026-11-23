import assert from "node:assert/strict";
import { test } from "node:test";
import { homeHref, landingHref } from "./home.ts";

const boards = [{ id: "b1", name: "Lab", icon: null }];
test("Home shows the Home board; /app opens the default board", () => {
  assert.equal(homeHref({ home_board: "h", default_board: "d", scoped: false, boards }), "/app/boards/h");
  assert.equal(landingHref({ home_board: "h", default_board: "d", scoped: false, boards }), "/app/boards/d");
  assert.equal(landingHref({ home_board: "h", default_board: null, scoped: false, boards }), "/app/boards/h");
});

test("without a Home board", () => {
  assert.equal(homeHref({ home_board: null, default_board: null, scoped: false, boards }), "/app");
  assert.equal(homeHref(null), "/app");
  assert.equal(landingHref({ home_board: null, default_board: null, scoped: false, boards }), "/app/containers");
  assert.equal(landingHref({ home_board: null, default_board: null, scoped: true, boards }), "/app/boards/b1");
  assert.equal(landingHref({ home_board: null, default_board: null, scoped: true, boards: [] }), "/app/containers");
});
