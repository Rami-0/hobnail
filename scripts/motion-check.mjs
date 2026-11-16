// Motion, loading and empty-state check (DESIGN.md §11.3, §11.6 to §11.9;
// ux-pass wave 4): drives a running test instance in headless Chromium.
//   HB_PASSWORD=... node scripts/motion-check.mjs <base URL> <container> [base URL of an empty instance]
// <container>: a running container whose page and logs may be opened.
// [empty instance]: one with no containers and no rules (no Docker socket is
// fine: it then also shows "Docker unreachable"); its empty states are
// checked with the script blocked.
// Checks: under prefers-reduced-motion every transition and animation of
// the dialog, phone sheet, drawer, toast, menu and row highlight is 0, and
// without it they run on the duration tokens; nothing is unclickable while
// it animates (a dialog, a toast; no page-to-page view transition); a
// skeleton shows only after 150 ms and never outside an aria-busy region,
// and the server marks nothing busy; layout shift on /, /history and a
// container page stays under 0.02, and on /host under 0.01 over 20 s of
// its 3 s poll, at 1440 and at 390 px; a live stream the server refuses
// (502, made up in the browser) shows the "stopped" pill, which sits at the
// bottom, clear of the heading and top bar, lets clicks through, and goes
// once the stream is back (no tight retry loop); a quiet log tail ends in
// "No lines yet", not busy; the page-load line ends when a navigation is
// cancelled (a dismissed "leave site?") at the next press or after 10 s;
// with /static/*.js blocked (JavaScript on) every page shows its plain
// text, no skeleton and nothing busy; focus-visible rings on a sample of controls in all four themes;
// a switch and a reorder arrow move at once and revert when the server
// refuses (the refusal is made up in the browser: no write reaches the
// server); every empty state renders with the script blocked.
// Signs in once (the login is rate limited: never run it in a loop) and
// writes nothing. Exits 1 when a check fails, 2 on bad usage. Headless
// Chromium from the Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";
import fs from "node:fs";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const CTR = process.argv[3];
const EMPTY = (process.argv[4] || "").replace(/\/+$/, "");
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(url) || !CTR || !PW || (EMPTY && !/^https?:\/\//.test(EMPTY))) {
  console.error("usage: HB_PASSWORD=<password> node scripts/motion-check.mjs <base URL of a test instance> <container> [base URL of an empty instance]");
  process.exit(2);
}
for (const u of [url, EMPTY].filter(Boolean)) if (new URL(u).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const ORIGIN = new URL(url).origin;

// ---- one sign-in ----
const lr = await fetch(url + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: ORIGIN }, body: JSON.stringify({ password: PW }) });
if (lr.status !== 200) { console.error(`sign-in failed: HTTP ${lr.status}`); process.exit(1); }
const cookie = lr.headers.get("set-cookie").split(";")[0];

