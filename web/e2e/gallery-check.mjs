// Branded catalog, connect pages, widget gallery and a board of provider
// widgets, at 1440 dark and light and 390 dark. Run against a throwaway fake
// instance through scripts/e2e.sh:
//   HB_PASSWORD=... E2E_ENV=INSTALLS INSTALLS='[...]' scripts/e2e.sh web/e2e/gallery-check.mjs http://127.0.0.1:3396
// INSTALLS (optional) is a JSON array of {kind, name, base_url, options?,
// credentials?, widgets: [id...]} installed on a fresh board through
// POST /api/v1/providers/install, all against fake services. Screenshots:
// OUT/B-<page>-<width>-<scheme>.png. Fails on CSP violations, console
// errors, exceptions, failed requests and horizontal page overflow.
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, sleep } from "./cdp.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:3396");
assert.match(base.hostname, /^(127\.0\.0\.1|localhost)$/);
assert.ok(/^339[5-9]$/.test(base.port), "use a throwaway instance on 3395-3399");
const password = process.env.HB_PASSWORD;
const out = process.env.OUT;
assert.ok(password && out, "set HB_PASSWORD and OUT");
const installs = JSON.parse(process.env.INSTALLS || "[]");

async function call(route, method = "GET", body, session) {
  const response = await fetch(new URL(route, base), {
    method,
    headers: { Origin: base.origin, "Content-Type": "application/json", ...(session ? { Cookie: session.cookie, "X-RH-CSRF": session.csrf } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  let data;
  try { data = JSON.parse(text); } catch { /* not JSON */ }
  return { status: response.status, data, text, cookie: response.headers.get("set-cookie")?.split(";")[0] };
}
const ok = (r, what) => { assert.ok(r.status >= 200 && r.status < 300, `${what}: HTTP ${r.status} ${r.text.slice(0, 200)}`); return r.data; };
const boot = ok(await call("/api/v1/session"), "session");
const login = await call(boot.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", "POST", { username: "admin", password });
const owner = { cookie: login.cookie, csrf: ok(login, "sign in").csrf };

const board = ok(await call("/api/v1/boards", "POST", { name: "Homelab" }, owner), "create board");
for (const spec of installs) {
  ok(await call("/api/v1/providers/install", "POST", {
    integration: { kind: spec.kind, name: spec.name, base_url: spec.base_url, enabled: true, options: spec.options ?? {}, credentials: spec.credentials ?? {} },
    board_id: board.id, widgets: spec.widgets.map((id) => ({ id, options: spec.widget_options?.[id] ?? {} })),
  }, owner), `install ${spec.kind}`);
}
const integrations = ok(await call("/api/v1/integrations", "GET", undefined, owner), "integrations");
const pihole = integrations.find((i) => i.kind === "pihole");

const browser = await launch();
const failures = [];
const dialog = '[role="dialog"][data-open]';
try {
  for (const [width, scheme] of [[1440, "dark"], [1440, "light"], [390, "dark"]]) {
    const label = `${width}-${scheme}`;
    const tab = await browser.tab();
    const inp = input(tab);
    inp.touch = width < 700;
    const rec = recorder(tab, base.origin);
    const wait = async (expression, message, ms = 10000) => assert.ok(await tab.until(expression, ms), `${label}: ${message}`);
    const frame = () => tab.ev("new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))");
    const shot = async (name) => {
      await frame();
      await sleep(250);
      const overflow = await tab.ev("document.documentElement.scrollWidth - document.documentElement.clientWidth");
      if (overflow > 1) failures.push(`${label} ${name}: page overflows by ${overflow}px`);
      await tab.shot(path.join(out, `B-${name}-${label}.png`));
    };
    const full = async (name) => {
      const height = await tab.ev("Math.ceil(document.documentElement.scrollHeight)");
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: Math.min(height, 4000), deviceScaleFactor: 1, mobile: width < 700 });
      await shot(name);
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width < 700 ? 844 : 900, deviceScaleFactor: 1, mobile: width < 700 });
    };
    const go = async (route, expression) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`location.pathname === ${q(route)} && (${expression})`, route);
    };
    const fill = async (selector, value) => {
      await inp.clear(selector);
      await tab.send("Input.insertText", { text: value });
      await frame();
    };
    const footer = async (caption) => {
      await tab.ev(`[...document.querySelectorAll(${q(`${dialog} .dlg-ft button`)})].find((b) => b.textContent.trim() === ${q(caption)}).click()`);
      await frame();
    };
    const choose = async (name) => {
      await fill(`${dialog} input[type=search]`, name);
      await inp.click(`${dialog} .catalog-card`);
      await wait(`!!document.querySelector(${q(`${dialog} .provider-head`)})?.textContent.includes(${q(name)})`, `${name} connect page`);
      await wait(`[...document.querySelectorAll(${q(`${dialog} .brand-icon img`)})].every((img) => img.complete && img.naturalWidth > 0)`, `${name} logos load`);
    };
    try {
      for (const domain of ["Page", "Network", "Runtime", "Log"]) await tab.send(domain + ".enable");
      await tab.send("Runtime.addBinding", { name: "__hbCsp" });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      const equals = owner.cookie.indexOf("=");
      await tab.send("Network.setCookie", { name: owner.cookie.slice(0, equals), value: owner.cookie.slice(equals + 1), url: base.origin, httpOnly: true, sameSite: "Strict", path: "/" });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width < 700 ? 844 : 900, deviceScaleFactor: 1, mobile: width < 700 });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });

      await go("/app/settings/integrations", '!!document.querySelector(\'[data-testid="integration-create"]\')');
      await inp.click('[data-testid="integration-create"]');
      await wait(`document.querySelectorAll(${q(`${dialog} .catalog-card`)}).length > 30`, "catalog cards");
      await wait(`[...document.querySelectorAll(${q(`${dialog} .catalog-card .brand-icon img`)})].filter((img) => img.complete && img.naturalWidth > 0).length > 30`, "catalog logos load");
      const counts = await tab.ev(`[...document.querySelectorAll(${q(`${dialog} .catalog-count`)})].map((e) => e.textContent)`);
      assert.ok(counts.every((t) => /^\d+ widgets?$/.test(t)), `${label}: every card counts its widgets`);
      await shot("catalog");

      await choose("Pi-hole");
      const fields = () => tab.ev(`[...document.querySelectorAll(${q(`${dialog} input[name]`)})].map((i) => i.name)`);
      assert.ok((await fields()).includes("password") && !(await fields()).includes("api_token"), `${label}: v6 shows only the password`);
      await shot("connect-pihole");
      await inp.click(`${dialog} [aria-label="API version"]`);
      await inp.click('[role="option"][data-value="v5"]');
      await wait(`!!document.querySelector(${q(`${dialog} input[name="api_token"]`)}) && !document.querySelector(${q(`${dialog} input[name="password"]`)})`, "v5 shows only the token");
      await footer("Back");

      await wait(`!!document.querySelector(${q(`${dialog} input[type=search]`)})`, "back to catalog");
      await choose("Sonarr");
      await shot("connect-sonarr");
      await footer("Back");
      await footer("Cancel");
      await wait(`!document.querySelector(${q(dialog)})`, "dialog closed");

      if (pihole) {
        await tab.ev(`[...[...document.querySelectorAll('[data-testid="integration-row"]')].find((r) => r.textContent.includes(${q(pihole.name)})).querySelectorAll("button")].find((b) => b.textContent.trim() === "Edit").click()`);
        await wait(`!!document.querySelector(${q(dialog)})`, "edit dialog");
        await footer("Choose widgets");
        await wait(`document.querySelectorAll(${q(`${dialog} .widget-gallery .widget-preview`)}).length >= 3`, "gallery cards");
        await shot("gallery");
        await footer("Back");
        await footer("Cancel");
      }

      if (installs.length) {
        await go(`/app/board/${board.id}`, '!!document.querySelector(\'[data-testid="board"]\')');
        await wait(`document.querySelectorAll('[data-testid="board-tile"] .widget-content [class^="sw-"]').length >= ${Math.min(6, installs.length)}`, "provider widgets render", 30000);
        await sleep(1500);
        await full("board");
      }
      for (const kind of ["csp", "console", "exceptions", "failed"]) for (const entry of rec[kind]) failures.push(`${label} ${kind}: ${entry}`);
    } catch (error) {
      failures.push(`${label}: ${error.message}`);
      await tab.shot(path.join(out, `B-FAIL-${label}.png`)).catch(() => {});
    } finally {
      await tab.close();
    }
  }
} finally {
  browser.quit();
}
if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("gallery-check: ok");
