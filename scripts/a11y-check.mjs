// Hit targets and keyboard reachability (DESIGN.md §11.3 sizes, §11.10;
// ux-pass wave 5: A11Y-18, A11Y-01, A11Y-03): drives a running test
// instance in headless Chromium.
//   HB_PASSWORD=... node scripts/a11y-check.mjs <base URL> <container>
// <container>: a running container whose pages may be opened.
// Hit targets: at 390 px with touch emulation (pointer: coarse), on every
// page signed in (and the login page, and the open drawer), each visible
// control is hit-tested 21 px left, right, above and below its centre with
// elementFromPoint: a control whose own box, or the hit area its ::after
// adds, answers at all four points is a 44 px target; one that loses a point
// to a neighbour is not (an overlapping hit area counts against both). Inline
// links inside a sentence are exempt (WCAG 2.5.8) and left out, and so are
// chart markers (24 px by design). A stretched row link passes when some
// 44 px square of its row answers for it. Fails when the shell (top bar,
// bottom bar, drawer, palette) has a control under 44 px, or when a page's
// share of controls under 44 px is over its limit (LIMITS below: none on
// most pages since the wave 5 leftovers).
// REPORT=1 prints every control under 44 px.
// Overflow (ROB-03): every page's document is as tall as its content (no
// blank area below it) and, on the shell's own pages, as wide as the screen.
// Keyboard: at 1440 px Tab reaches the skip link, the top bar (search,
// theme, account), the sidebar (collapse, every nav item, the filter) and the
// page; Ctrl K opens the palette with focus in its box, arrows move the
// selection, Esc closes it and focus comes back; at 390 px Tab goes from the
// skip link to the top bar (the closed drawer is out of the tab order),
// "More" opens the drawer with focus inside, Tab stays inside it, Esc closes
// it and focus returns to "More".
// Signs in once (the login is rate limited: never run it in a loop) and
// writes nothing. Exits 1 when a check fails, 2 on bad usage. Headless
// Chromium from the Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const CTR = process.argv[3];
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(url) || !CTR || !PW) {
  console.error("usage: HB_PASSWORD=<password> node scripts/a11y-check.mjs <base URL of a test instance> <container>");
  process.exit(2);
}
if (new URL(url).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const ORIGIN = new URL(url).origin;
const C = encodeURIComponent(CTR);

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
let id = 0; const pending = new Map(); const errors = [];
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text);
});
setTimeout(() => { console.log("FAIL the run timed out"); ch.kill(); process.exit(1); }, 540000).unref();
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
await send("Page.enable"); await send("Runtime.enable");
const [cn, cv] = cookie.split("=");
const setCookie = () => send("Network.setCookie", { name: cn, value: cv, url, httpOnly: true, sameSite: "Strict" });
await setCookie();
const KEYS = { Escape: 27, Enter: 13, Tab: 9, ArrowDown: 40, ArrowUp: 38 };
async function key(k, mods = 0, text) {
  const base = { key: k, code: k.length === 1 ? "Key" + k.toUpperCase() : k, windowsVirtualKeyCode: KEYS[k] || k.toUpperCase().charCodeAt(0), modifiers: mods };
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  if (text) await send("Input.dispatchKeyEvent", { type: "char", ...base, text });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await sleep(40);
}
const until = async (expr, ms = 5000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };
async function view(w, touch) {
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: w < 500 ? 844 : 900, deviceScaleFactor: 1, mobile: w < 500 });
  await send("Emulation.setTouchEmulationEnabled", { enabled: !!touch, maxTouchPoints: touch ? 5 : 1 });
}
async function load(path, settle = 500) {
  await send("Page.navigate", { url: url + path });
  await until("document.readyState === 'complete'", 15000);
  await sleep(settle);
}
let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };

