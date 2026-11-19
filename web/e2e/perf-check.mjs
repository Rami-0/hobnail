// Performance check of the React slice against the budgets of
// docs/plans/react-port.md 4.9, with the old server-rendered page as the
// baseline. Raw DevTools protocol in headless Chromium (no npm packages).
// For every target x profile, in a fresh tab with the cache disabled:
//   - load the page cold; time until the ready selector (the table with its
//     rows, or both charts) is in the DOM; FCP, LCP, CLS, DOM size;
//   - every main-thread task of the load (Tracing: RunTask on the page's
//     renderer main thread, from navigationStart to 2 s after ready), with
//     PerformanceObserver "longtask" (buffered) and Performance.getMetrics as
//     cross-checks;
//   - wait for UPDATES (3) live updates (SSE `stats` events; `frag` bursts on
//     the old pages) and record the tasks in the second after each one;
//   - a scripted interaction (type in the filter, click a sortable header
//     twice; on chart pages: sweep the pointer over the CPU chart, click it)
//     with PerformanceObserver "event" (durationThreshold 16): INP-style
//     worst interaction, plus the longest task in the interaction window.
// Targets (TARGETS, default "app chart old old-ctr"):
//   app     <base>/app/containers          ready: table[data-testid=containers] tr[data-testid=row]
//   chart   <base>/app/containers/<name>   ready: svg in figure[data-testid=chart-cpu] and chart-mem
//   old     <base>/containers              ready: #ctable tr.row (baseline)
//   old-ctr <base>/containers/<name>       ready: figure.chart (baseline)
// Profiles (PROFILES, default "phone desktop"): phone = 390x844, touch,
// Emulation.setCPUThrottlingRate 4; desktop = 1440x900, unthrottled.
//   node web/e2e/perf-check.mjs <base URL> [container name, default svc002]
//   scripts/e2e.sh web/e2e/perf-check.mjs <base URL> [name]     (no host Node)
//   E2E_ENV="TARGETS PROFILES UPDATES MIN_ROWS" TARGETS=old scripts/e2e.sh ...
// Env: TARGETS, PROFILES, UPDATES (3), MIN_ROWS (rows the ready selector
// waits for, default 1), READY_MS (30000), LIVE_MS (wait for the updates,
// 45000), TRACE (1; 0 = no Tracing, long tasks from the observer only),
// OUT (perf.json and perf.md are written there), CH.
// Budgets (exit 1, printing which): a task > 200 ms in the load of
// /app/containers on the phone profile (4x); a task > 50 ms in the second
// after a live update on app or chart (any profile); app not ready or fewer
// than UPDATES updates on app. The old pages are measured, never failed.
// Authenticates and writes throwaway board and config fixtures for live updates.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { launch, input, sleep, q } from "./cdp.mjs";

