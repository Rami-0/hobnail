// Shared pieces of the cross-browser matrix (scripts/e2e-matrix.sh): the
// profiles, the browser launch, and the checks that run on every page state
// a flow visits. Runs inside mcr.microsoft.com/playwright with playwright-core
// and axe-core from the matrix volume (/pw/node_modules), never from web/.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { MEASURE } from "../hit-targets.mjs";

const require = createRequire("/pw/package.json");
export const pw = require("playwright-core");
const AXE = fs.readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");

// Viewports. `flows` profiles run every flow; all of them run the page sweep.
// zoom200: a 1440 x 900 screen at 200 % browser zoom has a 720 x 450 CSS
// viewport at device pixel ratio 2, which is what Playwright can emulate in
// all three engines (none of them exposes browser zoom itself).
export const PROFILES = {
  "360": { width: 360, height: 740, dpr: 3, touch: true },
  "390": { width: 390, height: 844, dpr: 3, touch: true, flows: true },
  "412": { width: 412, height: 915, dpr: 3, touch: true },
  "tablet-p": { width: 820, height: 1180, dpr: 2, touch: true, flows: true, tablet: true },
  "tablet-l": { width: 1180, height: 820, dpr: 2, touch: true, flows: true, tablet: true },
  "1280": { width: 1280, height: 800, dpr: 1 },
  "1440": { width: 1440, height: 900, dpr: 1, flows: true },
  "1920": { width: 1920, height: 1080, dpr: 1 },
  "2560": { width: 2560, height: 1440, dpr: 1 },
  "zoom200": { width: 720, height: 450, dpr: 2 },
};

export function assertTarget(base) {
  const url = new URL(base);
  if (url.port === "3200") { console.error("matrix: refusing live port 3200"); process.exit(2); }
  if (url.hostname !== "127.0.0.1" || !/^339[5-9]$/.test(url.port)) throw new Error(`matrix: ${url.origin} is not a throwaway instance`);
}

export async function launch(engine) {
  return pw[engine].launch({ headless: true, args: engine === "chromium" ? ["--disable-dev-shm-usage"] : [] });
}

export function contextOptions(engine, profile, { theme, motion }) {
  return {
    viewport: { width: profile.width, height: profile.height },
    deviceScaleFactor: profile.dpr,
    hasTouch: !!profile.touch,
    // Firefox has no mobile emulation (Playwright refuses isMobile there).
    ...(engine === "firefox" ? {} : { isMobile: !!profile.touch }),
    colorScheme: theme, reducedMotion: motion, locale: "en-US", timezoneId: "UTC",
    acceptDownloads: true,
  };
}

// CSP violations: the document's own securitypolicyviolation event, sent out
// through a binding (works across navigations and in all three engines).
const CSP_INIT = `addEventListener("securitypolicyviolation", (e) => {
  try { window.__hbCsp(JSON.stringify({ directive: e.violatedDirective, blocked: e.blockedURI, source: e.sourceFile, line: e.lineNumber })); } catch {}
}, true);`;
// console.error's arguments, written out in the page as it is called (an
// Error's name and message, JSON for objects): the console event's own
// argument handles are gone when the page has navigated away meanwhile.
const CONSOLE_INIT = `(() => { const orig = console.error; console.error = function (...args) {
  try { window.__hbErr(args.map((v) => v && typeof v === "object" && "message" in v ? (v.name || "Error") + ": " + v.message : typeof v === "object" ? (() => { try { return JSON.stringify(v); } catch { return String(v); } })() : String(v)).join(" ").slice(0, 400)); } catch {}
  return orig.apply(this, args);
}; })();`;

// Requests anywhere but the throwaway instance and its fixtures (127.0.0.1,
// localhost) are aborted and reported: the matrix never contacts a
// third-party service.
export async function guardNetwork(context, report) {
  await context.route((url) => !/^(127\.0\.0\.1|localhost)$/.test(url.hostname), (route) => {
    report(route.request().url());
    return route.abort("blockedbyclient");
  });
}

