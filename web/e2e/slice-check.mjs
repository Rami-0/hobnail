// Functional browser check of the Phase 1 slice (docs/plans/react-port.md
// section 5, Phase 1; selectors from the Phase 1 brief, "Test selectors"),
// raw DevTools protocol in headless Chromium (no npm packages), at each width
// in dark and light, with real pointer, touch and key events:
//   - every pass: CSP violations, console errors, uncaught exceptions,
//     failed requests (as app-check.mjs; expected 4xx of the dialog flows
//     are allowed by name);
//   - shell: sidebar (wide) or the drawer button (phone), live status "live";
//   - Containers (/app/containers): "Showing N of M" against the rows and
//     GET /api/v1/containers; the filter (URL ?q=, survives a reload, clears);
//     a state chip (URL ?state=, the running container leaves); the name
//     header toggles aria-sort and the order; group=flat / group=stack (and a
//     group header's aria-expanded toggle when there is one); a full view in
//     the URL (?q&state&sort&dir&group) survives a reload;
//   - container page (/app/containers/<name>): the head, both charts, the
//     range picker links (clicking 24h), the crosshair on BOTH charts while
//     hovering the CPU chart (mouse passes), a drag on the CPU chart going to
//     ?from=&to= (mouse on wide passes, Input.dispatchTouchEvent on phone);
//   - keyboard: "?" opens [data-testid=shortcuts], Escape closes it;
//   - signed in (sign-in through the API, the cookie set in the browser), on
//     the first scheme of each width, the Edit dialog with the real API:
//     (a) a display-name change: toast, dialog closes, the head shows it;
//     (b) 409: the config changed behind the open dialog (another container's
//         override) -> dialog-note "Reload and keep my input" -> reload-keep
//         -> Save keeps the typed value and the other change;
//     (c) 401: the browser's session cookie deleted -> "Sign in (new tab)"
//         link + Retry; signed in again (new cookie) -> Retry saves;
//     (d) a cross-origin write from the check (valid cookie + csrf, Origin
//         http://evil.example) -> 403 "cross-origin write refused", config
//         hash unchanged.
//     The config file is put back byte for byte at the end (also on failure
//     and timeout), as scripts/nav-check.mjs does.
// Screenshots of every state go to OUT.
//   HB_PASSWORD=... node web/e2e/slice-check.mjs <base URL> <container> [other container]
//   HB_PASSWORD=... scripts/e2e.sh web/e2e/slice-check.mjs <base URL> <container> [other]
// <container>: a running container (its page, its override dialog); [other]:
// the container whose override the 409 step changes (default: another
// container from the list). Env: WIDTHS (default "1440x900 390x844"),
// SCHEMES (default "dark light"), FILTER (default: <container>), EDIT
// (default 1; 0 skips the signed-in flows), TOAST (selector of a toast,
// default '[data-testid="toast"], [data-testid="toaster"] [role="status"],
// [role="status"]'), CH, OUT.
// Exits 1 on any failed check, 2 on bad usage. Refuses port 3200. Every wait
// has a timeout: a missing selector fails the step, it never hangs.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { launch, input, recorder, sleep, q, CSP_LISTENER } from "./cdp.mjs";

const base = (process.argv[2] || "").replace(/\/+$/, "");
const CTR = process.argv[3];
let OTHER = process.argv[4] || "";
const PW = process.env.HB_PASSWORD;
if (!/^https?:\/\//.test(base) || !CTR || !PW) {
  console.error("usage: HB_PASSWORD=<password> node web/e2e/slice-check.mjs <base URL of a test instance> <container> [other container]");
  process.exit(2);
}
if (new URL(base).port === "3200") { console.error("refusing port 3200: that is the live instance"); process.exit(2); }
const ORIGIN = new URL(base).origin;
const SIZES = (process.env.WIDTHS || "1440x900 390x844").split(/[\s,]+/).filter(Boolean).map((s) => {
  const [w, h] = s.split("x").map(Number);
  return { w, h: h || (w < 500 ? 844 : 900) };
});
const SCHEMES = (process.env.SCHEMES || "dark light").split(/[\s,]+/).filter(Boolean);
const FILTER = process.env.FILTER || CTR;
const EDIT = process.env.EDIT !== "0";
const TOAST = process.env.TOAST || '[data-testid="toast"], [data-testid="toaster"] [role="status"], [role="status"]';
const OUT = process.env.OUT || fs.mkdtempSync(path.join(os.tmpdir(), "slice-check-"));
fs.mkdirSync(OUT, { recursive: true });
const enc = encodeURIComponent(CTR);

