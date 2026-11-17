// Navigation and click-count check (DESIGN.md §11.10, §11.11 as amended by
// the Owner's answers): drives a running test instance with real pointer
// and key events at 1440 and 390 px and counts the clicks of the top tasks
// against the design table; then the phone drawer (inert, Back closes it,
// "More" is a link to a plain page), the command palette, the top bar and
// what a signed-out visitor gets.
//   HB_PASSWORD=... node scripts/nav-check.mjs <base URL> <container> [unhealthy container]
// <container>: a running container whose logs may be read (the check opens
// its page, its logs and its override dialog). [unhealthy container]: one
// with a failing health check, for "why is this red?" (optional).
// Signs in once (the login is rate limited: never run it in a loop with a
// wrong password). It adds a bookmark, a widget, a channel (its test goes to
// the URL the instance has in RH_SLACK_WEBHOOK_URL: point that at a mock
// receiver), a rule and a display name, all named "nav-check-*", and puts
// the config file back byte for byte at the end, also when a check fails.
// Exits 1 when a check fails, 2 on bad usage. Headless Chromium from the
// Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const CTR = process.argv[3];
const SICK = process.argv[4] || "";
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(url) || !CTR || !PW) {
  console.error("usage: HB_PASSWORD=<password> node scripts/nav-check.mjs <base URL of a test instance> <container> [unhealthy container]");
  process.exit(2);
}
const ORIGIN = new URL(url).origin;