// WebKit's page.screenshot() always runs Playwright's animation sync, which
// appends a <style> element to the page (playwright-core 1.64.1,
// lib/coreBundle.js: inPagePrepareForScreenshots(..., syncAnimations) with
// WKPage.shouldToggleStyleSheetToSyncAnimations() === true; Chromium and
// Firefox return false). The page's style-src 'self' refuses it. Exactly that
// refusal, in WebKit, while a screenshot is being taken, is set aside (and
// counted in the unit's JSON as `excluded`); every other CSP event counts.
const screenshotProbe = (engine, e) => engine === "webkit" && e.kind === "csp" && e.shooting &&
  (/"directive":"style-src(-elem)?","blocked":"inline"/.test(e.text) || /^Refused to apply a stylesheet because its hash, its nonce, or 'unsafe-inline' does not appear in the style-src directive/.test(e.text));

// Leaving a page cancels its loads in flight (the event stream, a lazy
// chunk's CSS, the favicon fetch). WebKit then reports "EventSource / Fetch
// API cannot load <url> due to access control checks." as page errors, and
// Vite's preload helper (WebKit, Firefox) throws "Unable to preload CSS for
// <url>", and a lazy chunk fails with "error loading dynamically imported
// module: <url>" (Firefox) or "TypeError: Importing a module script
// failed." (WebKit); React logs them. Exactly these messages are set aside when they
// arrive while a main-frame navigation is under way (its request sent, the
// new document not yet committed, at most 1.5 s): only the document being
// left exists then, so they cannot come from the page the user lands on.
// Counted in the unit's JSON as `excluded`.
const CANCEL_SIGNATURE = /^(EventSource cannot load|Fetch API cannot load) (.+?) due to access control checks\.|Unable to preload CSS for (\S+)|error loading dynamically imported module: (\S+)|^TypeError: Importing a module script failed\.( @ |$)/;

// Chromium on Linux aborts the requests it has in flight with
// net::ERR_NETWORK_CHANGED whenever a network interface of its namespace
// changes. The browsers run with --network host, and the matrix's own
// throwaway containers (every unit's instance and fixtures, the network-drop
// restart, on all three engines at once) add and remove veth interfaces on
// the host as they start and stop. Exactly Chromium's "Failed to load
// resource: net::ERR_NETWORK_CHANGED" is set aside when the host's
// docker-events.log (e2e-matrix.sh) shows a container start or stop, or a
// network connect or disconnect, from 3 s before to 1 s after it; counted
// in the unit's JSON as `excluded`.
const hostEvents = () => { try { return fs.readFileSync(path.join(process.env.OUT || "/out", "docker-events.log"), "utf8").split("\n").filter(Boolean).map((l) => { const [ns, type, action, name] = l.split(" "); return { at: Number(BigInt(ns) / 1000000n), what: `${type} ${action} ${name}` }; }); } catch { return []; } };
const netChange = (engine, e) => {
  if (engine !== "chromium" || e.kind !== "console" || !/^Failed to load resource: net::ERR_NETWORK_CHANGED\b/.test(e.text)) return null;
  const near = hostEvents().filter((x) => x.at - e.at >= -3000 && x.at - e.at <= 1000);
  if (!near.length) { e.text += " [no container or network event on the host within 3 s]"; return null; }
  return near.map((x) => `${x.what} ${x.at - e.at} ms`).slice(0, 3).join(", ");
};

