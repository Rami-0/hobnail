// Integrations as API connections, with local container detection: the
// board Add picker (Hobnail widgets, Apps, Integrations and Containers tabs, logos, "Detected on
// this host" cards), Connect opening the provider's connect step with the
// URL filled in and the credential focused, and the Settings integration
// cards. At 1440 dark and light and 390 dark, against a throwaway fake
// instance started with `fake-docker.mjs --services`:
//   HB_PASSWORD=... E2E_ENV=INSTALLS INSTALLS='[...]' scripts/e2e.sh web/e2e/integrations-check.mjs http://127.0.0.1:3396
// INSTALLS: as in gallery-check.mjs (fake services only). Screenshots:
// OUT/C-<page>-<width>-<scheme>.png. Fails on CSP violations, console
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
ok(await call("/api/v1/account/board-tour", "POST", undefined, owner), "dismiss the board tour");
for (const spec of installs) {
  ok(await call("/api/v1/providers/install", "POST", {
    integration: { kind: spec.kind, name: spec.name, base_url: spec.base_url, enabled: true, options: spec.options ?? {}, credentials: spec.credentials ?? {} },
    board_id: board.id, widgets: spec.widgets.map((id) => ({ id, options: spec.widget_options?.[id] ?? {} })),
  }, owner), `install ${spec.kind}`);
}

