// Log viewer check (DESIGN.md §11.10, LG-12): loads a busy container's logs
// on a running test instance and asserts that "following" means the newest
// line is in view, measured after layout: after the first load, while
// batches arrive and after "Jump to latest"; at 1440 and 390 px, Wrap on and
// off, in the single view, the stack view (when the container belongs to a
// compose project) and side by side (with a second container). Any move up
// by hand stops following and shows the pill: a big wheel, slow wheels of a
// few pixels, and (at 390 px, touch emulated) slow and fast finger drags;
// only a move down into the bottom band, "Jump to latest" or End resumes.
// Scrolled up, Wrap, Time and a resize keep the reader on the same line.
// Also checks that a config change ("reload" on the live stream) never
// reloads a logs page (each shape, the /logs list included, which also keeps
// its ticks after Back), while other pages still reload; and that at 390 px
// with touch no two row actions' hit areas overlap on the pages that put a
// logs link in a row. Reads only; writes nothing to the server.
//   HB_PASSWORD=... node scripts/logs-check.mjs http://127.0.0.1:3210 <container> [<second container>]
// The container should print lines steadily (some of them long). HB_PASSWORD
// is needed when logs require a sign-in; it signs in once (the login is rate
// limited: never run it in a loop with a wrong password). HB_COOKIE=name=value
// reuses a session instead. CPU_SECONDS (default 15, 0 to skip) samples the
// page's main-thread cost while following, with the default tail and with a
// full ring; WIDTHS=1440 or WIDTHS=390 runs one width only; PARTS=views,
// reload,hits,features,cpu (the default: all) runs some parts only. "features":
// search steps (Enter / Shift+Enter), line actions by keyboard and tap, the
// phone toolbar and pane height, custom range checks, Load older, closing a
// pane of the split view.
// Exits 1 when a check fails, 2 on bad usage. Headless Chromium (no
// back/forward cache, so Back restores the form the way a reload does) from the
// Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const NAME = process.argv[3];
const OTHER = process.argv[4];
const PW = process.env.HB_PASSWORD;
const CPU_SECONDS = +(process.env.CPU_SECONDS ?? 15);
const PARTS = (process.env.PARTS || "views,reload,hits,features,cpu").split(",");
const SLACK = 2; // px from the bottom that still counts as "at the newest line"
if (!/^https?:\/\//.test(url) || !NAME) {
  console.error("usage: [HB_PASSWORD=<password>] node scripts/logs-check.mjs <base URL of a test instance> <busy container> [<second container>]");
  process.exit(2);
}
const ORIGIN = new URL(url).origin;
const enc = encodeURIComponent;

// ---- a session, only when logs need one ----
let cookie = process.env.HB_COOKIE || null;
const probe = await fetch(url + "/containers/" + enc(NAME) + "/logs", { redirect: "manual", headers: cookie ? { Cookie: cookie } : {} });
if (probe.status === 401 && cookie) {
  console.error("HB_COOKIE is not a valid session"); process.exit(2);
} else if (probe.status === 401) {
  if (!PW) { console.error("logs need a sign-in here: set HB_PASSWORD"); process.exit(2); }
  const lr = await fetch(url + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: ORIGIN }, body: JSON.stringify({ password: PW }) });
  if (lr.status !== 200) { console.error(`sign-in failed: HTTP ${lr.status}`); process.exit(1); }
  cookie = lr.headers.get("set-cookie").split(";")[0];
} else if (probe.status !== 200) {
  console.error(`${NAME}: the logs page answered HTTP ${probe.status}`);
  process.exit(2);
}

