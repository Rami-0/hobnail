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

// One harness per page: records console errors, page errors and CSP
// violations, and checks a page state on demand.
export async function harness(page, { engine, profileName, profile, out, unit, motion, results }) {
  const ctx = { flow: "", state: "", allow: [] };
  const events = [];
  const allowed = (text) => ctx.allow.some((re) => re.test(text));
  await page.context().exposeBinding("__hbCsp", (_src, json) => { events.push({ kind: "csp", text: json, flow: ctx.flow }); }).catch(() => {});
  await page.addInitScript(CSP_INIT);
  page.on("console", (m) => {
    if (m.type() !== "error") return;
    const text = m.text();
    if (/Content.Security.Policy/i.test(text)) events.push({ kind: "csp", text, flow: ctx.flow });
    else if (!allowed(text)) events.push({ kind: "console", text: text.slice(0, 300), flow: ctx.flow, url: page.url() });
  });
  page.on("pageerror", (e) => events.push({ kind: "pageerror", text: String(e.stack || e).slice(0, 400), flow: ctx.flow }));
  let shots = 0;
  const h = {
    page, engine, profile, profileName, unit, ctx, events,
    coarse: () => page.evaluate(() => matchMedia("(pointer:coarse)").matches),
    async shot(name) {
      if (shots >= 25) return;
      shots++;
      const file = path.join(out, "shots", `${engine}-${profileName}-${unit}-${name}`.replace(/[^a-z0-9.-]+/gi, "_") + ".jpg");
      fs.mkdirSync(path.dirname(file), { recursive: true });
      await page.screenshot({ path: file, type: "jpeg", quality: 45 }).catch(() => {});
    },
    /** Run every check on the current page state. */
    async state(label, { focus = true, axe = true } = {}) {
      ctx.state = label;
      await settle(page);
      const problems = [];
      const before = events.length;
      const layout = await page.evaluate(LAYOUT).catch((e) => ({ error: String(e) }));
      if (layout.error) problems.push({ kind: "harness", detail: layout.error });
      else {
        if (layout.overflow) problems.push({ kind: "overflow", detail: layout.overflow });
        for (const c of layout.clipped) problems.push({ kind: "clipped", detail: c });
        if (motion === "reduce" && layout.motion) problems.push({ kind: "motion", detail: layout.motion });
      }
      const coarse = await h.coarse();
      const narrow = profile.width < 700;
      // With a dialog or menu open, the page behind it is inert or covered:
      // measure the open layer's own controls.
      const layer = await page.evaluate(() => {
        const open = [...document.querySelectorAll('[role="dialog"][data-open], [role="menu"], [role="listbox"]')].filter((e) => e.getClientRects().length);
        const top = open.at(-1);
        if (!top) return null;
        top.setAttribute("data-hb-layer", "");
        document.querySelectorAll("[data-hb-layer]").forEach((e) => e !== top && e.removeAttribute("data-hb-layer"));
        return "[data-hb-layer]";
      }).catch(() => null);
      const controls = await page.evaluate(`${MEASURE}(${JSON.stringify(layer)})`).catch(() => []);
      // 44 px where the design promises it (coarse pointer or a phone-width
      // layout, shell.css); 24 px (WCAG 2.5.8) for a fine pointer.
      for (const c of controls) if (coarse || narrow ? !c.ok44 : !c.ok24) problems.push({ kind: "target", detail: `${c.sel} "${c.name}" ${c.w}x${c.h} (need ${coarse || narrow ? 44 : 24})` });
      if (axe) for (const v of await runAxe(page)) problems.push({ kind: "axe", detail: v });
      if (focus) { const f = await focusCheck(page); if (f) problems.push({ kind: "focus", detail: f }); }
      for (const e of events.slice(before)) problems.push({ kind: e.kind, detail: e.text });
      const record = { engine, profile: profileName, unit, flow: ctx.flow, state: label, url: page.url().replace(/^https?:\/\/[^/]+/, ""), controls: controls.length, coarse, problems };
      results.states.push(record);
      if (problems.length) { await h.shot(`${ctx.flow}-${label}`); console.log(`  ! ${ctx.flow} / ${label}: ${problems.length} problem(s): ${problems.slice(0, 4).map((p) => p.kind + " " + String(p.detail).slice(0, 140)).join(" | ")}`); }
      return record;
    },
    /** Console errors matching `re` are expected while `fn` runs. */
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

// Two Tab presses; each focused element must match :focus-visible, show an
// outline or a box shadow, and be on screen.
async function focusCheck(page) {
  const bad = [];
  for (let i = 0; i < 2; i++) {
    await page.keyboard.press("Tab");
    await page.waitForTimeout(60);
    const r = await page.evaluate(() => {
      let el = document.activeElement;
      while (el?.shadowRoot?.activeElement) el = el.shadowRoot.activeElement;
      if (!el || el === document.body || el === document.documentElement) return null;
      const s = getComputedStyle(el), b = el.getBoundingClientRect();
      const ring = (s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0 && !/rgba\(.*,\s*0\)$/.test(s.outlineColor)) || (s.boxShadow && s.boxShadow !== "none");
      const onScreen = b.bottom > 0 && b.right > 0 && b.top < innerHeight && b.left < innerWidth;
      const name = `${el.tagName.toLowerCase()}${el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\s+/).slice(0, 2).join(".") : ""} "${(el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 30)}"`;
      return { name, visible: el.matches(":focus-visible"), ring: !!ring, onScreen };
    }).catch(() => null);
    if (!r) continue;
    if (!r.visible || !r.ring) bad.push(`${r.name}: ${!r.visible ? "not :focus-visible" : "no outline or shadow"}`);
    else if (!r.onScreen) bad.push(`${r.name}: focused off screen`);
  }
  return bad.join("; ");
}

// Sideways scroll, clipped controls and (under reduced motion) any running
// transition or animation.
const LAYOUT = `(() => {
  const vw = innerWidth, d = document.documentElement;
  const overflow = d.scrollWidth > vw + 1 ? "document " + d.scrollWidth + " > " + vw : document.body.scrollWidth > vw + 1 ? "body " + document.body.scrollWidth + " > " + vw : "";
  const tag = (e) => e.tagName.toLowerCase() + (typeof e.className === "string" && e.className.trim() ? "." + e.className.trim().split(/\\s+/).slice(0, 2).join(".") : "");
  const hidden = (el) => { const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) return true; const s = getComputedStyle(el); if (s.visibility !== "visible" || +s.opacity === 0) return true; return !!el.closest("[inert],[aria-hidden=true],[hidden]"); };
  const clipped = [];
  for (const el of document.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=switch],[role=combobox],[role=menuitem],[role=option]')) {
    if (hidden(el) || el.classList.contains("skip")) continue;
    const r = el.getBoundingClientRect(); let why = "", scrolls = false;
    for (let p = el.parentElement; p && p !== d && p !== document.body; p = p.parentElement) {
      const s = getComputedStyle(p);
      if (s.overflowX === "visible" && s.overflowY === "visible") continue;
      const sx = /auto|scroll/.test(s.overflowX) && p.scrollWidth > p.clientWidth, sy = /auto|scroll/.test(s.overflowY) && p.scrollHeight > p.clientHeight;
      if (sx || sy) { scrolls = true; break; }
      const q = p.getBoundingClientRect();
      const w = Math.min(r.right, q.right) - Math.max(r.left, q.left), h = Math.min(r.bottom, q.bottom) - Math.max(r.top, q.top);
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
})()`;