// ---- the browser (DevTools protocol, as the other checks) ----
const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const ch = spawn(CH, ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 50; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(100); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); const errors = []; let paused = []; const dialogs = [];
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text);
  if (d.method === "Fetch.requestPaused") paused.push(d.params);
  if (d.method === "Page.javascriptDialogOpening") dialogs.push(d.params);
});
setTimeout(() => { console.log("FAIL the run timed out"); ch.kill(); process.exit(1); }, 540000).unref();
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
await send("Page.enable"); await send("Runtime.enable"); await send("Network.enable");
const [cn, cv] = cookie.split("=");
await send("Network.setCookie", { name: cn, value: cv, url, httpOnly: true, sameSite: "Strict" });
// Every page: the layout shifts it makes, and whether a view transition ran
// (and what a click during it reached).
await send("Page.addScriptToEvaluateOnNewDocument", { source: `
  window.__cls = 0; window.__shifts = [];
  try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) { window.__cls += e.value; window.__shifts.push([Math.round(performance.now()), +e.value.toFixed(4), (e.sources || []).map((s) => s.node && (s.node.id || s.node.className || s.node.nodeName)).join(" ")]); } }).observe({ type: "layout-shift", buffered: true }); } catch (e) {}
  window.__vt = null;
  addEventListener("pagereveal", (e) => { window.__vt = !!e.viewTransition; if (e.viewTransition) { window.__vtOn = true; e.viewTransition.finished.finally(() => { window.__vtOn = false; }); } });
  addEventListener("pointerdown", (e) => { window.__hit = e.target && (e.target.id || e.target.className || e.target.tagName); }, true);
` });
const KEYS = { Escape: 27, Enter: 13, Tab: 9, Shift: 16 };
async function key(k) {
  const base = { key: k, code: k, windowsVirtualKeyCode: KEYS[k], nativeVirtualKeyCode: KEYS[k] };
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await sleep(40);
}
async function clickAt(x, y) { for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 }); }
const centre = (sel) => ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const b = e.getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()`);
const until = async (expr, ms = 5000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };
async function view(w) {
  const phone = w < 500;
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: phone ? 844 : 900, deviceScaleFactor: 1, mobile: phone });
  await send("Emulation.setTouchEmulationEnabled", { enabled: false });
}
const motion = (reduce) => send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: reduce ? "reduce" : "no-preference" }] });
async function load(base, path, settle = 400) {
  await send("Page.navigate", { url: base + path });
  await until("document.readyState === 'complete'", 15000);
  await sleep(settle);
}
// Screenshots of what is checked, when SHOTS=<dir> is set.
async function shot(name) {
  if (!process.env.SHOTS) return;
  const r = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(`${process.env.SHOTS}/${name}.png`, Buffer.from(r.result.data, "base64"));
}
let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };

// The computed durations (ms) of `sel`'s transitions and animations.
const DUR = `((el, pseudo) => { const s = getComputedStyle(el, pseudo || null), ms = (v) => v.split(",").map((x) => parseFloat(x) * (/ms$/.test(x.trim()) ? 1 : 1000));
  return { t: ms(s.transitionDuration), a: s.animationName === "none" ? [] : ms(s.animationDuration) }; })`;
const tokens = () => ev(`["1","2","3","4"].map((n) => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--dur-" + n)))`);
// What each moving part runs on: opened (or shown) the way the page does it.
async function parts() {
  const out = {};
  const d = (sel, pseudo) => ev(`${DUR}(document.querySelector(${JSON.stringify(sel)}), ${JSON.stringify(pseudo || "")})`);
  await ev("HB.open('dlg-bookmark')"); await sleep(30);
  out.dialog = await d("#dlg-bookmark"); out.backdrop = await d("#dlg-bookmark", "::backdrop");
  await ev("HB.close(true)"); await sleep(30);
  out.dialogExit = await d("#dlg-bookmark");
  await ev("document.getElementById('keys').showModal()"); await sleep(30);
  out.keys = await d("#keys"); await ev("document.getElementById('keys').close()");
  await ev("window.__tg = HB.toast('motion-check')"); await sleep(20);
  out.toast = await d("body>.toasts>.toast:last-child"); await ev("window.__tg()");
  await ev(`(() => { const m = document.createElement("div"); m.className = "menu"; m.id = "mc-menu"; m.setAttribute("popover", ""); m.innerHTML = '<button class="mi">A</button>'; document.body.append(m); m.showPopover(); })()`);
  await sleep(20); out.menu = await d("#mc-menu"); await ev("document.getElementById('mc-menu').remove()");
  await ev("(() => { const r = document.querySelector('.attn .rows li, .strip-list li, ul.events li'); window.__row = r; HB.flash(r); })()");
  await sleep(20); out.row = await ev(`${DUR}(window.__row)`);
  return out;
}
async function phoneParts() {
  const out = {};
  const d = (sel) => ev(`${DUR}(document.querySelector(${JSON.stringify(sel)}))`);
  await ev("HB.open('dlg-bookmark')"); await sleep(30);
  out.sheet = await d("#dlg-bookmark");
  out.sheetBox = await ev("[document.getElementById('dlg-bookmark').offsetWidth, innerWidth]");
  await ev("HB.close(true)"); await sleep(300);
  await ev("document.querySelector('.bbar [data-act=more]').click()"); await sleep(30);
  out.drawer = await d("#side"); out.scrim = await d("#scrim");
  await key("Escape"); await sleep(30);
  out.drawerExit = await d("#side");
  return out;
}
const flat = (o) => Object.entries(o).filter(([k]) => k !== "sheetBox").flatMap(([k, v]) => [...v.t.map((x) => [k, "transition", x]), ...v.a.map((x) => [k, "animation", x])]);

// ---- 1. reduced motion: everything is instant; otherwise the tokens ----
await view(1440);
await motion(true);
await load(url, "/");
check((await tokens()).every((x) => x === 0), "reduced motion: the four duration tokens are 0", await tokens());
const calm = flat({ ...(await parts()) });
await view(390); await load(url, "/");
const calmPhone = flat(await phoneParts());
const moving = [...calm, ...calmPhone].filter((x) => x[2] !== 0);
check(moving.length === 0, "reduced motion: dialog, sheet, drawer, scrim, toast, menu, row highlight all 0 ms", moving);
{
  // The page-load line: there, and still under reduced motion (no 8 s crawl).
  const nb = await ev("(() => { const n = document.querySelector('.navbar'); if (!n) return null; n.classList.add('on'); const s = getComputedStyle(n); const r = [s.animationName, parseFloat(s.animationDuration), +s.opacity]; n.classList.remove('on'); return r; })()");
  check(nb !== null && nb[0] === "nav" && nb[1] === 0 && nb[2] === 1, "reduced motion: the page-load line is there and does not crawl", nb);
}

await motion(false);
await view(1440); await load(url, "/");
const T = await tokens();
check(T.join() === "90,160,240,1600", "no preference: the tokens are 90 / 160 / 240 / 1600 ms", T);
const p = await parts();
const allowed = new Set([0, ...T]);
const off = flat(p).filter((x) => !allowed.has(x[2]));
check(off.length === 0, "every duration is a token (or 0 for a discrete switch)", off);
check(p.dialog.t.includes(T[2]) && p.backdrop.t.includes(T[2]) && p.keys.t.includes(T[2]), "dialogs open over --dur-3", p);
check(p.dialogExit.t.includes(T[1]), "a dialog leaves over --dur-2", p.dialogExit);
check(p.toast.t.includes(T[1]) && p.menu.t.includes(T[1]), "toast and menu over --dur-2", [p.toast, p.menu]);
check(p.row.a.includes(T[3]), "a changed row highlights over --dur-4", p.row);
check(Math.max(...flat(p).filter((x) => x[0] !== "row").map((x) => x[2])) <= T[2], "no interaction motion longer than --dur-3");
await view(390); await load(url, "/");
const ph = await phoneParts();
check(ph.sheet.t.includes(T[2]) && ph.sheetBox[0] === ph.sheetBox[1], "phone: the bottom sheet opens over --dur-3, full width", ph);
check(ph.drawer.t.includes(T[2]) && ph.drawerExit.t.includes(T[1]), "phone: the drawer slides in over --dur-3, out over --dur-2", [ph.drawer, ph.drawerExit]);
check(ph.scrim.t.includes(T[2]), "phone: the scrim fades in over --dur-3", ph.scrim);

// ---- 2. nothing is unclickable while it animates ----
await view(1440); await load(url, "/");
await ev("HB.open('dlg-bookmark')");
// Hit testing works mid-entry (the box is still moving, so the point is
// taken and clicked at once): Cancel under the pointer takes the click.
const cx = await ev(`(() => { const b = document.querySelector("#dlg-bookmark .dlg-ft [data-dlg-close]"), r = b.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2; return [x, y, document.elementFromPoint(x, y).closest("[data-dlg-close]") === b, getComputedStyle(document.getElementById("dlg-bookmark")).opacity]; })()`);
await clickAt(cx[0], cx[1]); await sleep(60);
// (The box still moves until the click lands, so the click may meet the
// footer next to Cancel: it must land in the dialog, never on the backdrop.)
const hit = await ev("window.__hit");
check(cx[2] && +cx[3] < 1 && /dlg|btn/.test(String(hit)), "hit testing reaches Cancel while the dialog is still fading in, and the click lands in the dialog", [cx, hit]);
await ev("HB.close(true)");
await ev("HB.close(true)"); await sleep(300);
await ev("HB.toast('motion-check: dismiss me')");
const tx = (await centre("body>.toasts>.toast:last-child [aria-label=Dismiss]")) || [0, 0];
await clickAt(tx[0], tx[1]); await sleep(260);
check(!(await ev("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => t.textContent.includes('dismiss me'))")), "a toast is dismissed during its entry");
// Page to page: no view transition. (Chromium sends every click to the
// root while a cross-document one runs, so it is left out; see
// components.css.) A click right after a navigation reaches its target.
{
  const nx = await centre('.nav-item[href="/containers"]');
  await clickAt(nx[0], nx[1]);
  await until("location.pathname === '/containers' && !!document.getElementById('cq')", 8000);
  const vt = await ev("window.__vt");
  check(vt !== true, "page to page: no view transition (it would make links dead while it runs)", vt);
  const hx = await centre("#cq");
  await clickAt(hx[0], hx[1]); await sleep(30);
  check(/cq/.test(String(await ev("window.__hit"))), "a click right after a navigation reaches the control", await ev("window.__hit"));
}

// ---- 3. skeletons: after 150 ms, only in a busy region ----
await load(url, "/");
await ev(`(() => { const b = document.createElement("div"); b.id = "mc-sk"; b.setAttribute("aria-busy", "true"); b.innerHTML = '<span class="sk" style="width:80px"></span>'; document.querySelector("main").prepend(b); })()`);
const op0 = await ev("+getComputedStyle(document.querySelector('#mc-sk .sk')).opacity");
await sleep(350);
const op1 = await ev("+getComputedStyle(document.querySelector('#mc-sk .sk')).opacity");
check(op0 < 0.05 && op1 > 0.95, "a skeleton is invisible for its first 150 ms, then shows", [op0, op1]);
await ev("document.getElementById('mc-sk').remove()");
const SKPAGES = ["/", "/containers", "/host", "/history", "/alerts", "/logs", `/containers/${encodeURIComponent(CTR)}`, `/containers/${encodeURIComponent(CTR)}/logs`];
for (const path of SKPAGES) {
  await load(url, path, 60);
  const bad = await ev("[...document.querySelectorAll('.sk')].filter((s) => s.getClientRects().length && !s.closest('[aria-busy=\"true\"]')).map((s) => s.parentElement.className || s.parentElement.id)");
  check(bad.length === 0, `${path}: every skeleton sits in an aria-busy region`, bad);
}
// The log pane: served plain (nothing busy, no skeleton: the script makes
// it busy while it connects), not busy once its first lines are in.
await load(url, `/containers/${encodeURIComponent(CTR)}/logs`, 50);
const served = await ev(`fetch(location.href, { credentials: "same-origin" }).then((r) => r.text()).then((t) => [/aria-busy="/.test(t), /<span class="sk"/.test(t), /<div class="lbody" tabindex="0">/.test(t)])`);
check(!served[0] && !served[1] && served[2], "the server marks nothing busy and serves no skeleton (the log page)", served);
await until("document.querySelector('.lines .ll')", 8000);
const busy1 = await ev("document.querySelector('.lbody').getAttribute('aria-busy')");
check(busy1 === "false", "the log pane is not busy once its lines are in", busy1);
// A quiet container (made up in the browser: a stream that says hello and
// nothing else): after a moment the pane says so, not busy, no skeleton.
await send("Page.addScriptToEvaluateOnNewDocument", { source: `if (location.search.includes("mc-quiet")) { const Real = window.EventSource;
  window.EventSource = function (u, o) { if (!String(u).includes("/api/logs/stream")) return new Real(u, o);
    const t = new EventTarget(); t.readyState = 0; t.url = u; t.close = () => { t.readyState = 2; };
    setTimeout(() => { t.readyState = 1; t.dispatchEvent(new MessageEvent("hello", { data: "" })); }, 80); return t; }; }` });
await load(url, `/containers/${encodeURIComponent(CTR)}/logs?mc-quiet=1`, 50);
const q0 = await ev("[document.querySelector('.lbody').getAttribute('aria-busy'), document.querySelector('.lmsg').dataset.t]");
await sleep(1800);
const q1 = await ev("(() => { const m = document.querySelector('.lmsg'); return [document.querySelector('.lbody').getAttribute('aria-busy'), m.dataset.t, (m.querySelector('.empty h3') || {}).textContent, (m.querySelector('.empty p') || {}).textContent, [...m.querySelectorAll('.sk')].length]; })()");
check(q0[0] === "true" && q1[0] === "false" && q1[2] === "No lines yet" && q1[3] === "Waiting for the container to log." && q1[4] === 0, "a quiet tail: busy while it connects, then \"No lines yet\", not busy", [q0, q1]);

// ---- 4. layout shift when content replaces skeletons ----
const CLS = {};
for (const w of [1440, 390]) {
  await view(w);
  for (const path of ["/", "/host", "/history", `/containers/${encodeURIComponent(CTR)}`]) {
    // /host polls every 3 s: watched for 20 s, held to 0.01.
    const host = path === "/host", max = host ? 0.01 : 0.02;
    await load(url, path, host ? 20000 : 6000);
    const v = await ev("[+window.__cls.toFixed(4), window.__shifts]");
    CLS[`${path} @${w}`] = v;
    check(v[0] < max, `${path} at ${w} px: layout shift ${v[0]} under ${max}${host ? " over 20 s" : ""}`, v[1]);
  }
}
await view(1440);

// ---- 4b. live updates refused: the pill, where it sits, and recovery ----
// The stream is refused with a 502 made up in the browser (nothing reaches
// the server): the browser closes such an EventSource for good, so the
// page itself must retry, with a growing wait.
const PILL = `(() => { const p = document.querySelector(".live-off"); if (!p) return null; const r = p.getBoundingClientRect(), s = getComputedStyle(p);
  const box = (sel) => { const e = document.querySelector(sel); if (!e || !e.getClientRects().length) return null; const b = e.getBoundingClientRect(); return [b.left, b.top, b.right, b.bottom]; };
  const hits = (b) => b && r.left < b[2] && r.right > b[0] && r.top < b[3] && r.bottom > b[1];
  const head = document.querySelector("main h1, main .page-h, main h2"), bar = box(".bbar");
  const cx = r.left + r.width / 2, cy = r.top + r.height / 2, under = document.elementFromPoint(cx, cy);
  return { text: p.textContent, rect: [r.left, r.top, r.width, r.height].map(Math.round), vw: innerWidth, vh: innerHeight, pe: s.pointerEvents,
    inView: r.left >= 11 && r.right <= innerWidth - 11 && r.bottom <= innerHeight, overTop: hits(box(".top")), overHead: hits(head && (() => { const b = head.getBoundingClientRect(); return [b.left, b.top, b.right, b.bottom]; })()),
    overBar: hits(bar), clickThrough: !!under && !p.contains(under), n: document.querySelectorAll(".live-off").length }; })()`;
async function refuseStream(w, restoreCheck) {
  await view(w); await load(url, "/", 1500);
  paused = [];
  await send("Fetch.enable", { patterns: [{ urlPattern: "*/api/stream*", requestStage: "Request" }] });
  let refused = 0, stop = false;
  const pump = (async () => { while (!stop) { while (paused.length) { const q = paused.shift(); refused++; await send("Fetch.fulfillRequest", { requestId: q.requestId, responseCode: 502, responseHeaders: [{ name: "Content-Type", value: "text/plain" }], body: Buffer.from("bad gateway").toString("base64") }); } await sleep(50); } })();
  // Drop the open stream the way a hidden-then-shown page does.
  await ev("dispatchEvent(new PageTransitionEvent('pagehide')); dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }))");
  const t0 = Date.now();
  await until("!!document.querySelector('.live-off')", 12000);
  const shown = Date.now() - t0, pill = await ev(PILL);
  check(!!pill && /Trying to reconnect/.test(pill.text), `${w} px: a refused stream shows the pill (after ${shown} ms), saying it retries`, pill);
  check(pill && pill.pe === "none" && pill.clickThrough && pill.inView && !pill.overTop && !pill.overHead && !pill.overBar && pill.n === 1,
    `${w} px: the pill sits in view, clear of the top bar, the heading and the tab bar, and clicks go through it`, pill);
  if (pill && w < 500) check(pill.rect[2] > w / 2 && pill.rect[3] <= 60, `${w} px: the pill is wide (not half the screen) and at most two lines`, pill.rect);
  await shot(`pill-${w}`);
  if (restoreCheck) {
    // Still refused: the retries wait longer each time (no tight loop).
    const n0 = refused; await sleep(12000);
    check(refused - n0 <= 2 && !!(await ev(PILL)), `${w} px: while refused, at most 2 retries in 12 s and the pill stays`, refused - n0);
  }
  stop = true; await pump; await send("Fetch.disable");
  const t1 = Date.now();
  const gone = await until("!document.querySelector('.live-off') && !document.documentElement.classList.contains('stale')", 70000);
  check(gone, `${w} px: once the stream is back the pill clears (${Math.round((Date.now() - t1) / 1000)} s)`, await ev(PILL));
  check(await ev("(document.querySelector('#live .live-t') || { textContent: 'live' }).textContent") === "live", `${w} px: live again`);
}
await refuseStream(1440, true);
await refuseStream(390, false);
await view(1440);

// ---- 4c. the page-load line ends when the navigation does not happen ----
await load(url, "/", 300);
await send("Page.addScriptToEvaluateOnNewDocument", { source: `if (location.search.includes("mc-stay")) addEventListener("beforeunload", (e) => { if (window.__mcGo) return; e.preventDefault(); e.returnValue = ""; });` });
// A spot that is no control: the middle of the page heading.
const quiet = async () => (await centre("main h1")) || (await centre("main"));
async function cancelledNav() {
  dialogs.length = 0;
  await clickAt(...(await quiet())); // a user activation, so the browser asks "leave site?"
  const nx = await centre('.nav-item[href="/containers"]');
  clickAt(nx[0], nx[1]);
  for (let i = 0; i < 100 && !dialogs.length; i++) await sleep(30);
  const asked = dialogs.length > 0 && dialogs[0].type === "beforeunload";
  if (asked) await send("Page.handleJavaScriptDialog", { accept: false });
  await sleep(200);
  return [asked, await ev("[location.pathname, document.querySelector('.navbar').classList.contains('on')]")];
}
await load(url, "/?mc-stay=1", 300);
let nav = await cancelledNav();
check(nav[0] && nav[1][0] === "/" && nav[1][1], "a cancelled navigation: still on the page, the line on until something happens", nav);
await clickAt(...(await quiet())); await sleep(60);
check(!(await ev("document.querySelector('.navbar').classList.contains('on')")), "the next press in the page ends the line");
nav = await cancelledNav();
await key("Shift");
check(nav[1][1] && !(await ev("document.querySelector('.navbar').classList.contains('on')")), "so does the next key");
nav = await cancelledNav();
await sleep(10400);
check(nav[1][1] && !(await ev("document.querySelector('.navbar').classList.contains('on')")), "and without a press, 10 s end it");
await ev("window.__mcGo = true");

// ---- 4d. scripts blocked (JavaScript on, /static/*.js failing) ----
// The inline boot script runs (html.js) but app.js and logs.js do not:
// every page keeps its plain words, shows no skeleton and nothing is busy.
// (Blocked by URL, which also covers a copy in the cache.)
await send("Network.setBlockedURLs", { urls: ["*/static/*.js*"] });
for (const path of ["/", "/host", "/alerts", `/containers/${encodeURIComponent(CTR)}`, `/containers/${encodeURIComponent(CTR)}/logs`]) {
  await send("Page.navigate", { url: url + path });
  await until("document.readyState === 'complete'", 15000);
  await sleep(2400); // past the log pane's 2 s grace
  const r = await ev(`(() => ({ js: document.documentElement.classList.contains("js"), ready: document.documentElement.classList.contains("hb-ready"), HB: typeof window.HB,
    busy: [...document.querySelectorAll('[aria-busy="true"]')].map((x) => x.id || x.className),
    sk: [...document.querySelectorAll(".sk")].filter((x) => x.getClientRects().length).length,
    words: [...document.querySelectorAll(".sk-t")].filter((x) => x.getClientRects().length && x.getBoundingClientRect().width <= 2).length,
    pane: (() => { const p = document.querySelector(".lmsg .js-late"); return p ? [getComputedStyle(p).visibility, p.innerText.slice(0, 40)] : null; })() }))()`);
  const paneOk = !path.endsWith("/logs") || (r.pane && r.pane[0] === "visible" && /needs JavaScript/.test(r.pane[1]));
  check(r.js && !r.ready && r.HB === "undefined" && r.busy.length === 0 && r.sk === 0 && r.words === 0 && paneOk, `scripts blocked: ${path} shows its plain text, no skeleton, nothing busy`, r);
}
await send("Network.setBlockedURLs", { urls: [] });

// ---- 5. focus is visible in every theme ----
const RING = `((sel) => { const e = document.querySelector(sel); if (!e) return null; e.focus(); const s = getComputedStyle(e);
  const ring = e.matches(":focus-visible") && s.outlineStyle !== "none" && parseFloat(s.outlineWidth) >= 2;
  const c = (v) => (v.match(/[\\d.]+/g) || []).slice(0, 3).map(Number);
  const lum = (v) => { const [r, g, b] = c(v).map((x) => { x /= 255; return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const bg = getComputedStyle(document.body).backgroundColor, a = lum(s.outlineColor), b = lum(bg);
  return { ring, ratio: +((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)).toFixed(2) }; })`;
const SAMPLE = { "/": [".nav-item", "#theme", ".cmd", ".hs-cell", "a.ia.logs", "#tree-filter", ".stack-name"], [`/containers/${encodeURIComponent(CTR)}`]: [".cd-tabs a", ".seg a"], "/containers": ["#cq", ".ctbl th a", ".btn.sm"] };
for (const theme of ["dark", "light", "ember", "contrast"]) {
  const bad = [];
  for (const [path, sels] of Object.entries(SAMPLE)) {
    await load(url, path, 150);
    await ev(`document.documentElement.dataset.theme = ${JSON.stringify(theme)}`);
    await key("Shift"); // keyboard modality: programmatic focus now shows the ring
    for (const s of sels) { const r = await ev(`${RING}(${JSON.stringify(s)})`); if (!r || !r.ring || r.ratio < 3) bad.push([path, s, r]); }
  }
  check(bad.length === 0, `${theme}: a 2 px focus ring at 3:1 or more on ${Object.values(SAMPLE).flat().length} sample controls`, bad);
}
await ev("try { localStorage.removeItem('rh-theme') } catch (e) {}");

// ---- 6. optimistic: a switch and an arrow move at once and revert when refused ----
async function refusing(pattern, act) {
  paused = [];
  await send("Fetch.enable", { patterns: [{ urlPattern: pattern, requestStage: "Request" }] });
  await act();
  await until("true", 50);
  for (let i = 0; i < 100 && !paused.length; i++) await sleep(20);
  const seen = paused[0];
  const now = await ev(`window.__mid ? window.__mid() : null`);
  if (seen) await send("Fetch.fulfillRequest", { requestId: seen.requestId, responseCode: 409, responseHeaders: [{ name: "Content-Type", value: "application/json" }], body: Buffer.from(JSON.stringify({ ok: false, stale: true, error: "made up by motion-check" })).toString("base64") });
  await sleep(600);
  await send("Fetch.disable");
  return { seen: !!seen, method: seen && seen.request.method, now };
}
await load(url, "/alerts", 600);
if (await ev("!!document.querySelector('#ru-list [data-toggle]:not([disabled])')")) {
  await ev("window.__sw = document.querySelector('#ru-list [data-toggle]'); window.__was = __sw.checked; window.__mid = () => [__sw.checked !== __was, __sw.getAttribute('aria-busy')]");
  const r = await refusing("*/api/settings/alerts*", async () => { await ev("document.querySelector('#ru-list [data-toggle]').scrollIntoView({ block: 'center' })"); const c = await centre("#ru-list [data-toggle]"); await clickAt(c[0], c[1]); });
  check(r.seen && r.method === "POST", "the switch's save was caught in the browser (nothing reached the server)", r);
  check(r.now && r.now[0] === true && r.now[1] === "true", "a rule switch moves at once and waits busy", r.now);
  const after = await ev("[__sw.checked === __was, __sw.getAttribute('aria-busy'), [...document.querySelectorAll('body>.toasts>.toast.bad')].length > 0]");
  check(after[0] && after[1] === null && after[2], "refused: the switch goes back, with a message", after);
} else console.log("note: no rule switch on /alerts; the optimistic switch was not checked");
await load(url, "/settings/bookmarks", 600);
if (await ev("document.querySelectorAll('#bm-list tr[data-key]').length >= 2")) {
  await ev("window.__first = document.querySelector('#bm-list tr[data-key]').dataset.key; window.__mid = () => document.querySelector('#bm-list tr[data-key]').dataset.key !== __first");
  const r = await refusing("*/api/settings/bookmarks*", async () => { const c = await centre("#bm-list tr[data-key] [data-move=down]"); await clickAt(c[0], c[1]); });
  check(r.seen && r.now === true, "a reorder arrow moves the row at once", r);
  await sleep(400);
  check(await ev("document.querySelector('#bm-list tr[data-key]').dataset.key === __first"), "refused: the row goes back");
} else console.log("note: fewer than two bookmarks; the optimistic arrow was not checked");

// ---- 7. empty states with the script blocked ----
await send("Emulation.setScriptExecutionDisabled", { value: true });
const EMPTIES = [
  [url, "/containers?q=zzzz-motion-check-none", "No containers match"],
  [url, "/logs?q=zzzz-motion-check-none", "No container matches"],
  [url, "/no-such-page", "Page not found"],
  [url, `/containers/${encodeURIComponent(CTR)}?tab=events&kind=oom`, null],
];
if (EMPTY) EMPTIES.push([EMPTY, "/", null], [EMPTY, "/containers", null], [EMPTY, "/alerts", "No alert rules"], [EMPTY, "/logs", null]);
for (const [base, path, want] of EMPTIES) {
  await load(base, path, 100);
  const r = await ev(`(() => { const e = [...document.querySelectorAll(".empty")].filter((x) => x.getClientRects().length); const sk = [...document.querySelectorAll(".sk")].filter((x) => x.getClientRects().length).length;
    return { js: document.documentElement.classList.contains("js"), titles: e.map((x) => (x.querySelector("h3") || {}).textContent), icon: e.every((x) => x.querySelector(".empty-ic svg")), sk, dk: !!document.querySelector("#dk-err .banner") }; })()`);
  const ok = !r.js && r.titles.length > 0 && r.icon && r.sk === 0 && (!want || r.titles.includes(want));
  check(ok, `script blocked: ${base === EMPTY ? "empty instance " : ""}${path} shows ${want || "an empty state"}${r.dk ? " (and the Docker banner)" : ""}`, r);
}
// The log pane says what to do instead, and is not busy.
await load(url, `/containers/${encodeURIComponent(CTR)}/logs`, 100);
const nl = await ev(`(() => { const p = document.querySelector(".lmsg .js-late"); return { busy: document.querySelectorAll('[aria-busy="true"]').length, text: p && p.getClientRects().length ? p.innerText : null }; })()`);
check(nl.busy === 0 && /needs JavaScript\. You can still download/.test(nl.text || ""), "script blocked: the log pane says how to get the logs, and nothing is busy", nl);
await send("Emulation.setScriptExecutionDisabled", { value: false });

check(errors.length === 0, "no script errors on the pages", errors);
console.log("layout shift:", Object.entries(CLS).map(([k, v]) => `${k} ${v[0]}`).join(", "));
console.log(fails ? `${fails} check(s) failed` : "all checks passed");
ws.close(); ch.kill(); process.exit(fails ? 1 : 0);