// ---- the browser (DevTools protocol, as scripts/ui-check.mjs) ----
const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const ch = spawn(CH, ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--disable-back-forward-cache", `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 50; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(100); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); const errors = [];
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.text);
});
setTimeout(() => { console.log("FAIL the run timed out"); ch.kill(); process.exit(1); }, 900000).unref();
// A protocol call that gets no answer fails the run instead of hanging it.
const send = (method, params = {}) => new Promise((r, j) => {
  const n = ++id;
  const tm = setTimeout(() => { pending.delete(n); j(new Error(`${method}: no answer in 30 s`)); }, 30000);
  pending.set(n, (d) => { clearTimeout(tm); r(d); });
  ws.send(JSON.stringify({ id: n, method, params }));
});
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
const until = async (expr, ms = 8000) => { for (let i = 0; i < ms / 100; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(100); } return false; };
async function clickAt(x, y) {
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  await sleep(80);
}
// Scroll a pane by hand with the wheel (negative: up).
async function wheel(x, y, dy) {
  await send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY: dy });
}
const scrollUp = (x, y) => wheel(x, y, -900);
// A finger drag down (the content moves up) of `total` px in steps of
// `step` px every `ms` ms: real touch events (Input.synthesizeScrollGesture
// does not scroll an inner pane in the headless shell; these do).
async function drag(x, y, total, step, ms) {
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  for (let d = 0; d < total; d += step) { y += step; await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y }] }); await sleep(ms); }
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}
async function key(k, code) {
  for (const type of ["keyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: k, code, windowsVirtualKeyCode: code === "End" ? 35 : 0 });
}
async function click(sel) {
  const r = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); e.scrollIntoView({ block: "center" }); const b = e.getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()`);
  await clickAt(r[0], r[1]);
}

let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };

await send("Page.enable");
await send("Runtime.enable");
await send("Network.enable");
if (cookie) {
  const [n, ...v] = cookie.split("=");
  await send("Network.setCookie", { name: n, value: v.join("="), url });
}
// Every EventSource a page opens, so the check can deliver a "reload" the
// way the live stream does after a config change.
await send("Page.addScriptToEvaluateOnNewDocument", { source: "(() => { const E = window.EventSource; if (!E) return; window.__es = []; window.EventSource = function (u, o) { const e = new E(u, o); window.__es.push(e); return e; }; window.EventSource.prototype = E.prototype; })();" });

async function load(path, { width = 1440, mobile = false, wrap = true } = {}) {
  // The phone profile is the 390 px mobile viewport, driven with the mouse.
  // Clicks go to the middle of the screen, never under the phone's bottom bar.
  await send("Emulation.setDeviceMetricsOverride", { width, height: mobile ? 844 : 900, deviceScaleFactor: mobile ? 2 : 1, mobile });
  // The phone profile has touch (pointer: coarse, real touch events).
  await send("Emulation.setTouchEmulationEnabled", { enabled: mobile, maxTouchPoints: mobile ? 5 : 1 });
  // The Wrap choice is remembered per browser: set it before the page loads.
  await send("Page.navigate", { url: url + "/healthz" });
  await until("document.readyState === 'complete'");
  await ev(`localStorage.setItem("rh-logs-wrap", ${JSON.stringify(wrap ? "1" : "0")})`);
  await send("Page.navigate", { url: url + path });
  await until("document.readyState === 'complete' && !!window.__rhLogs");
}
// Distance from the newest line, per pane, after two frames (after layout).
// (Reading scrollHeight forces layout; the timer covers a headless page that
// stops producing frames under touch emulation.)
const DIST = "new Promise((r) => { const m = () => r(window.__rhLogs.panes.map((p) => Math.round(p.body.scrollHeight - p.body.scrollTop - p.body.clientHeight))); requestAnimationFrame(() => requestAnimationFrame(m)); setTimeout(m, 1000); })";
const dist = () => ev(DIST);
const lines = () => ev("window.__rhLogs.panes.map((p) => p.lines.length)");
const following = () => ev("window.__rhLogs.panes.map((p) => p.follow && p.jump.hidden)");
const worst = (a) => Math.max(...a);
const first = () => ev("(() => { const p = window.__rhLogs.panes[0]; return { follow: p.follow, pill: !p.jump.hidden, d: Math.round(p.body.scrollHeight - p.body.scrollTop - p.body.clientHeight) }; })()");
const toBottom = async () => { await ev("window.__rhLogs.panes[0].toBottom()"); await sleep(300); };
// The reader's line in the first pane (the first that starts in view, or
// line n) and its offset from the top of the pane, after layout.
const MARK = (n) => `new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => { const p = window.__rhLogs.panes[0], t = p.body.scrollTop; const l = ${n == null ? "p.lines.find((x) => !x.e.hidden && x.e.offsetTop >= t)" : `p.lines.find((x) => x.n === ${+n})`}; r({ n: l ? l.n : -1, off: l ? Math.round(l.e.offsetTop - t) : 0, follow: p.follow, pill: !p.jump.hidden }); })))`;

async function viewCase(label, path, opt, busy) {
  await load(path, opt);
  // The first batch: the tail the page asks for (and the lines since).
  const loaded = await until("window.__rhLogs.panes.every((p) => p.lines.length > 0)", 10000);
  check(loaded, `${label}: lines arrive`);
  if (!loaded) return;
  await sleep(600);
  const d0 = await dist();
  check(worst(d0) <= SLACK, `${label}: rests on the newest line after the first load (px from the bottom: ${d0.join(", ")})`, d0);
  check((await following()).every(Boolean), `${label}: following, no "Jump to latest" pill`);
  // While batches arrive.
  const n0 = (await lines())[0];
  let w = 0;
  for (let i = 0; i < 12; i++) { await sleep(250); w = Math.max(w, worst(await dist())); }
  const grew = (await lines())[0] > n0 || (await ev("window.__rhLogs.panes[0].lines.length")) >= (await ev("+document.getElementById('lv').dataset.ring"));
  if (busy) check(grew, `${label}: new lines arrived while watching`);
  check(w <= SLACK, `${label}: stays on the newest line while batches arrive (worst ${w} px)`, w);
  // Scrolling up by hand stops following and shows the pill.
  // Where to wheel or drag: the first pane, in view (clicks on the toolbar
  // may have scrolled the page).
  const at = () => ev("(() => { const b = window.__rhLogs.panes[0].body; b.scrollIntoView({ block: 'nearest' }); const r = b.getBoundingClientRect(); return [r.left + r.width / 2, r.top + Math.min(r.height / 2, 200)]; })()");
  let c = await at();
  await scrollUp(c[0], c[1]);
  await sleep(500);
  const st = await ev("(() => { const p = window.__rhLogs.panes[0]; return { follow: p.follow, pill: !p.jump.hidden, top: p.body.scrollTop }; })()");
  check(!st.follow && st.pill, `${label}: scrolling up stops following and shows "Jump to latest"`, st);
  if (busy) {
    await sleep(2500);
    const st2 = await ev("(() => { const p = window.__rhLogs.panes[0]; return { follow: p.follow, pill: !p.jump.hidden, d: p.body.scrollHeight - p.body.scrollTop - p.body.clientHeight, n: p.jn.textContent }; })()");
    check(!st2.follow && st2.pill && st2.d > 200, `${label}: new lines do not pull the view down while scrolled up`, st2);
    check(/new/.test(st2.n), `${label}: the pill counts the new lines`, st2.n);
  }
  // Scrolled up, a view change keeps the reader on the same line and does
  // not resume following (LG-12 follow-up): Time, Wrap, a resize.
  const viewChange = async (what, act, undo) => {
    await toBottom(); c = await at(); await scrollUp(c[0], c[1]); await sleep(500);
    const a = await ev(MARK()); await act(); const b = await ev(MARK(a.n)); await undo(); await sleep(200);
    const same = a.n === b.n && Math.abs(a.off - b.off) <= 2;
    check(same && !b.follow && b.pill, `${label}: scrolled up, ${what} keeps the same line in view and stays scrolled up`, { before: a, after: b });
  };
  await viewChange("Time off", () => click("#ltime"), () => click("#ltime"));
  await viewChange("Wrap " + (opt.wrap ? "off" : "on"), () => click("#lwrap"), () => click("#lwrap"));
  const metrics = (w) => send("Emulation.setDeviceMetricsOverride", { width: w, height: opt.mobile ? 844 : 900, deviceScaleFactor: opt.mobile ? 2 : 1, mobile: !!opt.mobile });
  await viewChange("a resize", async () => { await metrics(opt.mobile ? 430 : 1100); await sleep(300); }, () => metrics(opt.width));
  // Any move up stops following, however slow: small wheel steps ...
  for (const [px, ms, n] of [[8, 30, 50], [20, 50, 20], [4, 16, 100]]) {
    await toBottom(); c = await at();
    for (let i = 0; i < n; i++) { await wheel(c[0], c[1], -px); await sleep(ms); }
    await sleep(400);
    const s = await first();
    check(!s.follow && s.pill && s.d > 40, `${label}: a slow wheel up (${n} x ${px} px every ${ms} ms) stops following`, s);
  }
  // ... and finger drags (touch, phone profile only).
  if (opt.mobile) {
    for (const [total, step] of [[300, 6], [150, 2], [300, 30]]) {
      await toBottom(); c = await at();
      await drag(Math.round(c[0]), Math.round(c[1]) - 60, total, step, 16);
      await sleep(500);
      const s = await first();
      check(!s.follow && s.pill && s.d > 40, `${label}: a finger drag of ${total} px at about ${Math.round(step * 1000 / 16)} px/s stops following`, s);
    }
  }
  // A move down into the bottom band resumes following; so does End.
  // (Wheel down as a reader does: at 390 px the busy stream grows by about
  // 1,500 px a second and a batch landing during a wheel can leave it just
  // short of the band, so wheel in big steps until it follows.)
  await toBottom(); c = await at(); await scrollUp(c[0], c[1]); await sleep(300);
  let back = false;
  for (let i = 0; i < 20 && !back; i++) { await wheel(c[0], c[1], 3000); await sleep(250); back = (await following())[0]; }
  check(back, `${label}: scrolling back down to the bottom follows again`);
  await scrollUp(c[0], c[1]); await sleep(300);
  await ev("document.activeElement && document.activeElement.blur && document.activeElement.blur()");
  await key("End", "End"); await sleep(300);
  const dEnd = await dist();
  check((await following())[0] && dEnd[0] <= SLACK, `${label}: End lands on the newest line and follows (${dEnd[0]} px)`, dEnd);
  await scrollUp(c[0], c[1]); await sleep(500);
  // "Jump to latest" lands on the newest line and follows again.
  if (await ev("!!document.querySelector('.lpane .ljump:not([hidden])')")) await click(".lpane .ljump:not([hidden])");
  const d1 = await dist();
  check(worst(d1.slice(0, 1)) <= SLACK, `${label}: "Jump to latest" lands on the newest line (${d1[0]} px)`, d1);
  check((await following())[0], `${label}: following again after "Jump to latest"`);
  await sleep(1500);
  const d2 = await dist();
  check(worst(d2) <= SLACK, `${label}: still on the newest line after more batches (${d2.join(", ")} px)`, d2);
  // Changing Wrap while following keeps the newest line in view.
  await click("#lwrap");
  const d3 = await dist();
  check(worst(d3) <= SLACK, `${label}: Wrap ${opt.wrap ? "off" : "on"} keeps the newest line in view (${d3.join(", ")} px)`, d3);
  await click("#lwrap");
}

// ---- the views ----
const single = "/containers/" + enc(NAME) + "/logs";
await load(single);
// The page's own "whole stack" link (only shown for a compose project).
const stackHref = await ev("(() => { const a = document.querySelector('.lfoot a[href^=\"/stacks/\"]'); return a ? a.getAttribute('href') : null; })()");
const views = [["single", single, true]];
if (stackHref) views.push(["stack", stackHref, true]); else console.log(`skip stack view: ${NAME} is not part of a compose project`);
if (OTHER) views.push(["side by side", "/logs?c=" + enc(NAME) + "," + enc(OTHER), true]); else console.log("skip side by side: no second container given");
// WIDTHS=390 (or 1440) runs one profile only.
const widths = (process.env.WIDTHS || "1440,390").split(",").map(Number);
for (const [w, mobile] of [[1440, false], [390, true]].filter(([w]) => widths.includes(w) && PARTS.includes("views"))) {
  for (const wrap of [true, false]) {
    for (const [v, path, busy] of views) await viewCase(`${v}, ${w} px, Wrap ${wrap ? "on" : "off"}`, path, { width: w, mobile, wrap }, busy);
  }
}

// ---- a config change never reloads a logs page (owner decision Y12) ----
const fireReload = "(() => { const e = (window.__es || []).find((x) => /\\/api\\/stream$/.test(x.url)); if (!e) return false; e.dispatchEvent(new Event('reload')); return true; })()";
const STREAM_OPEN = "(window.__es || []).some((x) => /\\/api\\/stream$/.test(x.url) && x.readyState === 1)";
// Every shape of logs page: each viewer, and the /logs list (with a tick).
const shapes = views.map(([v, path]) => [v, path]).concat([["/logs list", "/logs"]]);
for (const [v, path] of PARTS.includes("reload") ? shapes : []) {
  await load(path).catch(() => {});
  if (path === "/logs") await until("!!document.getElementById('lpick')");
  await until(STREAM_OPEN);
  const kept = path === "/logs"
    ? "(() => { const b = document.querySelector('#lpick input[name=c]'); b.click(); window.__kept = 1; return b.checked; })()"
    : "(() => { document.getElementById('lq').value = 'kept'; window.__kept = 1; return true; })()";
  await ev(kept);
  check(await ev(fireReload), `${v}: the live stream is open`);
  await sleep(1500);
  const same = path === "/logs"
    ? "window.__kept === 1 && document.querySelector('#lpick input[name=c]').checked"
    : "window.__kept === 1 && document.getElementById('lq').value === 'kept'";
  check(await ev(same).catch(() => false), `${v}: a config change does not reload it (${path === "/logs" ? "tick" : "search"} kept)`);
}
// The /logs list after Back: "Side by side" reflects the restored ticks.
if (PARTS.includes("reload")) {
  await load("/logs");
  await until("!!document.getElementById('lpick')");
  const n = await ev("(() => { const bs = [...document.querySelectorAll('#lpick input[name=c]')].slice(0, 2); bs.forEach((b) => b.click()); return bs.length; })()");
  if (n === 2) {
    check(await ev("!document.getElementById('lpick-go').disabled"), "/logs list: two ticks enable \"Side by side\"");
    await ev("location.href = document.querySelector('#lpick a.row-link').href");
    await until("!!document.getElementById('lv')");
    await ev("history.back()");
    await until("!!document.getElementById('lpick') && document.readyState === 'complete'");
    await sleep(600);
    const st = await ev("({ ticked: document.querySelectorAll('#lpick input[name=c]:checked').length, disabled: document.getElementById('lpick-go').disabled })");
    check(st.disabled === (st.ticked < 2), `/logs list: after Back, "Side by side" matches the ticks (${st.ticked} ticked)`, st);
  } else console.log("skip /logs Back check: fewer than two containers listed");
}
if (PARTS.includes("reload")) {
await load("/containers");
await until("(window.__es || []).some((x) => /\\/api\\/stream$/.test(x.url) && x.readyState === 1)");
await ev("window.__kept = 1");
await ev(fireReload);
await sleep(1500);
check(await ev("window.__kept === undefined").catch(() => false), "other pages: a config change still reloads them");
}

// ---- hit areas: at 390 px with touch, no row action reaches another row ----
// For each row that carries a link (the logs links of this wave and their
// neighbours), sample its box: elementFromPoint must never land on a link or
// button of another row, and each action's own box must reach that action.
const HIT = `((sel) => {
  const rows = [...document.querySelectorAll(sel)].filter((r) => r.getClientRects().length && r.getBoundingClientRect().width > 0).slice(0, 16);
  const act = (t) => t && t.closest("a,button,input,summary");
  const out = { rows: rows.length, bad: [] };
  for (const r of rows) {
    r.scrollIntoView({ block: "center" });
    const b = r.getBoundingClientRect();
    for (let y = b.top + 1; y < b.bottom - 0.5; y += 4) for (let x = b.left + 1; x < b.right - 0.5; x += 6) {
      const a = act(document.elementFromPoint(x, y));
      if (!a || r.contains(a) || a.contains(r)) continue;
      if (rows.some((o) => o !== r && o.contains(a))) out.bad.push((r.textContent.trim().slice(0, 12)) + " @" + Math.round(x - b.left) + "," + Math.round(y - b.top) + " -> " + (a.getAttribute("aria-label") || a.textContent.trim()).slice(0, 16));
    }
    for (const a of r.querySelectorAll("a,button")) {
      // Its box as shown: a cell that clips its overflow clips the hit area too.
      const a0 = a.getBoundingClientRect(), cl = (a.closest("td,th,li,summary") || r).getBoundingClientRect();
      const q = { left: Math.max(a0.left, cl.left), right: Math.min(a0.right, cl.right), top: Math.max(a0.top, cl.top), bottom: Math.min(a0.bottom, cl.bottom) };
      q.width = q.right - q.left; q.height = q.bottom - q.top;
      if (q.width < 4 || q.height < 4) continue;
      for (const [x, y] of [[q.left + 1.5, q.top + q.height / 2], [q.right - 1.5, q.top + q.height / 2], [q.left + q.width / 2, q.top + 1.5], [q.left + q.width / 2, q.bottom - 1.5]]) {
        if (y < 0 || y > innerHeight - 70) continue;
        const t = act(document.elementFromPoint(x, y));
        if (t && t !== a && !a.contains(t) && (t.matches(".ia,.btn,.row-link") || r.contains(t))) out.bad.push("own " + (a.getAttribute("aria-label") || a.textContent.trim()).slice(0, 14) + " -> " + (t.getAttribute("aria-label") || t.textContent.trim()).slice(0, 14));
      }
    }
  }
  out.bad = [...new Set(out.bad)].slice(0, 6);
  return out;
})`;
if (widths.includes(390) && PARTS.includes("hits")) {
  const zones = [
    ["/", ".bars li, .attn .rows li, .events li"],
    ["/containers", "tbody tr.row"],
    ["/host", ".hx-p tbody tr"],
    ["/history", "tbody tr, .events li"],
    ["/alerts", ".incident, .events li, .al-log li"],
    ["/more", ".more-stack li, .more-nav li"],
    ["/logs", "#lpick li, .lpick-h"],
    ["/settings/containers", "tbody tr"],
  ];
  for (const [path, sel] of zones) {
    await load(path, { width: 390, mobile: true }).catch(() => {});
    await sleep(500);
    const coarse = await ev("matchMedia('(pointer: coarse)').matches");
    const r = await ev(`${HIT}(${JSON.stringify(sel)})`);
    if (!r.rows) { console.log(`skip hit areas on ${path}: no rows`); continue; }
    check(coarse && r.bad.length === 0, `390 px, touch: ${path}: ${r.rows} rows, no hit area reaches another row or covers its row's own link`, r.bad);
  }
  // The sidebar tree, in the open drawer.
  await load("/containers", { width: 390, mobile: true }).catch(() => {});
  await sleep(400);
  await ev("(() => { const b = document.querySelector('[data-act=more]'); if (b) b.click(); })()");
  await sleep(600);
  const r = await ev(`${HIT}("#tree li.ctr, #tree details.stack > summary")`);
  check(r.rows > 0 && r.bad.length === 0, `390 px, touch: the sidebar tree (drawer open): ${r.rows} rows, no hit area reaches another row or covers its row's own link`, r.bad);
}

