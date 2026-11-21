// Guided onboarding against a fresh throwaway instance (fake Docker).
// HB_PASSWORD=<fixture password> scripts/e2e.sh web/e2e/setup-check.mjs http://127.0.0.1:3397
import assert from "node:assert/strict";
import { CSP_LISTENER, input, launch, q, recorder, rejectLiveUrl } from "./cdp.mjs";

const base = new URL(process.argv[2]);
rejectLiveUrl(base.href);
assert.equal(base.hostname, "127.0.0.1", "Use a throwaway local instance");
const password = process.env.HB_PASSWORD;
assert.ok(password?.length >= 8, "Supply a fixture password via HB_PASSWORD");

const browser = await launch({ width: 390, height: 844 });
const tab = await browser.tab();
const user = input(tab);
const events = recorder(tab, base.origin);
let checks = 0;
const require = async (label, expression) => {
  assert.ok(await tab.until(expression, 12000), label);
  console.log(`ok: ${label}`);
  checks++;
};
const attr = (id) => `[data-testid="${id}"]`;
const step = (title) => require(title, `document.querySelector(${q(attr("setup-step"))})?.textContent === ${q(title)}`);
const nav = async (action) => {
  const selector = attr(`setup-${action}`);
  assert.ok(await tab.until(`document.querySelector(${q(selector)})?.matches('button:not(:disabled):not([aria-disabled=true])')`), `${action} is enabled`);
  await user.click(selector);
};
async function fill(selector, value) {
  await user.clear(selector);
  const response = await tab.send("Input.insertText", { text: value });
  assert.ok(!response.error, `Could not enter value in ${selector}`);
}
try {
  for (const domain of ["Page", "Runtime", "Log"]) await tab.send(`${domain}.enable`);
  await tab.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
  await tab.send("Runtime.addBinding", { name: "__hbCsp" });
  await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
  await tab.send("Page.navigate", { url: new URL("/app", base).href });
  await require("fresh instance opens owner creation", `!!document.querySelector(${q(attr("onboarding"))})`);
  for (const [name, value] of [["username", "admin"], ["password", password], ["confirm", password]]) {
    await fill(`form[data-testid="onboarding"] input[name="${name}"]`, value);
  }
  await user.click('form[data-testid="onboarding"] button[type="submit"]');

  await step("Appearance");
  await nav("skip");
  await step("Discover services");
  const empty = process.env.SETUP_EMPTY === "1";
  if (!empty) await require("Docker suggestions loaded", 'document.body.textContent.includes("Docker is reachable")');
  await nav("continue");
  await step("Select what to include");

  if (!empty) {
    await require("suggestions are available to select", '!!document.querySelector(".suggestions .cb")');
    await user.click(".suggestions .cb");
    const selected = attr("setup-selection");
    await require("selected suggestion has name and section fields", `document.querySelectorAll(${q(selected + " input:not([type=hidden])")}).length >= 2`);
    await fill(`${selected} input[id$="-name"]`, "Selected service");
    await fill(`${selected} input[id$="-group"]`, "Home tools");
    await require("edits autosaved", 'document.querySelector(".setup-actions [role=status]")?.textContent === "Draft saved"');
    await tab.send("Page.reload");
    await step("Select what to include");
    await require("name and section survived reload", `document.querySelector(${q(selected + ' input[id$="-name"]')})?.value === "Selected service" && document.querySelector(${q(selected + ' input[id$="-group"]')})?.value === "Home tools"`);
    await user.click('.setup-fields .acts button:first-child');
  }
  await nav("continue");
  await step("Configure connections");

  if (!empty) {
    await fill('.setup-item:last-of-type input[name="url"]', "http://127.0.0.1:1/");
    const target = `.setup-item:last-of-type ${attr("setup-test")}`;
    await require("test action available", `document.querySelector(${q(target)})?.matches('button:not(:disabled):not([aria-disabled=true])')`);
    await user.click(target);
    await require("failed connection test is shown", `document.querySelector(${q(attr("setup-verdict"))})?.textContent.includes("blocked")`);
    await nav("back");
    await step("Select what to include");
    await nav("continue");
    await step("Configure connections");
    await require("URL survived Back and Continue", 'document.querySelector(".setup-item:last-of-type input[name=url]")?.value === "http://127.0.0.1:1/"');
  }
  await nav("skip");
  await step("Review dashboard");
  if (!empty) await require("review names the selected service and its section", 'document.querySelector(".setup-fields").textContent.includes("Selected service") && document.querySelector(".setup-fields").textContent.includes("Home tools")');
  await nav("create");
  await require("created board opens", `location.pathname.startsWith("/app/boards/") && !!document.querySelector(${q(attr("board"))})`);
  if (empty) {
    await require("empty board has Add and Discover", `!!document.querySelector(${q(attr("board-empty-add"))}) && !!document.querySelector(${q(attr("board-empty-discover"))})`);
    await user.click(attr("board-empty-discover"));
    await require("normal board picker opens from Discover", `!!document.querySelector(${q(attr("board-picker"))} + "[data-open]")`);
  } else {
    await require("board contains exactly the two chosen items", `document.querySelectorAll(${q(attr("board-tile"))}).length === 2 && document.body.textContent.includes("Selected service") && document.body.textContent.includes("Home tools")`);
  }
  for (const kind of ["csp", "console", "exceptions", "failed"]) {
    assert.equal(events[kind].length, 0, `Unexpected ${kind} during setup (${events[kind].length})`);
  }
  console.log(`${checks} setup checks passed; no CSP, console, exception or request errors`);
} finally {
  await tab.close();
  browser.quit();
}