// Detection, through the API: real-looking images match, plain ones do not,
// and the proposed URLs follow the published port, then the shared network.
const detected = ok(await call("/api/v1/providers/detected", "GET", undefined, owner), "detected");
const by = Object.fromEntries(detected.map((d) => [d.provider, d]));
assert.equal(by.sonarr?.candidates[0]?.url, "http://192.168.1.50:8989", "Sonarr: published port on its LAN address");
assert.equal(by.pihole?.candidates[0]?.url, "http://192.168.1.50:8053", "Pi-hole: the published web port, not DNS");
assert.equal(by.radarr?.matched, "name", "a renamed Radarr is found by its container name");
assert.equal(by.radarr?.candidates.length, 0, "a loopback-only port is never proposed");
assert.ok(!detected.some((d) => ["postgres", "redis", "hobnail"].includes(d.container)), "plain services and Hobnail itself are not integrations");
console.log("detected:", detected.map((d) => `${d.provider}(${d.container}) ${d.candidates.map((c) => `${c.via}:${c.url}`).join(" ") || "-"}`).join("; "));

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
      await sleep(300);
      const overflow = await tab.ev("document.documentElement.scrollWidth - document.documentElement.clientWidth");
      if (overflow > 1) failures.push(`${label} ${name}: page overflows by ${overflow}px`);
      await tab.shot(path.join(out, `C-${name}-${label}.png`));
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
    const logos = (scope) => `[...document.querySelectorAll(${q(`${scope} .brand-icon img`)})]`;
    const logosLoaded = (scope, min) => wait(`${logos(scope)}.filter((img) => img.complete && img.naturalWidth > 0).length >= ${min}`, `${scope}: ${min}+ logos load`);
    const footer = async (caption) => {
      await wait(`[...document.querySelectorAll(${q(`${dialog} .dlg-ft button`)})].some((b) => b.textContent.trim() === ${q(caption)})`, `${caption} button`);
      await tab.ev(`[...document.querySelectorAll(${q(`${dialog} .dlg-ft button`)})].find((b) => b.textContent.trim() === ${q(caption)}).click()`);
      await frame();
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

      await call(`/api/v1/boards/${board.id}/draft`, "DELETE", undefined, owner);
      // The board Add picker opens on Integrations: detected services with
      // logos, the connected integrations, and Connect a service.
      await go(`/app/boards/${board.id}`, '!!document.querySelector(\'[data-testid="board"]\')');
      await inp.click('[data-testid="board-edit"]');
      await wait('!!document.querySelector(\'[data-testid="board-editor"]\')', "editor");
      await inp.click('[data-testid="board-add-tile"]');
      // It opens on Hobnail widgets: live widgets from Hobnail's own data,
      // shown as gallery previews with Add.
      await wait(`!!document.querySelector('[data-testid="picker-tab-widgets"][aria-pressed="true"]')`, "picker opens on Hobnail widgets");
      await wait(`document.querySelectorAll('[data-testid="hobnail-widgets"] [data-testid="hobnail-add"]').length === 6`, "six Hobnail widgets");
      await shot("picker-hobnail");
      if (scheme === "dark" && width === 1440) {
        // Adding one installs the built-in integration and lands it live in the edit.
        await inp.click('[data-testid="hobnail-add"][data-widget="host"]');
        await wait(`!document.querySelector('[data-testid="board-picker"][data-open]')`, "picker closes after Add");
        await wait(`!!document.querySelector('.hobnail-widget[data-view="host"] .sw-tiles')`, "host overview draws live tiles", 20000);
        await inp.click('[data-testid="board-add-tile"]');
        await wait(`!!document.querySelector('[data-testid="board-picker"][data-open]')`, "picker reopens");
      }
      // Apps: web UIs running on this host as branded links on their
      // published address; databases and Hobnail itself are not apps.
      await inp.click('[data-testid="picker-tab-apps"]');
      await wait(`document.querySelectorAll('[data-testid="board-suggestion"][data-kind="link"]').length >= 4`, "detected app links");
      await logosLoaded('[data-testid="board-picker"] [data-testid="picker-list"]', 4);
      const appText = await tab.ev(`[...document.querySelectorAll('[data-testid="board-suggestion"][data-kind="link"]')].map((b) => b.textContent).join(" | ")`);
      assert.match(appText, /Sonarr.*192\.168\.1\.50:8989/, `${label}: Sonarr app on its published address (${appText})`);
      assert.ok(!/postgres|redis|hobnail/i.test(appText), `${label}: no database or Hobnail among apps (${appText})`);
      assert.match(await tab.ev(`document.querySelector('[data-testid="board-picker"] :is(h1,h2)')?.textContent ?? ""`), /^Add to Homelab$/, `${label}: the dialog names the board`);
      await wait(`!!document.querySelector('[data-testid="picker-add-all"]')`, "Add all");
      await shot("picker-apps");
      await inp.click('[data-testid="picker-tab-integrations"]');
      await wait(`document.querySelectorAll('[data-testid="detected-service"]').length >= 4`, "detected cards");
      await logosLoaded('[data-testid="board-picker"] [data-testid="detected"]', 4);
      if (installs.length) await logosLoaded('[data-testid="board-picker"] [data-testid="picker-list"]', 1);
      const cards = await tab.ev(`[...document.querySelectorAll('[data-testid="detected-service"]')].map((c) => c.dataset.provider)`);
      assert.ok(cards.includes("sonarr") && cards.includes("pihole") && cards.includes("jellyfin"), `${label}: Sonarr, Pi-hole and Jellyfin detected (${cards})`);
      assert.ok(!(await tab.ev(`!!document.querySelector('[data-testid="board-picker"] [data-kind="container"]')`)), `${label}: no container is presented as an integration`);
      await shot("picker-integrations");

      // Containers: plain status tiles, with real logos picked from the image.
      await inp.click('[data-testid="picker-tab-containers"]');
      await wait(`document.querySelectorAll('[data-testid="board-suggestion"][data-kind="container"]').length >= 5`, "container suggestions");
      await logosLoaded('[data-testid="board-picker"] [data-testid="picker-list"]', 5);
      const sonarrLogo = await tab.ev(`!![...document.querySelectorAll('[data-kind="container"]')].find((b) => b.textContent.includes("sonarr"))?.querySelector('.brand-icon img[src$="/sonarr"]')`);
      assert.ok(sonarrLogo, `${label}: the sonarr container shows the Sonarr logo`);
      const postgresLogo = await tab.ev(`!![...document.querySelectorAll('[data-kind="container"]')].find((b) => b.textContent.includes("postgres"))?.querySelector('.brand-icon img[src$="/postgres"]')`);
      assert.ok(postgresLogo, `${label}: the postgres container shows the PostgreSQL logo`);
      await shot("picker-containers");

      // Connect a detected service: its connect step, URL filled in, credential focused.
      await inp.click('[data-testid="picker-tab-integrations"]');
      await wait(`!!document.querySelector('[data-testid="detected-service"][data-provider="sonarr"] [data-testid="detected-connect"]')`, "Sonarr card");
      await inp.click('[data-testid="detected-service"][data-provider="sonarr"] [data-testid="detected-connect"]');
      await wait(`!!document.querySelector(${q(`${dialog} .provider-head`)})?.textContent.includes("Sonarr")`, "Sonarr connect step");
      await wait(`document.querySelector(${q(`${dialog} input[name="base_url"]`)})?.value === "http://192.168.1.50:8989"`, "URL prefilled");
      await wait(`document.activeElement?.name === "api_key"`, "focus on the API key");
      const hint = await tab.ev(`document.querySelector(${q(`${dialog} input[name="api_key"]`)})?.closest(".fld")?.textContent ?? ""`);
      assert.match(hint, /Settings/, `${label}: the API key field says where to find the key`);
      await logosLoaded(dialog, 1);
      await shot("connect-prefilled");
      await footer("Back");
      await wait(`!!document.querySelector(${q(`${dialog} .catalog-grid`)})`, "catalog");
      await wait(`!!document.querySelector(${q(`${dialog} [data-testid="detected"]`)})`, "catalog shows detected services");
      await shot("add-integration-detected");
      await footer("Cancel");
      await wait(`!!document.querySelector('[data-testid="board-picker"][data-open]')`, "picker returns after cancel");

      // Dismiss hides a suggestion (per browser).
      const before = await tab.ev(`document.querySelectorAll('[data-testid="board-picker"] [data-testid="detected-service"]').length`);
      await inp.click('[data-testid="board-picker"] [data-testid="detected-service"][data-provider="uptime-kuma"] [data-testid="detected-dismiss"]');
      await wait(`document.querySelectorAll('[data-testid="board-picker"] [data-testid="detected-service"]').length === ${before - 1}`, "dismissed");
      await tab.ev(`localStorage.removeItem("rh-detect-dismissed")`);
      await inp.key("Escape");
      await wait(`!document.querySelector('[data-testid="board-picker"][data-open]')`, "picker closed");
      await inp.click('[data-testid="board-cancel"]');
      await wait('!!document.querySelector(\'[data-testid="board"]\')', "editor cancelled");

      // Settings → Integrations: detected cards and a branded card per integration.
      await go("/app/settings/integrations", '!!document.querySelector(\'[data-testid="integration-create"]\')');
      await wait(`document.querySelectorAll('[data-testid="detected-service"]').length >= 4`, "settings detected cards");
      // Phones show two detected cards until asked for the rest.
      const visibleCards = `[...document.querySelectorAll('[data-testid="settings-integrations"] [data-testid="detected-service"]')].filter((c) => c.offsetParent).length`;
      if (width < 700) {
        assert.equal(await tab.ev(visibleCards), 2, `${label}: two detected cards on a phone`);
        await full("integrations-list-capped");
        await inp.click('[data-testid="settings-integrations"] [data-testid="detected-more"]');
        await wait(`${visibleCards} >= 4`, "Show more reveals the rest");
      } else assert.ok((await tab.ev(visibleCards)) >= 4, `${label}: all detected cards on a desktop`);
      assert.ok(await tab.ev(`!document.querySelector('[data-testid="provider-contacts"]')?.open`), `${label}: provider destinations are collapsed`);
      if (installs.length) {
        await wait(`document.querySelectorAll('[data-testid="integration-row"]').length >= ${installs.length}`, "integration cards");
        await logosLoaded('[data-testid="settings-integrations"] .integration-grid', installs.length);
        const actions = await tab.ev(`[...document.querySelector('[data-testid="integration-row"] footer')?.querySelectorAll("button") ?? []].map((b) => b.textContent.trim())`);
        assert.deepEqual(actions, ["Edit", "Test", "Add widget", "Remove"], `${label}: card actions`);
      }
      await full("integrations-list");
      for (const kind of ["csp", "console", "exceptions", "failed"]) for (const entry of rec[kind]) failures.push(`${label} ${kind}: ${entry}`);
    } catch (error) {
      failures.push(`${label}: ${error.message}`);
      await tab.shot(path.join(out, `C-FAIL-${label}.png`)).catch(() => {});
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
console.log("integrations-check: ok");