// ---- hit targets ----
// In the page: every visible control, scrolled into view, hit-tested at its
// centre +-21 px (44 px) and +-11 px (24 px) on both axes.
const MEASURE = `((scope) => {
  const root = scope ? document.querySelector(scope) : document;
  const vis = (el) => { const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) return false; const s = getComputedStyle(el); if (s.visibility !== "visible") return false;
    for (let e = el; e; e = e.parentElement) { if (e.hidden || e.inert || getComputedStyle(e).display === "none") return false; if (e.tagName === "DETAILS" && !e.open && e !== el && !e.querySelector(":scope>summary").contains(el)) return false; } return true; };
  const inline = (el) => el.tagName === "A" && getComputedStyle(el).display === "inline" && el.parentElement && el.parentElement.textContent.trim().length > el.textContent.trim().length + 20;
  const name = (el) => (el.getAttribute("aria-label") || el.title || el.textContent || el.placeholder || el.name || "").trim().replace(/\\s+/g, " ").slice(0, 40);
  const sel = (el) => { let s = el.tagName.toLowerCase(); const c = (el.getAttribute("class") || "").trim().split(/\\s+/).filter(Boolean).slice(0, 2).join("."); if (c) s += "." + c; const p = el.parentElement; if (p) { let ps = p.tagName.toLowerCase(); const pc = (p.getAttribute("class") || "").trim().split(/\\s+/).filter(Boolean).slice(0, 1).join("."); if (pc) ps += "." + pc; s = ps + ">" + s; } return s; };
  const mine = (el, x, y) => { if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false; const h = document.elementFromPoint(x, y); if (!h) return false; if (h === el || el.contains(h)) return true; const l = h.closest("label"); return !!(l && l.control === el); };
  const fits = (el, cx, cy, d) => mine(el, cx - d, cy) && mine(el, cx + d, cy) && mine(el, cx, cy - d) && mine(el, cx, cy + d);
  const x0 = scrollX, y0 = scrollY, out = [];
  for (const el of root.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=switch],[tabindex]:not([tabindex="-1"])')) {
    // Chart markers (.mk) are 24 px by design: a row of them sits on the
    // chart's top edge, one per event, and a 44 px target would cover its
    // neighbours and the plot; each also has its timeline row (a 44 px
    // target) and the crosshair read-out. Left out of the count.
    if (el.classList.contains("skip") || el.classList.contains("mk") || !vis(el) || inline(el)) continue;
    el.scrollIntoView({ block: "center", inline: "center" });
    // A stretched row link's target is its row (.rl).
    let r = (el.classList.contains("row-link") && el.closest(".rl") || el).getBoundingClientRect();
    if ((el.type === "checkbox" || el.type === "radio") && el.closest("label")) { const lr = el.closest("label").getBoundingClientRect(); if (lr.width * lr.height > r.width * r.height) r = lr; }
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    // A stretched row link is a target when some 44 px (24 px) square of
    // its row is its own: rows also hold their own 44 px secondary links (a
    // rule, "alert firing", logs), which take part of the row. Every point
    // of the square must resolve to the row's own link (a link that is not
    // stretched, or text laid over it, does not count).
    if (el.classList.contains("row-link") && el.closest(".rl")) { const hit = (x, y) => mine(el, x, y);
      // (The row's middle is always tried: a 45 px row with a 1 px border
      // has exactly 44 px of link, centred, which a 6 px step can miss.)
      const square = (d) => { const ys = [r.top + r.height / 2]; for (let y = r.top + d; y <= r.bottom - d; y += 6) ys.push(y); for (const y of ys) for (let x = r.left + d; x <= r.right - d; x += 8) if ([[0, 0], [-d, 0], [d, 0], [0, -d], [0, d]].every(([dx, dy]) => hit(x + dx, y + dy))) return true; return false; };
      out.push({ sel: sel(el), name: name(el), w: Math.round(r.width), h: Math.round(r.height), ok44: square(21), ok24: square(11), shell: false }); continue; }
    out.push({ sel: sel(el), name: name(el), w: Math.round(r.width), h: Math.round(r.height), ok44: fits(el, cx, cy, 21), ok24: fits(el, cx, cy, 11),
      shell: !!el.closest("header.top, #bbar, #side, #palette") });
  }
  scrollTo(x0, y0);
  return out;
})`;
// The share of controls under 44 px a page may have: none, except
// Overview's memory map (a tile's size is its container's share of memory,
// so a small one stays small) and a log viewer's four search toggles (26 px
// wide: four 44 px ones would leave the phone's one-row search box no room
// to type). Chart markers are left out above.
const LIMITS = { default: 0, "/": 0.08, [`/containers/${C}/logs`]: 0.15 };
const PAGES = [
  "/", "/containers", `/containers/${C}`, `/containers/${C}?tab=metrics`, `/containers/${C}?tab=inspect`, `/containers/${C}?tab=processes`, `/containers/${C}?tab=events`,
  `/containers/${C}/logs`, "/logs", "/host", "/history", "/alerts", "/more",
  "/settings/general", "/settings/containers", "/settings/bookmarks", "/settings/widgets", "/settings/notifications", "/settings/appearance",
  "/settings/history", "/settings/config", "/settings/security", "/login",
];
await view(390, true);
let all = 0, under = 0, under24 = 0;
const rows = [];
async function measure(label, scope) {
  const m = await ev(`${MEASURE}(${JSON.stringify(scope || "")})`);
  const bad = m.filter((x) => !x.ok44), shellBad = bad.filter((x) => x.shell);
  all += m.length; under += bad.length; under24 += m.filter((x) => !x.ok24).length;
  rows.push([label, m.length, bad.length, m.filter((x) => !x.ok24).length]);
  const share = m.length ? bad.length / m.length : 0, lim = LIMITS[label] ?? LIMITS.default;
  check(shellBad.length === 0, `${label}: every shell control is a 44 px target`, shellBad.map((x) => `${x.sel} "${x.name}" ${x.w}x${x.h}`));
  check(share <= lim, `${label}: ${bad.length} of ${m.length} controls under 44 px (${(share * 100).toFixed(0)} %, limit ${(lim * 100).toFixed(0)} %)`);
  if (!scope) {
    // ROB-03: no blank scroll area under the page and no sideways scroll at 390.
    const [sh, col, sw, cw] = await ev(`[document.documentElement.scrollHeight, Math.ceil(document.querySelector(".col").getBoundingClientRect().height), document.documentElement.scrollWidth, document.documentElement.clientWidth]`);
    check(sh <= col + 1, `${label}: the document is as tall as the page (${sh} / ${col})`);
    check(sw <= cw, `${label}: no sideways scroll at 390 (${sw} / ${cw})`);
  }
  if (process.env.REPORT) {
    const by = new Map();
    for (const x of bad) { const k = `${x.sel} ${x.w}x${x.h}${x.ok24 ? "" : " <24"}`; by.set(k, (by.get(k) || 0) + 1); }
    for (const [k, n] of [...by].sort((a, b) => b[1] - a[1])) console.log(`       ${n} x ${k}`);
  }
}
for (const p of PAGES) {
  if (p === "/login") { await send("Network.clearBrowserCookies"); await load("/login"); await measure("/login (signed out)"); await setCookie(); continue; }
  await load(p);
  await measure(p);
}
// The open drawer and the palette (shell only).
await load("/");
await ev(`document.querySelector('[data-act="more"]').click()`); await sleep(400);
await measure("drawer", "#side");
await ev(`document.querySelector('[data-act="side-close"]').click()`); await sleep(300);
await ev(`document.getElementById("cmd") ? document.getElementById("cmd").click() : null`); await sleep(500);
await measure("palette", "#palette");
console.log(`\n${"page".padEnd(40)} controls  <44  <24`);
for (const [l, n, b, b24] of rows) console.log(`${l.padEnd(40)} ${String(n).padStart(8)} ${String(b).padStart(4)} ${String(b24).padStart(4)}`);
console.log(`${"all".padEnd(40)} ${String(all).padStart(8)} ${String(under).padStart(4)} ${String(under24).padStart(4)}\n`);

