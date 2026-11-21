// Start a fresh fake-instance.sh with --m2-fixtures (and --docker-unavailable
// for "empty"), then pass its URL and printed fixture control URL:
// HB_PASSWORD=... OUT=... scripts/e2e.sh web/e2e/m2-check.mjs URL FIXTURE 390 light selected
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, rejectLiveUrl, sleep } from "./cdp.mjs";

const [base, fixture] = process.argv.slice(2, 4).map((url) => new URL(url));
const width = Number(process.argv[4]);
const theme = process.argv[5];
const scenario = process.argv[6];
const empty = scenario === "empty";
const password = process.env.HB_PASSWORD;
const out = process.env.OUT;
for (const url of [base, fixture]) {
  rejectLiveUrl(url.href);
  assert.equal(url.hostname, "127.0.0.1", "Use throwaway local fixtures");
  assert.notEqual(url.port, "3300", "Start a fresh fixture for every run");
}
assert.ok([390, 1440].includes(width) && ["light", "dark"].includes(theme));
assert.ok(["empty", "selected"].includes(scenario));
assert.ok(password?.length >= 8 && out, "Supply HB_PASSWORD and OUT");
const height = width === 390 ? 844 : 900;
const prefix = `${width}-${theme}-${scenario}`;
const attr = (id) => `[data-testid="${id}"]`;
const el = (selector) => `document.querySelector(${q(selector)})`;
const text = (selector, value) => `${el(selector)}?.textContent.includes(${q(value)})`;
const wizard = attr("guided-setup");
const board = attr("board");
const tour = attr("board-tour");
const stale = attr("stale-badge");
const steps = ["Appearance", "Discover services", "Select what to include", "Configure connections", "Review dashboard"];
const shots = ["appearance", "discover", "select", "configure", "review"];
const contexts = [];
let checks = 0;
let screenshots = 0;
let tab;
let user;

async function context() {
  const browser = await launch({ width, height });
  const ctx = { browser };
  contexts.push(ctx);
  tab = ctx.tab = await browser.tab();
  user = input(tab);
  user.touch = width === 390;
  ctx.events = recorder(tab, base.origin);
  for (const domain of ["Page", "Runtime", "Log"]) await tab.send(`${domain}.enable`);
  await tab.send("Runtime.addBinding", { name: "__hbCsp" });
  await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
  await tab.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: user.touch });
  await tab.send("Emulation.setEmulatedMedia", { features: [
    { name: "prefers-color-scheme", value: theme }, { name: "prefers-reduced-motion", value: "reduce" },
  ] });
}

async function require(label, expression, ms = 12000) {
  assert.ok(await tab.until(expression, ms), label);
  checks++;
  console.log(`ok: ${label}`);
}

async function navigate(route) {
  await tab.send("Page.navigate", { url: new URL(route, base).href });
}
async function click(selector) {
  await require(`action is enabled and visible: ${selector}`, `!!${el(selector)}?.matches(':not(:disabled):not([aria-disabled=true])') && ${el(selector)}.getBoundingClientRect().width > 0`);
  const point = await user.box(selector);
  assert.ok(point && await tab.ev(`(() => {
    const control = ${el(selector)};
    const r = control.getBoundingClientRect();
    return r.left >= -1 && r.right <= innerWidth + 1 && r.top >= -1 && r.bottom <= innerHeight + 1 &&
      control.contains(document.elementFromPoint(${point.x}, ${point.y}));
  })()`), `control is clipped or covered: ${selector}`);
  await user.clickAt(point.x, point.y);
}
async function fill(selector, value) {
  await click(selector);
  await user.key("a", { modifiers: 2 });
  await user.key("Backspace");
  const response = await tab.send("Input.insertText", { text: value });
  assert.ok(!response.error, `could not fill ${selector}`);
  assert.equal(await tab.ev(`${el(selector)}.value === ${q(value)}`), true);
}

async function credentials(id) {
  const form = `form${attr(id)}`;
  await require(id, `!!${el(form)}`);
  await fill(`${form} input[name="username"]`, "admin");
  await fill(`${form} input[name="password"]`, password);
  if (id === "onboarding") await fill(`${form} input[name="confirm"]`, password);
  await click(`${form} button[type="submit"]`);
}

