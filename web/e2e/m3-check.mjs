// Start fake-instance.sh with --m2-fixtures. Pass its app and fixture URLs.
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, rejectLiveUrl, sleep } from "./cdp.mjs";

const [base, fixture] = process.argv.slice(2, 4).map((url) => new URL(url));
const password = process.env.HB_PASSWORD;
const out = process.env.OUT;
for (const url of [base, fixture]) {
  rejectLiveUrl(url.href);
  assert.equal(url.hostname, "127.0.0.1");
  assert.notEqual(url.port, "3300");
}
const jsonHeaders = { Origin: base.origin, "Content-Type": "application/json" };
let cookie = process.env.HB_COOKIE;
assert.ok((cookie || password?.length >= 8) && out, "Supply HB_PASSWORD or a fixture HB_COOKIE, and OUT");
if (!cookie) {
  const session = await fetch(new URL("/api/v1/session", base)).then((r) => r.json());
  const signIn = await fetch(new URL(session.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", base), {
    method: "POST", headers: jsonHeaders, body: JSON.stringify({ username: "m3-owner", password }),
  });
  assert.ok(signIn.ok, "Fixture owner can sign in");
  cookie = signIn.headers.get("set-cookie").split(";")[0];
}
const { csrf } = await fetch(new URL("/api/v1/session", base), { headers: { Cookie: cookie } }).then((r) => r.json());
assert.ok(csrf, "Fixture session is authenticated");
const api = (method, route, body) => fetch(new URL(`/api/v1${route}`, base), {
  method, headers: { ...jsonHeaders, Cookie: cookie, "X-RH-CSRF": csrf },
  ...(body === undefined ? {} : { body: JSON.stringify(body) }),
});
const fixtureState = async (state) => {
  assert.equal((await fetch(new URL(`/fixture/m2/${state}`, fixture), { method: "PUT" })).status, 204);
};
const el = (selector) => `document.querySelector(${q(selector)})`;
const attr = (id) => `[data-testid="${id}"]`;
const dialog = attr("integration-form");
let checks = 0;
let shots = 0;

for (const width of [390, 1440]) {
  for (const theme of ["light", "dark"]) {
    const profile = `${width}-${theme}`;
    const created = await api("POST", "/boards", { name: `M3 ${profile}` });
    assert.equal(created.status, 201);
    const board = (await created.json()).id;
    const browser = await launch({ width, height: 900 });
    try {
      const tab = await browser.tab();
      const user = input(tab);
      const events = recorder(tab, base.origin);
      user.touch = width === 390;
      for (const domain of ["Page", "Runtime", "Network", "Log"]) await tab.send(`${domain}.enable`);
      await tab.send("Runtime.addBinding", { name: "__hbCsp" });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem("rh-theme", ${q(theme)});` });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: user.touch });
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
      await tab.send("Network.setCookie", {
        name: cookie.slice(0, cookie.indexOf("=")), value: cookie.slice(cookie.indexOf("=") + 1), url: base.origin, httpOnly: true,
      });
      const check = async (expression, label) => {
        assert.ok(await tab.until(expression, 12000), `${profile} ${label}`);
        checks++;
      };
      const click = async (selector) => {
        await check(`!!${el(selector)}?.matches(":not(:disabled):not([aria-disabled=true])")`, selector);
        await user.click(selector);
      };
      const fill = async (selector, value) => {
        await click(selector);
        await user.key("a", { modifiers: 2 });
        await user.key("Backspace");
        await user.type(value);
        await check(`${el(selector)}?.value === ${q(value)}`, `fill ${selector}`);
      };
      const shot = async (name) => {
        await check("document.documentElement.scrollWidth <= innerWidth + 1", "page fits viewport");
        await tab.shot(path.join(out, `${profile}-${name}.png`));
        shots++;
      };

      await tab.send("Page.navigate", { url: new URL(`/app/settings/integrations?board=${board}`, base).href });
      await check(`document.documentElement.dataset.theme === ${q(theme)}`, "theme applied");
      const catalogCode = 'performance.getEntriesByType("resource").some(entry => entry.name.includes("/assets/ProviderDialog-"))';
      await check(`!${catalogCode}`, "catalog code is deferred before opening");
      await click(attr("integration-create"));
      await check(`!!${el(dialog)} && ${catalogCode}`, "catalog opened from a lazy chunk");
      await fill("#provider-search", "zz-no-match");
      await check(`!${el(dialog + " .settings-row")}`, "search filters registry");
      await fill("#provider-search", "Generic");
      await check(`${el(dialog)}.textContent.includes("Utilities") && !${el(dialog + " .service-icon")}.matches("img")`, "category and opt-out monogram");
      await shot("catalog");
      await click(`${dialog} .settings-row button`);
      await fill("#integration-url", "http://127.0.0.1:8081/fixture/m2/reading");
      await fill("#integration-name", `Fixture ${profile}`);
      await fill('input[name="secret"]', "m3-fake-bearer");
      await check(`${el('input[name="secret"]')}.type === "password"`, "credential is masked");
      await fixtureState("fail");
      await click(attr("provider-draft-test"));
      await check(`${el(attr("provider-draft-verdict"))}?.textContent.includes("non-success")`, "actionable failure");
      await check(`${el("#integration-name")}.value === ${q(`Fixture ${profile}`)}`, "draft retained");
      await fixtureState("good");
      await click(attr("provider-draft-test"));
      await check(`${el(attr("provider-draft-verdict"))}?.textContent.includes("succeeded")`, "backend test succeeds");
      await shot("connection");
      await click(`${dialog} .dlg-ft .btn-default:not([type="submit"])`);
      await check(`${el(dialog + " [role=switch]")}?.getAttribute("aria-checked") === "true"`, "recommended widget selected");
      await fill('textarea[name="paths"]', "reading");
      await shot("widgets");
      await click('button[form="integration-form"][type="submit"]');
      await check(`!${el(dialog)}`, "atomic install saved");
      await tab.send("Page.navigate", { url: new URL(`/app/boards/${board}`, base).href });
      await check(`${el(attr("board-tile"))}?.textContent.includes("42 units")`, "live provider data on board");
      await shot("board");
      for (const kind of ["csp", "console", "exceptions", "failed"]) assert.deepEqual(events[kind], [], `${profile} ${kind}`);
      console.log(`PASS ${profile}`);
    } finally {
      browser.quit();
    }
    await api("DELETE", `/boards/${board}`);
    // Connection tests are limited per account to three attempts per ten seconds.
    if (profile !== "1440-dark") await sleep(10000);
  }
}
console.log(`PASS ${checks} checks, ${shots} screenshots`);
