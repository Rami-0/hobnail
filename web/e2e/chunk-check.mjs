// Simulate a deployment that removed route, chart and shell assets while the
// shell was open. WIDTHS ("1440x900 390x844") and SCHEMES ("light dark")
// select the viewports and colour schemes; the default is 1440x900 light.
import assert from "node:assert/strict";
import { input, launch, sleep } from "./cdp.mjs";
import { sweepFixture } from "./sweep-fixture.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:3396");
const sizes = (process.env.WIDTHS || "1440x900").split(/\s+/);
const schemes = (process.env.SCHEMES || "light").split(/\s+/);
const fixture = await sweepFixture(base);
const browser = await launch();
try {
  for (const size of sizes) for (const scheme of schemes) {
    const [width, height = 900] = size.split("x").map(Number);
    const label = `${width} ${scheme}`;
    const tab = await browser.tab();
    try {
      const inp = input(tab);
      inp.touch = width < 700;
      await fixture.installCookie(tab);
      await tab.send("Network.setCacheDisabled", { cacheDisabled: true });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: inp.touch });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
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
      // Waits for the 404 and then gives the failed import time to reach React.
      const blocked = async () => {
        for (let i = 0; !missing && i < 100; i++) await sleep(50);
        await sleep(1000);
        return !!missing;
      };
      const shellAlive = async (what) => {
        assert.ok(await tab.until(`!!document.querySelector('#root[data-ready="1"] [data-testid="sidebar"]') && !!document.querySelector('main [data-testid="row"]')`), `${what}: page survives`);
        assert.equal(await tab.ev(`document.body.textContent.includes("Could not load")`), false, `${what}: no load error`);
      };

      // Shell chunks fail soft: the account menu keeps its trigger, the
      // palette does not open, and the page stays.
      await block("*.js");
      await inp.click('button[data-testid="user"]');
      assert.ok(await blocked(), "menu asset returned 404");
      await shellAlive("missing menu chunk");
      assert.ok(await inp.box('button[data-testid="user"]', false), "menu trigger stays");
      assert.equal(await tab.ev(`document.querySelector('[role="menu"]')`), null, "no menu opened");
      console.log(`${label}: missing menu chunk keeps the trigger and the page`);

      // A missing palette chunk closes the palette again (shortcuts work)
      // and offers a reload.
      await block("*.js");
      await inp.click('[data-testid="palette-open"]');
      assert.ok(await blocked(), "palette asset returned 404");
      assert.ok(await tab.until(`!!document.querySelector('#root[data-ready="1"] [data-testid="sidebar"]') && !!document.querySelector('main [data-testid="row"]')`), "missing palette chunk: page survives");
      assert.ok(await tab.until(`document.querySelector('main [role="alert"]')?.textContent.includes("Could not load part of Hobnail.") && document.querySelector('main [role="alert"] button')?.textContent === "Reload"`), "missing palette chunk offers Reload");
      assert.equal(await tab.ev(`document.querySelector('[data-testid="palette"]')`), null, "no palette opened");
      assert.equal(await tab.ev(`document.querySelector('[data-testid="palette-open"]').getAttribute("aria-expanded")`), "false", "palette state closed again");
      await tab.send("Fetch.disable");
      await inp.key("?");
      assert.ok(await tab.until(`!!document.querySelector('[data-testid="shortcuts"]')`), "shortcuts still work after the palette failed");
      await inp.key("Escape");
      assert.ok(await tab.until(`!document.querySelector('[data-testid="shortcuts"]')`), "shortcuts close");
      console.log(`${label}: missing palette chunk resets the palette, keeps shortcuts and offers Reload`);

      // A fresh document loads them again.
      await tab.send("Fetch.disable");
      await tab.send("Page.reload");
      assert.ok(await tab.until(`!!document.querySelector('main [data-testid="row"]')`), "table renders after reload");
      assert.equal(await tab.ev(`document.body.textContent.includes("Could not load")`), false, "reload clears the load error");
      await inp.click('[data-testid="palette-open"]');
      assert.ok(await tab.until(`!!document.querySelector('[data-testid="palette"] [role="combobox"]')`), "palette opens after reload");
      await inp.key("Escape");
      assert.ok(await tab.until(`!document.querySelector('[data-testid="palette"]')`), "palette closes");

      if (inp.touch) {
        // Opening the drawer focuses its first link, which prefetches that
        // page: let it finish before blocking the next chunk.
        await inp.click('button[data-testid="drawer-open"]');
        await sleep(1000);
      }
      await block("*.js");
      await inp.click('[data-testid="sidebar"] a[href="/app/history"]');
      const reload = 'main [role="alert"] button';
      assert.ok(await tab.until(`document.querySelector('${reload}')?.textContent === "Reload"`), "missing chunk offers Reload");
      assert.ok(missing, "one asset request returned 404");
      assert.ok(await inp.box(reload, false), "Reload is visible");
      assert.ok(await tab.until(`!!document.querySelector('[data-testid="sidebar"]')`), "shell survives");

      await tab.send("Fetch.disable");
      await inp.click(reload);
      assert.ok(await tab.until(`!!document.querySelector('[data-testid="history"] .ch-canvas svg')`, 20000), "reload recovers the route and plot");
      console.log(`${label}: missing route chunk shows Reload and recovers after reload`);

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
      console.log(`${label}: missing chart chunk shows Reload and recovers after reload`);
    } finally {
      await tab.close();
    }
  }
} finally {
  browser.quit();
  await fixture.cleanup();
}