// ---- one sign-in, then the API for clean-up ----
const lr = await fetch(url + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: ORIGIN }, body: JSON.stringify({ password: PW }) });
if (lr.status !== 200) { console.error(`sign-in failed: HTTP ${lr.status}`); process.exit(1); }
const cookie = lr.headers.get("set-cookie").split(";")[0];
const token = (await lr.json()).csrf;
const api = async (method, path, body) => {
  const r = await fetch(url + "/api/settings/" + path, { method, headers: { Cookie: cookie, "X-RH-CSRF": token, Origin: ORIGIN, "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  return { status: r.status, data: await r.json().catch(() => ({})) };
};
const start = (await api("GET", "config")).data;
if (/nav-check-/.test(start.text)) { console.error("the config already holds nav-check-* entries: clean it first"); process.exit(1); }
async function restore() {
  const now = (await api("GET", "config")).data;
  if (now.hash !== start.hash) await api("POST", "config", { base_hash: now.hash, text: start.text });
  return (await api("GET", "config")).data.hash === start.hash;
}

// ---- the browser (DevTools protocol, as the other checks) ----
const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const ch = spawn(CH, ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 50; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(100); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); const errors = [];
const opened = []; // window.open calls (the palette's outside links)
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.text);
  if (d.method === "Page.javascriptDialogOpening") { errors.push(`unexpected ${d.params.type} prompt`); ws.send(JSON.stringify({ id: ++id, method: "Page.handleJavaScriptDialog", params: { accept: true } })); }
});
setTimeout(async () => { console.log("FAIL the run timed out"); await restore().catch(() => {}); ch.kill(); process.exit(1); }, 480000).unref();
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
await send("Page.enable"); await send("Runtime.enable"); await send("Network.enable");
const KEYS = { Escape: 27, Enter: 13, Tab: 9, ArrowDown: 40, Home: 36, End: 35 };
async function key(k, mods = 0) {
  const base = { key: k, code: k.length === 1 ? "Key" + k.toUpperCase() : k, windowsVirtualKeyCode: KEYS[k] || k.toUpperCase().charCodeAt(0), nativeVirtualKeyCode: KEYS[k] || k.toUpperCase().charCodeAt(0), modifiers: mods };
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  if (k === "Enter") await send("Input.dispatchKeyEvent", { type: "char", ...base, text: "\r" });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await sleep(80);
}
async function typeText(s) { await send("Input.insertText", { text: s }); await sleep(40); }
const until = async (expr, ms = 5000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };
let W = 1440, touch = false;
async function view(w) {
  W = w; touch = w < 500;
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: touch ? 844 : 900, deviceScaleFactor: 1, mobile: touch });
  await send("Emulation.setTouchEmulationEnabled", { enabled: touch, maxTouchPoints: touch ? 5 : 0 });
}
// A click (a tap at 390) on the element, counted for the task in progress.
let clicks = 0;
async function clickAt(x, y) {
  if (touch) {
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  } else for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  await sleep(120);
}
async function click(sel, count = true) {
  const r = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; e.scrollIntoView({ block: "center" }); const b = e.getBoundingClientRect(); return [b.left + Math.min(b.width / 2, 40), b.top + b.height / 2]; })()`);
  if (!r) throw new Error("no element " + sel);
  if (count) clicks++;
  await clickAt(r[0], r[1]);
}
async function load(path) {
  await send("Page.navigate", { url: url + path });
  await until("document.readyState === 'complete'", 10000);
  await sleep(500); // deferred scripts, the dialog-on-load timer
}
const here = () => ev("location.pathname + location.search + location.hash");
// Waits for a navigation the last click started.
async function landed(test, ms = 6000) { return until(`(${test})(location)`, ms).then(async (ok) => { await until("document.readyState === 'complete'"); await sleep(300); return ok; }); }

let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };
const counts = [];
// One task: `run` performs it from its start page; `want` is the design's
// count; the result line says what was measured.
async function task(name, want, run) {
  clicks = 0;
  let ok = false, note = "";
  try { ok = await run(); } catch (e) { note = String(e.message || e).slice(0, 160); }
  counts.push([W, name, clicks, want, ok]);
  check(ok && clicks <= want, `${W}: ${name}: ${clicks} click${clicks === 1 ? "" : "s"} (design ${want})`, note || (ok ? clicks : "did not finish"));
}
const enc = encodeURIComponent;
const toasts = () => ev("[...document.querySelectorAll('body>.toasts>.toast')].map((t) => t.textContent.trim())");
// An outside change: a second tab adds a bookmark "nav-check-out<n>" through
// the Bookmarks tab's own dialog (the app's form, the same session).
let outN = 0;
async function outside() {
  const name = `nav-check-out${++outN}`;
  const t2 = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
  const w2 = new WebSocket(t2.webSocketDebuggerUrl);
  await new Promise((r) => w2.addEventListener("open", r));
  let n2 = 0; const p2 = new Map();
  w2.addEventListener("message", (m) => { const d = JSON.parse(m.data); if (d.id && p2.has(d.id)) { p2.get(d.id)(d); p2.delete(d.id); } });
  const s2 = (method, params = {}) => new Promise((r) => { const k = ++n2; p2.set(k, r); w2.send(JSON.stringify({ id: k, method, params })); });
  const e2 = async (expression) => { const r = await s2("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }); return r.result && r.result.result && r.result.result.value; };
  const u2 = async (expr, ms = 8000) => { for (let i = 0; i < ms / 50; i++) { if (await e2(expr).catch(() => false)) return true; await sleep(50); } return false; };
  let ok = false;
  try {
    await s2("Page.enable");
    await s2("Page.navigate", { url: url + "/settings/bookmarks" });
    await u2("document.readyState === 'complete' && !!window.HB && !!document.getElementById('bm-add')");
    await sleep(400);
    await e2("document.getElementById('bm-add').click()");
    await u2("document.getElementById('dlg-bookmark').open");
    await e2(`(() => { const f = document.querySelector('#dlg-bookmark form'); f.elements.namedItem('name').value = ${JSON.stringify(name)}; f.elements.namedItem('url').value = 'https://nav-check.example/out'; f.requestSubmit(); })()`);
    ok = await u2(`!document.getElementById('dlg-bookmark').open && document.getElementById('bm-list').textContent.includes(${JSON.stringify(name)})`);
  } finally {
    w2.close();
    await send("Target.closeTarget", { targetId: t2.id });
  }
  return ok;
}

try {
  // ================================================== signed out
  await view(1440);
  await load("/");
  check(await ev("(() => { const a = document.querySelector('#account a.acct-in'); return !!a && /Sign in/.test(a.textContent) && a.getAttribute('href').startsWith('/login?next=') && !document.getElementById('account-menu'); })()"), "signed out: the account control is a plain Sign in link (NAV-43)");
  check(await ev("!document.querySelector('.head-add') && !document.querySelector('dialog.dlg') && !document.querySelector('meta[name=rh-csrf]')"), "signed out: Overview has no Add, no dialog, no token");
  await load("/settings/general");
  check(await ev("!!document.querySelector('.set-lock') && !document.querySelector('.set-nav') && !document.querySelector('form')"), "signed out: Settings is the lock panel only (SET-21)");
  await load("/alerts");
  check(await ev("!document.querySelector('#set-main, dialog.dlg, [data-test], [data-toggle]') && !!document.getElementById('al-pub')"), "signed out: Alerts shows the read-only tables, no editing");
  await load(`/containers/${enc(CTR)}`);
  check(await ev("!document.querySelector('.cd-probes, code.probe')"), "signed out: no probe output on a container page");
  await send("Runtime.evaluate", { expression: "1" });
  await view(390);
  await load("/");
  // JavaScript blocked: "More" is a link to the plain page (ROB-16).
  await send("Emulation.setScriptExecutionDisabled", { value: true });
  await load("/");
  const moreHref = await ev("(document.querySelector('#bbar [data-act=more]') || {}).getAttribute && document.querySelector('#bbar [data-act=more]').getAttribute('href')");
  await load("/more");
  const morePage = await ev("['/host','/history','/settings','/logs','/alerts'].every((h) => !!document.querySelector('.more-nav a[href=\"' + h + '\"]')) && !!document.querySelector('.more-stack a.ia.logs')");
  check(moreHref === "/more" && morePage, "no JS: More is a link to /more, which lists every page and the stacks with logs", { moreHref, morePage });
  const drawerHidden = await ev("getComputedStyle(document.getElementById('side')).visibility");
  check(drawerHidden === "hidden", "no JS: the closed drawer is out of the tab order (visibility hidden, A11Y-01)", drawerHidden);
  await send("Emulation.setScriptExecutionDisabled", { value: false });

  // ================================================== signed in
  const [cn, cv] = cookie.split("=");
  await send("Network.setCookie", { name: cn, value: cv, url, httpOnly: true, sameSite: "Strict" });

  for (const w of [1440, 390]) {
    await view(w);
    const phone = w < 500;
    // ---- logs, one click from where a container is named ----
    await load("/");
    await task("logs from Overview Top CPU", 1, async () => { await click("#ov-top .bars li .ia.logs"); return landed("(l) => /\\/logs$/.test(l.pathname)"); });
    await load("/");
    const tmIcon = await ev("(() => { const i = [...document.querySelectorAll('.tm-logs')].find((x) => getComputedStyle(x).display !== 'none'); if (!i) return null; i.id = 'nc-tm'; return true; })()");
    // A tile of 64 × 40 px or more shows its logs icon at 1440 (OV-01); at
    // 390 the design allows 2 clicks (tile, then Logs), so no icon is a skip.
    const tmBig = await ev("[...document.querySelectorAll('.tm:not(.tm-rest)')].some((t) => { const r = t.getBoundingClientRect(); return r.width >= 64 && r.height >= 40; })");
    if (tmIcon) await task("logs from an Overview treemap tile", 1, async () => { await click("#nc-tm"); return landed("(l) => /\\/logs$/.test(l.pathname)"); });
    else if (phone) console.log(`skip ${w}: treemap: no tile shows its logs icon at this width (88 × 56 px on touch; the design's 2-click exception)`);
    else if (tmBig) check(false, `${w}: treemap: a tile of at least 64 × 40 px shows its logs icon`, "a big tile without the icon");
    else console.log(`skip ${w}: treemap: no tile of 64 × 40 px or more`);
    // The treemap tiles hold their names: nothing is cut off at the top.
    const tm = await ev("[...document.querySelectorAll('.tm .tm-n')].map((n) => { const t = n.closest('.tm').getBoundingClientRect(), r = n.getBoundingClientRect(); return r.top >= t.top - 0.5 && r.bottom <= t.bottom + 0.5 && r.height > 0; }).every(Boolean)");
    check(tm, `${w}: every treemap tile shows its name inside the tile (OV-09)`);
    await load("/containers");
    await task("logs from the containers table", 1, async () => { await click(`#ctable tr.row[data-c="${CTR}"] a[href$="/logs"]`); return landed("(l) => /\\/logs$/.test(l.pathname)"); });
    await load("/");
    await task("logs from the sidebar tree", phone ? 2 : 1, async () => {
      if (phone) { await click("#bbar [data-act=more]"); await sleep(400); } // the drawer slides in
      await click(`#tree a.ia.logs[href="/containers/${enc(CTR)}/logs"]`);
      return landed("(l) => /\\/logs$/.test(l.pathname)");
    });
    // ---- why is this red? ----
    if (SICK) {
      await load("/");
      const row = await ev(`(() => { const a = document.querySelector('#ov-attn a.row-link[data-c="${SICK}"]'); return a ? a.closest('li').textContent : null; })()`);
      check(!!row && /unhealthy/.test(row) && !!(await ev(`!!document.querySelector('#ov-attn a.row-link[data-c="${SICK}"]').closest('li').querySelector('.probe')`)), `${w}: the attention row says why, with the probe line (0 clicks, NAV-11)`, row);
      await task("see why a container is unhealthy", 1, async () => { await click(`#ov-attn a.row-link[data-c="${SICK}"]`); return (await landed(`(l) => l.pathname === "/containers/${enc(SICK)}"`)) && ev("!!document.querySelector('.cd-why .cd-probes') && /Why is this red/.test(document.querySelector('.cd-why').textContent)"); });
    }
    // ---- add a bookmark (Overview: Add, type, Add bookmark) ----
    await load("/");
    await task("add a bookmark", 2, async () => {
      await click("#ov-marks .head-add a");
      await until("document.getElementById('dlg-bookmark').open");
      await typeText(`nav-check-${w}`); // the first field has focus
      await ev("document.querySelector('#dlg-bookmark [name=url]').focus()"); await typeText("https://nav-check.example/");
      await click("#dlg-bookmark [type=submit]");
      return until(`!document.getElementById('dlg-bookmark').open && /nav-check-${w}/.test(document.getElementById('ov-marks').textContent)`, 6000);
    });
    // ---- add a widget with one field (Add, type, Add widget; Fetch now is the optional test) ----
    await load("/");
    await task("add a widget with one field (without Fetch now)", 2, async () => {
      await click("#ov-w .head-add a");
      await until("document.getElementById('dlg-widget').open");
      await typeText(`nav-check-w${w}`);
      await ev("document.querySelector('#dlg-widget [name=url]').focus()"); await typeText("http://nav-check.example/api");
      await ev("document.querySelector('#dlg-widget [data-rows=field] input').focus()"); await typeText("v");
      await ev("document.querySelectorAll('#dlg-widget [data-rows=field] input')[1].focus()"); await typeText("value");
      await click("#dlg-widget [type=submit]");
      return until(`!document.getElementById('dlg-widget').open && !!document.getElementById('w-nav-check-w${w}')`, 6000);
    });
    // ---- add a channel and send a test (Alerts, Add channel, type, Save and send test) ----
    await load("/");
    await task("add a channel and send a test", 3, async () => {
      await click(phone ? "#bbar a[href='/alerts']" : ".nav a[href='/alerts']");
      await landed("(l) => l.pathname === '/alerts'");
      await click("#ch-add");
      await until("document.getElementById('dlg-channel').open");
      await typeText(`nav-check-c${w}`);
      await ev("document.querySelector('#dlg-channel [name=url_env]').focus()"); await typeText("RH_SLACK_WEBHOOK_URL");
      await click("#dlg-channel [data-submit=test]");
      return until("document.getElementById('dlg-channel').open && /Test sent/.test(document.querySelector('#dlg-channel .dlg-note').textContent)", 10000);
    });
    await key("Escape"); await sleep(300);
    // ---- add an alert rule (Alerts, Add rule, type, Add rule) ----
    await load("/");
    await task("add an alert rule", 3, async () => {
      await click(phone ? "#bbar a[href='/alerts']" : ".nav a[href='/alerts']");
      await landed("(l) => l.pathname === '/alerts'");
      await click("#ru-add");
      await until("document.getElementById('dlg-rule').open");
      await typeText(`nav-check-r${w}`);
      await click("#dlg-rule [type=submit]");
      return until(`!document.getElementById('dlg-rule').open && !!document.querySelector('#ru-list tr[data-key="rule:nav-check-r${w}"]')`, 6000);
    });
    // ---- rename a container, from its page (Edit, type, Save) ----
    await load(`/containers/${enc(CTR)}`);
    await task("rename a container (from its page)", 2, async () => {
      await click(".ctr-actions a[href*='edit=override']");
      await landed("(l) => l.pathname === '/settings/containers'");
      await until("document.getElementById('dlg-override') && document.getElementById('dlg-override').open");
      await ev("document.querySelector('#dlg-override [name=name]').select()"); await typeText(`nav-check name ${w}`);
      await click("#dlg-override [type=submit]");
      return until("!document.getElementById('dlg-override').open", 6000);
    });
    // ---- change the theme: one click per step, the name is said ----
    await load("/");
    const th0 = await ev("document.documentElement.dataset.theme || 'auto'");
    await task("change the theme one step", 1, async () => {
      await click("#theme");
      const th1 = await ev("document.documentElement.dataset.theme");
      return th1 !== th0 && (await toasts()).some((x) => /^Theme: /.test(x));
    });
    // put this browser's theme back
    await ev(`(() => { try { localStorage.setItem('rh-theme', ${JSON.stringify(th0)}); } catch {} })()`);
    // ---- the last incident's logs (Alerts, then "Logs at that moment") ----
    await load("/");
    const hasLog = await ev("true");
    if (hasLog) await task("find the last incident and its logs", 2, async () => {
      await click(phone ? "#bbar a[href='/alerts']" : ".nav a[href='/alerts']");
      await landed("(l) => l.pathname === '/alerts'");
      const any = await ev("!!document.querySelector('.al-log .ia.logs, .incident .ia.logs')");
      if (!any) { clicks++; return true; } // nothing about a container has fired here: the path is there, the row is not
      await click(".al-log .ia.logs, .incident .ia.logs");
      return landed("(l) => /\\/logs$/.test(l.pathname) && /#t=\\d+/.test(l.hash)");
    });
    // ---- Settings opens the tab used last (Y17) ----
    await load("/settings/notifications");
    await load("/");
    const setHref = await ev(`document.querySelector(${JSON.stringify(phone ? "#bbar a[href^='/settings'], .nav a[href^='/settings']" : ".nav a[href^='/settings']")}).getAttribute('href')`);
    check(setHref === "/settings/notifications", `${w}: the Settings link opens the tab used last`, setHref);
    // ---- wave 5: the containers table and a container page at this width ----
    const noX = "document.documentElement.scrollWidth <= innerWidth";
    await load("/containers");
    check(await ev(noX), `${w}: /containers does not scroll sideways (ROB-07)`, await ev("document.documentElement.scrollWidth"));
    if (phone) {
      check(await ev("(() => { const s = document.getElementById('csort'); return !!s && s.offsetParent !== null && s.options.length === 6; })()"), "390: the table can be sorted from the toolbar (OV-19)");
      await ev("(() => { const s = document.getElementById('csort'); s.value = 'mem'; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
      check(await until("/[?&]sort=mem/.test(location.search) && document.querySelector('#ctable th[data-sort=mem]').getAttribute('aria-sort') === 'descending'"), "390: Sort: Memory sorts the table and is in the URL (OV-19)", await here());
      await click("#cdir", false);
      check(await until("/dir=asc/.test(location.search) && document.getElementById('cdir').dataset.dir === 'asc'"), "390: the direction button flips the sort (OV-19)", await here());
      await ev("localStorage.removeItem('rh-ctr-view')"); // the remembered sort goes back to the default
    } else {
      check(await ev("getComputedStyle(document.getElementById('csortw')).display === 'none'"), "1440: the toolbar's sort select is hidden (the header sorts)");
      await load("/containers?group=0");
      check(await ev("[...document.querySelectorAll('#ctable tbody.flat td.c-state')].every((td) => td.scrollWidth <= td.clientWidth + 1)"), "1440: flat view: no State cell is cut (OV-21)");
      await ev("localStorage.removeItem('rh-ctr-view')");
    }
    await load(`/containers/${enc(CTR)}`);
    check(await ev(noX), `${w}: a container page does not scroll sideways (OV-40)`, await ev("document.documentElement.scrollWidth"));
    if (phone) {
      const t = await ev("(() => { const n = document.querySelector('.cd-tabs'); return n.scrollWidth > n.clientWidth + 1 ? n.classList.contains('more-r') : 'fits'; })()");
      check(t === true || t === "fits", "390: the tab strip fades where more tabs are (OV-36)", t);
    }
  }

  // ================================================== outside changes (Y12) and the 409 way out
  // An outside change is a save in another tab, through the Bookmarks tab's
  // own dialog. `/` and `/alerts` reload for it, but never over a dialog,
  // a confirm or input: there the base-hash rule answers 409 and "Reload
  // and keep my input" moves the dialog to the file as it is now.
  await view(1440);
  await load("/settings/general"); // the last-used tab, for the Settings link check below
  const e0 = errors.length;
  const stayed = () => ev("window.__nc === 1");
  for (const path of ["/", "/alerts"]) {
    await load(path); await ev("window.__nc = 1");
    const ok = await outside();
    check(ok && (await until("window.__nc !== 1 && document.readyState === 'complete'", 6000)), `${path}: an outside config change reloads the page when no dialog is open and nothing is typed (Y12)`);
  }
  const conflict = async (path, label, trigger, dlg, fill, saved) => {
    await load(path); await ev("window.__nc = 1");
    await click(trigger, false);
    await until(`document.getElementById('${dlg}').open`);
    await fill();
    const ok = await outside(); await sleep(1500);
    check(ok && (await stayed()) && (await ev(`document.getElementById('${dlg}').open`)), `${path}: ${label}: an outside change with the dialog open and typed in does not reload the page`);
    await click(`#${dlg} [type=submit]`, false);
    check(await until(`/changed on disk/.test(document.querySelector('#${dlg} .dlg-note').textContent)`), `${path}: ${label}: Save after it is refused (409, the base-hash rule)`);
    const e1 = errors.length;
    await click(`#${dlg} .dlg-note .acts .btn.primary`, false);
    const re = await until(`/Reloaded the current file/.test(document.querySelector('#${dlg} .dlg-note').textContent)`);
    check(re && errors.length === e1, `${path}: ${label}: "Reload and keep my input" moves the dialog to the current file`, errors.slice(e1));
    await click(`#${dlg} [type=submit]`, false);
    check(await until(`!document.getElementById('${dlg}').open && (${saved})`, 6000), `${path}: ${label}: then Save succeeds`);
  };
  await conflict("/", "Add bookmark", "#ov-marks .head-add a", "dlg-bookmark", async () => {
    await typeText("nav-check-c409");
    await ev("document.querySelector('#dlg-bookmark [name=url]').focus()"); await typeText("https://nav-check.example/409");
  }, "/nav-check-c409/.test(document.getElementById('ov-marks').textContent)");
  check(await ev("document.activeElement === document.querySelector('#ov-marks .head-add a')"), "/: after the save the panels are swapped in and focus is back on Add (not <body>)", await ev("document.activeElement.outerHTML.slice(0, 80)"));
  await conflict("/", "Add widget", "#ov-w .head-add a", "dlg-widget", async () => {
    await typeText("nav-check-w409");
    await ev("document.querySelector('#dlg-widget [name=url]').focus()"); await typeText("http://nav-check.example/api");
    await ev("document.querySelector('#dlg-widget [data-rows=field] input').focus()"); await typeText("v");
    await ev("document.querySelectorAll('#dlg-widget [data-rows=field] input')[1].focus()"); await typeText("value");
  }, "!!document.getElementById('w-nav-check-w409')");
  await conflict("/alerts", "Add channel", "#ch-add", "dlg-channel", async () => {
    await typeText("nav-check-ch409");
    await ev("document.querySelector('#dlg-channel [name=url_env]').focus()"); await typeText("RH_SLACK_WEBHOOK_URL");
  }, "!!document.querySelector('#ch-list tr[data-key=\"channel:nav-check-ch409\"]')");
  await conflict("/alerts", "Add rule", "#ru-add", "dlg-rule", async () => { await typeText("nav-check-ru409"); },
    "!!document.querySelector('#ru-list tr[data-key=\"rule:nav-check-ru409\"]')");
  // A delete confirm left open over an outside change: no reload, and the
  // delete is refused, never applied to what now sits at that place.
  const RK = 'tr[data-key="rule:nav-check-ru409"]';
  await load("/alerts"); await ev("window.__nc = 1");
  await click(`${RK} [data-menu]`, false); await sleep(150);
  await click("#m-ru [data-row-act=delete]", false);
  await until("document.getElementById('dlg-confirm').open");
  const okc = await outside(); await sleep(1500);
  check(okc && (await stayed()) && (await ev("document.getElementById('dlg-confirm').open")), "/alerts: an outside change with a delete confirm open does not reload the page");
  await click("#dlg-confirm [data-ok]", false);
  check(await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted: the config file changed/.test(t.textContent))"), "/alerts: that delete is refused (409)", await toasts());
  await sleep(1500);
  check((await stayed()) && (await ev(`!!document.querySelector('${RK}')`)), "/alerts: the rule is still there and the page did not reload over the message");
  // A rule switch clicked before the change's event arrives (the live
  // stream held back): refused, and the switch goes back.
  await send("Network.setBlockedURLs", { urls: ["*/api/stream*"] });
  await load("/alerts");
  const was = await ev(`document.querySelector('${RK} [data-toggle]').checked`);
  await outside();
  await click(`${RK} [data-toggle]`, false);
  check(await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not switched: the config file changed/.test(t.textContent))") && (await ev(`document.querySelector('${RK} [data-toggle]').checked`)) === was, "/alerts: a rule switch based on the old file is refused (409) and goes back", await toasts());
  await send("Network.setBlockedURLs", { urls: [] });
  check(errors.length === e0, "outside changes: no script errors", errors.slice(e0));
  // A remembered tab that does not exist is not used (no 404 behind Settings).
  await ev("localStorage.setItem('rh-set-tab', 'nosuchtab')");
  await load("/");
  check(await ev("document.querySelector('.nav a[href^=\"/settings\"]').getAttribute('href')") === "/settings", "a stale remembered Settings tab falls back to /settings");
  await load("/settings/general");

  // ================================================== the phone drawer
  await view(390);
  await load("/");
  check(await ev("document.getElementById('side').inert === true && !document.getElementById('menu-btn')"), "390: the closed drawer is inert; there is no hamburger (Y3, A11Y-01)");
  // Tab from the skip link goes to the top bar, not into the drawer.
  await ev("document.querySelector('.skip').focus()"); await key("Tab");
  check(await ev("!!document.activeElement.closest('.col, #bbar') && !document.activeElement.closest('#side')"), "390: Tab from the skip link lands in the page, not the hidden drawer", await ev("document.activeElement.outerHTML.slice(0, 80)"));
  const p0 = await here();
  await click("#bbar [data-act=more]", false); await sleep(400);
  check(await ev("document.documentElement.classList.contains('side-open') && !document.getElementById('side').inert && document.querySelector('.col').inert && document.getElementById('bbar').inert && document.querySelector('#bbar [data-act=more]').getAttribute('aria-expanded') === 'true'"), "390: More opens the drawer and makes the page inert (A11Y-03)");
  check(await ev("!!document.activeElement.closest('#side')"), "390: focus moves into the drawer");
  check(await ev("(() => { const b = document.querySelector('#side [data-act=side-close]'); return !!b && getComputedStyle(b).display !== 'none' && b.getAttribute('aria-label') === 'Close menu'; })()"), "390: the drawer has a Close button (NAV-34)");
  await ev("history.back()"); await sleep(500);
  check(await ev("!document.documentElement.classList.contains('side-open') && document.getElementById('side').inert && !document.querySelector('.col').inert") && (await here()) === p0, "390: Back closes the drawer and stays on the page (NAV-33)", await here());
  await ev("history.forward()"); await sleep(500);
  check(await ev("document.documentElement.classList.contains('side-open') && !document.getElementById('side').inert") && (await here()) === p0, "390: Forward after Back opens the drawer again (no dead history entry)", await here());
  await key("Escape"); await sleep(400);
  check(await ev("!document.documentElement.classList.contains('side-open')") && (await here()) === p0, "390: Esc closes it again", await here());
  await click("#bbar [data-act=more]", false); await sleep(400); await click("#side [data-act=side-close]", false); await sleep(300);
  check(await ev("!document.documentElement.classList.contains('side-open') && document.activeElement === document.querySelector('#bbar [data-act=more]')"), "390: Close closes it and focus returns to More");
  await click("#bbar [data-act=more]", false); await key("Escape"); await sleep(300);
  check(await ev("!document.documentElement.classList.contains('side-open')"), "390: Esc closes it");
  for (const p of ["/host", "/history", "/settings/general"]) {
    await load(p);
    check(await ev("document.querySelector('#bbar [data-act=more]').getAttribute('aria-current') === 'page'"), `390: More is marked current on ${p} (NAV-31)`);
  }
  await load("/containers");
  check(await ev("!document.querySelector('#bbar [data-act=more]').hasAttribute('aria-current')"), "390: More is not current on Containers");

  // ================================================== 700 to 1100 px: the toggle
  await view(900);
  await load("/");
  const before = await ev("Math.round(document.querySelector('.col').getBoundingClientRect().left)");
  await click("#rail-toggle", false);
  const after = await ev("({ left: Math.round(document.querySelector('.col').getBoundingClientRect().left), open: document.documentElement.classList.contains('side-open'), label: document.getElementById('rail-toggle').getAttribute('aria-label') })");
  check(after.open && after.left === before && before >= 56, "900: the sidebar opens over the page, which keeps its place (NAV-21)", { before, ...after });
  await key("Escape"); await sleep(200);
  check(await ev("document.getElementById('rail-toggle').getAttribute('aria-label')") === "Open sidebar", "900: the toggle says what it does (NAV-26)");

  // ================================================== palette and top bar
  await view(1440);
  await load("/");
  check(await ev("[...document.querySelectorAll('#vitals a.chip')].every((a) => a.getAttribute('href') === '/host')"), "1440: the CPU, MEM and DISK chips open the Host page (NAV-40)");
  await click(".menu-btn-acct", false); await sleep(150);
  check(await ev("!document.querySelector('#account-menu a[href^=\"/settings\"]') && !!document.querySelector('#account-menu [data-act=logout]')"), "1440: the account menu has no Settings item (Y4)");
  await key("Escape");
  const pal = async (q) => {
    await ev("document.getElementById('palette').hidden || document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true }))");
    await key("k", 2); await until("!document.getElementById('palette').hidden"); await until("document.querySelectorAll('#pal-list li').length > 3");
    await ev("(() => { const i = document.getElementById('pal-q'); i.value = ''; })()");
    if (q) await typeText(q);
    await sleep(150);
    return ev("[...document.querySelectorAll('#pal-list li')].map((li) => li.querySelector('.pk').textContent + ': ' + li.querySelector('.pt').textContent)");
  };
  let rows = await pal("");
  const kinds = rows.map((r) => r.split(":")[0]);
  const lastB = kinds.lastIndexOf("Bookmark"), firstC = kinds.indexOf("Container");
  check(lastB < 0 || firstC < 0 || lastB < firstC, "palette: empty, bookmarks come before the containers (NAV-54)", rows.slice(0, 12));
  check(!rows.some((r) => /Toggle sidebar|Toggle theme/.test(r)) && ["Auto", "Dark", "Light", "Ember", "High contrast"].every((n) => rows.some((r) => r.startsWith("Action: Theme: " + n))), "palette: one action per theme, no Toggle sidebar (NAV-55, Y5)");
  // The container is found by its name; the palette shows its display name.
  rows = await pal(CTR);
  const top = (rows[0] || "").replace(/^Container: /, "");
  check(rows[0].startsWith("Container: ") && rows[1] === `Logs: Logs: ${top}`, "palette: Logs: X ranks directly under X (NAV-51)", rows.slice(0, 4));
  check(rows.slice(1, 4).includes(`Action: Edit ${top}`), "palette: Edit X, signed in (NAV-52)", rows.slice(0, 5));
  await key("Enter");
  check(await landed("(l) => /^\\/containers\\/[^/]+$/.test(l.pathname)"), "palette: Enter on a container opens its Hobnail page (NAV-50, Y8)", await here());
  rows = await pal("add");
  check(["Add bookmark", "Add widget", "Add channel", "Add alert rule"].every((n) => rows.includes("Action: " + n)), "palette: the top tasks are actions (NAV-52)", rows);
  rows = await pal("add channel");
  await key("Enter");
  check(await landed("(l) => l.pathname === '/alerts'") && await until("document.getElementById('dlg-channel').open"), "palette: Add channel opens the channel dialog");
  await key("Escape"); await sleep(300);
  rows = await pal("zzqx-no-such-thing");
  const sel = await ev("document.querySelectorAll('#pal-list li[aria-selected=true]').length");
  const p1 = await here();
  await key("Enter"); await sleep(300);
  check(rows.length === 1 && /^Search/.test(rows[0]) && sel === 0 && !(await ev("document.getElementById('palette').hidden")) && (await here()) === p1, "palette: Enter on a query with no match does nothing (NAV-57)", { rows, sel });
  await key("ArrowDown");
  check(await ev("document.querySelectorAll('#pal-list li[aria-selected=true]').length") === 1, "palette: arrow down chooses the web search");
  await ev("(() => { const i = document.getElementById('pal-q'); i.value = ''; i.dispatchEvent(new Event('input')); })()"); await sleep(100);
  await key("End");
  const endAt = await ev("[...document.querySelectorAll('#pal-list li')].findIndex((li) => li.getAttribute('aria-selected') === 'true') === document.querySelectorAll('#pal-list li').length - 1");
  await key("Home");
  const homeAt = await ev("document.querySelector('#pal-list li').getAttribute('aria-selected') === 'true'");
  check(endAt && homeAt, "palette: End and Home go to the last and first row (NAV-58)");
  await key("Escape");
  // ---- the sidebar (NAV-22 to NAV-25) ----
  await load("/");
  await click("#tree-filter", false); await typeText(CTR); await sleep(150);
  await key("ArrowDown");
  check(await ev("!!document.activeElement.closest('#tree li.ctr')"), "1440: Down in the sidebar filter moves into the results (NAV-22)");
  await ev("document.getElementById('tree-filter').focus()"); await key("Enter");
  check(await landed("(l) => /^\\/containers\\/[^/]+$/.test(l.pathname)"), "1440: Enter in the sidebar filter opens the first match (NAV-22)", await here());
  check(await ev(`document.getElementById('tree-filter').value === ${JSON.stringify(CTR)}`), "1440: the filter text survives the navigation (NAV-23)");
  check(await ev("(() => { const a = document.querySelector('#tree a[aria-current=page]'), t = document.getElementById('tree'); if (!a) return false; const r = a.getBoundingClientRect(), b = t.getBoundingClientRect(); return r.top >= b.top - 1 && r.bottom <= b.bottom + 1; })()"), "1440: the open container's item is in view in the tree (NAV-24)");
  await ev("(() => { const f = document.getElementById('tree-filter'); f.value = ''; f.dispatchEvent(new Event('input')); })()");
  const stackHref = await ev("document.querySelector('#tree a.stack-name').getAttribute('href')");
  await click("#tree a.stack-name", false);
  check(await landed("(l) => l.pathname === '/containers' && /stack=/.test(l.search)"), "1440: a stack's name in the tree opens it in the table (NAV-25)", stackHref);

  await view(390);
  await load("/");
  await key("k", 2); await until("!document.getElementById('palette').hidden");
  check(await ev("getComputedStyle(document.querySelector('.pal-x')).display !== 'none'"), "390: the palette has a close button");
  await click(".pal-x", false);
  check(await ev("document.getElementById('palette').hidden"), "390: the close button closes it");
  // Breadcrumbs at 390: the parent keeps its name (NAV-44).
  await load(`/containers/${enc(CTR)}`);
  check(await ev("(() => { const a = document.querySelector('.crumb > a'); return !!a && a.getBoundingClientRect().width > 30 && document.documentElement.scrollWidth <= innerWidth; })()"), "390: the breadcrumb's parent link keeps its width, no sideways scroll (NAV-44)");

  // ================================================== History, Host: windows, markers, tables
  // Every table on Host fits its panel; History does not scroll sideways
  // (LG-35, LG-36, LG-45).
  const tablesFit = "[...document.querySelectorAll('.hx-t')].filter((t) => t.offsetParent && t.scrollWidth > (t.closest('.scroll-x') || t.parentElement).clientWidth + 1).map((t) => (t.querySelector('caption') || t.closest('section').querySelector('h2')).textContent)";
  for (const w of [390, 1440]) {
    await view(w);
    await load("/host");
    const over = await ev(tablesFit);
    check(over.length === 0 && (await ev("document.documentElement.scrollWidth <= innerWidth")), `${w}: /host: every table fits its panel, signed in (LG-35, LG-36)`, over);
    // The 3 s poll re-ranks the process tables and rewrites the figures:
    // the page's height stays put (wave 5 review: Processes at 390 moved).
    const hs = [];
    for (let i = 0; i < 4; i++) { hs.push(await ev("document.documentElement.scrollHeight")); await sleep(3200); }
    check(hs.every((h) => h === hs[0]), `${w}: /host: the page keeps its height across live updates (${hs.join(", ")})`, hs);
  }
  await view(390);
  await load("/history?range=24h&events=all");
  check(await ev("document.documentElement.scrollWidth <= innerWidth"), "390: /history does not scroll sideways (LG-45)", await ev("document.documentElement.scrollWidth"));
  // The timeline shows the chart window (LG-43).
  // { chart, rows, ok }: no chart fails; no rows is a skip, never an ok.
  const inWindow = "(() => { const f = document.querySelector('figure.chart[data-t0]'); if (!f) return { chart: false, rows: 0, ok: false }; const t0 = +f.dataset.t0, t1 = +f.dataset.t1, ts = [...document.querySelectorAll('.tl .ev time[data-t]')]; return { chart: true, rows: ts.length, ok: ts.every((t) => +t.dataset.t >= t0 && +t.dataset.t <= t1) }; })()";
  const windowCheck = (w, what) => {
    if (!w.chart) return check(false, `${what}: the chart is there`, w);
    if (!w.rows) return console.log(`skip ${what}: no timeline rows in this window`);
    check(w.ok, what, w);
  };
  await view(1440);
  await load("/history?range=6h&events=all");
  windowCheck(await ev(inWindow), "1440: /history: every timeline row is inside the chart window (LG-43)");
  // Zoom by a plain link: by touch and without the script too (LG-46).
  clicks = 0;
  await click(".zoom-nav a.btn:nth-child(2)");
  check((await landed("(l) => /from=\\d+&to=\\d+/.test(l.search)")) && clicks === 1, "1440: Zoom in is one click (LG-46)", await here());
  windowCheck(await ev(inWindow), "1440: the timeline follows the zoom (LG-43)");
  await click(".zoom-nav a.btn:first-child", false);
  check(await landed("(l) => /from=\\d+&to=\\d+/.test(l.search)") && (await ev("!!document.querySelector('.zoom-nav a[href*=\"to=\"]:last-child')")), "1440: Earlier moves the window back; Later is then a link");
  // A marker opens its event's row, highlighted (LG-44).
  await load("/history?range=24h");
  const mk = await ev("(() => { const m = document.querySelector('.evm a.mk'); return m && m.getAttribute('href'); })()");
  if (mk) {
    await click(".evm a.mk", false);
    const id = mk.split("#ev-")[1];
    check(await until(`location.hash === '#ev-${id}' && !!document.querySelector('.tl .ev:target')`, 6000), "1440: a chart marker opens its timeline row, as the target (LG-44)", { mk, at: await here() });
  } else console.log("skip 1440: a chart marker opens its timeline row (LG-44): no event markers in 24 h");
  // Legend toggles act on every chart and are remembered (LG-51).
  if (await ev("!!document.querySelector('.lg[data-ser=max]')")) {
    await click(".lg[data-ser=max]", false);
    const all = "[...document.querySelectorAll('figure.chart:has(.lg[data-ser=max])')].every((f) => f.classList.contains('off-max'))";
    const offNow = await ev(all);
    await load("/history?range=24h");
    check(offNow && (await ev(all)), "1440: a legend toggle hides that layer on every chart, still after a reload (LG-51)");
    await click(".lg[data-ser=max]", false);
    check(!(await ev("!!document.querySelector('figure.chart.off-max')")), "1440: and shows it again");
  }
  // Touch: a sideways drag across a chart zooms (LG-46).
  await view(390);
  await load("/history?range=6h");
  const box = await ev("(() => { const c = document.querySelector('figure.chart[data-zoom] .canvas'); c.scrollIntoView({ block: 'center' }); const b = c.getBoundingClientRect(); return [b.left, b.top + b.height / 2, b.width]; })()");
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box[0] + box[2] * 0.3, y: box[1] }] });
  for (let i = 1; i <= 6; i++) { await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: box[0] + box[2] * (0.3 + i * 0.07), y: box[1] }] }); await sleep(30); }
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  check(await landed("(l) => /from=\\d+&to=\\d+/.test(l.search)"), "390: a sideways touch drag across a chart zooms (LG-46)", await here());
  // A stored legend choice that is not a known layer is ignored: it goes
  // into a selector, and a throw there stopped the rest of app.js.
  await view(1440);
  await load("/history?range=24h");
  await ev(`localStorage.setItem("hb.chartOff", JSON.stringify(['"]x', "max", 7]))`);
  const err0 = errors.length;
  await load("/history?range=24h");
  const bad = await ev("({ off: !!document.querySelector('figure.chart.off-max') || !document.querySelector('.lg[data-ser=max]'), live: !!window.HB_LIVE || !!document.querySelector('#live .dot.ok') })");
  check(errors.length === err0 && bad.off && bad.live, "a bad stored legend value is ignored; the known one still applies and the script runs on", { errors: errors.slice(err0), ...bad });
  await ev(`localStorage.removeItem("hb.chartOff")`);
  // Sign-out stays on this site, even on an address whose path starts with
  // "//" (a 404 inside the shell). /api/logout is stubbed: no real sign-out.
  await load("//example.invalid/x?y=1");
  const lo = await ev(`(() => { const f = window.fetch; window.fetch = (u, o) => String(u).includes("/api/logout") ? Promise.resolve(new Response("{}", { status: 200 })) : f(u, o); const b = document.querySelector('[data-act="logout"]'); if (!b) return false; b.click(); return true; })()`);
  await sleep(1500);
  await until("document.readyState === 'complete'");
  const at = await ev("({ origin: location.origin, path: location.pathname, search: location.search })");
  check(lo && at.origin === ORIGIN && at.path === "//example.invalid/x" && at.search === "?y=1", "sign-out on a //host path stays on this origin (same page)", { lo, at });
  check(errors.length === 0, "no script errors", errors);
} catch (e) {
  check(false, "the run finished", String(e.stack || e).slice(0, 600));
} finally {
  // ---- clean-up: the config file back byte for byte ----
  check(await restore(), "clean-up: the config file is back at its starting hash");
  ch.kill();
}
console.log("\nclick counts (width · task · measured · design):");
for (const [w, n, c, want, ok] of counts) console.log(`  ${w} · ${n} · ${c} · ${want}${ok ? "" : " · did not finish"}`);
console.log(fails ? `\n${fails} check(s) failed` : "\nall checks passed");
process.exit(fails ? 1 : 0);