// ---- keyboard: desktop ----
await view(1440, false);
await load("/");
await ev("localStorage.removeItem('rh-side'); delete document.documentElement.dataset.side");
await load("/");
const focused = `(() => { const a = document.activeElement; return a ? (a.id || a.getAttribute("data-act") || a.getAttribute("aria-label") || a.className || a.tagName) + "|" + (a.closest("header.top") ? "top" : a.closest("#side") ? "side" : a.closest("main") ? "main" : "other") : ""; })()`;
const seen = [];
for (let i = 0; i < 40; i++) { await key("Tab"); seen.push(await ev(focused)); }
const has = (re) => seen.some((s) => re.test(s));
check(/skip/i.test(seen[0]) || /Skip/.test(await ev("document.activeElement.textContent")), "Tab: the skip link comes first", seen[0]);
check(has(/^rail-toggle\|side/), "Tab reaches the sidebar's collapse button", seen);
check(await ev(`[...document.querySelectorAll("#side .nav-item:not(.rail-only)")].length`) === 7 && has(/^tree-filter\|side/), "Tab reaches the sidebar filter", seen);
const navSeen = seen.filter((s) => /\|side$/.test(s)).length;
check(navSeen >= 9, `Tab passes the nav items (${navSeen} sidebar stops in 40 presses)`);
// From the skip link, the top bar and the page.
await load("/");
await key("Tab"); await key("Enter"); await sleep(100);
check(await ev("document.activeElement.id") === "main", "the skip link moves focus to the page");
const topSeen = [];
await ev("document.getElementById('cmd').focus()"); topSeen.push(await ev(focused));
for (let i = 0; i < 8; i++) { await key("Tab"); topSeen.push(await ev(focused)); }
check(topSeen[0] === "cmd|top" && topSeen.some((s) => /^theme\|top/.test(s)) && topSeen.some((s) => /^Account|^Sign in|acct/.test(s)), "Tab walks the top bar: search, vitals, theme, account", topSeen);
// Palette: Ctrl K, arrows, Esc returns focus.
await ev("document.getElementById('theme').focus()");
await key("k", 2);
check(await until("!document.getElementById('palette').hidden && document.activeElement.id === 'pal-q'", 2000), "Ctrl K opens the palette with focus in its box");
await until("document.querySelectorAll('#pal-list li[role=option]').length > 3", 3000);
const a0 = await ev("document.getElementById('pal-q').getAttribute('aria-activedescendant')");
await key("ArrowDown");
const a1 = await ev("document.getElementById('pal-q').getAttribute('aria-activedescendant')");
check(a0 === "pal-0" && a1 === "pal-1", "ArrowDown moves the palette selection", [a0, a1]);
await key("Tab");
check(await ev("document.activeElement.id") === "pal-q", "Tab stays in the palette");
await key("Escape");
check(await until("document.getElementById('palette').hidden && document.activeElement.id === 'theme'", 2000), "Esc closes the palette and focus returns to where it was");
// The shortcuts sheet: ? opens, Esc closes.
await ev("document.activeElement.blur()");
await key("?", 8, "?");
check(await until("document.getElementById('keys').open", 1000), "? opens the shortcuts sheet");
await key("Escape");
check(await until("!document.getElementById('keys').open", 1000), "Esc closes it");