// ---- the API, from the check (its own session) ----
async function login() {
  const r = await fetch(base + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: ORIGIN }, body: JSON.stringify({ password: PW, next: "/app" }) });
  if (r.status !== 200) throw new Error(`sign-in failed: HTTP ${r.status}`);
  const c = r.headers.get("set-cookie").split(";")[0];
  const i = c.indexOf("=");
  return { name: c.slice(0, i), value: c.slice(i + 1), cookie: c, csrf: (await r.json()).csrf };
}
let me;
try { me = await login(); } catch (e) { console.error(String(e.message || e)); process.exit(1); }
const api = async (method, p, body, origin = ORIGIN) => {
  const r = await fetch(base + "/api/settings/" + p, { method, headers: { Cookie: me.cookie, "X-RH-CSRF": me.csrf, Origin: origin, "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const text = await r.text();
  let data = {}; try { data = JSON.parse(text); } catch {}
  return { status: r.status, data, text };
};
const start = (await api("GET", "config")).data;
if (typeof start.text !== "string" || !start.hash) { console.error("cannot read the config through /api/settings/config"); process.exit(1); }
if (/slice-check-/.test(start.text)) { console.error("the config already holds slice-check-* values: clean it first"); process.exit(1); }
async function restore() {
  const now = (await api("GET", "config")).data;
  if (now.hash !== start.hash) await api("POST", "config", { base_hash: now.hash, text: start.text });
  const after = (await api("GET", "config")).data;
  return after.hash === start.hash && after.text === start.text;
}
const override = async (name) => (await api("GET", "containers")).data?.overrides?.[name] ?? null;
if (!OTHER) {
  const known = (await api("GET", "containers")).data?.known || [];
  OTHER = known.map((k) => k.name).find((n) => n !== CTR) || "";
}

// ---- the browser ----
const browser = await launch();
const tab = await browser.tab();
const rec = recorder(tab, ORIGIN);
const inp = input(tab);
async function quit(code) {
  let ok = false;
  try { ok = await restore(); } catch {}
  console.log(ok ? "config restored byte for byte" : "FAIL the config could NOT be restored: check it by hand");
  try { await tab.close(); } catch {}
  browser.quit();
  process.exit(ok ? code : 1);
}
setTimeout(() => { console.log("FAIL the run timed out"); quit(1); }, 15 * 60 * 1000).unref();
process.on("SIGINT", () => quit(1));
process.on("SIGTERM", () => quit(1));
for (const d of ["Page", "Runtime", "Network", "Log"]) await tab.send(d + ".enable");
await tab.send("Runtime.addBinding", { name: "__hbCsp" });
await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });

let fails = 0, label = "";
const results = [];
const check = (ok, what, got) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${label ? label + ": " : ""}${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)?.slice(0, 400)})`}`);
  if (!ok) fails++;
  return ok;
};
const step = async (what, fn) => {
  try { const r = await fn(); return check(r === true || (r && r.ok === true), what, r && r.got !== undefined ? r.got : undefined); }
  catch (e) { return check(false, what, String(e.message || e)); }
};
const ev = (x, ms) => tab.ev(x, ms);
const until = (x, ms) => tab.until(x, ms);
let shotN = 0, shotPrefix = "";
const shot = (name) => tab.shot(path.join(OUT, `${shotPrefix}-${String(++shotN).padStart(2, "0")}-${name}.png`));
const go = async (p, ready, ms = 10000) => { await tab.send("Page.navigate", { url: base + p }); return ready ? until(ready, ms) : true; };
const reload = async (ready, ms = 10000) => { await tab.send("Page.reload", { ignoreCache: false }); await sleep(200); return ready ? until(ready, ms) : true; };
const loc = () => ev("location.pathname + location.search");
const param = (k) => ev(`new URLSearchParams(location.search).get(${q(k)})`);

