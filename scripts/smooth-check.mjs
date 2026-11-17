// Page-change smoothness check (ux-pass, "page changes flicker"): every
// navigation must be one clean swap from the old page to the finished new
// one. Drives a running test instance in headless Chromium.
//   HB_PASSWORD=... node scripts/smooth-check.mjs <base URL> <container>
// <container>: a running container whose page, tabs and logs may be opened.
// Walks Overview, Containers, the container page and two of its tabs, Host,
// History, Alerts, Logs, Settings (and a second Settings tab signed in),
// signed out and then signed in, at 1440 and 390 px. For each click it
// records every painted frame (DevTools screencast) and, in the new
// document, the page state at every animation frame, and asserts for every
// frame of the new page from its first paint on:
//   - icons: the sprite is in the document and sampled visible `use`
//     elements resolve (a non-empty box);
//   - content: <main> has rendered children, and its visible area has
//     non-background pixels (at least half of what the settled page has);
//   - no state flip in the shell after first paint: live indicator text,
//     sidebar width, tree scroll offset, tree filter text, open stacks,
//     current nav item, current tree item, theme, rail state, the number of
//     drawn sparklines;
//   - no skeleton appears after first paint, and none is visible outside a
//     region the server marked as waiting.
// Prints, per navigation, the time from click to first paint of the new
// page and to its last visual change (the screen equals the settled page
// from then on). CPU=4 throttles the CPU 4x; FRAMES=<dir> keeps the frames.
// Signs in once (the login is rate limited: never run it in a loop) and
// writes nothing. Exits 1 when a check fails, 2 on bad usage. Headless
// Chromium from the Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";
import fs from "node:fs";
import zlib from "node:zlib";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const CTR = process.argv[3];
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(url) || !CTR || !PW) {
  console.error("usage: HB_PASSWORD=<password> node scripts/smooth-check.mjs <base URL of a test instance> <container>");
  process.exit(2);
}
if (new URL(url).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const ORIGIN = new URL(url).origin;
const CPU = Number(process.env.CPU || 1);
const FRAMES = process.env.FRAMES || "";

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
const tgt = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(tgt.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); let frames = null;
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Page.screencastFrame") {
    if (frames) frames.push({ ts: d.params.metadata.timestamp * 1000, data: d.params.data });
    send("Page.screencastFrameAck", { sessionId: d.params.sessionId });
  }
});
setTimeout(() => { console.log("FAIL the run timed out"); ch.kill(); process.exit(1); }, 600000).unref();
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
await send("Page.enable"); await send("Runtime.enable"); await send("Network.enable");
if (CPU > 1) await send("Emulation.setCPUThrottlingRate", { rate: CPU });

// In every new document: the page state at each animation frame (what that
// frame paints), from the first one on.
await send("Page.addScriptToEvaluateOnNewDocument", { source: `
  window.__sm = [];
  (function rec() {
    requestAnimationFrame(function (t) {
      try {
        var d = document, m = d.querySelector("main"), side = d.getElementById("side"), tree = d.getElementById("tree");
        var vis = function (e) { if (!e || !e.getClientRects().length) return false; var s = getComputedStyle(e); return s.visibility !== "hidden" && s.display !== "none"; };
        var uses = Array.from(d.querySelectorAll("header.top svg.i use, .nav-item svg.i use, .bbar svg.i use")).filter(function (u) { return vis(u.ownerSVGElement); }).slice(0, 6);
        var sk = Array.from(d.querySelectorAll(".sk")).filter(vis);
        var live = d.querySelector("#live .live-t"), tf = d.getElementById("tree-filter");
        var cur = d.querySelector(".nav-item[aria-current], .bbar [aria-current]"), tcur = d.querySelector("#tree a[aria-current]");
        window.__sm.push({
          t: performance.timeOrigin + t,
          sprite: !!d.getElementById("i-grid"),
          icons: uses.length ? uses.filter(function (u) { try { return u.getBBox().width > 0; } catch (e) { return false; } }).length + "/" + uses.length : "none",
          main: !!m && m.children.length > 0 && m.getBoundingClientRect().height > 0,
          live: live ? live.textContent : null,
          sideW: side ? Math.round(side.getBoundingClientRect().width) : null,
          treeY: tree ? tree.scrollTop : null,
          open: tree ? Array.from(tree.querySelectorAll("details.stack")).map(function (x) { return x.open && !x.hidden ? 1 : 0; }).join("") : null,
          filter: tf ? tf.value : null,
          cur: cur ? cur.getAttribute("href") : null,
          tcur: tcur ? tcur.getAttribute("href") : null,
          theme: d.documentElement.dataset.theme || "", rail: d.documentElement.dataset.side || "",
          sparks: d.querySelectorAll("svg.spark .line[d]").length,
          sk: sk.length,
          skOut: sk.filter(function (s) { return !s.closest("[data-wait],[aria-busy]"); }).length,
        });
      } catch (e) { window.__sm.push({ t: performance.timeOrigin + t, err: String(e) }); }
      if (window.__sm.length < 400) rec();
    });
  })();
` });