// ---- shortcuts (NAV-56, NAV-60 to NAV-64) ----
// "/" on Overview opens the palette (the sidebar filter is not "this page's");
// on Containers it focuses the table filter, and the sheet says so.
await load("/");
await key("/", 0, "/");
check(await until("!document.getElementById('palette').hidden", 1000), "/ on Overview opens the palette, not the sidebar filter");
await key("Escape");
await load("/containers");
await key("/", 0, "/");
check(await ev("document.activeElement.id") === "cq", "/ on Containers focuses the table filter");
await ev("document.activeElement.blur()");
await key("?", 8, "?");
check(/filter/i.test(await ev("document.querySelector('#keys [data-k=slash]').textContent")), "the sheet says / is this page's filter");
await key("Escape");
// j / k only over rows that open something; Enter follows the row's link.
await load("/settings/config");
await key("j");
check(await ev("!document.querySelector('.kfocus')"), "j selects nothing on Config file (its rows open nothing)");
await load("/host");
await key("j");
check(await ev("(() => { const r = document.querySelector('.kfocus'); return !!r && !!r.querySelector('a[href]') && r.contains(document.activeElement); })()"), "j on Host selects a row with a link and focuses it");
// A ticked checkbox does not switch the keys off; g y is History.
await load("/settings/general");
await ev("(() => { const c = document.querySelector('main input[type=checkbox]'); c && c.focus(); })()");
await key("g"); await key("y");
check(await until("location.pathname === '/history'", 4000), "g y from a focused checkbox opens History");

