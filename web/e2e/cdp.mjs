// Shared raw DevTools-protocol plumbing for the browser checks in web/e2e/
// (the same approach as app-check.mjs: Node 26 globals fetch and WebSocket,
// no npm packages). One headless Chromium per check, one WebSocket per tab.
//   const b = await launch();           // honours CH (scripts/e2e.sh sets it)
//   const tab = await b.tab();          // a fresh tab (its own renderer)
//   await tab.send("Page.navigate", { url });
//   await tab.until("!!document.querySelector('table')", 8000); // never hangs
//   await tab.close(); b.quit();
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const q = (s) => JSON.stringify(s);

export async function launch({ width = 1440, height = 900, extra = [] } = {}) {
  const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
  const port = 9300 + Math.floor(Math.random() * 600);
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), "e2e-profile-"));
  const ch = spawn(CH, ["--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check",
    `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, `--window-size=${width},${height}`, ...extra, "about:blank"], { stdio: "ignore" });
  const http = `http://127.0.0.1:${port}`;
  let up = false;
  for (let i = 0; i < 100; i++) { try { await (await fetch(`${http}/json/version`)).json(); up = true; break; } catch { await sleep(100); } }
  const quit = () => { ch.kill(); try { fs.rmSync(profile, { recursive: true, force: true }); } catch {} };
  if (!up) { quit(); throw new Error(`Chromium did not start (${CH})`); }
  return {
    http,
    quit,
    async tab() {
      const t = await (await fetch(`${http}/json/new?about:blank`, { method: "PUT" })).json();
      return connect(t, http);
    },
  };
}

async function connect(t, http) {
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.addEventListener("open", r); ws.addEventListener("error", j); });
  let id = 0;
  const pending = new Map();
  const handlers = new Map(); // method -> [fn]
  ws.addEventListener("message", (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); return; }
    for (const fn of handlers.get(d.method) || []) fn(d.params);
    for (const fn of handlers.get("*") || []) fn(d.method, d.params);
  });
  const tab = {
    id: t.id,
    // Resolves with the raw reply ({ result } or { error }); a stuck call
    // gives up after `ms` instead of hanging the check.
    send(method, params = {}, ms = 30000) {
      return new Promise((r) => {
        const n = ++id;
        const timer = setTimeout(() => { pending.delete(n); r({ error: { message: `${method} timed out` } }); }, ms);
        pending.set(n, (d) => { clearTimeout(timer); r(d); });
        ws.send(JSON.stringify({ id: n, method, params }));
      });
    },
    on(method, fn) { if (!handlers.has(method)) handlers.set(method, []); handlers.get(method).push(fn); },
    off(method, fn) { const l = handlers.get(method) || []; const i = l.indexOf(fn); if (i >= 0) l.splice(i, 1); },
    once(method, ms = 30000) {
      return new Promise((r) => {
        const timer = setTimeout(() => { tab.off(method, f); r(null); }, ms);
        const f = (p) => { clearTimeout(timer); tab.off(method, f); r(p); };
        tab.on(method, f);
      });
    },
    async ev(expression, ms = 15000) {
      const r = await tab.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, ms);
      if (r.error) throw new Error(r.error.message);
      if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + (r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
      return r.result.result.value;
    },
    // Polls `expr` until truthy; false after `ms` (every wait has a timeout).
    async until(expr, ms = 8000, every = 50) {
      const end = Date.now() + ms;
      while (Date.now() < end) { if (await tab.ev(expr, 5000).catch(() => false)) return true; await sleep(every); }
      return false;
    },
    async shot(file) {
      const r = await tab.send("Page.captureScreenshot", { format: "png" });
      if (r.result?.data) fs.writeFileSync(file, Buffer.from(r.result.data, "base64"));
    },
    async close() { try { ws.close(); } catch {} try { await fetch(`${http}/json/close/${t.id}`); } catch {} },
  };
  return tab;
}