// ---- PNG decoding (screencast frames), RGB/RGBA 8 bit ----
function png(buf) {
  let p = 8, w = 0, h = 0, ct = 0; const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p), type = buf.toString("ascii", p + 4, p + 8), data = buf.subarray(p + 8, p + 8 + len);
    if (type === "IHDR") { w = data.readUInt32BE(0); h = data.readUInt32BE(4); ct = data[9]; }
    else if (type === "IDAT") idat.push(data);
    p += 12 + len;
  }
  const bpp = ct === 6 ? 4 : 3, raw = zlib.inflateSync(Buffer.concat(idat)), stride = w * bpp, out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)), row = y * stride;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? out[row + x - bpp] : 0, b = y ? out[row - stride + x] : 0, c = x >= bpp && y ? out[row - stride + x - bpp] : 0;
      let v = src[x];
      if (f === 1) v += a; else if (f === 2) v += b; else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) { const pp = a + b - c, pa = Math.abs(pp - a), pb = Math.abs(pp - b), pc = Math.abs(pp - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      out[row + x] = v & 255;
    }
  }
  return { w, h, bpp, px: out };
}
// Share of pixels in `r` (fractions of the frame) that differ from `bg`.
function filled(img, r, bg, skip = []) {
  let n = 0, all = 0;
  const out = (x, y) => skip.some((k) => x >= k.x * img.w && x < (k.x + k.w) * img.w && y >= k.y * img.h && y < (k.y + k.h) * img.h);
  const x0 = Math.floor(r.x * img.w), x1 = Math.ceil((r.x + r.w) * img.w), y0 = Math.floor(r.y * img.h), y1 = Math.ceil((r.y + r.h) * img.h);
  for (let y = y0; y < y1; y += 2) for (let x = x0; x < x1; x += 2) {
    if (out(x, y)) continue;
    const i = (y * img.w + x) * img.bpp; all++;
    if (Math.abs(img.px[i] - bg[0]) + Math.abs(img.px[i + 1] - bg[1]) + Math.abs(img.px[i + 2] - bg[2]) > 30) n++;
  }
  return all ? n / all : 0;
}
function differ(a, b) {
  if (a.w !== b.w || a.h !== b.h) return 1;
  let n = 0, all = 0;
  for (let i = 0; i < a.px.length; i += a.bpp * 7) { all++; if (Math.abs(a.px[i] - b.px[i]) + Math.abs(a.px[i + 1] - b.px[i + 1]) + Math.abs(a.px[i + 2] - b.px[i + 2]) > 30) n++; }
  return n / all;
}