const TABLE = 'table[data-testid="containers"]';
const ROW = `${TABLE} tr[data-testid="row"]`;
const tableReady = `document.querySelectorAll(${q(ROW)}).length > 0`;
const visibleRows = `[...document.querySelectorAll(${q(ROW)})].filter((r) => r.getClientRects().length > 0)`;
const rowNames = () => ev(`${visibleRows}.map((r) => r.dataset.name)`);
const showing = () => ev(`(() => { const t = document.querySelector('[data-testid="showing"]')?.textContent || ""; const m = t.match(/Showing\\s+(\\d+)\\s+of\\s+(\\d+)/i); return m ? { n: +m[1], m: +m[2], text: t.trim() } : { text: t.trim() }; })()`);
const apiContainers = () => ev(`fetch("/api/v1/containers", { cache: "no-store" }).then((r) => r.json())`);
const DIALOG = 'dialog[data-testid="edit-dialog"]';
const dialogOpen = `!!document.querySelector(${q(DIALOG)})?.open`;
const NAME_IN = `${DIALOG} input[name="name"]`;
const note = () => ev(`document.querySelector('[data-testid="dialog-note"]')?.textContent?.trim() ?? null`);
const headText = () => ev(`document.querySelector('[data-testid="ctr-head"]')?.textContent ?? ""`);
const toastText = () => ev(`[...document.querySelectorAll(${q(TOAST)})].filter((e) => !e.closest("dialog") && e.getClientRects().length && e.textContent.trim()).map((e) => e.textContent.trim()).join(" | ")`);

