import assert from "node:assert/strict";
import { test } from "node:test";

import { appHref, interceptedPath, matchRoute, safeNext, stripBase, type LinkClick } from "./paths.ts";

test("basename: only /app and below belong to the client", () => {
  assert.equal(stripBase("/app"), "/");
  assert.equal(stripBase("/app/"), "/");
  assert.equal(stripBase("/app/login"), "/login");
  assert.equal(stripBase("/apple"), null);
  assert.equal(stripBase("/"), null);
  assert.equal(stripBase("/settings"), null);
  assert.equal(appHref("/"), "/app");
  assert.equal(appHref("/login"), "/app/login");
  assert.equal(appHref("login"), "/app/login");
});

test("route matching", () => {
  assert.deepEqual(matchRoute("/app"), { name: "home" });
  assert.deepEqual(matchRoute("/app/"), { name: "home" });
  assert.deepEqual(matchRoute("/app/login"), { name: "login" });
  assert.deepEqual(matchRoute("/app/login/"), { name: "login" });
  assert.deepEqual(matchRoute("/app/nope"), { name: "not_found", path: "/app/nope" });
  assert.deepEqual(matchRoute("/applogin"), { name: "not_found", path: "/applogin" });
});

test("next: same-origin /app paths pass unchanged", () => {
  for (const good of ["/app", "/app/", "/app/login", "/app?x=1", "/app#t=1h", "/app/containers/web-1", "/app/a%2Db"]) {
    assert.equal(safeNext(good), good, good);
  }
});

test("next: everything auth.rs safe_next refuses, plus paths outside /app, gives /app", () => {
  const bad = [
    null,
    undefined,
    "",
    "/",
    "/settings",
    "/apple",
    "/app/../settings",
    "/app/%2E%2E/settings",
    "/app/%2e%2e/%2e%2e/x",
    "/\t/evil.example",
    "/app\t/x",
    "/app/\u0000",
    "/app/ x",
    "/app/%09/evil.example",
    "/app/%20",
    "/app/%00",
    "/app/%zz",
    "/app/%E2%80%",
    "/app/ü",
    "/app/\u007fx",
    "/app/\u0085x",
    "//evil.example",
    "//evil.example/app",
    "/\\evil.example",
    "/app\\evil",
    "/app/%5Cevil",
    "/%2F/evil.example",
    "https://evil.example/app",
    "javascript:alert(1)",
    "evil",
    "\\\\evil.example",
    "/app/" + "a".repeat(5000),
  ];
  for (const n of bad) assert.equal(safeNext(n), "/app", String(n));
});

const click = (over: Partial<LinkClick> = {}): LinkClick => ({
  button: 0,
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  defaultPrevented: false,
  target: "",
  hasDownload: false,
  href: "http://h.example/app/login?next=%2Fapp",
  origin: "http://h.example",
  current: "/app",
  ...over,
});

test("link interception: plain left clicks on /app links", () => {
  assert.equal(interceptedPath(click()), "/app/login?next=%2Fapp");
  assert.equal(interceptedPath(click({ target: "_self" })), "/app/login?next=%2Fapp");
  assert.equal(interceptedPath(click({ href: "http://h.example/app" })), "/app");
});

test("link interception: the browser keeps modifiers, other buttons, targets, downloads, other origins and pages", () => {
  for (const over of [
    { metaKey: true },
    { ctrlKey: true },
    { shiftKey: true },
    { altKey: true },
    { button: 1 },
    { defaultPrevented: true },
    { target: "_blank" },
    { hasDownload: true },
    { href: "http://other.example/app/login" },
    { href: "http://h.example/" },
    { href: "http://h.example/settings" },
    { href: "http://h.example/apple" },
    { href: "http://h.example/app#main" },
    { href: "not a url" },
  ] satisfies Partial<LinkClick>[]) {
    assert.equal(interceptedPath(click(over)), null, JSON.stringify(over));
  }
  // A hash on another client page is a navigation.
  assert.equal(interceptedPath(click({ href: "http://h.example/app/login#pw" })), "/app/login#pw");
});