let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };
const until = async (expr, ms = 15000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };
async function view(w) {
  const phone = w < 500;
  await send("Emulation.setDeviceMetricsOverride", { width: w, height: phone ? 844 : 900, deviceScaleFactor: 1, mobile: phone });
  await send("Emulation.setTouchEmulationEnabled", { enabled: phone });
}
async function load(path) {
  await send("Page.navigate", { url: url + path });
  await until("document.readyState === 'complete'");
  await sleep(1200);
}
const rows = [];
// Click the first element matching `sel` (a real click when it is on
// screen, else the element's own click, e.g. a link in the closed drawer)
// and judge the page change it starts.
async function nav(label, sel) {
  const at = await ev(`(() => { const e = Array.from(document.querySelectorAll(${JSON.stringify(sel)})).find((x) => x.getClientRects().length) || document.querySelector(${JSON.stringify(sel)}); if (!e) return null; e.scrollIntoView({ block: "center" }); const b = e.getBoundingClientRect(), on = b.width > 0 && b.top >= 0 && b.bottom <= innerHeight && b.left >= 0 && b.right <= innerWidth && document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2)?.closest("a,button") === e.closest("a,button"); window.__old = 1; return { x: b.left + b.width / 2, y: b.top + b.height / 2, on, href: e.getAttribute("href") }; })()`);
  if (!at) { check(false, `${label}: link ${sel} exists`); return; }
  await sleep(300);
  frames = [];
  await send("Page.startScreencast", { format: "png", maxWidth: 720, maxHeight: 900, everyNthFrame: 1 });
  await sleep(150);
  const t0 = Date.now();
  if (at.on) {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: at.x, y: at.y });
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x: at.x, y: at.y, button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: at.x, y: at.y, button: "left", clickCount: 1 });
  } else {
    await ev(`(() => { const e = Array.from(document.querySelectorAll(${JSON.stringify(sel)})).find((x) => x.getClientRects().length) || document.querySelector(${JSON.stringify(sel)}); e.click(); })()`);
  }
  await until("!window.__old && document.readyState === 'complete'", 20000);
  await sleep(2000 * Math.max(1, CPU / 2));
  await send("Page.stopScreencast");
  await sleep(100);
  const got = frames; frames = null;
  const info = await ev(`(() => {
    const fp = performance.getEntriesByType("paint").find((e) => e.name === "first-paint");
    const m = document.querySelector("main"), r = m.getBoundingClientRect();
    const vx = Math.max(0, r.left), vy = Math.max(0, r.top), vw = Math.min(innerWidth, r.right) - vx, vh = Math.min(innerHeight, r.bottom) - vy;
    let e = m, bg = "rgba(0, 0, 0, 0)";
    while (e && (bg = getComputedStyle(e).backgroundColor).endsWith(", 0)")) e = e.parentElement;
    return { fp: fp ? performance.timeOrigin + fp.startTime : null, sm: window.__sm || [], path: location.pathname + location.search,
      // A log viewer's lines come over their own stream after the page (the
      // server sends none), so its line box is not judged here.
      skip: Array.from(document.querySelectorAll(".lbody")).map((e) => { const b = e.getBoundingClientRect(); return { x: b.left / innerWidth, y: b.top / innerHeight, w: b.width / innerWidth, h: b.height / innerHeight }; }),
      region: { x: vx / innerWidth, y: vy / innerHeight, w: vw / innerWidth, h: Math.min(vh, 600) / innerHeight },
      bg: (bg.match(/\\d+/g) || [0, 0, 0]).slice(0, 3).map(Number) };
  })()`);
  const fp = info.fp || (info.sm[0] && info.sm[0].t);
  const sm = info.sm.filter((s) => !s.err);
  // The state painted at first paint is the last one recorded before it.
  let first = 0;
  sm.forEach((s, i) => { if (s.t <= fp + 1) first = i; });
  const after = sm.slice(first);
  const bad = [];
  if (!after.length) bad.push("no animation frames recorded");
  const s0 = after[0] || {};
  after.forEach((s, i) => {
    if (!s.sprite || (s.icons !== "none" && !/^(\d+)\/\1$/.test(s.icons))) bad.push(`frame ${i}: icons ${s.sprite ? s.icons : "no sprite"}`);
    if (!s.main) bad.push(`frame ${i}: main empty`);
    for (const k of ["live", "sideW", "treeY", "open", "filter", "cur", "tcur", "theme", "rail", "sparks"]) if (s[k] !== s0[k]) bad.push(`frame ${i}: ${k} ${JSON.stringify(s0[k])} -> ${JSON.stringify(s[k])}`);
    if (s.sk > s0.sk) bad.push(`frame ${i}: a skeleton appeared (${s0.sk} -> ${s.sk})`);
    if (s.skOut) bad.push(`frame ${i}: ${s.skOut} skeleton(s) outside a waiting region`);
  });
  // Pixels: every frame of the new page has its content area filled.
  const imgs = got.map((f) => ({ ts: f.ts, img: png(Buffer.from(f.data, "base64")), data: f.data }));
  const fin = imgs[imgs.length - 1];
  const full = fin ? filled(fin.img, info.region, info.bg, info.skip) : 0;
  const fresh = imgs.filter((f) => f.ts >= fp - 12);
  fresh.forEach((f, i) => {
    const v = filled(f.img, info.region, info.bg, info.skip);
    if (v < Math.max(0.003, full * 0.5)) bad.push(`painted frame ${i} (+${Math.round(f.ts - t0)} ms): content ${(v * 100).toFixed(1)}% filled, settled ${(full * 100).toFixed(1)}%`);
  });
  // Last visual change: from then on every frame looks like the settled one.
  let settle = fresh.length ? fresh[0].ts : fp;
  for (let i = 0; i < imgs.length; i++) if (differ(imgs[i].img, fin.img) > 0.004) settle = imgs[i + 1] ? imgs[i + 1].ts : imgs[i].ts;
  // One line per kind of problem (its first frame).
  const seen = new Set(), uniq = bad.filter((b) => { const k = b.replace(/^(painted )?frame \d+( \(\+\d+ ms\))?: /, "").replace(/[\d.]+%.*$/, "%"); if (seen.has(k)) return false; seen.add(k); return true; });
  const row = { label, path: info.path, fp: Math.round(fp - t0), last: Math.round(Math.max(settle, fp) - t0), frames: fresh.length, issues: uniq.length };
  rows.push(row);
  check(!uniq.length, `${label} -> ${info.path}: first paint +${row.fp} ms, last change +${row.last} ms, ${fresh.length} new-page frames`, uniq.slice(0, 6));
  if (FRAMES) {
    const dir = `${FRAMES}/${rows.length.toString().padStart(2, "0")}-${label.replace(/[^\w-]+/g, "_")}`;
    fs.mkdirSync(dir, { recursive: true });
    imgs.forEach((f, i) => fs.writeFileSync(`${dir}/${String(i).padStart(2, "0")}-${Math.round(f.ts - t0)}ms${f.ts >= fp - 12 ? "-new" : ""}.png`, Buffer.from(f.data, "base64")));
  }
}