// One harness per page: records console errors, page errors and CSP
// violations, and checks a page state on demand. Every event recorded since
// the previous state (navigation, clicks, saves, the settle wait) is reported
// with the next state; run.mjs flushes what is left after each flow.
export async function harness(page, { engine, profileName, profile, out, unit, motion, results }) {
  const ctx = { flow: "", state: "", allow: [], shooting: false };
  const events = [];
  let seen = 0;
  results.excluded ??= [];
  const allowed = (text) => ctx.allow.some((re) => re.test(text));
  // Each event notes how long after the last main-frame navigation it came
  // (engine notes: errors a navigation causes in the page it leaves).
  let lastNav = Date.now();
  let navStart = 0;
  page.on("framenavigated", (fr) => { if (fr === page.mainFrame()) lastNav = Date.now(); });
  page.on("request", (r) => { if (r.isNavigationRequest() && r.frame() === page.mainFrame()) navStart = Date.now(); });
  const leaving = () => navStart > lastNav && Date.now() - navStart <= 1500;
  const push = (e) => events.push({ ...e, flow: ctx.flow, shooting: ctx.shooting, sinceNav: Date.now() - lastNav, leaving: leaving(), at: Date.now() });
  // Failed requests, for the record only (the console reports the ones a
  // page sees): URL, the engine's reason, time since the last navigation.
  results.requestsFailed ??= [];
  page.on("requestfailed", (r) => {
    const error = r.failure()?.errorText ?? "";
    if (results.requestsFailed.length < 300) results.requestsFailed.push({ flow: ctx.flow, url: r.url().replace(/^https?:\/\/[^/]+/, ""), error, sinceNav: Date.now() - lastNav });
  });
  /** A message about a load that leaving the page cancelled (see CANCEL_SIGNATURE). */
  const leftPage = (e) => {
    if (e.kind !== "pageerror" && e.kind !== "console") return false;
    if (!CANCEL_SIGNATURE.test(e.kind === "console" ? e.text.replace(/^Error \[Error: /, "Error: ").replace(/^Error: /, "") : e.text)) return false;
    // Evidence for a signature that was not set aside.
    if (!e.leaving) e.text += ` [left-page signature outside a navigation: ${e.sinceNav} ms after the last commit]`;
    return e.leaving;
  };
  await page.context().exposeBinding("__hbCsp", (_src, json) => { push({ kind: "csp", text: json }); }).catch(() => {});
  await page.addInitScript(CSP_INIT);
  const written = [];
  await page.context().exposeBinding("__hbErr", (_src, text) => { written.push({ text, at: Date.now() }); }).catch(() => {});
  await page.addInitScript(CONSOLE_INIT);
  // console.error(object) prints as "JSHandle@object" or "Error": the
  // record then carries the arguments themselves (an Error's stack, JSON).
  const pending = new Set();
  const describe = async (m) => {
    const at = Date.now();
    const xs = await Promise.all(m.args().map((a) => a.evaluate((v) => v && typeof v === "object" && "message" in v ? `${v.name || "Error"}: ${v.message} ${v.stack ?? ""}` : typeof v === "object" ? (() => { try { return JSON.stringify(v); } catch { return String(v); } })() : String(v)).catch(() => null)));
    if (xs.length && xs.every((x) => x !== null)) return xs.join(" ");
    // The handles are gone (the page navigated away): what the page wrote
    // out for the same call (CONSOLE_INIT), the oldest unclaimed one.
    await new Promise((r) => setTimeout(r, 200));
    const i = written.findIndex((w) => !w.used && Math.abs(w.at - at) < 3000);
    if (i < 0) return "arguments unavailable";
    written[i].used = true;
    return written[i].text;
  };
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    const text = m.text(), where = m.location()?.url ?? "";
    if (/Content.Security.Policy/i.test(text)) { push({ kind: "csp", text }); return; }
    // Allowances see the message and the URL it is about.
    if (allowed(`${text} @ ${where}`)) return;
    const at = where ? " @ " + where.replace(/^https?:\/\/[^/]+/, "").slice(0, 120) + (m.location()?.lineNumber ? ":" + m.location().lineNumber : "") : "";
    const meta = { flow: ctx.flow, shooting: ctx.shooting, sinceNav: Date.now() - lastNav, leaving: leaving(), at: Date.now() };
    if (!/JSHandle@|^Error$|^\s*$/.test(text)) { push({ kind: "console", text: `${text.slice(0, 300)}${at}`, url: page.url() }); return; }
    const p = describe(m).then((args) => { events.push({ kind: "console", text: `${text} [${args.slice(0, 400)}]${at}`, url: page.url(), ...meta }); });
    pending.add(p); p.finally(() => pending.delete(p));
  });
  // WebKit reports failed loads (a dropped event stream) as page errors, so
  // the flow's allowances apply to them too.
  page.on("pageerror", (e) => { const text = String(e.stack || e).slice(0, 400); if (!allowed(text)) push({ kind: "pageerror", text }); });
  let shots = 0;
  /** Problems from the events recorded since the last call. */
  const take = async () => {
    await Promise.allSettled([...pending]);
    const fresh = events.slice(seen); seen = events.length;
    const problems = [];
    let why;
    for (const e of fresh) {
      if (screenshotProbe(engine, e)) results.excluded.push({ reason: "screenshot style probe (WebKit)", flow: e.flow, text: e.text.slice(0, 200) });
      else if (leftPage(e)) results.excluded.push({ reason: "load cancelled by leaving the page", flow: e.flow, text: e.text.slice(0, 200) });
      else if ((why = netChange(engine, e))) results.excluded.push({ reason: "host network change (Chromium)", flow: e.flow, text: `${e.text.slice(0, 160)} [${why}]` });
      else problems.push({ kind: e.kind, detail: e.text, sinceNav: e.sinceNav, leaving: e.leaving });
    }
    return problems;
  };
  const h = {
    page, engine, profile, profileName, unit, ctx, events, out, motion, results,
    coarse: () => page.evaluate(() => matchMedia("(pointer:coarse)").matches),
    /** The target size the design promises on this page (shell.css A11Y-18). */
    async minTarget() { return (await h.coarse()) || profile.width < 700 ? 44 : 24; },
    record(event) { push(event); },
    async shot(name) {
      if (shots >= 25 && !name.endsWith("FAIL")) return;
      shots++;
      const file = path.join(out, "shots", `${engine}-${profileName}-${unit}-${name}`.replace(/[^a-z0-9.-]+/gi, "_") + ".jpg");
      fs.mkdirSync(path.dirname(file), { recursive: true });
      ctx.shooting = true;
      try {
        await page.screenshot({ path: file, type: "jpeg", quality: 45 }).catch(() => {});
        // The refusal's event and console report arrive a task later.
        await page.waitForTimeout(150);
      } finally { ctx.shooting = false; }
    },
    /** Whether a host network change (Chromium's ERR_NETWORK_CHANGED,
     * matched to a container or network event on the host) cut a load since
     * `t0`. If so, every event since `t0` is set aside with that reason and
     * true is returned, so the caller loads the page again: a stylesheet
     * the host cut leaves an unstyled page whose checks say nothing about
     * Hobnail. */
    async setAsideCutLoad(t0) {
      await page.waitForTimeout(300);
      await Promise.allSettled([...pending]);
      const fresh = events.slice(seen).filter((e) => e.at >= t0);
      if (!fresh.some((e) => netChange(engine, e))) return false;
      for (const e of fresh) results.excluded.push({ reason: "load cut by a host network change, page loaded again (Chromium)", flow: e.flow, text: e.text.slice(0, 200) });
      events.splice(seen, events.length - seen, ...events.slice(seen).filter((e) => e.at < t0));
      return true;
    },
    /** Problems recorded after the flow's last state (run.mjs adds them to the flow). */
    flush() { return take(); },
    /** Run every check on the current page state. */
    async state(label, { focus = true, axe = true, targets = true } = {}) {
      ctx.state = label;
      await settle(page);
      const problems = [];
      const coarse = await h.coarse();
      const min = coarse || profile.width < 700 ? 44 : 24;
      const layout = await page.evaluate(`${LAYOUT}(${min})`).catch((e) => ({ error: String(e) }));
      if (layout.error) problems.push({ kind: "harness", detail: `layout: ${layout.error}` });
      else {
        if (layout.overflow) problems.push({ kind: "overflow", detail: layout.overflow });
        for (const c of layout.clipped) problems.push({ kind: "clipped", detail: c });
        if (motion === "reduce" && layout.motion) problems.push({ kind: "motion", detail: layout.motion });
      }
      // An empty state's title is a heading under its section's own heading
      // (Settings: section h2, empty state h3), not a sibling of it.
      const headings = await page.evaluate(() => [...document.querySelectorAll(".empty > :is(h2,h3,h4)")].flatMap((t) => {
        const own = t.closest("section")?.querySelector(":is(h1,h2,h3,h4)");
        return own && own !== t && +own.tagName[1] >= +t.tagName[1] ? [`empty state "${t.textContent}" is an ${t.tagName.toLowerCase()} beside its section's ${own.tagName.toLowerCase()} "${own.textContent.trim().slice(0, 40)}"`] : [];
      })).catch((e) => [`heading check failed: ${e}`]);
      for (const d of headings) problems.push({ kind: "heading", detail: d });
      // Once onboarding has finished the board tour, it must not open again
      // (except as the replay a user asks for with ?tour=1).
      if (ctx.tourDone && !/[?&]tour=1/.test(page.url())) {
        const tour = await page.evaluate(() => { const t = document.querySelector('[data-testid="board-tour"]'); return t?.getClientRects().length ? t.querySelector("h2")?.textContent ?? "shown" : null; }).catch(() => null);
        if (tour) problems.push({ kind: "tour", detail: `board tour open again after onboarding finished it: ${tour}` });
      }
      // With a dialog or menu open, the page behind it is inert or covered:
      // measure the open layer's own controls.
      const layer = await page.evaluate(() => {
        document.querySelectorAll("[data-hb-layer]").forEach((e) => e.removeAttribute("data-hb-layer"));
        const open = [...document.querySelectorAll('[role="dialog"][data-open], [role="menu"], [role="listbox"], [data-testid="board-tour"]')].filter((e) => e.getClientRects().length);
        const top = open.at(-1);
        if (!top) return null;
        top.setAttribute("data-hb-layer", "");
        return { kind: top.getAttribute("role") || "tour", modal: top.getAttribute("role") === "dialog" };
      }).catch((e) => ({ error: String(e) }));
      if (layer?.error) problems.push({ kind: "harness", detail: `layer: ${layer.error}` });
      const scope = layer && !layer.error ? "[data-hb-layer]" : null;
      // Toasts float over the page for a few seconds by design: they are
      // measured on their own, not as part of the page or layer under them.
      await page.evaluate(() => document.querySelectorAll('[data-testid="toast"]').forEach((t) => { t.dataset.hbVis = t.style.visibility; t.style.visibility = "hidden"; })).catch(() => {});
      const measure = async (sel, what) => {
        const r = await page.evaluate(`(() => { const m = ${MEASURE}; const root = ${JSON.stringify(sel)} ? document.querySelector(${JSON.stringify(sel)}) : document; const all = root ? [...root.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=switch],[role=menuitem],[role=option]')].filter((e) => e.getClientRects().length && getComputedStyle(e).visibility === "visible" && !e.closest("[inert],[hidden]")).length : 0; return { controls: m(${JSON.stringify(sel)}), present: all }; })()`).catch((e) => ({ error: String(e) }));
        if (r.error) { problems.push({ kind: "harness", detail: `targets (${what}): ${r.error.slice(0, 200)}` }); return []; }
        // A page with visible controls measures some: none means the
        // measurement missed them.
        if (r.present > 0 && r.controls.length === 0) problems.push({ kind: "harness", detail: `targets (${what}): ${r.present} visible controls, none measured` });
        return r.controls;
      };
      const controls = await measure(scope, scope ? `${layer.kind} layer` : "page");
      await page.evaluate(() => document.querySelectorAll("[data-hb-vis]").forEach((t) => { t.style.visibility = t.dataset.hbVis; delete t.dataset.hbVis; })).catch(() => {});
      const toasts = await page.evaluate(() => [...document.querySelectorAll('[data-testid="toast"]')].some((t) => t.getClientRects().length && getComputedStyle(t).visibility === "visible")).catch(() => false);
      const toastControls = toasts ? await measure(".toasts", "toasts") : [];
      // 44 px where the design promises it (coarse pointer or a phone-width
      // layout, shell.css); 24 px (WCAG 2.5.8) for a fine pointer.
      if (targets) for (const c of [...controls, ...toastControls]) if (min === 44 ? !c.ok44 : !c.ok24) problems.push({ kind: "target", detail: `${c.sel} "${c.name}" ${c.w}x${c.h} (need ${min})` });
      if (axe) for (const v of await runAxe(page)) problems.push({ kind: "axe", detail: v });
      if (focus) { const f = await focusCheck(page, scope ? layer : null); if (f) problems.push({ kind: "focus", detail: f }); }
      problems.push(...(await take()));
      const record = { engine, profile: profileName, unit, flow: ctx.flow, state: label, url: page.url().replace(/^https?:\/\/[^/]+/, ""), controls: controls.length, toastControls: toastControls.length, layer: layer?.kind ?? null, coarse, problems };
      results.states.push(record);
      if (problems.length) { await h.shot(`${ctx.flow}-${label}`); console.log(`  ! ${ctx.flow} / ${label}: ${problems.length} problem(s): ${problems.slice(0, 4).map((p) => p.kind + " " + String(p.detail).slice(0, 140)).join(" | ")}`); }
      return record;
    },
    /** Console errors matching `re` (tested against "message @ url") are expected while `fn` runs. */
    async expecting(res, fn) { ctx.allow.push(...res); try { return await fn(); } finally { ctx.allow.splice(ctx.allow.length - res.length, res.length); } },
  };
  return h;
}