// ---- Containers ----
async function containersPage(size) {
  if (!(await step("Containers: the table has rows", () => go("/app/containers", tableReady, 12000)))) { await shot("containers-missing"); return; }
  await sleep(400);
  await shot("containers");
  const wide = size.w >= 1000;
  if (wide) await step('shell: nav[data-testid="sidebar"] is visible', () => ev(`!!document.querySelector('nav[data-testid="sidebar"]')?.getClientRects().length`));
  else await step('shell: button[data-testid="drawer-open"] is visible', () => ev(`!!document.querySelector('button[data-testid="drawer-open"]')?.getClientRects().length`));
  await step('shell: [data-testid="live-status"] reaches data-state="live"', () => until(`document.querySelector('[data-testid="live-status"]')?.dataset.state === "live"`, 12000));

  const data = await apiContainers().catch(() => null);
  const all = (data?.groups || []).flatMap((g) => g.containers);
  const M = all.length;
  const s0 = await showing();
  const rows0 = (await rowNames()).length;
  check(s0.m === M && s0.n === rows0, `"Showing N of M": M = the API's ${M} containers, N = the ${rows0} rows shown`, s0);
  check(all.some((c) => c.name === CTR), `the API lists ${CTR}`);

  // filter
  const F = 'input[data-testid="filter"]';
  await step(`filter "${FILTER}": only matching rows, URL ?q=`, async () => {
    if (!(await inp.box(F))) return { ok: false, got: "no " + F };
    await inp.clear(F); await inp.type(FILTER);
    const lc = FILTER.toLowerCase();
    const ok = await until(`(() => { const r = ${visibleRows}; return r.length > 0 && r.every((x) => (x.dataset.name + " " + x.textContent).toLowerCase().includes(${q(lc)})) && new URLSearchParams(location.search).get("q") === ${q(FILTER)}; })()`, 5000);
    return { ok, got: { url: await loc(), rows: (await rowNames()).slice(0, 10) } };
  });
  const s1 = await showing();
  check(s1.n === (await rowNames()).length && s1.m === M, `"Showing" follows the filter (${s1.text})`, s1);
  await shot("filtered");
  await step("the filter survives a reload", async () => {
    await reload(tableReady);
    const v = await ev(`document.querySelector(${q(F)})?.value ?? null`);
    const rows = await rowNames();
    return { ok: v === FILTER && rows.length === s1.n, got: { value: v, rows: rows.length } };
  });
  await step("clearing the filter brings every row back and drops ?q", async () => {
    await inp.clear(F);
    const ok = await until(`${visibleRows}.length === ${rows0} && !new URLSearchParams(location.search).get("q")`, 5000);
    return { ok, got: { url: await loc(), rows: (await rowNames()).length } };
  });

  // state chip
  const chips = await ev(`[...document.querySelectorAll('[data-testid="state-filter"] button[data-state]')].map((b) => b.dataset.state)`);
  check(chips.length >= 2, `state chips: ${chips.join(", ")}`, chips);
  if (chips.includes("stopped")) {
    await step("state chip stopped: URL ?state=stopped, pressed, the running container leaves", async () => {
      await inp.click('[data-testid="state-filter"] button[data-state="stopped"]');
      const ok = await until(`new URLSearchParams(location.search).get("state") === "stopped" && !${visibleRows}.some((r) => r.dataset.name === ${q(CTR)})`, 5000);
      const pressed = await ev(`(() => { const b = document.querySelector('[data-testid="state-filter"] button[data-state="stopped"]'); return b.getAttribute("aria-pressed") || b.getAttribute("aria-current") || b.getAttribute("aria-checked"); })()`);
      const s = await showing();
      const notRunning = all.filter((c) => c.state !== "running").length;
      console.log(`       stopped: showing ${s.text}; the API has ${notRunning} not in state "running"`);
      return { ok: ok && /true|page/.test(String(pressed)) && s.n === (await rowNames()).length, got: { url: await loc(), pressed, showing: s } };
    });
    await shot("state-stopped");
    const allChip = chips.includes("all") ? "all" : chips[0];
    await step(`state chip ${allChip}: every row is back`, async () => {
      await inp.click(`[data-testid="state-filter"] button[data-state="${allChip}"]`);
      return until(`${visibleRows}.length === ${rows0}`, 5000);
    });
  }

  // sort
  const TH = 'th[data-sort="name"]';
  // The URL leaves defaults out, as the old UI does (static/app.js: name
  // ascending is no sort/dir; cpu and mem default to descending, so their
  // dir appears only when ascending). The phone layout has no header row:
  // it sorts with [data-testid=sort-select] and [data-testid=sort-dir].
  const narrow = !(await inp.box(TH, false));
  if (!narrow) {
    await step("the name header toggles aria-sort, the order and ?dir=", async () => {
      const a0 = await ev(`document.querySelector(${q(TH)}).getAttribute("aria-sort")`);
      const o0 = await rowNames();
      await inp.click(TH);
      const want = a0 === "descending" ? "ascending" : "descending";
      const flipped = await until(`document.querySelector(${q(TH)}).getAttribute("aria-sort") === ${q(want)}`, 3000);
      await sleep(200);
      const o1 = await rowNames();
      const u1 = { sort: await param("sort"), dir: await param("dir") };
      await inp.click(TH);
      const back = await until(`document.querySelector(${q(TH)}).getAttribute("aria-sort") === ${q(a0 === "none" ? "ascending" : a0)}`, 3000);
      const u2 = { sort: await param("sort"), dir: await param("dir") };
      const changed = o0.length < 2 || o0.join() !== o1.join();
      const urlOk = (want === "descending" ? u1.dir === "desc" : !u1.dir) && !u1.sort && !u2.sort && u1.dir !== u2.dir;
      return { ok: flipped && back && changed && urlOk, got: { a0, flipped, back, changed, u1, u2 } };
    });
    await shot("sorted");
    await step("another header (cpu) takes aria-sort (descending by default, no dir in the URL), the name header goes back to none", async () => {
      await inp.click('th[data-sort="cpu"]');
      const ok = await until(`document.querySelector('th[data-sort="cpu"]').getAttribute("aria-sort") === "descending" && document.querySelector(${q(TH)}).getAttribute("aria-sort") === "none"`, 3000);
      return { ok: ok && (await param("sort")) === "cpu" && !(await param("dir")), got: await loc() };
    });
  } else {
    await step("phone: the sort control sorts by cpu and the direction button flips it (URL follows)", async () => {
      const okSel = await ev(`(() => { const s = document.querySelector('[data-testid="sort-select"] select, select[data-testid="sort-select"]'); if (!s) return false; s.value = "cpu"; s.dispatchEvent(new Event("change", { bubbles: true })); return true; })()`);
      if (!okSel) return { ok: false, got: "no sort select" };
      const s1 = await until(`new URLSearchParams(location.search).get("sort") === "cpu"`, 3000);
      await inp.click('[data-testid="sort-dir"]');
      const s2 = await until(`new URLSearchParams(location.search).get("dir") === "asc"`, 3000);
      await shot("sorted");
      return { ok: s1 && s2, got: await loc() };
    });
  }

  // grouping
  const stacks = (data?.groups || []).filter((g) => g.containers.length).length;
  await step("?group=flat: no group headers", async () => {
    await go("/app/containers?group=flat", tableReady);
    await sleep(300);
    const n = await ev(`document.querySelectorAll('[data-testid="group"]').length`);
    return { ok: n === 0, got: n };
  });
  await shot("flat");
  await step(`?group=stack: a group header per stack (${stacks})`, async () => {
    await go("/app/containers?group=stack", tableReady);
    await until(`document.querySelectorAll('[data-testid="group"]').length > 0`, 4000);
    const n = await ev(`document.querySelectorAll('[data-testid="group"][data-stack]').length`);
    return { ok: n === stacks, got: n };
  });
  const toggle = await ev(`(() => { const b = document.querySelector('[data-testid="group"] button[aria-expanded]'); return b ? b.closest('[data-testid="group"]').dataset.stack : null; })()`);
  if (toggle) await step(`the group header of ${toggle} toggles aria-expanded`, async () => {
    const sel = `[data-testid="group"][data-stack="${toggle}"] button[aria-expanded]`;
    const a = await ev(`document.querySelector(${q(sel)}).getAttribute("aria-expanded")`);
    await inp.click(sel);
    const ok = await until(`document.querySelector(${q(sel)}).getAttribute("aria-expanded") !== ${q(a)}`, 3000);
    await shot("group-toggled");
    await inp.click(sel);
    return ok && (await until(`document.querySelector(${q(sel)}).getAttribute("aria-expanded") === ${q(a)}`, 3000));
  });
  else console.log("       note: no group header button with aria-expanded (no collapse toggle to check)");
  const groupSwitch = await ev(`(() => { const e = document.querySelector('[data-testid="group-toggle"], a[href*="group=flat"], button[data-group="flat"]'); return e ? (e.dataset.testid ? '[data-testid="group-toggle"]' : e.tagName === "A" ? 'a[href*="group=flat"]' : 'button[data-group="flat"]') : null; })()`);
  if (groupSwitch) await step(`the layout control (${groupSwitch}) switches to flat`, async () => {
    await inp.click(groupSwitch);
    return until(`new URLSearchParams(location.search).get("group") === "flat" && document.querySelectorAll('[data-testid="group"]').length === 0`, 4000);
  });
  else console.log("       note: no layout control found ([data-testid=group-toggle], a[href*=group=flat], button[data-group=flat])");

  // the whole view in the URL
  const view = `?q=svc&state=running&sort=cpu&dir=asc&group=flat`;
  const viewOk = `(() => { const f = document.querySelector(${q(F)}); const th = document.querySelector('th[data-sort="cpu"]'); const chip = document.querySelector('[data-testid="state-filter"] button[data-state="running"]'); return !!f && f.value === "svc" && (!th || !th.getClientRects().length || th.getAttribute("aria-sort") === "ascending") && document.querySelectorAll('[data-testid="group"]').length === 0 && (!chip || /true|page/.test(chip.getAttribute("aria-pressed") || chip.getAttribute("aria-current") || chip.getAttribute("aria-checked") || "")); })()`;
  await step(`the view ${view} is applied from the URL`, async () => { await go("/app/containers" + view, tableReady); return until(viewOk, 4000); });
  await step("and survives a reload", async () => { await reload(tableReady); const ok = await until(viewOk, 4000); const u = await loc(); return { ok: ok && u.includes("sort=cpu") && u.includes("dir=asc"), got: u }; });
  await shot("view-from-url");

  // keyboard
  await step('keyboard: "?" opens [data-testid="shortcuts"]', async () => {
    await ev("document.activeElement && document.activeElement.blur(), true");
    await inp.key("?", { modifiers: 8 });
    return until(`!!document.querySelector('[data-testid="shortcuts"]')?.getClientRects().length`, 3000);
  });
  await shot("shortcuts");
  await step("Escape closes it", async () => {
    await inp.key("Escape");
    return until(`!document.querySelector('[data-testid="shortcuts"]')?.getClientRects().length`, 3000);
  });
}