const enc = encodeURIComponent(CTR);
async function walk(tag, w, signed) {
  await view(w);
  await load("/");
  const P = w < 500;
  const navTo = (href) => P ? `.bbar a[href="${href}"], .nav-item[href="${href}"]` : `.nav-item[href="${href}"]`;
  await nav(`${tag} overview->containers`, navTo("/containers"));
  await nav(`${tag} containers->container`, `main a[href="/containers/${enc}"]`);
  await nav(`${tag} container->tab 2`, ".cd-tabs a:nth-child(2)");
  await nav(`${tag} tab 2->tab 3`, ".cd-tabs a:nth-child(3)");
  await nav(`${tag} container->host`, navTo("/host"));
  await nav(`${tag} host->history`, navTo("/history"));
  await nav(`${tag} history->alerts`, navTo("/alerts"));
  await nav(`${tag} alerts->logs`, navTo("/logs"));
  await nav(`${tag} logs->settings`, P ? `.nav-item[href^="/settings"]` : `.nav-item[href^="/settings"]`);
  if (signed) await nav(`${tag} settings->tab`, `#set-nav li:nth-child(3) a`);
  await nav(`${tag} settings->overview`, navTo("/"));
}

for (const signed of [false, true]) {
  if (signed) {
    const [cn, cv] = cookie.split("=");
    await send("Network.setCookie", { name: cn, value: cv, url, httpOnly: true, sameSite: "Strict" });
  } else await send("Network.clearBrowserCookies");
  for (const w of [1440, 390]) await walk(`${w} ${signed ? "in " : "out"}`, w, signed);
}

console.log("\nclick to first paint / to last visual change (ms)" + (CPU > 1 ? `, CPU ${CPU}x slower` : ""));
for (const r of rows) console.log(`${r.label.padEnd(34)} ${String(r.fp).padStart(5)} ${String(r.last).padStart(6)}  ${r.issues ? r.issues + " issue(s)" : ""}`);
const med = (k) => { const v = rows.map((r) => r[k]).sort((a, b) => a - b); return v[Math.floor(v.length / 2)]; };
console.log(`median: first paint ${med("fp")} ms, last change ${med("last")} ms`);
ch.kill();
console.log(fails ? `\n${fails} check(s) failed` : "\nall checks passed");
process.exit(fails ? 1 : 0);
