import assert from "node:assert/strict";
import { test } from "node:test";

import {
  canRetarget,
  changes,
  EMPTY_VALUES,
  hideFromSwitch,
  isDirty,
  outcome,
  readOverride,
  requestBody,
  savedText,
  signInHref,
  validate,
  valuesOf,
} from "./override.ts";

const SETTINGS = {
  hash: "abc123",
  docker: { exclude: [], standalone_group: "standalone" },
  groups: [{ name: "media" }, { name: "tools", title: "Tools" }],
  overrides: { web: { name: "Web", group: "media", hide: false, order: 3 } },
  known: [],
};

test("the form's values from the file (ov_item)", () => {
  const r = readOverride(SETTINGS, "web");
  assert.ok(r);
  assert.equal(r.hash, "abc123");
  assert.deepEqual(r.values, { ...EMPTY_VALUES, name: "Web", group: "media", hide: "false", order: "3" });
  assert.deepEqual(r.groups, ["media", "tools"]);
  assert.deepEqual(readOverride(SETTINGS, "other")?.values, EMPTY_VALUES, "no override: all empty");
  assert.equal(readOverride({ nope: 1 }, "web"), null);
  assert.equal(valuesOf({ hide: true }).hide, "true");
});

test("only changed fields are sent; emptied ones are cleared", () => {
  const was = valuesOf(SETTINGS.overrides.web);
  assert.equal(changes(was, was), null);
  assert.equal(changes({ ...was, name: " Web " }, was), null, "trimmed text is the same");
  const c = changes({ ...was, name: "", icon: "grafana", order: "7", hide: "true" }, was);
  assert.deepEqual(c, { set: { icon: "grafana", hide: true, order: 7 }, clear: ["name"] });
  assert.ok(c);
  assert.deepEqual(requestBody("web", "abc123", c), {
    base_hash: "abc123",
    names: ["web"],
    clear: ["name"],
    set: { icon: "grafana", hide: true, order: 7 },
  });
  assert.deepEqual(requestBody("web", "h", { set: {}, clear: ["url"] }), { base_hash: "h", names: ["web"], clear: ["url"] });
  assert.ok(isDirty({ ...was, url: "http://x" }, was));
});

test("field checks as the old dialog's", () => {
  const was = EMPTY_VALUES;
  assert.deepEqual(validate(was, was), {});
  assert.equal(validate({ ...was, name: "x".repeat(81) }, was).name, "At most 80 characters.");
  assert.equal(validate({ ...was, description: "x".repeat(201) }, was).description, "At most 200 characters.");
  assert.equal(validate({ ...was, url: "ftp://x" }, was).url, "Use a link that starts with http:// or https://.");
  assert.deepEqual(validate({ ...was, url: "https://example.com/x" }, was), {});
  assert.deepEqual(validate({ ...was, url: "/files" }, { ...was, url: "/files" }), {}, "an unchanged hand-written value stays editable");
  assert.equal(validate({ ...was, order: "1.5" }, was).order, "Use a whole number from -100000 to 100000.");
  assert.equal(validate({ ...was, order: "200000" }, was).order, "Use a number from -100000 to 100000.");
});

test("the RETARGET RULE: adopt the new file only when every opened value is unchanged", () => {
  const was = valuesOf(SETTINGS.overrides.web);
  assert.ok(canRetarget(was, valuesOf({ name: "Web", group: "media", hide: false, order: 3 })));
  assert.ok(!canRetarget(was, valuesOf({ name: "Web 2", group: "media", hide: false, order: 3 })));
  assert.ok(!canRetarget(was, EMPTY_VALUES), "removed meanwhile");
  assert.ok(canRetarget(EMPTY_VALUES, EMPTY_VALUES), "still no override");
});

test("the hide switch keeps an explicit 'show'", () => {
  assert.equal(hideFromSwitch(true, ""), "true");
  assert.equal(hideFromSwitch(false, "true"), "");
  assert.equal(hideFromSwitch(false, "false"), "false");
  assert.equal(hideFromSwitch(false, ""), "");
});

test("answers mapped as HB.result", () => {
  assert.deepEqual(outcome({ kind: "ok", status: 200, data: { ok: true, hash: "n", revision: 12, warnings: ["w"] } }), {
    kind: "saved",
    unchanged: false,
    hash: "n",
    revision: 12,
    warnings: ["w"],
  });
  assert.deepEqual(outcome({ kind: "invalid", status: 422, message: "Use a link", field: "url" }), {
    kind: "fields",
    fields: { url: "Use a link" },
  });
  assert.deepEqual(outcome({ kind: "invalid", status: 422, message: "bad file" }), {
    kind: "note",
    tone: "bad",
    message: "Not saved: bad file",
  });
  assert.equal(outcome({ kind: "stale", status: 409, message: "x" }).kind, "stale");
  assert.equal(outcome({ kind: "sign_in", status: 401, message: "x" }).kind, "signin");
  assert.deepEqual(outcome({ kind: "forbidden", status: 403, reason: "locked", message: "refused: locked" }), {
    kind: "note",
    tone: "warn",
    message: "refused: locked",
  });
  assert.deepEqual(outcome({ kind: "forbidden", status: 403, reason: "cross_origin", message: "cross-origin write refused" }), {
    kind: "note",
    tone: "bad",
    message: "Not saved: cross-origin write refused.",
  });
  assert.match((outcome({ kind: "network", message: "x" }) as { message: string }).message, /did not answer/);
  assert.equal(savedText("web", { unchanged: false, revision: 12 }), "Saved “web” · revision #12");
  assert.equal(savedText("web", { unchanged: true, revision: null }), "No changes");
  assert.equal(signInHref("/app/containers/web", "?edit=override"), "/app/login?next=%2Fapp%2Fcontainers%2Fweb%3Fedit%3Doverride");
});