// ---- container page ----
const chartsReady = `!!document.querySelector('figure[data-testid="chart-cpu"] svg') && !!document.querySelector('figure[data-testid="chart-mem"] svg')`;
const cursorIn = (fig) => `(() => { const f = document.querySelector('figure[data-testid="${fig}"]'); if (!f) return false; if (f.querySelector('.recharts-tooltip-cursor, .recharts-active-dot, [data-testid="crosshair"], [data-testid="chart-tooltip"]')) return true; const w = f.querySelector('.recharts-tooltip-wrapper'); return !!w && getComputedStyle(w).visibility !== "hidden" && w.textContent.trim().length > 0; })()`;
async function containerPage(size) {
  const ready = `!!document.querySelector('[data-testid="ctr-head"]')`;
  if (!(await step(`container page: [data-testid="ctr-head"] for ${CTR}`, () => go(`/app/containers/${enc}`, ready, 12000)))) { await shot("container-missing"); return; }
  check((await headText()).length > 0, "the head has text", await headText());
  await step('both charts: figure[data-testid="chart-cpu"] and chart-mem with an svg', () => until(chartsReady, 10000));
  await sleep(500);
  await shot("container");
  check(!(await ev(`!!document.querySelector('a[data-testid="edit"]')`)), 'signed out: no a[data-testid="edit"]');

  const links = await ev(`[...document.querySelectorAll('[data-testid="range"] a')].map((a) => ({ t: a.textContent.trim(), h: a.getAttribute("href"), cur: a.getAttribute("aria-current") }))`);
  check(links.length >= 3 && links.every((l) => /[?&](range|from)=/.test(l.h || "")), `range picker: ${links.map((l) => l.t).join(" ")}`, links);
  const l24 = links.find((l) => /range=24h/.test(l.h));
  if (l24) await step("range 24h: URL ?range=24h, the link is current, charts still there", async () => {
    await inp.click(`[data-testid="range"] a[href*="range=24h"]`);
    const ok = await until(`new URLSearchParams(location.search).get("range") === "24h" && ${chartsReady}`, 8000);
    const cur = await ev(`document.querySelector('[data-testid="range"] a[href*="range=24h"]')?.getAttribute("aria-current")`);
    return { ok: ok && !!cur, got: { url: await loc(), cur } };
  });
  else check(false, "range picker has a 24h link", links);
  await shot("range-24h");

  const plot = async () => {
    const b = await inp.box('figure[data-testid="chart-cpu"] svg');
    return b && { ...b, y: b.top + b.height * 0.5, x1: b.left + b.width * 0.35, x2: b.left + b.width * 0.65 };
  };
  if (!inp.touch) {
    await step("crosshair: hovering the CPU chart shows a cursor or tooltip on BOTH charts", async () => {
      const b = await plot();
      if (!b) return { ok: false, got: "no chart svg" };
      for (let i = 0; i <= 6; i++) { await inp.move(b.left + b.width * (0.4 + i * 0.03), b.y); await sleep(40); }
      const ok = await until(`${cursorIn("chart-cpu")} && ${cursorIn("chart-mem")}`, 2500);
      return { ok, got: { cpu: await ev(cursorIn("chart-cpu")), mem: await ev(cursorIn("chart-mem")) } };
    });
    await shot("crosshair");
  }
  await step(`${inp.touch ? "touch" : "mouse"} drag on the CPU chart goes to ?from=&to=`, async () => {
    await go(`/app/containers/${enc}?range=6h`, chartsReady, 10000);
    await sleep(400);
    const b = await plot();
    if (!b) return { ok: false, got: "no chart svg" };
    if (inp.touch) await inp.touchDrag(b.x1, b.y, b.x2, b.y); else await inp.drag(b.x1, b.y, b.x2, b.y);
    const ok = await until(`(() => { const p = new URLSearchParams(location.search); const f = +p.get("from"), t = +p.get("to"); return f > 0 && t > f; })()`, 4000);
    return { ok, got: await loc() };
  });
  await shot("drag-zoomed");
}

