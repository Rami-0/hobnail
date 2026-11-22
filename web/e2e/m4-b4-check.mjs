// Run against a throwaway instance with [http] allow_hosts = ["127.0.0.1"].
import assert from "node:assert/strict";
import { createPrivateKey, X509Certificate } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer as httpServer } from "node:http";
import { createServer as tlsServer } from "node:tls";
import { CSP_LISTENER, launch, q, recorder, rejectLiveUrl, sleep } from "./cdp.mjs";

const base = new URL(process.argv[2]);
rejectLiveUrl(base.href);
assert.equal(base.hostname, "127.0.0.1");
assert.notEqual(base.port, "3300");
const password = process.env.HB_PASSWORD;
const out = process.env.OUT;
assert.ok(password?.length >= 8 && out);

const fixtures = new URL("../../src/http/testdata/", import.meta.url);
const tls = tlsServer({
  cert: new X509Certificate(readFileSync(new URL("integration-expired.der", fixtures))).toString(),
  key: createPrivateKey({ key: readFileSync(new URL("integration-test-key.der", fixtures)), format: "der", type: "pkcs8" }).export({ format: "pem", type: "pkcs8" }),
}, (socket) => socket.end());
const rss = httpServer((req, res) => {
  assert.equal(req.url, "/feed");
  res.setHeader("Content-Type", "application/rss+xml");
  res.end(`<rss version="2.0"><channel><item><title>Fixture &amp; news</title><link>https://example.test/story</link><description><![CDATA[Latest <updates>]]></description></item></channel></rss>`);
});
const listen = (server) => new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
const [tlsPort, rssPort] = await Promise.all([listen(tls), listen(rss)]);

