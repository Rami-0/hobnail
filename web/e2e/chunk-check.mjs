// Simulate a deployment that removed a route asset while the shell was open.
import assert from "node:assert/strict";
import { input, launch } from "./cdp.mjs";
import { sweepFixture } from "./sweep-fixture.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:3396");
const fixture = await sweepFixture(base);
const browser = await launch();
try {
  const tab = await browser.tab();
  try {
    await fixture.installCookie(tab);
    await tab.send("Network.setCacheDisabled", { cacheDisabled: true });
    await tab.send("Page.navigate", { url: new URL("/app/containers", base).href });
    assert.ok(await tab.until(`!!document.querySelector('[data-testid="row"]')`), "table renders");

    let missing;
    tab.on("Fetch.requestPaused", ({ requestId, request }) => {
      missing ??= request.url;
      void (request.url === missing
        ? tab.send("Fetch.fulfillRequest", { requestId, responseCode: 404 })
        : tab.send("Fetch.continueRequest", { requestId }));
    });
    const block = async (name) => {
      missing = undefined;
      const interception = await tab.send("Fetch.enable", { patterns: [{ urlPattern: base.origin + "/assets/" + name }] });
      assert.ok(!interception.error, "enable chunk interception");
    };
    await block("*.js");
    const inp = input(tab);
    await inp.click('[data-testid="sidebar"] a[href="/app/history"]');
    const reload = 'main [role="alert"] button';
    assert.ok(await tab.until(`document.querySelector('${reload}')?.textContent === "Reload"`), "missing chunk offers Reload");
    assert.ok(missing, "one asset request returned 404");
    assert.ok(await inp.box(reload, false), "Reload is visible");
    assert.ok(await tab.until(`!!document.querySelector('[data-testid="sidebar"]')`), "shell survives");

    await tab.send("Fetch.disable");
    await inp.click(reload);
    assert.ok(await tab.until(`!!document.querySelector('[data-testid="history"] .ch-canvas svg')`, 20000), "reload recovers the route and plot");
    console.log("Missing route chunk shows Reload and recovers after reload");

    await block("RechartsUsage-*.js");
    await tab.send("Page.reload");
    assert.ok(await tab.until(`!!document.querySelector('[data-testid="history"]')`), "history loads with the chart offline");
    await tab.ev(`document.querySelector('[data-testid="history"] .ch-wait')?.scrollIntoView()`);
    assert.ok(await tab.until(`document.querySelector('${reload}')?.textContent === "Reload"`), "missing chart offers Reload");
    assert.ok(missing, "chart asset returned 404");
    assert.ok(await tab.until(`!!document.querySelector('[aria-label="Incident timeline"]')`), "timeline survives");
    await tab.send("Fetch.disable");
    await inp.click(reload);
    assert.ok(await tab.until(`!!document.querySelector('[data-testid="history"] .ch-canvas svg')`, 20000), "reload recovers the chart");
    console.log("Missing chart chunk shows Reload and recovers after reload");
  } finally {
    await tab.close();
  }
} finally {
  browser.quit();
  await fixture.cleanup();
}