async function layout(label, root) {
  const problems = await tab.ev(`(() => {
    const bad = [];
    if (innerWidth !== ${width} || document.documentElement.scrollWidth > innerWidth + 1) bad.push("horizontal page overflow");
    const root = ${el(root)};
    if (!root) return [...bad, "page absent"];
    for (const control of root.querySelectorAll('button, a[href], input:not([type=hidden]), select, [role=checkbox]')) {
      const r = control.getBoundingClientRect();
      if (r.width && r.height && getComputedStyle(control).visibility === "visible" && (r.left < -1 || r.right > innerWidth + 1)) {
        bad.push(control.getAttribute("aria-label") || control.textContent || control.name);
      }
    }
    return bad;
  })()`);
  assert.deepEqual(problems, [], `${label} layout`);
  checks++;
}
async function capture(label, root, anchor) {
  await tab.ev(anchor ? `${el(anchor)}?.scrollIntoView({block:"center",behavior:"instant"})` : 'window.scrollTo({top:0,behavior:"instant"})');
  await layout(label, root);
  await tab.shot(path.join(out, `${prefix}-${label}.png`));
  screenshots++;
}
async function step(index, shot) {
  await require(steps[index], `${el(attr("setup-step"))}?.textContent === ${q(steps[index])}`);
  await require("wizard moves focus to the visible step heading", `(() => {
    const heading = ${el(attr("setup-step"))};
    const r = heading.getBoundingClientRect();
    return document.activeElement === heading && r.top >= 0 && r.bottom <= innerHeight;
  })()`);
  if (shot) await capture(shots[index], wizard);
}
async function move(action, index, shot) {
  await click(attr(`setup-${action}`));
  await step(index, shot);
}
async function fixtureState(state) {
  assert.equal((await fetch(new URL(`/fixture/m2/${state}`, fixture), { method: "PUT" })).status, 204);
}
async function sessionPending(expected) {
  await require(`tour pending is ${expected} for the login`, `fetch('/api/v1/session').then(r => r.json()).then(s => s.board_tour_pending === ${expected})`);
}
async function finishTour(count, screenshotIndex) {
  const next = attr("tour-next");
  await require("tour opens with focus on Next", `document.activeElement === ${el(next)}`);
  await user.key("Tab");
  await require("Tab reaches Skip tour", `document.activeElement === ${el(attr("tour-skip"))}`);
  await user.key("Tab", { modifiers: 8 });
  for (let index = 1; index <= count; index++) {
    await require(`tour step ${index} of ${count} focuses Next or Finish`, `${text(tour, `${index} of ${count}`)} && document.activeElement === ${el(next)} && ${el(next)}.matches(':focus-visible') && getComputedStyle(${el(next)}).outlineStyle !== 'none'`);
    const anchor = "[data-board-tour-anchor]";
    await require("real tour control remains clickable", `(() => {
      const a = ${el(anchor)};
      if (!a?.getAttribute('aria-describedby')?.includes('board-tour-description')) return false;
      const r = a.getBoundingClientRect();
      return a.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
    })()`);
    if (index === screenshotIndex) await capture("tour", tour);
    await user.key("Enter");
  }
  await require("Finish closes the tour", `!${el(tour)}`);
  await sessionPending(false);
}