let checks = 0;
try {
  const headers = { Origin: base.origin, "Content-Type": "application/json" };
  const signIn = await fetch(new URL("/api/v1/onboarding/owner", base), {
    method: "POST", headers, body: JSON.stringify({ username: "fixture-owner", password }),
  });
  assert.ok(signIn.ok, "Fixture owner created");
  const cookie = signIn.headers.get("set-cookie").split(";")[0];
  const { csrf } = await fetch(new URL("/api/v1/session", base), { headers: { Cookie: cookie } }).then((res) => res.json());
  assert.ok(csrf);
  const api = async (route, body, method = "POST") => {
    const res = await fetch(new URL(`/api/v1${route}`, base), {
      method, headers: { ...headers, Cookie: cookie, "X-RH-CSRF": csrf }, body: JSON.stringify(body),
    });
    assert.ok(res.ok, `${route}: ${res.status} ${await res.clone().text()}`);
    return res.status === 204 ? undefined : res.json();
  };
  await api("/account/board-tour");
  const { id: board_id } = await api("/boards", { name: "Home" });
  const install = (kind, name, id, options, base_url = "") => api("/providers/install", {
    integration: { kind, name, base_url, enabled: true }, board_id, widgets: [{ id, options }],
  });
  await install("clock", "Time", "clock", { zones: ["UTC | Universal", "Asia/Tokyo | Tokyo"], calendar: true });
  await install("bookmarks", "Reading", "links", { links: ["Hobnail guide | https://example.test/docs | github", "Local router | http://192.168.1.1/"] });
  await install("markdown_note", "Instructions", "note", {
    note: "# Household\nRead **the manual** and use `restart`.\nNext line.\n\n- First\n- Second\n\n[Email](mailto:help@example.test)\n[Unsafe](javascript:alert(1))\n<img src=x onerror=alert(1)>\n<script>alert(2)</script>",
  });
  await install("ssl_expiry", "TLS fixture", "expiry", {}, `https://127.0.0.1:${tlsPort}`);
  await install("feed", "RSS fixture", "items", {}, `http://127.0.0.1:${rssPort}/feed`);

  const boardPath = `/boards/${board_id}`;
  const { document } = await api(boardPath, undefined, "GET");
  const first = document.sections[0];
  await api(boardPath, {
    ...document,
    sections: [{
      ...first,
      title: "Home utilities",
      items: document.sections.flatMap((section) => section.items).map((item, position) => ({
        ...item, position, spans: { ...item.spans, desktop: 2 },
      })),
    }],
  }, "PUT");

  for (const [width, theme, reduced] of [[1440, "light", false], [390, "dark", true]]) {
    const browser = await launch({ width, height: 900 });
    const profile = `${width}-${theme}`;
    try {
      const tab = await browser.tab();
      const events = recorder(tab, base.origin);
      for (const domain of ["Page", "Runtime", "Network", "Log"]) await tab.send(`${domain}.enable`);
      await tab.send("Runtime.addBinding", { name: "__hbCsp" });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem("rh-theme", ${q(theme)});` });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: width < 500 });
      await tab.send("Emulation.setEmulatedMedia", { features: [
        { name: "prefers-color-scheme", value: theme },
        { name: "prefers-reduced-motion", value: reduced ? "reduce" : "no-preference" },
      ] });
      const [cookieName, ...value] = cookie.split("=");
      await tab.send("Network.setCookie", { name: cookieName, value: value.join("="), url: base.origin, httpOnly: true });
      await tab.send("Page.navigate", { url: new URL(`/app/boards/${board_id}`, base).href });
      const check = async (expression, label) => {
        assert.ok(await tab.until(expression, 15000), `${profile}: ${label}`);
        checks++;
      };
      const tile = (title) => `[...document.querySelectorAll('[data-testid="board-tile"]')].find(element => element.querySelector('h3').textContent.startsWith(${q(title)}))`;
      const clock = tile("Time");
      await check(`document.documentElement.dataset.theme === ${q(theme)} && document.documentElement.scrollWidth <= innerWidth + 1`, "theme and responsive width");
      await check(`(() => { const tile = ${clock}; return tile?.querySelectorAll("dd").length === 2 && tile.textContent.includes("Universal") && tile.textContent.includes("Tokyo") && !!tile.querySelector(".widget-month [aria-current=date]"); })()`, "two clock zones and current day");
      const time = `(${clock}).querySelector("dd").textContent`;
      const firstTime = await tab.ev(time);
      assert.equal(firstTime.split(":").length, reduced ? 2 : 3);
      checks++;
      if (!reduced) {
        await check(`${time} !== ${q(firstTime)}`, "clock ticks");
        await tab.ev(`Object.defineProperty(document, "hidden", { configurable: true, value: true }); document.dispatchEvent(new Event("visibilitychange"));`);
        const hiddenTime = await tab.ev(time);
        await sleep(1300);
        assert.equal(await tab.ev(time), hiddenTime, "clock pauses when hidden");
        checks++;
        await tab.ev(`delete document.hidden; document.dispatchEvent(new Event("visibilitychange"));`);
        await check(`${time} !== ${q(hiddenTime)}`, "clock resumes when visible");
      }
      await check(`(() => { const tile = ${tile("Instructions")}; return tile?.querySelector("h1")?.textContent === "Household" && tile.querySelector("strong")?.textContent === "the manual" && tile.querySelector("br") && tile.querySelectorAll("ul li").length === 2 && tile.querySelector("a[href^='mailto:']") && !tile.querySelector("script, img[onerror], a[href^='javascript:']") && tile.textContent.includes("<img src=x onerror=alert(1)>") && tile.textContent.includes("<script>alert(2)</script>"); })()`, "markdown semantics and hostile HTML stays literal");
      await check(`(() => { const tile = ${tile("Reading")}; return tile?.querySelectorAll("li a").length === 2 && !/healthy|up|down/i.test(tile.querySelector("ul").textContent); })()`, "bookmark labels, links and no health claim");
      await check(`(() => { const text = (${tile("TLS fixture")})?.textContent; return text?.includes("Expired") && text.includes("Untrusted") && text.includes("Matches SAN") && text.includes("CN=Expired-fixture"); })()`, "expired self-signed certificate still reported");
      await check(`(() => { const tile = ${tile("RSS fixture")}; return tile?.querySelector("li a")?.textContent === "Fixture & news" && tile.textContent.includes("Latest <updates>"); })()`, "fake feed entries");
      await check("document.documentElement.scrollWidth <= innerWidth + 1", "widget content fits");
      const { result: { cssContentSize } } = await tab.send("Page.getLayoutMetrics");
      if (width < 500) {
        await tab.send("Emulation.setDeviceMetricsOverride", { width, height: Math.ceil(cssContentSize.height), deviceScaleFactor: 1, mobile: true });
      }
      const { result: { data } } = await tab.send("Page.captureScreenshot", {
        format: "png", captureBeyondViewport: true, clip: { x: 0, y: 0, width, height: cssContentSize.height, scale: 1 },
      });
      writeFileSync(`${out}/${profile}.png`, Buffer.from(data, "base64"));
      for (const kind of ["csp", "console", "exceptions", "failed"]) {
        assert.deepEqual(events[kind], [], `${profile}: ${kind}`);
        checks++;
      }
      console.log(`PASS ${profile}`);
    } finally {
      browser.quit();
    }
  }
} finally {
  tls.close();
  rss.close();
}
console.log(`PASS ${checks} checks, 2 screenshots`);