// ---- wave 5: search steps, line actions, phone layout, split view ----
const tap = async (x, y) => {
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await sleep(300);
};
const typeIn = async (sel, text) => { await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); e.focus(); e.value = ${JSON.stringify(text)}; e.dispatchEvent(new Event("input")); })()`); await sleep(500); };
const press = async (k, shift) => {
  for (const type of ["keyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: k, code: k, modifiers: shift ? 8 : 0, windowsVirtualKeyCode: { Enter: 13, Escape: 27, ArrowUp: 38, ArrowDown: 40 }[k] || 0, text: type === "keyDown" && k === "Enter" ? "\r" : undefined });
  await sleep(300);
};
const CUR = "(() => { const c = window.__rhLogs.current(), p = window.__rhLogs.panes[0]; if (!c) return null; const b = p.body.getBoundingClientRect(), r = c.e.getBoundingClientRect(); return { n: c.n, inView: r.top >= b.top - 1 && r.bottom <= b.bottom + 1, follow: p.follow, qn: document.getElementById('lqn').textContent }; })()";
if (PARTS.includes("features")) {
  // LG-13: Enter / Shift+Enter step through the matches; the view moves to
  // each; the search is kept in the address and survives a reload.
  await load(single);
  await until("window.__rhLogs.panes[0].lines.length > 50");
  await sleep(800);
  await typeIn("#lq", "short");
  const s0 = await ev(CUR);
  // Following, the newest match may already have scrolled away under new lines.
  check(s0 && /^\d+\/\d+$/.test(s0.qn) && (s0.inView || s0.follow), "search: the newest match is current (in view unless the tail moved on), counted (x/n)", s0);
  await ev("document.getElementById('lq').focus()");
  await press("Enter"); await press("Enter"); await press("Enter");
  const s1 = await ev(CUR);
  check(s1 && s1.n < s0.n && s1.inView && !s1.follow, "search: Enter steps to older matches, brings each into view and stops following", s1);
  await press("Enter", true);
  const s2 = await ev(CUR);
  check(s2 && s2.n > s1.n && s2.inView, "search: Shift+Enter steps to a newer match", s2);
  check(/[?&]search=short/.test(await ev("location.search")), "search: kept in the address");
  await send("Page.reload");
  await until("document.readyState === 'complete' && !!window.__rhLogs && window.__rhLogs.panes[0].lines.length > 0");
  await sleep(2000);
  const s3 = await ev("({ v: document.getElementById('lq').value, qn: document.getElementById('lqn').textContent })");
  check(s3.v === "short" && /\d+\/\d+/.test(s3.qn), "search: a reload keeps it and counts its matches", s3);
  await ev("(() => { const q = document.getElementById('lq'); q.value = ''; q.dispatchEvent(new Event('input')); })()"); await sleep(400);
  // LG-20: keyboard: Enter in a pane picks a line and focuses "Copy";
  // arrows move it; Esc goes back to the pane.
  await toBottom();
  await ev("window.__rhLogs.panes[0].body.focus()");
  await press("Enter");
  const k1 = await ev("(() => { const a = document.activeElement, l = document.querySelector('.ll.lsel'); return { focus: a && a.textContent, sel: !!l, shown: !!(a && a.getBoundingClientRect().width) , t: l && l.dataset.t }; })()");
  check(k1.focus === "Copy" && k1.sel && k1.shown, "line actions: Enter in a pane picks a line and focuses a visible Copy button", k1);
  await press("ArrowUp");
  const k2 = await ev("(() => { const l = document.querySelector('.ll.lsel'); return { t: l && l.dataset.t, focus: document.activeElement.textContent }; })()");
  check(k2.t && k2.t !== k1.t && k2.focus === "Copy", "line actions: Arrow Up moves to the line above, focus stays on Copy", k2);
  await press("Escape");
  check(await ev("document.activeElement === window.__rhLogs.panes[0].body"), "line actions: Esc returns to the pane");
  // LG-16: a To before From is refused on the field.
  await ev("(() => { const s = document.getElementById('lrange'); s.value = 'custom'; s.dispatchEvent(new Event('change')); })()");
  await ev("(() => { document.getElementById('lfrom').value = '2030-01-02T10:00'; document.getElementById('lto').value = '2030-01-01T10:00'; document.getElementById('lapply').click(); })()");
  const e1 = await ev("({ err: !document.getElementById('lrerr').hidden && document.getElementById('lrerr').textContent, inv: document.getElementById('lfrom').getAttribute('aria-invalid') })");
  check(e1.err && e1.inv === "true", "custom range: a From in the future is refused on its field", e1);
  // LG-33: the single page does not scroll at 1440.
  await load(single); await sleep(1500);
  // (The main column: a sidebar taller than the window is the shell's.)
  const over = await ev("Math.max(0, Math.ceil(document.querySelector('main').getBoundingClientRect().bottom + scrollY - innerHeight))");
  check(over <= 1, `1440 px: the single log page does not scroll (${over} px)`, over);
  // LG-21: "Load older lines" puts older lines in front, the reader stays.
  await until("window.__rhLogs.panes[0].lines.length > 50");
  await sleep(1000);
  const o0 = await ev("(() => { const p = window.__rhLogs.panes[0]; p.follow = false; p.body.scrollTop = 0; return { n: p.lines.length, first: p.lines[0].t, btn: !p.older.hidden }; })()");
  await sleep(400);
  check(o0.btn, "load older: offered at the top of a live tail", o0);
  if (o0.btn) {
    const m0 = await ev(MARK());
    await ev("window.__rhLogs.panes[0].older.click()");
    await until(`window.__rhLogs.panes[0].lines[0].t < ${o0.first}`, 10000);
    await sleep(500);
    const o1 = await ev("({ n: window.__rhLogs.panes[0].lines.length, first: window.__rhLogs.panes[0].lines[0].t })");
    const m1 = await ev(MARK(m0.n));
    check(o1.n > o0.n && o1.first < o0.first && Math.abs(m1.off - m0.off) <= 2, "load older: older lines added in front, the reader's line stays put", { o0, o1, m0, m1 });
  }
  // LG-07: closing a pane of a split view keeps the other one going.
  if (OTHER) {
    await load("/logs?c=" + enc(NAME) + "," + enc(OTHER));
    await until("window.__rhLogs.panes.every((p) => p.lines.length > 0)", 10000);
    await click(".lpane:nth-child(2) .lclose");
    await sleep(800);
    const c1 = await ev("({ panes: window.__rhLogs.panes.length, dom: document.querySelectorAll('.lpane').length, url: location.pathname + location.search, lines: window.__rhLogs.panes[0].lines.length, add: !document.getElementById('ladd').hidden })");
    check(c1.panes === 1 && c1.dom === 1 && c1.url === "/logs?c=" + enc(NAME) && c1.lines > 0 && c1.add, "split view: closing a pane removes it in place, the address follows, the other keeps its lines", c1);
    const d = await dist();
    check(d[0] <= SLACK, `split view: the remaining pane still rests on the newest line (${d[0]} px)`, d);
  }
  // LG-19 / LG-20 on a phone: one toolbar row, the pane ends above the
  // bottom bar, Filters opens the rest, a tap shows Copy and Link.
  if (widths.includes(390)) {
    for (const path of [single].concat(stackHref ? [stackHref] : [])) {
      await load(path, { width: 390, mobile: true });
      await until("window.__rhLogs.panes[0].lines.length > 0");
      await sleep(1200);
      const g = await ev("(() => { const t = document.querySelector('.ltool').getBoundingClientRect(), b = window.__rhLogs.panes[0].body.getBoundingClientRect(), bb = document.querySelector('.bbar').getBoundingClientRect(); return { tool: Math.round(t.height), top: Math.round(b.top), bottom: Math.round(b.bottom), bar: Math.round(bb.top), h: Math.round(b.height), wide: document.documentElement.scrollWidth }; })()");
      check(g.tool <= 48 && g.bottom <= g.bar && g.h >= 844 * 0.5 && g.wide <= 390, `390 px ${path}: one toolbar row (${g.tool} px), the pane ${g.top}..${g.bottom} px ends above the bottom bar (${g.bar}) and takes over half the screen`, g);
      await click("#lfilt");
      const f = await ev("(() => { const m = document.getElementById('lmore').getBoundingClientRect(); return { h: Math.round(m.height), exp: document.getElementById('lfilt').getAttribute('aria-expanded'), wide: document.documentElement.scrollWidth, other: document.querySelector('.lchip[data-lv=o]').getBoundingClientRect().right }; })()");
      check(f.h > 40 && f.exp === "true" && f.wide <= 390 && f.other <= 390, `390 px ${path}: Filters opens the rest, no chip cut off, no sideways scroll`, f);
      await click("#lfilt");
    }
    await load(single, { width: 390, mobile: true });
    await until("window.__rhLogs.panes[0].lines.length > 5");
    await sleep(800);
    const at = await ev("(() => { const p = window.__rhLogs.panes[0], b = p.body.getBoundingClientRect(); return [Math.round(b.left + 40), Math.round(b.top + b.height / 2)]; })()");
    await tap(at[0], at[1]);
    const t1 = await ev("(() => { const l = document.querySelector('.ll.lsel'), bs = l ? [...l.querySelectorAll('.lact button')] : []; return { sel: !!l, buttons: bs.map((b) => Math.round(b.getBoundingClientRect().height)) }; })()");
    check(t1.sel && t1.buttons.length === 2 && t1.buttons.every((h) => h >= 24), "390 px, touch: a tap on a line shows Copy and Link", t1);
  }
}

// ---- CPU while following a busy stream ----
// With the default tail, and with a full ring (5,000 lines held: the
// container's lines since it started), both as information.
async function cpu(what, fill) {
  await load(single);
  await until("window.__rhLogs.panes[0].lines.length > 0");
  if (fill) {
    await ev("(() => { const s = document.getElementById('lrange'); s.value = 'start'; s.dispatchEvent(new Event('change')); })()");
    await until("window.__rhLogs.panes[0].lines.length >= +document.getElementById('lv').dataset.ring", 30000);
  }
  await sleep(1500);
  await send("Performance.enable", { timeDomain: "timeTicks" });
  const m = async () => Object.fromEntries((await send("Performance.getMetrics")).result.metrics.map((x) => [x.name, x.value]));
  const a = await m(); const n0 = (await lines())[0], s0 = await ev("window.__rhLogs.panes[0].lines.reduce((n, r) => Math.max(n, r.n), 0)");
  await sleep(CPU_SECONDS * 1000);
  const b = await m(); const s1 = await ev("window.__rhLogs.panes[0].lines.reduce((n, r) => Math.max(n, r.n), 0)");
  const span = b.Timestamp - a.Timestamp;
  const pc = (k) => (((b[k] - a[k]) / span) * 100).toFixed(1) + " %";
  console.log(`info CPU over ${span.toFixed(1)} s following ${NAME} at 1440 px, Wrap on, ${what} (${s1 - s0} lines, ${n0} held): task ${pc("TaskDuration")}, script ${pc("ScriptDuration")}, layout ${pc("LayoutDuration")} (${b.LayoutCount - a.LayoutCount} layouts), style ${pc("RecalcStyleDuration")}`);
}
if (CPU_SECONDS > 0 && PARTS.includes("cpu")) {
  await cpu("default tail", false);
  await cpu("full ring", true);
}

check(errors.length === 0, "no script errors", errors);
console.log(fails ? `${fails} check(s) failed` : "all checks passed");
ch.kill();
process.exit(fails ? 1 : 0);
