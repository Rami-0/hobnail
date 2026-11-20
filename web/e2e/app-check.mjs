// React client smoke check (docs/plans/react-port.md 4.10: "a CSP check that
// fails on any violation report"): drives a running test instance's /app in
// headless Chromium over the raw DevTools protocol (Node 26 globals fetch and
// WebSocket, no npm packages), at each width in dark and light:
//   - loads <base>/app and waits for #root[data-ready="1"];
//   - records CSP violations (a `securitypolicyviolation` listener injected
//     before any page script, plus console/Log entries that mention the
//     Content Security Policy), console errors, uncaught exceptions, failed
//     requests and HTTP >= 400 responses;
//   - checks that the document carries a Content-Security-Policy header, that
//     every stylesheet answers text/css and every script text/javascript, and
//     that the styles applied (document.styleSheets non-empty and the custom
//     property CSS_VAR, default --bg, resolves on :root);
//   - onboards a fresh instance through the browser, then checks username
//     and password sign-in with real key and mouse events. One failed attempt
//     exercises the visible login error and the rate limit. The account menu
//     signs out and returns the browser to /app/login.
// Screenshots per width, scheme and state go to OUT (default: a new temp dir).
//   HB_PASSWORD=... node web/e2e/app-check.mjs <base URL>
//   HB_PASSWORD=... scripts/e2e.sh web/e2e/app-check.mjs <base URL>   (no host Node)
// Env: WIDTHS (default "1440x900 390x844"; a bare width means height 900, or
//      844 under 500 px), SCHEMES (default "dark light"), CSS_VAR (default
//      --bg), ALLOW (a regular expression: failed requests whose URL matches
//      are reported but not counted), CH (the Chromium binary; scripts/e2e.sh
//      sets it), OUT (screenshot dir).
// The login is rate limited: this sends exactly one wrong password per run,
// waits out the 1 s back-off, and never retries a wrong password.
// Exits 1 on any CSP violation, console error, exception, unexpected failed
// request or failed step; 2 on bad usage. Refuses port 3200 (the live instance).
import { blockLivePort, rejectLiveUrl } from "./cdp.mjs";
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const base = (process.argv[2] || "").replace(/\/+$/, "");
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(base) || !PW) {
  console.error("usage: HB_PASSWORD=<password> node web/e2e/app-check.mjs <base URL of a test instance>");
  process.exit(2);
}
if (new URL(base).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const ORIGIN = new URL(base).origin;
const SIZES = (process.env.WIDTHS || "1440x900 390x844").split(/[\s,]+/).filter(Boolean).map((s) => {
  const [w, h] = s.split("x").map(Number);
  return { w, h: h || (w < 500 ? 844 : 900) };
});
const SCHEMES = (process.env.SCHEMES || "dark light").split(/[\s,]+/).filter(Boolean);
const CSS_VAR = process.env.CSS_VAR || "--bg";
const ALLOW = process.env.ALLOW ? new RegExp(process.env.ALLOW) : null;
const OUT = process.env.OUT || fs.mkdtempSync(path.join(os.tmpdir(), "app-check-"));
fs.mkdirSync(OUT, { recursive: true });

// ---- the browser (DevTools protocol, as the other checks) ----
const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const profile = fs.mkdtempSync(path.join(os.tmpdir(), "app-check-profile-"));
const ch = spawn(CH, ["--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check",
  `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let ws;
function quit(code) {
  try { ws?.close(); } catch {}
  ch.kill();
  try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
  process.exit(code);
}
let up = false;
for (let i = 0; i < 100; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); up = true; break; } catch { await sleep(100); } }
if (!up) { console.error(`FAIL Chromium did not start (${CH})`); quit(1); }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
setTimeout(() => { console.log("FAIL the run timed out"); quit(1); }, 300000).unref();

// ---- what the page does, recorded per pass ----
let id = 0; const pending = new Map();

let pass = null; // { label, csp: [], console: [], exceptions: [], failed: [], allowed: [], notes: [], docHeaders, mime: [] }
const reqs = new Map(); // requestId -> { url, method, type }
let allowNext401 = false; // set right before the one wrong-password submit
const short = (u) => (u.startsWith(ORIGIN) ? u.slice(ORIGIN.length) : u);
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); return; }
  if (d.method === "Network.requestWillBeSent") rejectLiveUrl(d.params.request.url);
  if (!pass) return;
  const p = d.params;
  switch (d.method) {
    case "Runtime.bindingCalled":
      if (p.name === "__hbCsp") pass.csp.push("listener: " + p.payload);
      break;
    case "Runtime.exceptionThrown":
      pass.exceptions.push(p.exceptionDetails.exception?.description || p.exceptionDetails.text);
      break;
    case "Runtime.consoleAPICalled":
      if (p.type === "error" || p.type === "assert") {
        const text = p.args.map((a) => a.value ?? a.description ?? "").join(" ");
        (/Content Security Policy|violat/i.test(text) ? pass.csp : pass.console).push("console." + p.type + ": " + text);
      }
      break;
    case "Log.entryAdded": {
      const e = p.entry;
      if (/Content Security Policy|violat/i.test(e.text)) pass.csp.push(`log(${e.source}): ${e.text}`);
      else if (e.level === "error") {
        // The browser logs every 4xx/5xx as "Failed to load resource"; the
        // allowed 401 of the wrong password must not count twice.
        if (e.source === "network" && e.url && pass.allowedUrls.has(e.url)) pass.allowed.push(`log: ${e.text} ${short(e.url)}`);
        else pass.console.push(`log(${e.source}): ${e.text}${e.url ? " " + short(e.url) : ""}`);
      }
      break;
    }
    case "Network.requestWillBeSent":
      reqs.set(p.requestId, { url: p.request.url, method: p.request.method, type: p.type });
      break;
    case "Network.responseReceived": {
      const r = p.response, q = reqs.get(p.requestId) || { url: r.url, method: "GET", type: p.type };
      const what = `${q.method} ${short(r.url)} -> ${r.status}`;
      if (p.type === "Document" && new URL(r.url).pathname.startsWith("/app")) pass.docHeaders = r.headers;
      if (p.type === "Stylesheet" || p.type === "Script") pass.mime.push({ type: p.type, url: short(r.url), mime: r.mimeType, status: r.status });
      if (r.status >= 400) {
        if (allowNext401 && r.status === 401 && q.method === "POST" && new URL(r.url).pathname === "/api/login") {
          allowNext401 = false; pass.allowedUrls.add(r.url); pass.allowed.push(what + " (the wrong password)");
        } else if (ALLOW && ALLOW.test(r.url)) { pass.allowedUrls.add(r.url); pass.allowed.push(what + " (ALLOW)"); }
        else pass.failed.push(what);
      }
      break;
    }
    case "Network.loadingFailed": {
      const q = reqs.get(p.requestId) || { url: "?", method: "?" };
      const what = `${q.method} ${short(q.url)}: ${p.errorText}${p.blockedReason ? " blocked " + p.blockedReason : ""}`;
      if (p.blockedReason === "csp" || p.corsErrorStatus) pass.csp.push("network: " + what);
      // A stream or fetch cut short by a navigation or a sign-out is normal.
      else if (p.canceled || p.errorText === "net::ERR_ABORTED") pass.notes.push("cancelled: " + what);
      else if (ALLOW && ALLOW.test(q.url)) pass.allowed.push(what + " (ALLOW)");
      else pass.failed.push(what);
      break;
    }
  }
});
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.error) throw new Error(r.error.message);
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + (r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
const until = async (expr, ms = 8000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };
const q = (sel) => JSON.stringify(sel);

await send("Page.enable"); await send("Runtime.enable"); await blockLivePort(send); await send("Log.enable");
await send("Runtime.addBinding", { name: "__hbCsp" });
await send("Page.addScriptToEvaluateOnNewDocument", { source: `
  document.addEventListener("securitypolicyviolation", (e) => {
    try { __hbCsp(JSON.stringify({ directive: e.effectiveDirective || e.violatedDirective, blocked: e.blockedURI, source: e.sourceFile, line: e.lineNumber, sample: e.sample, disposition: e.disposition })); } catch {}
  }, true);` });

// ---- real input ----
let touch = false;
async function clickAt(x, y) {
  if (touch) {
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  } else for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
  await sleep(120);
}
async function click(sel) {
  const r = await ev(`(() => { const e = document.querySelector(${q(sel)}); if (!e) return null; e.scrollIntoView({ block: "center" }); const b = e.getBoundingClientRect(); if (!b.width || !b.height) return null; return [b.left + b.width / 2, b.top + b.height / 2]; })()`);
  if (!r) throw new Error("no visible element " + sel);
  await clickAt(r[0], r[1]);
}
const VK = { Enter: 13, Backspace: 8, Tab: 9, a: 65 };
async function key(k, modifiers = 0) {
  const code = VK[k] ?? k.toUpperCase().charCodeAt(0);
  const base = { key: k, code: k.length === 1 ? "Key" + k.toUpperCase() : k, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code, modifiers };
  const commands = k === "a" && modifiers === 2 ? ["selectAll"] : undefined;
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base, ...(commands ? { commands } : {}) });
  if (k === "Enter" && !modifiers) await send("Input.dispatchKeyEvent", { type: "char", ...base, text: "\r" });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}
// One keyDown (with its text) and keyUp per character, as a keyboard would.
async function typeText(s) {
  for (const c of s) {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: c, text: c, unmodifiedText: c });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: c });
  }
  await sleep(50);
}
async function clearField(sel) {
  await click(sel);
  await key("a", 2); // Ctrl+A
  await key("Backspace");
  return (await ev(`document.querySelector(${q(sel)})?.value ?? null`)) === "";
}
async function shot(name) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  if (r.result?.data) fs.writeFileSync(path.join(OUT, name + ".png"), Buffer.from(r.result.data, "base64"));
}

// ---- checks ----
let fails = 0;
const results = [];
const check = (ok, what, got) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${pass ? pass.label + ": " : ""}${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`);
  if (!ok) fails++;
  return ok;
};
const session = () => ev(`fetch("/api/v1/session", { credentials: "same-origin", cache: "no-store" }).then(async (r) => ({ status: r.status, body: await r.json().catch(() => null) }))`);
const pathname = () => ev("location.pathname");
const ready = `!!document.querySelector('#root[data-ready="1"]')`;

let wrongTried = false;
async function runPass(size, scheme) {
  const label = `${size.w}x${size.h} ${scheme}`;
  pass = { label, csp: [], console: [], exceptions: [], failed: [], allowed: [], allowedUrls: new Set(), notes: [], docHeaders: null, mime: [] };
  touch = size.w < 500;
  await send("Network.clearBrowserCookies");
  await send("Emulation.setDeviceMetricsOverride", { width: size.w, height: size.h, deviceScaleFactor: 1, mobile: touch });
  await send("Emulation.setTouchEmulationEnabled", { enabled: touch, maxTouchPoints: touch ? 5 : 0 });
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  const name = `${size.w}-${scheme}`;
  const step = async (what, fn) => {
    try { return check(await fn(), what); } catch (e) { return check(false, what, String(e.message || e)); }
  };

  // 1. load, styles, MIME types, headers
  await send("Page.navigate", { url: base + "/app" });
  if (!(await step('boots: #root[data-ready="1"]', () => until(ready, 10000)))) { await shot(name + "-boot-failed"); return summarise(); }
  await sleep(300);
  const csp = pass.docHeaders && Object.entries(pass.docHeaders).find(([k]) => k.toLowerCase() === "content-security-policy")?.[1];
  check(!!csp && /default-src 'self'/.test(csp) && /script-src 'self'/.test(csp), "index.html carries the Content-Security-Policy header", csp || null);
  const css = pass.mime.filter((m) => m.type === "Stylesheet"), js = pass.mime.filter((m) => m.type === "Script");
  check(js.length > 0, `at least one script loaded (${js.length})`);
  check(css.length > 0, `at least one stylesheet loaded (${css.length})`);
  const badMime = pass.mime.filter((m) => m.status < 400 && m.mime !== (m.type === "Stylesheet" ? "text/css" : "text/javascript"));
  check(badMime.length === 0, "every stylesheet is text/css and every script text/javascript", badMime.map((m) => `${m.url} ${m.mime}`));
  const applied = await ev(`({ sheets: document.styleSheets.length, v: getComputedStyle(document.documentElement).getPropertyValue(${q(CSS_VAR)}).trim(), bg: getComputedStyle(document.body).backgroundColor })`);
  check(applied.sheets > 0 && applied.v !== "", `styles applied (${applied.sheets} sheets, ${CSS_VAR} = ${applied.v || "unset"}, body ${applied.bg})`, applied);
  await shot(name + "-1-signed-out");

  // A fresh data directory starts at browser onboarding.
  const before = await session();
  if (before.body?.owner_exists === false) {
    const onboarding = 'form[data-testid="onboarding"]';
    if (!(await step("fresh instance opens owner onboarding", () => until(`!!document.querySelector('${onboarding}')`)))) return summarise();
    for (const [field, value] of [["username", "admin"], ["password", PW], ["confirm", PW]]) {
      await click(`${onboarding} input[name="${field}"]`);
      await typeText(value);
    }
    await click(`${onboarding} button[type="submit"]`);
    if (!(await step("onboarding signs in the owner", () => until(`!!document.querySelector('[data-testid="user"]')`, 10000)))) return summarise();
    await click('[data-testid="user"]');
    await until(`!!document.querySelector('button[data-testid="signout"]')`, 3000);
    await click('button[data-testid="signout"]');
  }
  if (!(await step("signed-out navigation opens the login form", () => until(`location.pathname === "/app/login" && !!document.querySelector('form[data-testid="login"] input[name="username"]')`)))) {
    await shot(name + "-login-missing"); return summarise();
  }
  await shot(name + "-2-login");
  const pwSel = 'form[data-testid="login"] input[name="password"]', submit = 'form[data-testid="login"] button[type="submit"]';
  await click('form[data-testid="login"] input[name="username"]');
  await typeText("admin");

  // 3. one wrong password, the first pass only
  if (!wrongTried) {
    wrongTried = true;
    await click(pwSel); await typeText("not-the-password-" + Date.now());
    allowNext401 = true;
    await click(submit);
    await step('a wrong password shows [data-testid="login-error"]', () => until(`(() => { const e = document.querySelector('[data-testid="login-error"]'); return !!e && e.textContent.trim().length > 0 && e.getBoundingClientRect().height > 0; })()`));
    allowNext401 = false;
    check((await pathname()) === "/app/login", "and stays on /app/login", await pathname());
    await shot(name + "-3-login-error");
    await sleep(1600); // the 1 s back-off after one failure
    await step("the password field can be cleared", () => clearField(pwSel));
  }

  // 4. the right password
  await click(pwSel); await typeText(PW);
  await key("Enter");
  const signedIn = `(() => { const u = document.querySelector('[data-testid="user"]'); return !!u && /admin/.test(u.textContent); })()`;
  if (!(await step('signed in: [data-testid="user"] shows admin', () => until(signedIn, 10000)))) {
    const err = await ev(`document.querySelector('[data-testid="login-error"]')?.textContent ?? null`).catch(() => null);
    if (err) console.log(`       login error shown: ${err}`);
    await shot(name + "-signin-failed"); return summarise();
  }
  check(!(await pathname()).startsWith("/app/login"), "and leaves the login page", await pathname());
  const s1 = await session();
  check(s1.status === 200 && s1.body?.user?.name === "admin" && s1.body?.user?.role === "owner" && typeof s1.body?.csrf === "string", "GET /api/v1/session: user admin with a csrf token", s1);
  await sleep(300);
  await shot(name + "-4-signed-in");

  // 5. sign out
  await step('signing out returns to the login form', async () => {
    // Phase 1: sign out sits in the account menu that [data-testid="user"] opens.
    if (!(await until(`!!document.querySelector('button[data-testid="signout"]') && document.querySelector('button[data-testid="signout"]').offsetParent !== null`, 300))) await click('[data-testid="user"]');
    if (!(await until(`!!document.querySelector('button[data-testid="signout"]')`, 3000))) return false;
    await click('button[data-testid="signout"]'); return until(`!!document.querySelector('form[data-testid="login"]') && !document.querySelector('[data-testid="user"]')`); });
  const s2 = await session();
  check(s2.status === 200 && s2.body?.user === null && s2.body?.csrf === null, "GET /api/v1/session after sign-out: user null, csrf null", s2);
  await sleep(300);
  await shot(name + "-5-signed-out-again");
  return summarise();
}
function summarise() {
  const p = pass;
  check(p.csp.length === 0, `no CSP violation (${p.csp.length})`, p.csp);
  check(p.exceptions.length === 0, `no uncaught exception (${p.exceptions.length})`, p.exceptions);
  check(p.console.length === 0, `no console error (${p.console.length})`, p.console);
  check(p.failed.length === 0, `no unexpected failed request or HTTP >= 400 (${p.failed.length})`, p.failed);
  for (const a of p.allowed) console.log(`       allowed: ${a}`);
  for (const n of p.notes) console.log(`       note: ${n}`);
  results.push({ label: p.label, csp: p.csp.length, console: p.console.length, exceptions: p.exceptions.length, failed: p.failed.length });
  pass = null;
}

for (const size of SIZES) for (const scheme of SCHEMES) {
  try { await runPass(size, scheme); } catch (e) { check(false, "pass crashed", String(e.stack || e)); if (pass) summarise(); }
}

console.log("\n| pass | CSP | console errors | exceptions | failed requests |\n|---|---|---|---|---|");
for (const r of results) console.log(`| ${r.label} | ${r.csp} | ${r.console} | ${r.exceptions} | ${r.failed} |`);
console.log(`\nscreenshots: ${OUT}`);
console.log(fails ? `${fails} check(s) FAILED` : "all checks passed");
quit(fails ? 1 : 0);
