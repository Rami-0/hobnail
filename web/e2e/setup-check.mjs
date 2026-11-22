// Guided onboarding on a fresh fake instance started with --m2-fixtures.
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, rejectLiveUrl } from "./cdp.mjs";

const base = new URL(process.argv[2]);
rejectLiveUrl(base.href);
assert.equal(base.hostname, "127.0.0.1", "Use a throwaway local instance");
assert.notEqual(base.port, "3300", "Guided setup needs a fresh data directory");
const password = process.env.HB_PASSWORD;
assert.ok(password?.length >= 8, "Supply a fixture password via HB_PASSWORD");
const empty = process.env.SETUP_EMPTY === "1";
const width = Number(process.env.WIDTHS ?? 390);
const height = width === 390 ? 844 : 900;
const theme = process.env.SETUP_THEME ?? "light";
const profile = `${width}-${theme}`;
const fixture = process.argv[3] ? new URL(process.argv[3]) : null;
if (!empty) {
  assert.ok(fixture, "Pass the fixture control URL from fake-instance.sh");
  rejectLiveUrl(fixture.href);
  assert.equal(fixture.hostname, "127.0.0.1");
  assert.equal((await fetch(new URL("/fixture/m2/good", fixture), { method: "PUT" })).status, 204);
}

const browser = await launch({ width, height });
const tab = await browser.tab();
const user = input(tab);
user.touch = width === 390;
const events = recorder(tab, base.origin);
let checks = 0;
let lastHit;
const require = async (label, expression) => {
  assert.ok(await tab.until(expression, 12000), label);
  console.log(`ok: ${label}`);
  checks++;
};
const attr = (id) => `[data-testid="${id}"]`;
const el = (selector) => `document.querySelector(${q(selector)})`;
const step = (title) => require(title, `${el(attr("setup-step"))}?.textContent === ${q(title)}`);
const shot = (name) => tab.shot(path.join(process.env.OUT, `${profile}-setup-${name}.png`));
async function hit(x, y, target) {
  lastHit = { target, ...await tab.ev(`(() => {
    const node = document.elementFromPoint(${x}, ${y});
    return { x: ${x}, y: ${y}, hit: node?.tagName, id: node?.id, role: node?.getAttribute("role"),
      testId: node?.closest("[data-testid]")?.getAttribute("data-testid"),
      viewport: { width: innerWidth, height: innerHeight, visualWidth: visualViewport.width,
        visualHeight: visualViewport.height, scale: visualViewport.scale, scrollY } };
  })()`) };
  await user.clickAt(x, y);
}
async function click(selector) {
  const box = await user.box(selector);
  assert.ok(box, `Visible element for ${selector}`);
  await hit(box.x, box.y, selector);
}
async function checkbox(name) {
  const root = (await tab.send("DOM.getDocument")).result.root;
  const { result } = await tab.send("Accessibility.queryAXTree", { nodeId: root.nodeId, role: "checkbox", accessibleName: name });
  const matches = result.nodes.filter((node) => !node.ignored);
  assert.equal(matches.length, 1, `One checkbox named ${name}`);
  const backendNodeId = matches[0].backendDOMNodeId;
  const scrolled = await tab.send("DOM.scrollIntoViewIfNeeded", { backendNodeId });
  assert.ok(!scrolled.error, `Scroll to checkbox ${name}`);
  const [quad] = (await tab.send("DOM.getContentQuads", { backendNodeId })).result.quads;
  await hit((quad[0] + quad[4]) / 2, (quad[1] + quad[5]) / 2, `checkbox ${name}`);
}
async function nav(action) {
  const selector = attr(`setup-${action}`);
  assert.ok(await tab.until(`${el(selector)}?.matches('button:not(:disabled):not([aria-disabled=true])')`), `${action} is enabled`);
  await click(selector);
}
async function fill(selector, value) {
  await click(selector);
  await user.key("a", { modifiers: 2 });
  await user.key("Backspace");
  const response = await tab.send("Input.insertText", { text: value });
  assert.ok(!response.error, `Could not enter value in ${selector}`);
  assert.ok(await tab.ev(`${el(selector)}?.value === ${q(value)}`), `Value reached ${selector}`);
}
// SETUP_FAILURES=1: answer the next matching request with an error once
// (Fetch interception), the way a failing server would.
const failures = process.env.SETUP_FAILURES === "1";
let inject;
async function failOnce(path, method, status, error) {
  events.allow(({ url, status: got }) => new URL(url).pathname === path && got === status && `injected ${status}`);
  inject = { path, method, status, error };
  await tab.send("Fetch.enable", { patterns: [{ urlPattern: `${base.origin}${path}*` }] });
}
tab.on("Fetch.requestPaused", ({ requestId, request }) => {
  const hit = inject && new URL(request.url).pathname === inject.path && request.method === inject.method;
  if (!hit) return void tab.send("Fetch.continueRequest", { requestId });
  const body = Buffer.from(JSON.stringify({ error: inject.error })).toString("base64");
  void tab.send("Fetch.fulfillRequest", { requestId, responseCode: inject.status, responseHeaders: [{ name: "Content-Type", value: "application/json" }], body });
  inject = undefined;
  void tab.send("Fetch.disable");
});
async function addProvider(id) {
  await click('button[aria-label="Add an integration"]');
  await click(`[role="option"][data-value="${id}"]`);
}
try {
  for (const domain of ["Page", "Runtime", "Log"]) await tab.send(`${domain}.enable`);
  await tab.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: user.touch });
  await tab.send("Emulation.setTouchEmulationEnabled", { enabled: user.touch });
  await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
  await tab.send("Runtime.addBinding", { name: "__hbCsp" });
  await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
  await tab.send("Page.navigate", { url: new URL("/app", base).href });
  await require("fresh instance opens owner creation", `!!${el(attr("onboarding"))}`);
  for (const [name, value] of [["username", "admin"], ["password", password], ["confirm", password]]) {
    await fill(`form[data-testid="onboarding"] input[name="${name}"]`, value);
  }
  await click('form[data-testid="onboarding"] button[type="submit"]');
  await step("Appearance");
  await click('button[aria-label="Default theme"]');
  await click(`[role="option"][data-value="${theme}"]`);
  await require("theme applied", `document.documentElement.dataset.theme === ${q(theme)}`);
  await shot("appearance");
  if (failures) {
    // A step change whose save fails on an item this step does not show:
    // the general note names it and "Retry save" makes the step change.
    await require("theme autosaved", 'document.querySelector(".setup-actions [role=status]")?.textContent === "Draft saved"');
    await failOnce("/api/v1/onboarding/draft", "PUT", 400, { code: "invalid", message: "Fixture item failure.", field: "items.fixture-item.name" });
    await nav("continue");
    await require("an item failure off this step shows the general note with Retry save", `[...document.querySelectorAll(".note")].some((n) => n.textContent.includes("Fixture item failure.") && [...n.querySelectorAll("button")].some((b) => b.textContent === "Retry save"))`);
    await step("Appearance");
    await click(".note button");
    await step("Discover services");
    await require("the retried step change cleared the note", `!document.body.textContent.includes("Fixture item failure.")`);
    await nav("back");
    await step("Appearance");
  }
  await nav("continue");
  await step("Discover services");
  if (!empty) await require("Docker suggestions loaded", 'document.body.textContent.includes("Docker is reachable")');
  await nav("continue");
  await step("Select what to include");

  if (!empty) {
    await require("fixture suggestion is available", '[...document.querySelectorAll(".suggestions .cb")].some(label => label.textContent.trim() === "Service 1")');
    await checkbox("Service 1");
    const selected = attr("setup-selection");
    await require("selected suggestion has name and section fields", `!!${el(selected + ' input[id$="-name"]')} && !!${el(selected + ' input[id$="-group"]')}`);
    await fill(`${selected} input[id$="-name"]`, "Selected service");
    await fill(`${selected} input[id$="-group"]`, "Home tools");
    await require("edits autosaved", 'document.querySelector(".setup-actions [role=status]")?.textContent === "Draft saved"');
    await tab.send("Page.reload");
    await step("Select what to include");
    await require("name and section survived reload", `${el(selected + ' input[id$="-name"]')}?.value === "Selected service" && ${el(selected + ' input[id$="-group"]')}?.value === "Home tools"`);
    await click(".setup-fields .acts button:first-child");
    await click('button[aria-label="Add an integration"]');
    await require("endpoint-free widgets are omitted from setup", `!!${el('[role="option"][data-value="http"]')} && !document.querySelector('[data-value="clock"], [data-value="bookmarks"], [data-value="markdown_note"]')`);
    await click('[role="option"][data-value="http"]');
    await addProvider("pihole");
    await require("both provider choices are selected", `[...document.querySelectorAll(${q(selected)})].slice(-2).every((item, i) => item.querySelector('input[id$="-name"]').value === ["Generic HTTP", "Pi-hole"][i]) && document.querySelectorAll(${q(selected)}).length === 4`);
    await shot("selection");
    if (process.env.SETUP_CATALOG_RETRY === "1") {
      await require("provider selection autosaved", 'document.querySelector(".setup-actions [role=status]")?.textContent === "Draft saved"');
      events.allow(({ url, status }) => new URL(url).pathname === "/api/v1/providers" && status === 503 && "injected catalog failure");
      let failCatalog = true;
      tab.on("Fetch.requestPaused", ({ requestId }) => {
        if (!failCatalog) return void tab.send("Fetch.continueRequest", { requestId });
        failCatalog = false;
        void tab.send("Fetch.fulfillRequest", { requestId, responseCode: 503 });
      });
      await tab.send("Fetch.enable", { patterns: [{ urlPattern: `${base.origin}/api/v1/providers` }] });
      await tab.send("Page.reload");
      await step("Select what to include");
      await require("catalog failure is visible", `!!${el(attr("setup-catalog-error"))}`);
    }
  }
  await nav("continue");
  await step("Configure connections");

  if (!empty) {
    if (process.env.SETUP_CATALOG_RETRY === "1") {
      await require("Configure exposes catalog failure and Retry", `!!${el(attr("setup-catalog-error") + " button")} && !${el('input[name="base_url"]')}`);
      await click(attr("setup-catalog-error") + " button");
    }
    const linkUrl = '.setup-item:nth-of-type(2) input[name="url"]';
    const http = '.setup-item:has(input[name="secret"])';
    const piHole = '.setup-item:has(input[name="password"])';
    const httpUrl = `${http} input[name="base_url"]`;
    const credential = `${http} input[name="secret"]`;
    const blocked = "http://127.0.0.2:2/";
    const good = "http://127.0.0.1:8081/fixture/http";
    await require("both provider connection forms loaded", `!!${el(httpUrl)} && !!${el(piHole + ' input[name="base_url"]')}`);
    await require("manual link URL field is available", `${el(linkUrl)}?.closest(".setup-item").querySelector("h3").textContent === "New link"`);
    await fill(linkUrl, "http://127.0.0.1:1/");
    await fill(httpUrl, blocked);
    await fill(credential, "invalid fixture");
    await nav("skip");
    await step("Review dashboard");
    await require("a pending credential does not prevent skipping", 'document.body.textContent.includes("Only saved credentials are included")');
    await nav("back");
    await step("Configure connections");
    await require("URL and pending credential survived Skip and Back", `${el(linkUrl)}?.value === "http://127.0.0.1:1/" && ${el(credential)}?.value.length > 0`);
    const test = async (section, result) => {
      const action = `${section} ${attr("setup-test")}`;
      await require("test action available", `${el(action)}?.matches('button:not(:disabled):not([aria-disabled=true])')`);
      await click(action);
      await require(result, `${el(section + " " + attr("setup-verdict"))}?.textContent.includes(${q(result)})`);
    };
    await fill(credential, "valid-fixture-token");
    await test(http, "blocked");
    await require("saved credential is masked on the form", `${el(credential)}?.value === "" && document.body.textContent.includes("Saved for http://127.0.0.2:2. Enter a replacement")`);
    await fill(httpUrl, good);
    await fill(credential, "guided-fixture-bearer");
    await test(http, "Connection succeeded");
    await fill(`${piHole} input[name="base_url"]`, "http://127.0.0.1:8081/fixture/pihole");
    await fill(`${piHole} input[name="password"]`, "pihole-fixture-password");
    await test(piHole, "Connection succeeded");
    await shot("configure");
    await nav("back");
    await step("Select what to include");
    await nav("continue");
    await step("Configure connections");
    await require("URL survived Back and Continue", `${el(httpUrl)}?.value === ${q(good)}`);
  }
  await nav("skip");
  await step("Review dashboard");
  if (!empty) await require("review names the selected service and its section", 'document.querySelector(".setup-fields").textContent.includes("Selected service") && document.querySelector(".setup-fields").textContent.includes("Home tools")');
  await shot("review");
  await nav("create");
  await require("created board opens", `location.pathname.startsWith("/app/boards/") && !!${el(attr("board"))}`);
  await require("board uses the chosen theme", `document.documentElement.dataset.theme === ${q(theme)}`);
  if (empty) {
    await require("empty board has Add and Discover", `!!${el(attr("board-empty-add"))} && !!${el(attr("board-empty-discover"))}`);
    await click(attr("board-empty-discover"));
    await require("normal board picker opens from Discover", `!!${el(attr("board-picker") + "[data-open]")}`);
  } else {
    await require("board contains the four chosen items", `document.querySelectorAll(${q(attr("board-tile"))}).length === 4 && ["Selected service", "Home tools", "New link", "Generic HTTP", "Pi-hole"].every(text => document.body.textContent.includes(text))`);
  }
  await shot("board");
  for (const kind of ["csp", "console", "exceptions", "failed"]) {
    assert.equal(events[kind].length, 0, `Unexpected ${kind} during setup (${events[kind].length})`);
  }
  console.log(`PASS ${profile} — ${checks} setup checks, no unexpected browser errors`);
} catch (error) {
  console.error("Last input hit", JSON.stringify(lastHit));
  await shot("failure");
  throw error;
} finally {
  await tab.close();
  browser.quit();
}
