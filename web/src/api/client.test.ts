import assert from "node:assert/strict";
import { test } from "node:test";

import { createClient, errorInfo, retryAfterSeconds, type ApiResult } from "./client.ts";

interface Call {
  url: string;
  init: RequestInit;
}

/** A fake fetch answering from a queue, recording every call. */
function fakeFetch(answers: (Response | Error)[]) {
  const calls: Call[] = [];
  const fetch = async (url: string, init: RequestInit): Promise<Response> => {
    calls.push({ url, init });
    const a = answers.shift();
    if (!a) throw new Error("unexpected request " + url);
    if (a instanceof Error) throw a;
    return a;
  };
  return { fetch, calls };
}

const json = (status: number, body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json", ...headers } });
const text = (status: number, body: string) =>
  new Response(body, { status, headers: { "Content-Type": "text/plain; charset=utf-8" } });

function client(answers: (Response | Error)[], csrf: string | null = "tok1", fresh: string | null = "tok2") {
  const f = fakeFetch(answers);
  let refreshes = 0;
  const c = createClient({
    fetch: f.fetch,
    csrf: () => csrf,
    refreshCsrf: async () => {
      refreshes++;
      return fresh;
    },
  });
  return { c, calls: f.calls, refreshes: () => refreshes };
}

const header = (call: Call | undefined, name: string): string | undefined =>
  (call?.init.headers as Record<string, string> | undefined)?.[name];

test("GET: JSON, same-origin credentials, no CSRF header", async () => {
  const { c, calls } = client([json(200, { user: null })]);
  const r = await c.get<{ user: null }>("/api/v1/session");
  assert.deepEqual(r, { kind: "ok", status: 200, data: { user: null } });
  assert.equal(calls[0]?.init.credentials, "same-origin");
  assert.equal(calls[0]?.init.method, "GET");
  assert.equal(header(calls[0], "X-RH-CSRF"), undefined);
  assert.equal(header(calls[0], "Accept"), "application/json");
});

test("POST: JSON body, Content-Type and the session's X-RH-CSRF", async () => {
  const { c, calls } = client([json(200, { ok: true })]);
  await c.post("/api/x", { a: 1 });
  assert.equal(calls[0]?.init.body, '{"a":1}');
  assert.equal(header(calls[0], "Content-Type"), "application/json");
  assert.equal(header(calls[0], "X-RH-CSRF"), "tok1");
});

test("POST without a session sends no CSRF header (login)", async () => {
  const { c, calls } = client([json(200, { ok: true, csrf: "c" })], null);
  await c.post("/api/login", { password: "p", next: "/app" });
  assert.equal(header(calls[0], "X-RH-CSRF"), undefined);
});

test("401: sign in (plain-text gate body and /api/v1 shape)", async () => {
  const { c } = client([text(401, "sign in required\n"), json(401, { error: { code: "sign_in_required", message: "sign in" } })]);
  assert.deepEqual(await c.post("/api/x"), { kind: "sign_in", status: 401, message: "sign in required" });
  assert.deepEqual(await c.get("/api/v1/x"), { kind: "sign_in", status: 401, message: "sign in", code: "sign_in_required" });
});

test("403 bad CSRF: refresh the session and retry once with the new token", async () => {
  const { c, calls, refreshes } = client([text(403, "missing or bad X-RH-CSRF header\n"), json(200, { ok: true })]);
  const r = await c.post("/api/logout");
  assert.equal(r.kind, "ok");
  assert.equal(refreshes(), 1);
  assert.equal(calls.length, 2);
  assert.equal(header(calls[0], "X-RH-CSRF"), "tok1");
  assert.equal(header(calls[1], "X-RH-CSRF"), "tok2");
});

test("403 bad CSRF twice: no third try", async () => {
  const { c, calls } = client([text(403, "missing or bad X-RH-CSRF header\n"), text(403, "missing or bad X-RH-CSRF header\n")]);
  const r = await c.post("/api/x");
  assert.equal(r.kind === "forbidden" && r.reason, "csrf");
  assert.equal(calls.length, 2);
});

test("403 bad CSRF and the refresh finds no session: sign in, no retry", async () => {
  const { c, calls } = client([text(403, "missing or bad X-RH-CSRF header\n")], "old", null);
  const r = await c.post("/api/x");
  assert.equal(r.kind, "sign_in");
  assert.equal(calls.length, 1);
});

test("403 reasons from the write gate, the login route and the settings guard", async () => {
  const { c, refreshes } = client([
    text(403, "cross-origin write refused\n"),
    text(403, "read-only: no password set (run `hobnail set-password`)\n"),
    json(403, { error: "no password set (run `hobnail set-password`)" }),
    json(403, { error: "command widgets are managed in the file", locked: true }),
    json(403, { error: "something else" }),
  ]);
  const reasons: string[] = [];
  for (let i = 0; i < 5; i++) {
    const r: ApiResult<unknown> = await c.post("/api/x");
    reasons.push(r.kind === "forbidden" ? r.reason : r.kind);
  }
  assert.deepEqual(reasons, ["cross_origin", "read_only", "read_only", "locked", "other"]);
  assert.equal(refreshes(), 0);
});

test("409 stale, 422 field error, 400 with a field", async () => {
  const { c } = client([
    json(409, { error: "config changed on disk; reload", stale: true }),
    json(422, { error: "bad url", field: "url", line: 3, col: 7 }),
    json(400, { error: { code: "bad_request", message: "name too long", field: "name" } }),
  ]);
  assert.deepEqual(await c.post("/api/settings/x"), { kind: "stale", status: 409, message: "config changed on disk; reload" });
  assert.deepEqual(await c.post("/api/settings/x"), {
    kind: "invalid",
    status: 422,
    message: "bad url",
    field: "url",
    line: 3,
    col: 7,
  });
  assert.deepEqual(await c.post("/api/v1/x"), { kind: "invalid", status: 400, message: "name too long", field: "name" });
});

test("429: seconds from Retry-After, else from the body", async () => {
  const { c } = client([
    json(429, { error: "too many failed attempts", retry_after: 9 }, { "Retry-After": "4" }),
    json(429, { error: "too many failed attempts", retry_after: 9 }),
    json(429, { error: "busy" }),
  ]);
  const secs: number[] = [];
  for (let i = 0; i < 3; i++) {
    const r = await c.post("/api/login", { password: "x" });
    secs.push(r.kind === "rate_limited" ? r.retryAfter : -1);
  }
  assert.deepEqual(secs, [4, 9, 1]);
  assert.equal(retryAfterSeconds("Wed, 21 Oct 2015 07:28:10 GMT", undefined, Date.parse("Wed, 21 Oct 2015 07:28:00 GMT")), 10);
  assert.equal(retryAfterSeconds("0", undefined), 1);
});

test("network error and other statuses", async () => {
  const { c } = client([new TypeError("Failed to fetch"), text(500, "boom"), json(404, { error: { code: "not_found", message: "no such route" } })]);
  assert.deepEqual(await c.get("/api/v1/x"), { kind: "network", message: "Could not reach the server." });
  assert.deepEqual(await c.get("/x"), { kind: "error", status: 500, message: "boom" });
  assert.deepEqual(await c.get("/api/v1/nope"), { kind: "error", status: 404, message: "no such route", code: "not_found" });
});

test("ok with a non-JSON body is an error, an empty body is ok", async () => {
  const { c } = client([new Response("<html>", { status: 200 }), new Response(null, { status: 204 })]);
  assert.equal((await c.get("/x")).kind, "error");
  assert.deepEqual(await c.get("/y"), { kind: "ok", status: 204, data: undefined });
});

test("errorInfo reads all three body shapes", () => {
  assert.deepEqual(errorInfo('{"error":{"code":"not_found","message":"m","field":"f"}}', 404), { message: "m", code: "not_found", field: "f" });
  assert.deepEqual(errorInfo('{"error":"m","locked":true}', 403), { message: "m", locked: true });
  assert.deepEqual(errorInfo("plain\n", 403), { message: "plain" });
  assert.deepEqual(errorInfo("", 502), { message: "HTTP 502" });
});