async function settle(page) {
  await page.waitForLoadState("domcontentloaded").catch(() => {});
  await page.waitForFunction(() => !document.querySelector(".ch-wait"), null, { timeout: 15000 }).catch(() => {});
  // Animations and the frame-batched live layer settle.
  await page.waitForTimeout(350);
}

async function runAxe(page) {
  const has = await page.evaluate(() => typeof window.axe === "object").catch(() => false);
  if (!has) await page.evaluate(`(() => { ${AXE}\n; return typeof axe; })()`);
  const result = await page.evaluate(async () => {
    const r = await window.axe.run(document, { resultTypes: ["violations"] });
    return r.violations.map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(", ")}${v.nodes.length > 3 ? ` +${v.nodes.length - 3}` : ""}`);
  }).catch((e) => [`axe failed: ${String(e).slice(0, 200)}`]);
  return result;
}

// In-page helpers for the focus checks. A control shows its focus when its
// own outline or box shadow, or one of its three nearest ancestors' (a field
// wrapper that draws the ring with :has(:focus-visible)), differs between
// focused and blurred; an outline must also be visible (width > 0, not
// transparent). A box shadow or outline that is there at rest is not a ring.
const FOCUS_INIT = `(() => {
  const clear = (c) => c === "transparent" || /rgba?\\(.*,\\s*0\\)$/.test(c);
  const look = (el) => { const out = []; for (let e = el, i = 0; e && i < 4 && e !== document.body && e !== document.documentElement; e = e.parentElement, i++) { const s = getComputedStyle(e);
    out.push({ o: s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0 && !clear(s.outlineColor) ? [s.outlineStyle, s.outlineWidth, s.outlineColor, s.outlineOffset].join(" ") : "", b: s.boxShadow === "none" ? "" : s.boxShadow }); } return out; };
  const active = () => { let el = document.activeElement; while (el?.shadowRoot?.activeElement) el = el.shadowRoot.activeElement; return !el || el === document.body || el === document.documentElement ? null : el; };
  window.__hbFocus = { stops: [], look, active, last: active(),
    // Called after every key press: blurred looks for the stops that lost
    // focus, then the newly focused control (or null).
    step(layerSel, prev) {
      const el = active();
      for (const s of this.stops) if (!s.b && s.el !== el) s.b = look(s.el);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const layer = layerSel ? document.querySelector(layerSel) : null;
      const name = el.tagName.toLowerCase() + (typeof el.className === "string" && el.className.trim() ? "." + el.className.trim().split(/\\s+/).slice(0, 2).join(".") : "") + ' "' + (el.getAttribute("aria-label") || el.textContent || "").trim().replace(/\\s+/g, " ").slice(0, 30) + '"';
      // The key press did not move focus (it left the document: Firefox
      // keeps activeElement; or the layer holds one control).
      const same = el === this.last;
      this.last = el;
      let idx = this.stops.findIndex((s) => s.el === el);
      if (!same && idx < 0) idx = this.stops.push({ el, f: look(el), b: null }) - 1;
      return { idx, name, same, key: el.outerHTML.slice(0, 80), visible: el.matches(":focus-visible"), onScreen: r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth, inLayer: !layer || layer.contains(el) };
    },
    // Blurred looks for stops that never lost focus (the last one), then
    // whether each stop showed a ring.
    finish() {
      for (const s of this.stops) if (!s.b) { const keep = document.activeElement === s.el; s.el.blur(); s.b = look(s.el); if (keep) s.el.focus({ preventScroll: true }); }
      return this.stops.map((s) => s.f.some((f, i) => (f.o && f.o !== s.b[i]?.o) || (f.b && f.b !== s.b[i]?.b)));
    },
  };
})()`;

/** Press `key` `n` times (one more, to see the last stop blurred) and
 * describe each focused control: name, :focus-visible, ring, on screen, and
 * whether it is inside the layer `layerSel`. With `restart`, a Tab that leaves
 * the document starts again from the top (Firefox keeps the old
 * activeElement then, without :focus-visible). */
export async function focusWalk(page, n, { key = "Tab", layerSel = null, restart = true } = {}) {
  await page.evaluate(FOCUS_INIT);
  const stops = [];
  for (let i = 0; i <= n; i++) {
    await page.keyboard.press(key);
    await page.waitForTimeout(100);
    let r = await page.evaluate((sel) => window.__hbFocus.step(sel), layerSel);
    if (restart && key === "Tab" && (r?.same || !(await page.evaluate(() => document.hasFocus())))) {
      await page.evaluate(() => document.activeElement?.blur());
      await page.keyboard.press("Tab");
      await page.waitForTimeout(100);
      r = await page.evaluate((sel) => { window.__hbFocus.last = null; return window.__hbFocus.step(sel); }, layerSel);
    }
    if (i < n && r && !r.same) stops.push(r);
  }
  const rings = await page.evaluate(() => window.__hbFocus.finish());
  for (const s of stops) s.ring = rings[s.idx] ?? false;
  return stops;
}

// Two key presses on the page, or inside the open layer (Tab in a dialog,
// arrows in a menu or listbox; focus is put back where it was afterwards, so
// the flow carries on). Each focused control must match :focus-visible, show
// a ring (see FOCUS_INIT), and be on screen; in a modal dialog, focus must
// stay inside it.
async function focusCheck(page, layer) {
  const key = layer && (layer.kind === "menu" || layer.kind === "listbox") ? "ArrowDown" : "Tab";
  const layerSel = layer ? "[data-hb-layer]" : null;
  if (layer) await page.evaluate(() => { document.querySelectorAll("[data-hb-orig]").forEach((e) => e.removeAttribute("data-hb-orig")); document.activeElement?.setAttribute("data-hb-orig", ""); });
  const stops = await focusWalk(page, 2, { key, layerSel, restart: !layer }).catch((e) => [{ error: String(e).slice(0, 200) }]);
  if (layer) await page.evaluate(() => { const el = document.querySelector("[data-hb-orig]"); el?.removeAttribute("data-hb-orig"); if (el?.isConnected && document.activeElement !== el) el.focus({ preventScroll: true }); }).catch(() => {});
  const bad = [];
  let inside = 0;
  for (const r of stops) {
    if (r.error) { bad.push(`focus walk failed: ${r.error}`); continue; }
    if (!r.inLayer) { if (layer.modal) bad.push(`${r.name}: focus left the open ${layer.kind}`); continue; }
    inside++;
    if (!r.visible || !r.ring) bad.push(`${r.name}: ${!r.visible ? "not :focus-visible" : "no focus ring (outline and box shadow as at rest)"}`);
    else if (!r.onScreen) bad.push(`${r.name}: focused off screen`);
  }
  if (layer?.modal && !inside && !bad.length) bad.push(`${key} reached no control inside the open ${layer.kind}`);
  return bad.join("; ");
}

// Sideways scroll, clipped controls and (under reduced motion) any running
// transition or animation.
const LAYOUT = `((min) => {
  const vw = innerWidth, d = document.documentElement;
  const overflow = d.scrollWidth > vw + 1 ? "document " + d.scrollWidth + " > " + vw : document.body.scrollWidth > vw + 1 ? "body " + document.body.scrollWidth + " > " + vw : "";
  const tag = (e) => e.tagName.toLowerCase() + (typeof e.className === "string" && e.className.trim() ? "." + e.className.trim().split(/\\s+/).slice(0, 2).join(".") : "");
  const hidden = (el) => { const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) return true; const s = getComputedStyle(el); if (s.visibility !== "visible" || +s.opacity === 0) return true; return !!el.closest("[inert],[aria-hidden=true],[hidden]"); };
  const clipped = [];
  for (const el of document.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=switch],[role=combobox],[role=menuitem],[role=option]')) {
    if (hidden(el) || el.classList.contains("skip")) continue;
    const r = el.getBoundingClientRect(); let why = "", scrolls = false;
    // Off-screen rows under content-visibility:auto are not laid out
    // (Firefox reports their cells at placeholder size): not judged.
    if ((r.bottom < 0 || r.top > innerHeight) && el.closest("tr,li,section") && [...(function* (e) { for (; e; e = e.parentElement) yield e; })(el)].some((e) => getComputedStyle(e).contentVisibility === "auto")) continue;
    for (let p = el.parentElement; p && p !== d && p !== document.body; p = p.parentElement) {
      const s = getComputedStyle(p);
      if (s.overflowX === "visible" && s.overflowY === "visible") continue;
      const sx = /auto|scroll/.test(s.overflowX) && p.scrollWidth > p.clientWidth, sy = /auto|scroll/.test(s.overflowY) && p.scrollHeight > p.clientHeight;
      if (sx || sy) { scrolls = true; break; }
      const q = p.getBoundingClientRect();
      const w = Math.min(r.right, q.right) - Math.max(r.left, q.left), h = Math.min(r.bottom, q.bottom) - Math.max(r.top, q.top);
      // Designed truncation: an ellipsis on the clipping cell, with the
      // control still at least the page's target size wide and fully tall;
      // the ancestors further out are still examined.
      if (s.textOverflow === "ellipsis" && w >= Math.min(min, r.width) && h >= r.height - 1.5) continue;
      if (w < r.width - 1.5 || h < r.height - 1.5) { why = "cut by " + tag(p) + " (" + Math.round(Math.max(0, w)) + "x" + Math.round(Math.max(0, h)) + " of " + Math.round(r.width) + "x" + Math.round(r.height) + ")"; break; }
    }
    if (!why && !scrolls && (r.left < -1 || r.right > vw + 1)) why = "outside the viewport (" + Math.round(r.left) + ".." + Math.round(r.right) + ")";
    if (!why && !el.matches("input,textarea,select")) {
      const s = getComputedStyle(el);
      if (s.overflowX !== "visible" && el.scrollWidth > el.clientWidth + 1 && s.textOverflow !== "ellipsis" && !el.querySelector("*") ) why = "text cut (" + el.scrollWidth + " > " + el.clientWidth + ")";
    }
    if (why) clipped.push(tag(el) + ' "' + (el.getAttribute("aria-label") || el.textContent || "").trim().replace(/\\s+/g, " ").slice(0, 30) + '": ' + why);
  }
  let motion = "";
  for (const el of document.querySelectorAll("*")) {
    if (hidden(el)) continue;
    const s = getComputedStyle(el);
    const on = (v) => v.split(",").some((x) => parseFloat(x) > 0.001);
    if ((on(s.transitionDuration) && s.transitionProperty !== "none") || (on(s.animationDuration) && s.animationName !== "none")) { motion = tag(el) + " " + (s.animationName !== "none" ? "animation " + s.animationName : "transition " + s.transitionProperty); break; }
  }
  return { overflow, clipped: clipped.slice(0, 12), motion };
})`;
