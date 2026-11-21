import assert from "node:assert/strict";
import { test } from "node:test";
import type { Field } from "../api/gen/Field.ts";
import { connectionValues, initial, schemaValues, secretHint } from "./schema.ts";

const field = (id: string, kind: Field["kind"], options: Partial<Field> = {}): Field => ({
  id, kind, label: id, required: false, max_length: null, min: null, max: null, choices: [], default: null, help: "", ...options,
});

test("validates URL, credential, interval and list beside their fields", () => {
  const schema = [
    field("url", "url", { required: true }), field("token", "secret"),
    field("refresh", "number", { min: 15, max: 3600, default: "60" }), field("paths", "list", { max_length: 6 }),
  ];
  assert.deepEqual(initial(schema), { refresh: "60" });
  assert.deepEqual(Object.keys(schemaValues(schema, {
    url: "https://user@fixture.test", token: "not a\ntoken", refresh: "1", paths: "too long",
  }).errors), ["url", "token", "refresh", "paths"]);
  assert.deepEqual(schemaValues(schema, {
    url: "https://fixture.test/status", token: "", refresh: "60", paths: "a\n\nb.c",
  }), { values: { url: "https://fixture.test/status", refresh: 60, paths: ["a", "b.c"] }, errors: {} });
});

test("saved credentials are status only and clear and origin mismatch are distinct states", () => {
  const bound = "https://fixture.test";
  assert.match(secretHint(bound, `${bound}/new-path`), /^Saved for /);
  assert.match(secretHint(bound, "https://other.test"), /^Origin changed/);
  assert.match(secretHint(bound, "https://other.test", true), /will be cleared/);
  assert.equal(secretHint(null, bound), "");
  assert.deepEqual(initial([field("token", "secret")]), {});
  assert.deepEqual(schemaValues([field("token", "secret")], { token: "replacement" }).values, { token: "replacement" });
});

test("connection schema keeps, replaces and clears each credential independently", () => {
  const schema = [field("base_url", "url"), field("enabled", "bool"), field("password", "secret"),
    field("token", "secret"), field("required", "secret", { required: true }), field("version", "text")];
  const base_url = "https://fixture.test";
  const bound = { password: base_url, token: base_url, required: base_url };
  const values = { base_url, enabled: true, version: "v6", password: "fixture password", token: "" };
  assert.deepEqual(connectionValues(schema, values, bound, { token: true }), {
    base_url, enabled: true, options: { version: "v6" },
    credentials: { password: "fixture password", token: null }, errors: {},
  });
  assert.deepEqual(Object.keys(connectionValues(schema, { ...values, base_url: "https://other.test" }, bound).errors),
    ["token", "required"]);
});