try {
  await context();
  await navigate("/app");
  await credentials("onboarding");
  await step(0, true);
  await require("Back is disabled on Appearance", `${el(attr("setup-back"))}?.disabled`);
  await click('button[aria-label="Default theme"]');
  await click(`[role="option"][data-value="${theme}"]`);
  await require("appearance previews the requested theme", `document.documentElement.dataset.theme === ${q(theme)}`);
  await user.key("Tab");
  await require("keyboard focus is visible on the wizard action", `document.activeElement.matches('button:focus-visible') && getComputedStyle(document.activeElement).outlineStyle !== 'none'`);
  for (let index = 0; index < 4; index++) {
    await move("skip", index + 1);
    await move("back", index);
    await move("continue", index + 1);
  }
  for (let index = 3; index >= 1; index--) await move("back", index);
  await require("Docker discovery result is explicit", text(wizard, empty ? "cannot reach Docker" : "Docker is reachable"));
  await capture(shots[1], wizard);
  await move("continue", 2);
  const selected = attr("setup-selection");
  const linkTitle = "Fixture link";
  const readingTitle = "Fixture reading";
  const name = `${selected} input[id$="-name"]`;
  const group = `${selected} input[id$="-group"]`;
  const testTimes = [];
  if (!empty) {
    await fill("#setup-search", readingTitle);
    await require("search shows the widget suggestion", text(".suggestions .cb", readingTitle));
    await click('.suggestions .cb .checkbox');
    await fill(group, "Home tools");
    await fill("#setup-search", "Service 1");
    await require("search shows a container suggestion", text(".suggestions .cb", "Service 1"));
    await click('.suggestions .cb .checkbox');
    await fill(`${selected}:nth-of-type(2) input[id$="-name"]`, "Selected service");
    await require("discovery edits autosave", text(".setup-actions [role=status]", "Draft saved"));
    const original = tab;
    await tab.send("Page.reload");
    await step(2);
    await require("draft fields survive reload", `${el(group)}?.value === "Home tools"`);
    await context();
    await original.close();
    await navigate("/app");
    await credentials("login");
    await step(2);
    await require("same login in a new browser context resumes selections and edits", `${el(name)}?.value === ${q(readingTitle)} && ${el(group)}?.value === "Home tools" && document.querySelectorAll(${q(selected)}).length === 2`);
    await click(".setup-fields .acts button:first-child");
    await click('button[aria-label="Add an integration"]');
    await click('[role="option"][data-value="http"]');
    await fill(`${selected}:nth-of-type(3) input[id$="-name"]`, linkTitle);
    await click(".setup-fields .acts button:first-child");
    await fill(`${selected}:nth-of-type(5) input[id$="-name"]`, "Unfinished link");
    await click('button[aria-label="Add an integration"]');
    await click('[role="option"][data-value="http"]');
    await fill(`${selected}:nth-of-type(6) input[id$="-name"]`, "Unfinished integration");
  }
  await capture(shots[2], wizard);
  await move("continue", 3);
  await capture(shots[3], wizard);
  if (!empty) {
    const items = ".setup-item";
    const link = `${items} input[name="url"][required]`;
    const url = `${items} input[name="base_url"]`;
    const secret = `${items} input[name="secret"]`;
    const endpoint = "http://127.0.0.1:8081/fixture/m2/reading";
    assert.equal(await tab.ev(`(async () => {
      const { csrf } = await fetch("/api/v1/session").then(r => r.json());
      const response = await fetch("/api/v1/integrations", {
        method: "POST", headers: { "Content-Type": "application/json", "X-RH-CSRF": csrf },
        body: JSON.stringify({ kind: "http", name: "Z Existing fixture", base_url: ${q(endpoint)}, enabled: true }),
      });
      return response.status;
    })()`), 201);

    await fill(link, "invalid");
    contexts.at(-1).events.allow(({ url, status }) => status === 400 && new URL(url).pathname === "/api/v1/onboarding/test" && "expected invalid test URL");
    await click(`${items}:has(input[name="url"][required]) ${attr("setup-test")}`);
    await require("the invalid URL is described by its own field error", `(() => {
      const field = ${el(link)};
      const description = document.getElementById(field.id + "-err");
      return description?.textContent.includes("Enter an HTTP") &&
        field.getAttribute("aria-describedby")?.split(" ").includes(description.id);
    })()`);
    await fill(link, "https://example.test/home");
    await fill(url, endpoint);
    await fill(secret, "unsaved-fixture-credential");
    await require("tab-only credential has its own draft status", text(".setup-actions [role=status]", "Credential not saved"));
    await require("tab-only credential blocks document unload", `!window.dispatchEvent(new Event("beforeunload", {cancelable:true}))`);
    await tab.ev(`(() => {
      const link = document.createElement("a");
      link.href = "/app/settings";
      document.body.append(link);
      link.click();
      link.remove();
    })()`);
    await require("in-app navigation prompts for unsaved credentials", `!!${el(attr("setup-leave") + "[data-open]")}`);
    await user.key("Escape");
    await require("keeping the draft preserves the pending credential", `!${el(attr("setup-leave") + "[data-open]")} && ${el(secret)}?.value.startsWith("unsaved")`);
    await move("skip", 4);
    await require("pending credentials do not block Skip or save automatically", text(wizard, "Only saved credentials are included"));
    await require("Review identifies both unfinished items", `[...${el(wizard)}.querySelectorAll("li")].filter(li => li.textContent.includes("incomplete — will not be added")).length === 2`);
    await click(".setup-fields li button.btn-link");
    await step(3);
    await require("Skip and Back retain entered connection fields", `${el(link)}?.value === "https://example.test/home" && ${el(url)}?.value === ${q(endpoint)} && ${el(secret)}?.value.startsWith("unsaved")`);
    await fill(secret, "fixture-credential");
    await click(`${items}:has(input[name="base_url"]) ${attr("setup-test")}`);
    testTimes.push(Date.now());
    await require("upstream 503 is explained in the wizard", text(attr("setup-verdict"), "non-success status"));
    await require("saved credential removes unload protection", `window.dispatchEvent(new Event("beforeunload", {cancelable:true}))`);
    await require("stored credential is masked", `${el(secret)}?.value === "" && ${text(wizard, "Saved for")}`);
    await capture("failed-connection", wizard, attr("setup-verdict"));
    await move("back", 2);
    await move("continue", 3);
    await require("the staged connection survives Back and Continue", `${el(url)}?.value === ${q(endpoint)} && ${el(secret)}?.value === ""`);
  }
  await move(empty ? "continue" : "skip", 4, true);
  await require("review reflects selected groups or the real empty board", text(wizard, empty ? "Your board will be empty" : "Home tools"));
  if (!empty) {
    await require("Review offers a match to an already saved integration", text(wizard, "Matches existing Z Existing fixture"));
    await click('button[aria-label="Connection for Generic HTTP"]');
    await require("matching saved integration can be reused", text('[role="option"]:not([data-value="new"])', "Reuse Z Existing fixture"));
    await click('[role="option"][data-value="new"]');
    await require("Add anyway is autosaved", text(".setup-actions [role=status]", "Draft saved"));
    await capture("review-match", wizard);
  }
  await click(attr("setup-create"));
  await require("first board opens in the requested theme", `location.pathname.startsWith("/app/boards/") && !!${el(board)} && document.documentElement.dataset.theme === ${q(theme)}`);
  const route = await tab.ev("location.pathname");
  await require("new accounts are offered the tour", `!!${el(tour)}`);
  if (empty) {
    await require("first empty board offers Add and Discover", `!!${el(attr("board-empty-add"))} && !!${el(attr("board-empty-discover"))} && !${el(attr("board-tile"))}`);
    let dismissalRequest;
    let dismissalAttempts = 0;
    contexts.at(-1).events.allow(({ url, status }) =>
      status === 503 && new URL(url).pathname === "/api/v1/account/board-tour" && "simulated tour save failure");
    tab.on("Fetch.requestPaused", ({ requestId }) => {
      dismissalAttempts++;
      if (dismissalAttempts === 1) dismissalRequest = requestId;
      else void tab.send("Fetch.continueRequest", { requestId });
    });
    await tab.send("Fetch.enable", { patterns: [{ urlPattern: "*/api/v1/account/board-tour" }] });
    await click(attr("tour-skip"));
    await require("Skip tour closes before the save responds", `!${el(tour)}`);
    for (let index = 0; index < 30 && !dismissalRequest; index++) await sleep(50);
    assert.ok(dismissalRequest, "tour dismissal request is pending");
    await tab.send("Fetch.fulfillRequest", { requestId: dismissalRequest, responseCode: 503 });
    await sessionPending(false);
    assert.equal(dismissalAttempts, 2, "the transient failure is retried");
    await require("the failed save did not reopen the tour", `!${el(tour)}`);
    await tab.send("Fetch.disable");
    await navigate(route + "?tour=1");
    await require("replayed tour opens with focus on Next", `document.activeElement === ${el(attr("tour-next"))}`);
    await user.key("Tab");
    await require("Tab reaches Skip tour", `document.activeElement === ${el(attr("tour-skip"))}`);
    for (let index = 0; index < 4 && await tab.ev(`${el(tour)}.contains(document.activeElement) || document.activeElement === document.body`); index++) {
      await user.key("Tab");
    }
    await require("Tab can move focus to a control outside the tour", `!${el(tour)}.contains(document.activeElement) && document.activeElement !== document.body`);
    await tab.ev("void (window.__m2OutsideTour = document.activeElement)");
    await user.key("Escape");
    await require("Escape from outside closes the tour without moving focus", `!${el(tour)} && document.activeElement === window.__m2OutsideTour`);
    await capture("first-board", board);
    await click(attr("board-empty-discover"));
    await require("the real Discover button opens the normal board picker", `!!${el(attr("board-picker") + "[data-open]")}`);
    await user.key("Escape");
    await click(attr("board-cancel"));
    await require("Cancel returns to the empty board", `!!${el(board)} && !${el(tour)}`);
    await navigate(route + "?tour=1");
    await finishTour(4, 2);
  } else {
    const tile = attr("board-tile");
    const widget = `${tile}:has(.board-tile-head h3):not(:has(a))`;
    await finishTour(3, 2);
    await require("first board contains four chosen items", `document.querySelectorAll(${q(tile)}).length === 4 && ${text(board, "Selected service")} && ${text(board, linkTitle)}`);
    await require("failed widget without a good value shows its error", `${text(board, "HTTP 503")} && !${el(stale)} && !${el(`${widget} .tile-value`)}`);
    await capture("widget-initial-failure", board);
    await fixtureState("good");
    await require("widget receives a good reading", text(board, "42 units"));
    await capture("first-board", board);
    await fixtureState("fail");
    await require("widget keeps its last good reading with an aged badge and error", `${text(board, "42 units")} && ${text(stale, "stale · ")} && ${el(stale)}.title.includes("HTTP 503")`);
    await capture("widget-stale", board, stale);
    await tab.send("Page.reload");
    await require("widget stale data survives board reload", `${text(board, "42 units")} && ${text(stale, "stale · ")} && !${el(tour)}`);
    await fixtureState("recover");
    await require("widget recovery updates the reading and clears the badge", `${text(board, "73 units")} && !${el(stale)}`);
    await capture("widget-recovered", board);

    const integration = attr("integration-verdict");
    const integrationTest = attr("integration-test");
    const settings = "/app/settings/integrations";
    const test = async (state, description, expression) => {
      if (testTimes.length === 3) await sleep(Math.max(0, 10500 - (Date.now() - testTimes.shift())));
      await fixtureState(state);
      await click(integrationTest);
      await require(description, expression);
      testTimes.push(Date.now());
    };
    await navigate(settings);
    await test("fail", "initial integration failure has no success or stale data",
      `${text(integration, "non-success status")} && !${text(integration, "Last success")} && !${el(stale)}`);
    await test("good", "integration records a successful connection and latency", `${text(integration, "Connection succeeded")} && ${text(integration, "Last success")}`);
    const latency = (await tab.ev(`${el(integration)}.querySelector("small").textContent`)).split(" · ").at(-1);
    await test("fail", "failed integration refresh retains previous latency under a stale badge",
      `${text(stale, "stale · ")} && ${el(stale)}.title.includes("non-success status") && ${text(integration, latency)}`);
    await navigate(route);
    await require("stale integration evidence is visible on the board", `${text(`${integration} ${stale}`, "stale · ")} && ${text(integration, latency)}`);
    await capture("integration-stale", board, `${integration} ${stale}`);
    await navigate(settings);
    await test("recover", "integration recovery clears the stale badge", `${text(integration, "Connection succeeded")} && !${el(stale)}`);
    await click('a[href$="?tour=1"]');
    await require("Settings replays the tour", `!!${el(tour)}`);
    await click(attr("tour-next"));
    await require("tour can be skipped after advancing", text(tour, "2 of 3"));
    await user.key("Escape");
    await require("Escape dismisses and restores focus to the real palette control", `!${el(tour)} && document.activeElement === ${el(attr("palette-open"))}`);
  }
  await tab.send("Page.reload");
  await require("completed or skipped tour stays closed after reload", `!!${el(board)} && !${el(tour)}`);
  await sessionPending(false);
  for (const { events } of contexts) {
    for (const kind of ["csp", "console", "exceptions", "failed"]) {
      assert.deepEqual(events[kind], [], `Unexpected ${kind} in ${prefix}`);
    }
  }
  console.log(`PASS ${prefix}: ${checks} checks, ${screenshots} screenshots, zero unexpected CSP, console, exception or request errors`);
} catch (error) {
  if (tab) await tab.shot(path.join(out, `${prefix}-failure.png`)).catch(() => {});
  throw error;
} finally {
  for (const ctx of contexts) ctx.browser.quit();
}
