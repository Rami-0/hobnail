// Run with HB_PASSWORD and scripts/e2e.sh against the fake instance on 3395–3399.
// The fake can use --churn 12 to produce lifecycle events for svc008.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, sleep } from "./cdp.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:3396");
assert.match(base.hostname, /^(127\.0\.0\.1|localhost)$/);
assert.match(base.port, /^339[5-9]$/);
const password = process.env.HB_PASSWORD;
assert.ok(password, "set HB_PASSWORD");
const out = process.env.OUT;
assert.ok(out, "set OUT to the screenshot directory");
fs.mkdirSync(out, { recursive: true });

const session = await fetch(new URL("/api/v1/session", base)).then((r) => r.json());
const signed = await fetch(new URL(session.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", base), {
  method: "POST",
  headers: { Origin: base.origin, "Content-Type": "application/json" },
  body: JSON.stringify({ username: "admin", password }),
});
assert.ok(signed.ok, `sign-in HTTP ${signed.status}`);
const rawCookie = signed.headers.get("set-cookie").split(";")[0];
const split = rawCookie.indexOf("=");
const csrf = (await signed.json()).csrf;
const headers = { Cookie: rawCookie, "X-RH-CSRF": csrf, Origin: base.origin, "Content-Type": "application/json" };
const api = async (route, body) => {
  const res = await fetch(new URL(route, base), { method: body ? "POST" : "GET", headers, body: body && JSON.stringify(body) });
  const text = await res.text();
  assert.ok(res.ok, `${route} HTTP ${res.status}: ${text}`);
  return JSON.parse(text);
};