// ---- the Edit dialog, signed in ----
async function setBrowserCookie(s) {
  const r = await tab.send("Network.setCookie", { name: s.name, value: s.value, url: base, httpOnly: true, sameSite: "Strict", path: "/" });
  return !!r.result?.success;
}
async function openDialog() {
  await go(`/app/containers/${enc}`, `!!document.querySelector('a[data-testid="edit"]')`, 12000);
  const href = await ev(`document.querySelector('a[data-testid="edit"]')?.getAttribute("href") ?? null`);
  if (!href) return { ok: false, got: "no a[data-testid=edit]" };
  await inp.click('a[data-testid="edit"]');
  const ok = await until(`${dialogOpen} && new URLSearchParams(location.search).get("edit") === "override" && !!document.querySelector(${q(NAME_IN)})`, 6000);
  return { ok: ok && /edit=override/.test(href), got: { href, url: await loc() } };
}
async function typeName(v) {
  if (!(await inp.clear(NAME_IN))) return false;
  await inp.type(v);
  return (await ev(`document.querySelector(${q(NAME_IN)}).value`)) === v;
}
const saved = (v) => until(`!${dialogOpen} && new URLSearchParams(location.search).get("edit") !== "override" && (document.querySelector('[data-testid="ctr-head"]')?.textContent || "").includes(${q(v)})`, 8000);
async function editFlows(tag) {
  const s = await login();
  check(await setBrowserCookie(s), "signed in: the session cookie set in the browser");
  // (d) a foreign origin, from the check (first: it needs no UI)
  await step("(d) a write with Origin http://evil.example: 403 cross-origin write refused, config unchanged", async () => {
    const before = (await api("GET", "config")).data.hash;
    const r = await api("POST", "containers/override", { base_hash: before, names: [CTR], set: { name: `slice-check-${tag}-evil` } }, "http://evil.example");
    const after = (await api("GET", "config")).data.hash;
    return { ok: r.status === 403 && /cross-origin write refused/.test(r.text) && after === before, got: { status: r.status, body: r.text.slice(0, 120), same: after === before } };
  });
  // (a)
  const A = `slice-check-${tag}-a`;
  if (!(await step("Edit: a[data-testid=edit] opens dialog[data-testid=edit-dialog] at ?edit=override", openDialog))) { await shot("edit-missing"); check(await restore(), "the config is back byte for byte"); return; }
  await shot("edit-open");
  await step(`(a) save a display name: toast, the dialog closes, the head shows "${A}"`, async () => {
    if (!(await typeName(A))) return { ok: false, got: "could not type into " + NAME_IN };
    await inp.click(`${DIALOG} button[data-testid="save"]`);
    const ok = await saved(A);
    const toast = await toastText();
    const o = await override(CTR);
    return { ok: ok && toast.length > 0 && o?.name === A, got: { closed: ok, toast, override: o, head: (await headText()).slice(0, 120) } };
  });
  await shot("edit-a-saved");

  // (b) 409
  const B = `slice-check-${tag}-b`, OB = `slice-check-${tag}-other`;
  if (await step("(b) the dialog open again", openDialog)) {
    const h = (await api("GET", "containers")).data.hash;
    const behind = OTHER ? await api("POST", "containers/override", { base_hash: h, names: [OTHER], set: { name: OB } }) : { status: 0 };
    check(behind.status === 200, `the config changed behind the dialog (an override of ${OTHER || "?"})`, behind.text?.slice(0, 200));
    const off = rec.allow(({ method, url, status }) => method === "POST" && status === 409 && new URL(url).pathname === "/api/settings/containers/override" && "the expected 409");
    await typeName(B);
    await inp.click(`${DIALOG} button[data-testid="save"]`);
    await step('409: [data-testid="dialog-note"] offers "Reload and keep my input"', async () => {
      const ok = await until(`!!document.querySelector('[data-testid="dialog-note"]') && !!document.querySelector('button[data-testid="reload-keep"]') && /Reload and keep my input/.test(document.querySelector('button[data-testid="reload-keep"]').textContent)`, 6000);
      return { ok, got: await note() };
    });
    await shot("edit-b-409");
    off();
    await step("reload-keep, then Save: saved with the typed value, the other change kept", async () => {
      await inp.click('button[data-testid="reload-keep"]');
      await sleep(500);
      const kept = await ev(`document.querySelector(${q(NAME_IN)})?.value ?? null`);
      await inp.click(`${DIALOG} button[data-testid="save"]`);
      const ok = await saved(B);
      const o = await override(CTR), other = OTHER ? await override(OTHER) : { name: OB };
      return { ok: ok && kept === B && o?.name === B && other?.name === OB, got: { kept, closed: ok, override: o, other, note: await note().catch(() => null) } };
    });
    await shot("edit-b-saved");
  }

  // (c) 401
  const C = `slice-check-${tag}-c`;
  if (await step("(c) the dialog open again", openDialog)) {
    await tab.send("Network.deleteCookies", { name: s.name, url: base });
    const off = rec.allow(({ status, url }) => status === 401 && /^\/api\/(settings|v1)\//.test(new URL(url).pathname) && "the expected 401 (cookie deleted)");
    await typeName(C);
    await inp.click(`${DIALOG} button[data-testid="save"]`);
    await step('401: "Sign in (new tab)" link and a Retry button', async () => {
      const ok = await until(`(() => { const a = document.querySelector('a[data-testid="signin-newtab"]'); return !!a && /Sign in \\(new tab\\)/.test(a.textContent) && a.target === "_blank" && !!document.querySelector('button[data-testid="retry"]'); })()`, 6000);
      return { ok, got: { note: await note(), href: await ev(`document.querySelector('a[data-testid="signin-newtab"]')?.getAttribute("href") ?? null`) } };
    });
    await shot("edit-c-401");
    off();
    const s2 = await login();
    check(await setBrowserCookie(s2), "signed in again: the new cookie set in the browser");
    await step("Retry saves", async () => {
      await inp.click('button[data-testid="retry"]');
      const ok = await saved(C);
      const o = await override(CTR);
      return { ok: ok && o?.name === C, got: { closed: ok, override: o, note: await note().catch(() => null) } };
    });
    await shot("edit-c-saved");
  }

  check(await restore(), "the config is back byte for byte after the edit flows");
}

function summarise() {
  check(rec.csp.length === 0, `no CSP violation (${rec.csp.length})`, rec.csp);
  check(rec.exceptions.length === 0, `no uncaught exception (${rec.exceptions.length})`, rec.exceptions);
  check(rec.console.length === 0, `no console error (${rec.console.length})`, rec.console);
  check(rec.failed.length === 0, `no unexpected failed request or HTTP >= 400 (${rec.failed.length})`, rec.failed);
  for (const a of rec.allowed) console.log(`       allowed: ${a}`);
  for (const n of rec.notes) console.log(`       note: ${n}`);
  results.push({ label, csp: rec.csp.length, console: rec.console.length, exceptions: rec.exceptions.length, failed: rec.failed.length });
}

for (const size of SIZES) for (const [si, scheme] of SCHEMES.entries()) {
  label = `${size.w}x${size.h} ${scheme}`;
  shotPrefix = `${size.w}-${scheme}`; shotN = 0;
  rec.reset();
  inp.touch = size.w < 500;
  try {
    await tab.send("Network.clearBrowserCookies");
    await tab.send("Emulation.setDeviceMetricsOverride", { width: size.w, height: size.h, deviceScaleFactor: 1, mobile: inp.touch });
    await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: inp.touch ? 5 : 0 });
    await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
    await containersPage(size);
    await containerPage(size);
    if (EDIT && si === 0) await editFlows(`${size.w}`);
  } catch (e) { check(false, "pass crashed", String(e.stack || e)); }
  summarise();
}

console.log("\n| pass | CSP | console errors | exceptions | failed requests |\n|---|---|---|---|---|");
for (const r of results) console.log(`| ${r.label} | ${r.csp} | ${r.console} | ${r.exceptions} | ${r.failed} |`);
console.log(`\nscreenshots: ${OUT}`);
console.log(fails ? `${fails} check(s) FAILED` : "all checks passed");
await quit(fails ? 1 : 0);