// custom.css: a syntax warning while typing (SET-44); nothing is saved.
await load("/settings/appearance");
await ev(`(() => { const t = document.getElementById("css-text"); t.value = "a{color:red}\\n}}} not css"; t.dispatchEvent(new Event("input", { bubbles: true })); })()`);
check(await until(`/line 2: a \\} without/.test((document.getElementById("css-syntax") || {}).textContent || "")`, 2000), "custom.css: a stray } is named with its line before saving", await ev(`(document.getElementById("css-syntax") || {}).textContent`));

// Bookmarks: a name another row has is said under the field (SET-43); the
// dialog is closed without saving.
await load("/settings/bookmarks");
const bmName = await ev(`(() => { const tr = document.querySelector("#bm-list tr[data-key]"); return tr ? JSON.parse(tr.querySelector("[data-item]").dataset.item).name : null; })()`);
if (bmName) {
  await ev(`document.getElementById("bm-add").click()`); await sleep(400);
  await ev(`(() => { const i = document.querySelector("#dlg-bookmark input[name=name]"); i.value = ${JSON.stringify(bmName)}; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
  check(await until(`/already called/.test(document.querySelector("#dlg-bookmark input[name=name]").nextElementSibling.textContent)`, 2000), "a bookmark name that exists already is pointed out while typing");
  await ev("HB.close(true)");
} else console.log("note no bookmark to compare with");

// ---- keyboard: phone drawer ----
await view(390, true);
await load("/");
await key("Tab"); await key("Tab");
const first = await ev(focused);
check(/\|top$/.test(first), "390: Tab goes from the skip link to the top bar (closed drawer out of the tab order)", first);
await ev(`document.querySelector('[data-act="more"]').focus()`);
await key("Enter"); await sleep(400);
check(await ev("document.documentElement.classList.contains('side-open') && !!document.activeElement.closest('#side')"), "390: More opens the drawer with focus inside");
let out = false;
for (let i = 0; i < 30; i++) { await key("Tab"); if (!(await ev("!!document.activeElement.closest('#side')"))) { out = await ev(focused); break; } }
check(out === false, "390: Tab stays inside the open drawer", out);
await key("Escape"); await sleep(300);
check(await ev(`!document.documentElement.classList.contains('side-open') && document.activeElement.matches('[data-act="more"]')`), "390: Esc closes the drawer and focus returns to More");

// ---- A11Y-20: a larger browser font (175 %: the root at 24.5 px instead
// of 14) grows the boxes that hold text: nothing clipped in the top bar,
// the nav items or the Settings tab column, which never overlaps the page.
await view(1440, false);
for (const path of ["/", "/settings/general"]) {
  await load(path);
  await ev("(() => { const s = document.createElement('style'); s.id = 'big-root'; s.textContent = 'html{font-size:153.125%!important}'; document.head.append(s); })()");
  await sleep(300);
  const b = await ev(`(() => {
    const clip = (e) => e.getClientRects().length && (e.scrollHeight > e.clientHeight + 1);
    const top = document.querySelector("header.top"), th = top.getBoundingClientRect();
    const out = { nav: [...document.querySelectorAll(".nav-item, .set-nav a")].filter(clip).map((e) => e.textContent.trim()),
      top: [...top.children].filter((c) => c.getClientRects().length).filter((c) => { const r = c.getBoundingClientRect(); return r.top < th.top - 1 || r.bottom > th.bottom + 1; }).map((c) => c.className || c.tagName),
      skip: document.querySelector(".skip").getBoundingClientRect().bottom };
    const sn = document.querySelector(".set-nav"), sm = document.querySelector(".set-main");
    if (sn && sm) out.overlap = Math.round(sn.getBoundingClientRect().right - sm.getBoundingClientRect().left);
    return out; })()`);
  check(!b.nav.length && !b.top.length && b.skip <= 0 && !(b.overlap > 0), `${path} at a 175 % font size: nav items, top bar and Settings tabs hold their text, nothing overlaps (A11Y-20)`, b);
}

check(errors.length === 0, "no script errors", errors);
ch.kill();
console.log(fails ? `\n${fails} check(s) failed` : "\nall checks passed");
process.exit(fails ? 1 : 0);