const base = (process.argv[2] || "").replace(/\/+$/, "");
const NAME = process.argv[3] || "svc002";
if (!/^https?:\/\//.test(base)) { console.error("usage: node web/e2e/perf-check.mjs <base URL of a test instance> [container name]"); process.exit(2); }
if (new URL(base).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const OUT = process.env.OUT || fs.mkdtempSync(path.join(os.tmpdir(), "perf-check-"));
fs.mkdirSync(OUT, { recursive: true });
const TARGETS = (process.env.TARGETS || "app chart overview history host board logs settings").split(/[\s,]+/).filter(Boolean);
const PROFILES = (process.env.PROFILES || "phone desktop").split(/[\s,]+/).filter(Boolean);
const UPDATES = Number(process.env.UPDATES || 3);
const MIN_ROWS = Number(process.env.MIN_ROWS || 150);
const READY_MS = Number(process.env.READY_MS || 30000);
const LIVE_MS = Number(process.env.LIVE_MS || 45000);
const TRACE = process.env.TRACE !== "0";
const LOAD_BUDGET = 200, LIVE_BUDGET = 50, LIVE_WINDOW = 1000, SETTLE = 2000;
const enc = encodeURIComponent(NAME);
let sessionCookie = process.env.HB_COOKIE;
let csrf;
let boardId;
let fixtureHeaders;
if (!/^339[5-9]$/.test(new URL(base).port)) throw new Error("performance fixtures require a throwaway instance");

if (!sessionCookie && process.env.HB_PASSWORD) {
  const origin = new URL(base).origin;
  const session = await fetch(`${base}/api/v1/session`).then((r) => r.json());
  const response = await fetch(base + (session.owner_exists ? "/api/login" : "/api/v1/onboarding/owner"), {
    method: "POST", headers: { Origin: origin, "Content-Type": "application/json" },
    body: JSON.stringify({ username: "admin", password: process.env.HB_PASSWORD }),
  });
  if (!response.ok) throw new Error(`Performance sign-in failed: ${response.status}`);
  csrf = (await response.json()).csrf;
  sessionCookie = response.headers.get("set-cookie")?.split(";")[0];
}

if (TARGETS.some((target) => ["board", "history", "settings"].includes(target))) {
  if (!sessionCookie) throw new Error("Performance fixtures require authentication");
  fixtureHeaders = { Cookie: sessionCookie, Origin: new URL(base).origin, "Content-Type": "application/json", "X-RH-CSRF": csrf ?? (await fetch(`${base}/api/v1/session`, { headers: { Cookie: sessionCookie } }).then(r => r.json())).csrf };
}

if (TARGETS.includes("board")) {
  const headers = fixtureHeaders;
  const created = await fetch(`${base}/api/v1/boards`, { method: "POST", headers, body: JSON.stringify({ name: "Performance 40 items" }) }).then(r => r.json());
  boardId = created.id;
  const document = (await fetch(`${base}/api/v1/boards/${boardId}`, { headers }).then(r => r.json())).document;
  document.sections = [{ title: "Services", position: 0, items: Array.from({ length: 40 }, (_, position) => ({
    position, spans: { mobile: 1, tablet: 3, desktop: 3, rows: 1 }, payload: { kind: "link", title: `Service ${position + 1}`, url: `https://example.com/${position}` },
  })) }];
  const saved = await fetch(`${base}/api/v1/boards/${boardId}`, { method: "PUT", headers, body: JSON.stringify(document) });
  if (!saved.ok) throw new Error(`Board fixture: ${saved.status} ${await saved.text()}`);
}
const T = {
  overview: { path: "/app/overview", ready: `!!document.querySelector('[data-testid="overview"] .overview-counts strong') && !document.querySelector('[data-testid="overview"] [aria-busy="true"]')`, rows: '.overview-counts', live: ["stats"], interaction: '.overview-strips summary', fails: true },
  history: { path: "/app/history", ready: `!!document.querySelector('[data-testid="history"] .history-controls button') && !document.querySelector('.ch-wait')`, rows: 'figure.chart', live: ["config"], interaction: '.history-controls button', fails: true },
  host: { path: "/app/host", ready: `!!document.querySelector('[data-testid="host-sample"]') && !document.querySelector('.ch-wait')`, rows: '[data-testid="host-disclosure"]', live: ["host"], interaction: '[data-testid="host-disclosure"] summary', fails: true },
  board: { path: `/app/boards/${boardId}`, ready: `document.querySelectorAll('[data-testid="board-tile"]').length === 40`, rows: '[data-testid="board-tile"]', live: ["boards"], interaction: '[data-testid="board-edit"]', fails: true },
  logs: { path: `/app/containers/${enc}/logs`, ready: `document.querySelector('[data-testid="log-body"]')?.dataset.searchLines >= 100`, rows: '[data-log-id]', live: ["l"], filter: '[data-testid="filter"]', fails: true },
  settings: { path: "/app/settings/general", ready: `!!document.querySelector('input[name="greeting"]')`, rows: '.field', live: ["config"], filter: 'input[name="greeting"]', fails: true },
  app: { path: "/app/containers", ready: `document.querySelectorAll('table[data-testid="containers"] tr[data-testid="row"]').length >= ${MIN_ROWS}`,
    rows: 'table[data-testid="containers"] tr[data-testid="row"]', live: ["stats"], filter: 'input[data-testid="filter"]', header: 'th[data-sort="name"]', fails: true },
  chart: { path: `/app/containers/${enc}`, ready: `!!document.querySelector('figure[data-testid="chart-cpu"] svg') && !!document.querySelector('figure[data-testid="chart-mem"] svg')`,
    rows: 'figure[data-testid^="chart-"]', live: ["stats"], chart: 'figure[data-testid="chart-cpu"]', fails: true },
  old: { path: "/containers", ready: `document.querySelectorAll('#ctable tr.row').length >= ${MIN_ROWS}`, rows: "#ctable tr.row", live: ["frag"], filter: "#cq", header: 'th[data-sort="name"] a' },
  "old-ctr": { path: `/containers/${enc}`, ready: `!!document.querySelector('figure.chart')`, rows: "figure.chart", live: ["frag"], chart: "figure.chart" },
};
const P = {
  phone: { w: 390, h: 844, mobile: true, rate: 4 },
  desktop: { w: 1440, h: 900, mobile: false, rate: 1 },
};
for (const t of TARGETS) if (!T[t]) { console.error(`unknown target ${t} (${Object.keys(T).join(", ")})`); process.exit(2); }
for (const p of PROFILES) if (!P[p]) { console.error(`unknown profile ${p} (${Object.keys(P).join(", ")})`); process.exit(2); }

// Installed before any page script: observers, the ready mark, and a mark
// in front of every live message (the first listener of every EventSource).
const probe = (t) => `(() => {
  const H = window.__hb = { long: [], lcp: [], cls: 0, shifts: 0, events: [], live: [], readyAt: null, rows: 0, fcp: null };
  const obs = (type, fn, extra) => { try { new PerformanceObserver((l) => l.getEntries().forEach(fn)).observe({ type, buffered: true, ...extra }); } catch (e) { H["no_" + type] = String(e); } };
  obs("longtask", (e) => H.long.push({ start: e.startTime, dur: e.duration }));
  obs("largest-contentful-paint", (e) => H.lcp.push({ at: e.startTime, size: e.size, el: e.element ? e.element.tagName.toLowerCase() + (e.element.className && typeof e.element.className === "string" ? "." + e.element.className.split(" ")[0] : "") : null }));
  obs("layout-shift", (e) => { if (!e.hadRecentInput) { H.cls += e.value; H.shifts++; } });
  obs("paint", (e) => { if (e.name === "first-contentful-paint") H.fcp = e.startTime; });
  obs("event", (e) => H.events.push({ name: e.name, id: e.interactionId || 0, start: e.startTime, dur: e.duration, delay: e.processingStart - e.startTime, proc: e.processingEnd - e.processingStart }), { durationThreshold: 16 });
  const NAMES = ["hello", "stats", "containers", "system", "host", "boards", "l", "config", "resync", "frag", "reload"];
  const ES = window.EventSource;
  if (ES) window.EventSource = class extends ES {
    constructor(...a) { super(...a); for (const n of NAMES) this.addEventListener(n, () => { performance.mark("hb-live:" + n); H.live.push({ n, at: performance.now() }); }); }
  };
  const ready = () => { try { return ${t.ready}; } catch { return false; } };
  const seen = new WeakSet();
  const mo = new MutationObserver((records) => {
    if (H.readyAt === null && ready()) {
      H.readyAt = performance.now(); H.rows = document.querySelectorAll(${q(t.rows)}).length; performance.mark("hb-ready");
    }
    for (const record of records) for (const node of record.addedNodes) {
      if (!(node instanceof Element)) continue;
      const plot = node.matches(".recharts-wrapper") ? node : node.querySelector(".recharts-wrapper");
      const figure = plot?.closest("figure[data-testid]");
      if (figure && !seen.has(figure)) { seen.add(figure); performance.mark("hb-plot:" + figure.dataset.testid); }
    }
  });
  mo.observe(document, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-search-lines", "aria-busy"] });
})();`;

async function readTrace(tab) {
  const done = tab.once("Tracing.tracingComplete", 120000);
  await tab.send("Tracing.end");
  const c = await done;
  if (!c?.stream) return null;
  let s = "";
  for (;;) {
    const r = await tab.send("IO.read", { handle: c.stream, size: 4 << 20 }, 60000);
    if (r.error) break;
    s += r.result.base64Encoded ? Buffer.from(r.result.data, "base64").toString() : r.result.data;
    if (r.result.eof) break;
  }
  await tab.send("IO.close", { handle: c.stream });
  try { const j = JSON.parse(s); return Array.isArray(j) ? j : j.traceEvents; } catch { return null; }
}

const r1 = (n) => (n == null ? null : Math.round(n * 10) / 10);
async function run(target, profile) {
  const t = T[target], p = P[profile];
  const label = `${target} ${profile}`;
  const res = { target, profile, url: base + t.path, ok: true, notes: [] };
  const tab = await browser.tab();
  const inp = input(tab);
  inp.touch = p.mobile;
  try {
    await tab.send("Page.enable"); await tab.send("Runtime.enable"); await tab.send("Network.enable");
    await tab.send("Performance.enable", { timeDomain: "timeTicks" });
    await tab.send("Network.setCacheDisabled", { cacheDisabled: true });
    await tab.send("Network.clearBrowserCache"); await tab.send("Network.clearBrowserCookies");
    if (sessionCookie) {
      const cookie = sessionCookie, split = cookie.indexOf("=");
      await tab.send("Network.setCookie", { name: cookie.slice(0, split), value: cookie.slice(split + 1), url: new URL(base).origin, path: "/", httpOnly: true, sameSite: "Strict" });
    }
    await tab.send("Emulation.setDeviceMetricsOverride", { width: p.w, height: p.h, deviceScaleFactor: 1, mobile: p.mobile });
    await tab.send("Emulation.setTouchEmulationEnabled", { enabled: p.mobile, maxTouchPoints: p.mobile ? 5 : 0 });
    await tab.send("Emulation.setCPUThrottlingRate", { rate: p.rate });
    await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: probe(t) });
    if (TRACE) {
      const tr = await tab.send("Tracing.start", { transferMode: "ReturnAsStream", traceConfig: { includedCategories: ["disabled-by-default-devtools.timeline", "blink.user_timing", "toplevel", "loading", "__metadata"] } });
      if (tr.error) { res.notes.push("tracing unavailable: " + tr.error.message); }
    }
    const t0 = Date.now();
    await tab.send("Page.navigate", { url: res.url });
    const ready = await tab.until("window.__hb && window.__hb.readyAt !== null", READY_MS, 100);
    res.wall_ready_ms = Date.now() - t0;
    if (!ready) {
      res.ok = false; res.notes.push(`not ready within ${READY_MS} ms (${t.ready})`);
      await tab.shot(path.join(OUT, `${target}-${profile}-not-ready.png`));
    }
    await sleep(SETTLE);
    await tab.ev(`performance.mark("hb-load-end"); window.__hb && (window.__hb.loadEndAt = performance.now())`).catch(() => {});
    const m0 = await tab.send("Performance.getMetrics");
    await tab.shot(path.join(OUT, `${target}-${profile}-loaded.png`));

    // Board and config changes are real writes on the throwaway instance.
    let fixtureUpdates;
    if (ready && ["board", "history", "settings"].includes(target)) {
      fixtureUpdates = (async () => {
        const update = async (route, method, body) => {
          const res = await fetch(base + route, { method, headers: fixtureHeaders, body: JSON.stringify(body) });
          if (!res.ok) throw new Error(`Live fixture ${route}: HTTP ${res.status}`);
          return res.json();
        };
        if (target === "board") {
          const route = `/api/v1/boards/${boardId}`;
          const { board } = await update(route, "GET");
          let version = board.version;
          for (let n = 0; n < UPDATES; n++) {
            await sleep(1600);
            const updated = await update(route, "PATCH", { version, name: `Performance 40 items ${profile} ${n}` });
            version = updated.version;
          }
        } else {
          let config = await update("/api/settings/general", "GET");
          const original = config.page?.greeting ?? null;
          for (let n = 0; n < UPDATES; n++) {
            await sleep(1600);
            config = await update("/api/settings/general", "POST", { base_hash: config.hash, values: { greeting: n === UPDATES - 1 ? original : `Performance update ${n}` } });
          }
        }
      })().catch((error) => { res.notes.push(error.message); });
    }
    // live updates
    let live = [];
    if (ready && t.live.length) {
      const want = `(() => { const l = (window.__hb?.live || []).filter((x) => ${q(t.live)}.includes(x.n) && x.at > (window.__hb.loadEndAt || 0)); if (!l.length) return 0; let n = 1; for (let i = 1; i < l.length; i++) if (l[i].at - l[i - 1].at > 500) n++; return n; })()`;
      const got = await tab.until(`${want} >= ${UPDATES}`, LIVE_MS, 250);
      await sleep(LIVE_WINDOW + 200);
      if (!got) res.notes.push(`fewer than ${UPDATES} live updates (${t.live.join("/")}) within ${LIVE_MS} ms: ${await tab.ev(want).catch(() => "?")}`);
    }

    // interaction
    await fixtureUpdates;
    await tab.ev(`performance.mark("hb-int-start")`).catch(() => {});
    const intStart = await tab.ev("performance.now()").catch(() => null);
    if (ready && t.filter) {
      const fb = await inp.box(t.filter);
      if (fb) { await inp.clickAt(fb.x, fb.y); await inp.type("svc1", 150); await sleep(800); }
      else res.notes.push(`no visible filter ${t.filter}`);
      const hb = t.header ? await inp.box(t.header, false) : null;
      if (hb) { await inp.clickAt(hb.x, hb.y); await sleep(800); if (await inp.box(t.header, false)) { const b2 = await inp.box(t.header, false); await inp.clickAt(b2.x, b2.y); await sleep(800); } }
      else if (t.header && profile !== "phone") res.notes.push(`no visible sortable header ${t.header}`);
    } else if (ready && t.chart) {
      const cb = await inp.box(t.chart);
      if (cb) {
        for (let i = 0; i <= 20; i++) { await inp.move(cb.left + (cb.width * (i + 0.5)) / 21, cb.top + cb.height / 2); await sleep(30); }
        await sleep(300);
        await inp.clickAt(cb.x, cb.y); await sleep(800);
      } else res.notes.push(`no visible chart ${t.chart}`);
    } else if (ready && t.interaction) {
      await inp.click(t.interaction); await sleep(800);
    }
    await tab.ev(`performance.mark("hb-int-end")`).catch(() => {});
    await tab.shot(path.join(OUT, `${target}-${profile}-after-interaction.png`));

    const H = await tab.ev("window.__hb || null").catch(() => null);
    const dom = await tab.ev("({ elements: document.getElementsByTagName('*').length, rows: document.querySelectorAll(" + q(t.rows) + ").length, path: location.pathname + location.search })").catch(() => ({}));
    const m1 = await tab.send("Performance.getMetrics");
    const metric = (m, k) => m.result?.metrics?.find((x) => x.name === k)?.value;
    const trace = TRACE ? await readTrace(tab) : null;
    if (trace && process.env.SAVE_TRACE) fs.writeFileSync(path.join(OUT, `${target}-${profile}.trace.json`), JSON.stringify(trace));

    res.ready_ms = r1(H?.readyAt);
    res.rows_at_ready = H?.rows ?? null;
    res.rows_end = dom.rows ?? null;
    res.end_url = dom.path;
    res.fcp_ms = r1(H?.fcp);
    res.requests = await tab.ev(`performance.getEntriesByType("resource").filter(e=>e.name.includes("/api/")).map(e=>({path:new URL(e.name).pathname + new URL(e.name).search,start:Math.round(e.startTime),end:Math.round(e.responseEnd),initiator:e.initiatorType}))`).catch(() => []);
    const lcp = (H?.lcp || []).filter((e) => intStart == null || e.at < intStart).pop();
    res.lcp_ms = r1(lcp?.at); res.lcp_el = lcp?.el ?? null;
    res.cls = H && !H["no_layout-shift"] ? Math.round(H.cls * 10000) / 10000 : null;
    res.dom_elements = dom.elements ?? null;
    res.nodes_metric = metric(m1, "Nodes") ?? null;
    res.metrics_load = { TaskDuration: r1((metric(m0, "TaskDuration") || 0) * 1000), ScriptDuration: r1((metric(m0, "ScriptDuration") || 0) * 1000), LayoutDuration: r1((metric(m0, "LayoutDuration") || 0) * 1000), RecalcStyleDuration: r1((metric(m0, "RecalcStyleDuration") || 0) * 1000), JSHeapUsedSize: metric(m0, "JSHeapUsedSize") };
    res.metrics_end = { TaskDuration: r1((metric(m1, "TaskDuration") || 0) * 1000), JSHeapUsedSize: metric(m1, "JSHeapUsedSize") };

    // the observer's long tasks (>= 50 ms only), by phase (page clock)
    const liveAll = (H?.live || []).filter((x) => t.live.includes(x.n));
    const clusters = [];
    for (const x of liveAll) { if (!clusters.length || x.at - clusters[clusters.length - 1].last > 500) clusters.push({ at: x.at, last: x.at, n: 1 }); else { clusters[clusters.length - 1].last = x.at; clusters[clusters.length - 1].n++; } }
    live = clusters.filter((c) => c.at > (H?.loadEndAt ?? 0) && (intStart == null || c.at < intStart));
    const loadEndPage = (H?.readyAt ?? 0) + SETTLE;
    const long = H?.long || [];
    res.longtask_observer = {
      load: long.filter((l) => l.start < loadEndPage).map((l) => r1(l.dur)),
      live: live.map((c) => long.filter((l) => l.start + l.dur >= c.at && l.start <= Math.min(c.last + LIVE_WINDOW, intStart ?? Infinity)).map((l) => r1(l.dur))),
      interaction: long.filter((l) => intStart != null && l.start >= intStart).map((l) => r1(l.dur)),
    };

    // Tracing: every task on the page's main thread
    if (trace) {
      const marks = trace.filter((e) => e.cat && e.cat.includes("blink.user_timing") && typeof e.name === "string" && e.name.startsWith("hb-"));
      const anchor = marks.find((e) => e.name === "hb-load-end") || marks[0];
      if (!anchor) res.notes.push("trace: no hb-* marks found");
      else {
        const { pid, tid } = anchor;
        const tasks = trace.filter((e) => e.ph === "X" && e.pid === pid && e.tid === tid && (e.name === "RunTask" || e.name === "ThreadControllerImpl::RunTask") && e.dur > 0);
        // RunTask nests inside ThreadControllerImpl::RunTask: keep the outermost.
        tasks.sort((a, b) => a.ts - b.ts || b.dur - a.dur);
        const top = []; let endAt = -1;
        for (const e of tasks) if (e.ts >= endAt) { top.push(e); endAt = e.ts + e.dur; }
        const nav = trace.filter((e) => e.name === "navigationStart" && e.pid === pid).map((e) => e.ts);
        const mark = (n) => marks.filter((e) => e.name === n).map((e) => e.ts);
        const loadEnd = mark("hb-load-end")[0];
        // The navigation of this document: the last navigationStart before
        // the ready mark (later ones are subframes or same-document ones).
        const readyTs = mark("hb-ready")[0];
        const navStart = (readyTs === undefined ? nav[0] : nav.filter((ts) => ts <= readyTs).pop()) ?? Math.min(...top.map((e) => e.ts));
        const intS = mark("hb-int-start")[0] ?? Infinity, intE = mark("hb-int-end")[0] ?? Infinity;
        const within = (a, b) => top.filter((e) => e.ts + e.dur > a && e.ts < b);
        const ms = (us) => r1(us / 1000);
        const loadTasks = within(navStart, loadEnd);
        res.trace = {
          tasks: top.length,
          load: { max_ms: ms(Math.max(0, ...loadTasks.map((e) => e.dur))), over50: loadTasks.filter((e) => e.dur > 50000).length, over200: loadTasks.filter((e) => e.dur > 200000).length, busy_ms: ms(loadTasks.reduce((s, e) => s + e.dur, 0)), top: loadTasks.map((e) => e.dur).sort((a, b) => b - a).slice(0, 5).map(ms) },
          live: [],
          interaction: (() => { const w = within(intS, intE); return { max_ms: ms(Math.max(0, ...w.map((e) => e.dur))), over50: w.filter((e) => e.dur > 50000).length }; })(),
        };
        res.trace.plot_mounts = marks.filter((e) => e.name.startsWith("hb-plot:")).map((mark) => ({
          plot: mark.name.slice("hb-plot:".length), at_ms: ms(mark.ts - navStart),
          task_ms: ms(Math.max(0, ...within(mark.ts, mark.ts + 1).map((task) => task.dur))),
        }));
        // live updates: cluster the trace marks the same way
        const lm = marks.filter((e) => t.live.some((n) => e.name === "hb-live:" + n) && e.ts < intS).map((e) => e.ts).sort((a, b) => a - b);
        const lc = [];
        for (const ts of lm) { if (!lc.length || ts - lc[lc.length - 1].last > 500000) lc.push({ at: ts, last: ts }); else lc[lc.length - 1].last = ts; }
        // Each update's window: its second, cut at the next update and at the
        // interaction, so one task is never counted for two updates.
        lc.forEach((c, i) => {
          if (c.at < loadEnd) return; // updates during the load count as load
          const w = within(c.at - 1, Math.min(c.last + LIVE_WINDOW * 1000, lc[i + 1]?.at ?? Infinity, intS));
          res.trace.live.push({ max_ms: ms(Math.max(0, ...w.map((e) => e.dur))), busy_ms: ms(w.reduce((s, e) => s + e.dur, 0)), tasks: w.length });
        });
      }
    }
    res.live_updates = res.trace ? res.trace.live.length : live.length;
    // INP-style: the worst interaction (max duration over its events)
    const ints = new Map();
    for (const e of (H?.events || []).filter((e) => e.id > 0)) { const cur = ints.get(e.id); if (!cur || e.dur > cur.dur) ints.set(e.id, e); }
    const worst = [...ints.values()].sort((a, b) => b.dur - a.dur);
    res.inp_ms = worst.length ? r1(worst[0].dur) : null;
    res.interactions = worst.slice(0, 6).map((e) => ({ name: e.name, dur: r1(e.dur), delay: r1(e.delay), proc: r1(e.proc) }));
    if (H?.no_longtask) res.notes.push("no longtask observer: " + H.no_longtask);
  } catch (e) {
    res.ok = false; res.notes.push("run crashed: " + String(e.stack || e));
  } finally {
    try { await tab.send("Emulation.setCPUThrottlingRate", { rate: 1 }); } catch {}
    await tab.close();
  }
  // budgets
  res.violations = [];
  if (t.fails) {
    if (!res.ok) res.violations.push(`${label}: ${res.notes.join("; ")}`);
    // Plan §4.9 sets the load-task budget at 4× CPU. Report desktop tasks without enforcing it.
    if (profile === "phone") {
      const traceMax = res.trace?.load.max_ms ?? 0;
      const observerMax = Math.max(0, ...(res.longtask_observer?.load || []));
      if (traceMax > LOAD_BUDGET) res.violations.push(`${label}: trace load task ${traceMax} ms (> ${LOAD_BUDGET})`);
      if (observerMax > LOAD_BUDGET) res.violations.push(`${label}: observer load task ${observerMax} ms (> ${LOAD_BUDGET})`);
    }
    const lcpBudget = profile === "phone" ? 1200 : 400;
    if (res.lcp_ms === null) res.violations.push(`${label}: missing LCP measurement`);
    else if (res.lcp_ms > lcpBudget) res.violations.push(`${label}: LCP ${res.lcp_ms} ms (> ${lcpBudget})`);
    if (res.cls === null) res.violations.push(`${label}: missing CLS measurement`);
    else if (res.cls > .1) res.violations.push(`${label}: CLS ${res.cls} (> 0.1)`);
    if (res.inp_ms === null) res.violations.push(`${label}: missing INP measurement`);
    else if (res.inp_ms > 200) res.violations.push(`${label}: INP ${res.inp_ms} ms (> 200)`);
    const liveMax = res.trace ? res.trace.live.map((l) => l.max_ms) : (res.longtask_observer?.live || []).map((l) => Math.max(0, ...l));
    liveMax.forEach((m, i) => { if (m > LIVE_BUDGET) res.violations.push(`${label}: live update ${i + 1}: a task took ${m} ms (> ${LIVE_BUDGET} ms)`); });
    if (t.live.length && res.ok && (res.live_updates ?? 0) < UPDATES) res.violations.push(`${label}: only ${res.live_updates} live update(s), wanted ${UPDATES}`);
  }
  console.log(`${res.violations.length ? "FAIL" : "ok  "} ${label}: ready ${res.ready_ms ?? "-"} ms, rows ${res.rows_at_ready ?? "-"}, LCP ${res.lcp_ms ?? "-"} ms, CLS ${res.cls ?? "-"}, load max task ${res.trace?.load.max_ms ?? "-"} ms, live max ${res.trace ? JSON.stringify(res.trace.live.map((l) => l.max_ms)) : "-"} ms, INP ${res.inp_ms ?? "-"} ms${res.notes.length ? "\n       " + res.notes.join("\n       ") : ""}`);
  return res;
}