const initial = await api("/api/settings/config");
const fixture = `${initial.text}
[[bookmarks]]
name = "Browser check bookmark"
url = "https://example.com"

[[alerts]]
name = "Browser check rule"
type = "container"
containers = ["svc004"]
on = ["stopped"]
for = "0s"

[channels.pages-hook]
type = "webhook"
url = "https://example.com/hook"

[[widgets]]
id = "pages-health"
type = "custom-api"
url = "http://127.0.0.1:3200/healthz"
output = "text"
template = "Health check: {{_text}}"
refresh = "3s"
`;
let browser;
const failures = [];
try {
  await api("/api/settings/config", { base_hash: initial.hash, text: fixture });
  browser = await launch();
  const sizes = (process.env.WIDTHS || "1440x900 390x844").split(/\s+/);
  const schemes = (process.env.SCHEMES || "dark light").split(/\s+/);
  for (const size of sizes) for (const scheme of schemes) {
    const [width, height = 900] = size.split("x").map(Number);
    const label = `${width}-${scheme}`;
    const tab = await browser.tab();
    const rec = recorder(tab, base.origin);
    const inp = input(tab);
    inp.touch = width < 700;
    const wait = async (expr, message, ms = 10000) => assert.ok(await tab.until(expr, ms), message);
    const exists = (selector) => `!!document.querySelector(${q(selector)})`;
    const ready = '#root[data-ready="1"]';
    const load = async (route, selector, shot) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`location.pathname === ${q(route.split("?")[0])} && ${exists(ready)} && ${exists(selector)}`, `render ${route}`);
      await wait(`document.documentElement.scrollWidth <= innerWidth + 1`, `no horizontal overflow at ${width} on ${route}`);
      if (shot) await tab.shot(path.join(out, `${label}-${shot}.png`));
    };
    try {
      for (const d of ["Page", "Runtime", "Network", "Log"]) await tab.send(d + ".enable");
      await tab.send("Runtime.addBinding", { name: "__hbCsp" });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      await tab.send("Network.setCookie", { name: rawCookie.slice(0, split), value: rawCookie.slice(split + 1), url: base.origin, httpOnly: true, sameSite: "Strict", path: "/" });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: inp.touch });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });

      await load("/app/overview", ".overview-map rect");
      await wait('document.querySelector("[data-testid=overview]")?.textContent.includes("Browser check bookmark")', "bookmark renders");
      await wait('document.querySelector("#w-pages-health")?.textContent.includes("Health check: ok")', "widget renders a structured body");
      await tab.shot(path.join(out, `${label}-overview.png`));

      const palette = '[data-testid="palette"] [role="combobox"]';
      await inp.click('[data-testid="palette-open"]');
      await wait(`document.activeElement?.matches(${q(palette)})`, "palette opens and focuses the query");
      await inp.type("no-local-result-xyz");
      await wait(`!document.querySelector(${q(palette)})?.hasAttribute("aria-activedescendant")`, "external search stays unselected");
      await inp.key("Enter");
      await wait(exists(palette), "Enter on no selection keeps the dialog open");
      await inp.key("Escape");
      await wait(`!${exists(palette)} && document.activeElement?.dataset.testid === "palette-open"`, "Escape restores focus");
      await inp.key("k", { modifiers: 2 });
      await wait(`document.activeElement?.matches(${q(palette)})`, "Ctrl+K opens and focuses the query");
      await inp.type("browser check bookmark");
      assert.equal(await tab.ev(`document.querySelector(${q(palette)})?.value`), "browser check bookmark");
      await wait('Array.from(document.querySelectorAll("[role=option]")).some((e) => e.textContent.includes("Browser check bookmark"))', "palette finds a bookmark");
      await inp.clear(palette);
      await inp.type("logs: svc002");
      await wait('Array.from(document.querySelectorAll("[role=option]")).some((e) => e.textContent.includes("Logs: svc002"))', "palette finds container logs action");
      await inp.key("k", { modifiers: 2 });
      await wait(`!${exists(palette)}`, "Ctrl+K toggles it closed");
      await inp.key("k", { modifiers: 4 });
      await wait(exists(palette), "Meta+K opens the palette");
      await inp.clear(palette);
      for (const key of ["ArrowDown", "ArrowUp", "End", "Home", "PageDown", "PageUp"]) {
        await inp.key(key);
        await wait(`!!document.querySelector(${q(palette)})?.getAttribute('aria-activedescendant')`, `palette ${key} selects a result`);
      }
      await inp.key("Escape");
      for (const [key,route] of [["o","overview"],["c","containers"],["l","logs"],["h","host"],["y","history"],["a","alerts"],["s","settings/general"]]) {
        await tab.ev('document.activeElement?.blur()');
        await inp.key('g'); await inp.key(key);
        await wait(`location.pathname === ${q('/app/'+route)}`, `g ${key} opens ${route}`);
      }
      await inp.key("?");
      await wait(exists('[data-testid="shortcuts"]'), "opens the shortcut sheet");
      await inp.key("Escape");
      await inp.key("g");
      await inp.key("a");
      await wait('location.pathname === "/app/alerts"', "g a opens Alerts");
      await wait('Number(localStorage.getItem("rh-seen")) > 0', "leaving Overview remembers the visit");

      await load("/app/containers", '[data-testid="row"]');
      await tab.ev('document.activeElement?.blur()');
      await inp.key('j'); await wait(`!!document.querySelector('tr.kfocus a:focus')`, "j focuses the first actionable row");
      const firstRow = await tab.ev(`document.querySelector('tr.kfocus').dataset.name`);
      await inp.key('j'); await wait(`document.querySelector('tr.kfocus')?.dataset.name !== ${q(firstRow)}`, "j advances a row");
      await inp.key('k'); await wait(`document.querySelector('tr.kfocus')?.dataset.name === ${q(firstRow)}`, "k returns to the previous row");
      await inp.key('Enter'); await wait(`location.pathname===${q('/app/containers/')}+${q(firstRow)}`, "Enter opens the focused row");
      await inp.key('l'); await wait(`location.pathname===${q('/app/containers/')}+${q(firstRow)}+'/logs'`, "l opens the current container logs");
      await load("/app/containers/svc001", '[data-testid="ctr-tabs"] a');
      await tab.ev(`document.querySelector('[data-testid="ctr-tabs"] a').focus()`);
      for (const key of ['ArrowRight','ArrowLeft','End','Home']) {
        await inp.key(key); await wait(`document.activeElement?.matches('[data-testid="ctr-tabs"] a')`, `detail tabs support ${key}`);
      }
      await inp.click('[data-testid="user"]');
      await wait(`!!document.querySelector('[role="menu"]')`, "account menu opens");
      for (const key of ['Home','End','ArrowUp','ArrowDown']) {
        await inp.key(key); await wait(`!!document.activeElement?.closest('[role="menu"]')`, `menu supports ${key}`);
      }
      await inp.key('Escape');
      await load("/app/alerts", ".history-events li");
      await wait(`document.querySelector(${q('[aria-label="Firing now"]')})?.textContent.includes("svc004")`, "firing alert arrives through the live model", 15000);
      await tab.shot(path.join(out, `${label}-alerts.png`));
      await wait('document.querySelector("[data-testid=alerts]")?.textContent.includes("Browser check rule")', "alert rule renders");
      await load("/app/history?range=1h&level=all", '[data-testid="chart-cpu"] .ch-canvas');
      await wait(exists('[data-testid="chart-net_tx"]'), "History chart group mounts");
      await tab.shot(path.join(out, `${label}-history.png`));
      const plot = await inp.box('[data-testid="chart-cpu"] .ch-canvas');
      assert.ok(plot, "CPU chart has a plot");
      const from = plot.left + plot.width * .28;
      const to = plot.left + plot.width * .77;
      const y = plot.top + 75;
      if (inp.touch) await inp.touchDrag(from, y, to, y);
      else await inp.drag(from, y, to, y);
      await wait('new URLSearchParams(location.search).has("from") && new URLSearchParams(location.search).has("to")', "History drag writes both bounds");

      await load("/app/host", '[data-testid="host-sample"]');
      const samples = '[data-testid="host-sample"]';
      const disclosure = '[data-testid="host-disclosure"] summary';
      await inp.click(disclosure);
      const sample = await tab.ev(`document.querySelector(${q(samples)}).dataset.sampledAt`);
      await wait(`document.querySelector(${q(samples)}).dataset.sampledAt !== ${q(sample)}`, "Host updates after the initial sample");
      await wait(`document.querySelector(${q(disclosure)}).parentElement.open`, "Host disclosure survives updates");
      await tab.shot(path.join(out, `${label}-host.png`));

      const root = "/app/containers/svc043";
      await load(root + "?range=24h&tab=metrics", '[data-testid="chart-pids"]', "metrics");
      assert.ok(await tab.ev(exists('[data-testid="why"]')), "Metrics keeps the Why banner");
      await inp.click('[data-testid="ctr-tabs"] a[href*="tab=inspect"]');
      await wait(exists('[data-testid="container-inspect"]'), "opens Inspect");
      assert.ok(await tab.ev(exists('[data-testid="why"]')), "Inspect keeps the Why banner");
      assert.equal(await tab.ev('new URLSearchParams(location.search).get("range")'), "24h");
      assert.equal(await tab.ev('document.body.textContent.includes("fake-not-a-secret")'), false);
      await tab.shot(path.join(out, `${label}-inspect.png`));
      await inp.click('[data-testid="reveal-env"]');
      await wait('document.body.textContent.includes("fake-not-a-secret")', "reveals environment on demand");
      await inp.click('[data-testid="reveal-env"]');
      await wait('!document.body.textContent.includes("fake-not-a-secret")', "hides environment");
      await inp.click('[data-testid="ctr-tabs"] a[href*="tab=processes"]');
      await wait(exists('[data-testid="container-processes"] tbody tr'), "process rows render");
      assert.ok(await tab.ev(exists('[data-testid="why"]')), "Processes keeps the Why banner");
      assert.equal(await tab.ev('new URLSearchParams(location.search).get("range")'), "24h");
      await tab.shot(path.join(out, `${label}-processes.png`));
      await inp.click('[data-testid="ctr-tabs"] a[href*="tab=events"]');
      await wait(exists('[data-testid="container-events"]'), "opens the container event page");
      assert.ok(await tab.ev(exists('[data-testid="why"]')), "Events keeps the Why banner");
      assert.equal(await tab.ev('new URLSearchParams(location.search).get("range")'), "24h");
      await tab.shot(path.join(out, `${label}-events.png`));
      const churn = await api("/api/v1/containers/svc008/events");
      if (churn.events.length) {
        await load("/app/containers/svc008?tab=events", '[data-testid="container-events"] .history-events li');
        await tab.shot(path.join(out, `${label}-events.png`));
      }
      await wait("document.documentElement.scrollWidth <= innerWidth + 1", "container tabs fit the viewport");
      await sleep(150);
    } catch (error) {
      failures.push(`${label} ${error.message}`);
      console.log(await tab.ev(`JSON.stringify({
        url: location.pathname + location.search,
        active: document.activeElement?.outerHTML.slice(0, 200),
        paletteQuery: document.querySelector("[data-testid=palette] input")?.value,
        paletteRows: Array.from(document.querySelectorAll("[data-testid=palette] [role=option]"), el => el.textContent).slice(0, 8)
      })`).catch(() => "Browser state unavailable"));
      await tab.shot(path.join(out, `${label}-failed.png`));
    } finally {
      const problems = [...rec.csp, ...rec.console, ...rec.exceptions, ...rec.failed];
      failures.push(...problems.map((p) => `${label} ${p}`));
      console.log(`${label} CSP=${rec.csp.length} console=${rec.console.length} exceptions=${rec.exceptions.length} failed requests=${rec.failed.length}`);
      await tab.close();
    }
  }
} finally {
  browser?.quit();
  const current = await api("/api/settings/config");
  if (current.hash !== initial.hash) await api("/api/settings/config", { base_hash: current.hash, text: initial.text });
}
for (const failure of failures) console.error(`FAIL ${failure}`);
if (failures.length) process.exitCode = 1;
else console.log("All read-page browser checks passed.");