// ---- real input ----
export function input(tab) {
  const self = {
    touch: false,
    async clickAt(x, y) {
      if (self.touch) {
        await tab.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
        await tab.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      } else for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await tab.send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
      await sleep(120);
    },
    // The centre of the first visible match (scrolled into view), or null.
    async box(sel, scroll = true) {
      if (scroll) await tab.ev(`new Promise(resolve => { const e=document.querySelector(${q(sel)}); if(e) { const popup=e.closest('[role="dialog"],[role="listbox"],[role="menu"],.sdk-select-list'); e.scrollIntoView({block:popup?"nearest":"center",inline:"nearest"}); } requestAnimationFrame(()=>requestAnimationFrame(resolve)); })`);
      return tab.ev(`(() => { const e = document.querySelector(${q(sel)}); if (!e) return null; const b = e.getBoundingClientRect(); if (!b.width || !b.height) return null; return { x: b.left + b.width / 2, y: b.top + b.height / 2, left: b.left, top: b.top, width: b.width, height: b.height }; })()`);
    },
    async click(sel) {
      const b = await self.box(sel);
      if (!b) throw new Error("no visible element " + sel);
      await self.clickAt(b.x, b.y);
    },
    async move(x, y, buttons = 0) { await tab.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, buttons, button: buttons ? "left" : "none" }); },
    async drag(x1, y1, x2, y2, steps = 8) {
      await self.move(x1, y1);
      await tab.send("Input.dispatchMouseEvent", { type: "mousePressed", x: x1, y: y1, button: "left", buttons: 1, clickCount: 1 });
      for (let i = 1; i <= steps; i++) { await self.move(x1 + ((x2 - x1) * i) / steps, y1 + ((y2 - y1) * i) / steps, 1); await sleep(16); }
      await tab.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: x2, y: y2, button: "left", buttons: 0, clickCount: 1 });
      await sleep(150);
    },
    async touchDrag(x1, y1, x2, y2, steps = 8) {
      await tab.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: x1, y: y1 }] });
      for (let i = 1; i <= steps; i++) { await tab.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: x1 + ((x2 - x1) * i) / steps, y: y1 + ((y2 - y1) * i) / steps }] }); await sleep(16); }
      await tab.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      await sleep(150);
    },
    async key(k, { modifiers = 0, text } = {}) {
      const VK = { Enter: 13, Backspace: 8, Tab: 9, Escape: 27, a: 65, "?": 191 };
      const code = VK[k] ?? k.toUpperCase().charCodeAt(0);
      const codeName = k === "?" ? "Slash" : k.length === 1 ? "Key" + k.toUpperCase() : k;
      const base = { key: k, code: codeName, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code, modifiers };
      const commands = k === "a" && modifiers === 2 ? ["selectAll"] : undefined;
      const t = text ?? (k.length === 1 && !modifiers ? k : k === "?" ? "?" : undefined);
      await tab.send("Input.dispatchKeyEvent", { type: t ? "keyDown" : "rawKeyDown", ...base, ...(t ? { text: t, unmodifiedText: t } : {}), ...(commands ? { commands } : {}) });
      if (k === "Enter" && !modifiers) await tab.send("Input.dispatchKeyEvent", { type: "char", ...base, text: "\r" });
      await tab.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
    },
    async type(s, delay = 0) {
      for (const c of s) {
        await tab.send("Input.dispatchKeyEvent", { type: "keyDown", key: c, text: c, unmodifiedText: c });
        await tab.send("Input.dispatchKeyEvent", { type: "keyUp", key: c });
        if (delay) await sleep(delay);
      }
      await sleep(50);
    },
    async clear(sel) {
      await self.click(sel);
      await self.key("a", { modifiers: 2 });
      await self.key("Backspace");
      return (await tab.ev(`document.querySelector(${q(sel)})?.value ?? null`)) === "";
    },
  };
  return self;
}

// Records CSP violations, console errors, exceptions and failed requests of
// a tab (as app-check.mjs). `allow(fn)` marks expected HTTP errors:
// fn({ method, url, status }) -> reason string or falsy.
export function recorder(tab, origin) {
  const short = (u) => (u && u.startsWith(origin) ? u.slice(origin.length) : u);
  const reqs = new Map();
  // Network log notifications can arrive after the next pass resets its arrays.
  // Keep only the IDs of explicitly expected failures for this tab lifetime.
  const allowedRequestIds = new Set();
  const r = { csp: [], console: [], exceptions: [], failed: [], allowed: [], notes: [], allowedUrls: new Set(), allowFns: [] };
  r.allow = (fn) => { r.allowFns.push(fn); return () => r.allowFns.splice(r.allowFns.indexOf(fn), 1); };
  r.reset = () => { for (const k of ["csp", "console", "exceptions", "failed", "allowed", "notes"]) r[k] = []; r.allowedUrls = new Set(); };
  tab.on("Runtime.bindingCalled", (p) => { if (p.name === "__hbCsp") r.csp.push("listener: " + p.payload); });
  tab.on("Runtime.exceptionThrown", (p) => r.exceptions.push(p.exceptionDetails.exception?.description || p.exceptionDetails.text));
  tab.on("Runtime.consoleAPICalled", (p) => {
    if (p.type !== "error" && p.type !== "assert") return;
    const text = p.args.map((a) => a.value ?? a.description ?? "").join(" ");
    (/Content Security Policy|violat/i.test(text) ? r.csp : r.console).push("console." + p.type + ": " + text);
  });
  tab.on("Log.entryAdded", ({ entry: e }) => {
    if (/Content Security Policy|violat/i.test(e.text)) r.csp.push(`log(${e.source}): ${e.text}`);
    else if (e.level === "error") {
      if (e.source === "network" && ((e.networkRequestId && allowedRequestIds.has(e.networkRequestId)) || (e.url && r.allowedUrls.has(e.url)))) r.allowed.push(`log: ${e.text} ${short(e.url)}`);
      else r.console.push(`log(${e.source}): ${e.text}${e.url ? " " + short(e.url) : ""}`);
    }
  });
  tab.on("Network.requestWillBeSent", (p) => reqs.set(p.requestId, { url: p.request.url, method: p.request.method }));
  tab.on("Network.responseReceived", (p) => {
    const res = p.response, rq = reqs.get(p.requestId) || { url: res.url, method: "GET" };
    if (res.status < 400) return;
    const what = `${rq.method} ${short(res.url)} -> ${res.status}`;
    const why = r.allowFns.map((f) => f({ requestId: p.requestId, method: rq.method, url: res.url, status: res.status })).find(Boolean);
    if (why) { allowedRequestIds.add(p.requestId); r.allowedUrls.add(res.url); r.allowed.push(`${what} (${why})`); } else r.failed.push(what);
  });
  tab.on("Network.loadingFailed", (p) => {
    const rq = reqs.get(p.requestId) || { url: "?", method: "?" };
    const what = `${rq.method} ${short(rq.url)}: ${p.errorText}${p.blockedReason ? " blocked " + p.blockedReason : ""}`;
    if (p.blockedReason === "csp" || p.corsErrorStatus) r.csp.push("network: " + what);
    else if (p.canceled || p.errorText === "net::ERR_ABORTED") r.notes.push("cancelled: " + what);
    else r.failed.push(what);
  });
  return r;
}

export const CSP_LISTENER = `
  document.addEventListener("securitypolicyviolation", (e) => {
    try { __hbCsp(JSON.stringify({ directive: e.effectiveDirective || e.violatedDirective, blocked: e.blockedURI, source: e.sourceFile, line: e.lineNumber, sample: e.sample })); } catch {}
  }, true);`;