const browser = await launch();
const killer = setTimeout(() => { console.log("FAIL the run timed out"); browser.quit(); process.exit(1); }, 20 * 60 * 1000);
killer.unref();
const results = [];
for (const target of TARGETS) for (const profile of PROFILES) results.push(await run(target, profile));
browser.quit();
if (boardId) await fetch(`${base}/api/v1/boards/${boardId}`, { method: "DELETE", headers: { Cookie: sessionCookie, Origin: new URL(base).origin, "X-RH-CSRF": csrf } });

const head = "| target | profile | ready ms | rows | FCP | LCP | CLS | DOM el. | load: max task / >50 / busy ms | longtask obs. (load) | live: max task per update (ms) | interaction: INP / max task | notes |";
const md = [head, "|" + "---|".repeat(13)];
for (const r of results) {
  const tr = r.trace;
  md.push(`| ${r.target} | ${r.profile} | ${r.ready_ms ?? "-"} | ${r.rows_at_ready ?? "-"} | ${r.fcp_ms ?? "-"} | ${r.lcp_ms ?? "-"} | ${r.cls ?? "-"} | ${r.dom_elements ?? "-"} | ${tr ? `${tr.load.max_ms} / ${tr.load.over50} / ${tr.load.busy_ms}` : "-"} | ${r.longtask_observer ? `${r.longtask_observer.load.length} (max ${Math.max(0, ...r.longtask_observer.load)})` : "-"} | ${tr ? tr.live.map((l) => l.max_ms).join(", ") || "none" : "-"} | ${r.inp_ms ?? "-"} / ${tr ? tr.interaction.max_ms : "-"} | ${[...r.notes, ...r.violations].join("; ").replace(/\|/g, "/").slice(0, 300)} |`);
}
const violations = results.flatMap((r) => r.violations);
const text = `# perf-check ${new Date().toISOString()}\n\nbase ${base}, container ${NAME}, phone = 390x844 4x CPU, desktop = 1440x900 1x; times in ms (page clock from navigation start; tasks from Tracing RunTask on the page's main thread).\n\n${md.join("\n")}\n\n${violations.length ? "Budget violations:\n" + violations.map((v) => "- " + v).join("\n") : "No budget violation."}\n`;
fs.writeFileSync(path.join(OUT, "perf.json"), JSON.stringify({ base, name: NAME, at: new Date().toISOString(), results }, null, 2));
fs.writeFileSync(path.join(OUT, "perf.md"), text);
console.log("\n" + text);
console.log(`output: ${OUT}/perf.json, ${OUT}/perf.md`);
process.exit(violations.length ? 1 : 0);
