// The matrix flows. Each takes the harness `h` (lib.mjs) and the run's
// fixture `f` ({ base, password, fixture }), drives the real UI, and calls
// h.state(label) on every page state it reaches (all checks run there).
import assert from "node:assert/strict";
import fs from "node:fs";
import { focusWalk, guardNetwork, harness } from "./lib.mjs";

const tid = (id) => `[data-testid="${id}"]`;
const dialog = '[role="dialog"][data-open]';

async function go(h, f, route, ready) {
  await h.page.goto(new URL(route, f.base).href);
  await h.page.locator(ready).first().waitFor();
  // A board's first visit opens its tour over the tiles. The sweep (whose
  // owner never saw it) dismisses it here; once onboarding has finished the
  // tour (ctx.tourDone) it must not come back, and every state checks that.
  if (h.ctx.tourDone) return;
  const tour = h.page.locator(tid("tour-skip"));
  if (await tour.isVisible().catch(() => false)) { await tour.click(); await tour.waitFor({ state: "hidden" }); }
}
async function fill(h, selector, value) {
  const el = h.page.locator(selector).first();
  await el.fill(value);
  assert.equal(await el.inputValue(), value, `value reached ${selector}`);
}
// Some engines need the drawer for navigation at phone and tablet widths.
async function navigate(h, href) {
  const link = h.page.locator(`nav a[href="${href}"]:visible`).first();
  if (!(await link.count())) {
    await h.page.locator(tid("drawer-open")).click();
    await h.page.locator(`#side a[href="${href}"]`).first().click();
  } else await link.click();
  await h.page.waitForURL((u) => u.pathname === href);
}
/** Walk the board tour that a new owner's first board visit opens, then
 * require the server to have recorded it as seen. */
async function finishTour(h) {
  const { page } = h;
  const tour = page.locator(tid("board-tour"));
  await tour.waitFor({ timeout: 10000 });
  let steps = 0;
  for (; steps < 10 && await tour.isVisible().catch(() => false); steps++) {
    await h.state(`tour step ${steps + 1}`);
    await page.locator(tid("tour-next")).click();
    await page.waitForTimeout(150);
  }
  assert.ok(steps >= 3, `the tour has its steps (${steps})`);
  let pending = true;
  for (let i = 0; i < 20 && pending; i++) {
    pending = await page.evaluate(async () => (await (await fetch("/api/v1/session")).json()).board_tour_pending);
    if (pending) await page.waitForTimeout(500);
  }
  assert.equal(pending, false, "the finished tour is saved for the account");
  h.ctx.tourDone = true;
}
/** Give a lazily loaded tour time to appear before judging that it did not. */
async function noTour(h, label) {
  await h.page.waitForTimeout(1500);
  await h.state(label);
  assert.equal(await h.page.locator(tid("board-tour")).count(), 0, `no board tour: ${label}`);
}

/** After onboarding finished the tour: visit the board again, save an edit,
 * leave and come back in the app, and reload; the tour stays closed. */
export async function tourStaysClosed(h, f) {
  const { page } = h;
  await page.goto(new URL(f.board, f.base).href);
  await page.locator(tid("board")).waitFor();
  await noTour(h, "board revisited");
  await page.locator(tid("board-edit")).click();
  await page.locator(tid("board-editor")).waitFor();
  await page.locator(tid("board-add-section")).click();
  await page.locator(tid("board-save")).click();
  await page.locator(tid("board-editor")).waitFor({ state: "detached" });
  await noTour(h, "board saved");
  await navigate(h, "/app/overview");
  await page.locator(tid("overview")).waitFor();
  await navigate(h, f.board);
  await page.locator(tid("board")).waitFor();
  await noTour(h, "board via navigation");
  await page.reload();
  await page.locator(tid("board")).waitFor();
  await noTour(h, "board reloaded");
}

/** Submit the filled login form, waiting out a rate-limit pause. */
async function signIn(h) {
  const { page } = h;
  for (let i = 0; i < 4; i++) {
    await h.expecting([/status of 429/, /429 \(Too Many Requests\)/], async () => {
      await page.waitForFunction(() => !/Too many attempts/.test(document.querySelector('form[data-testid="login"]')?.textContent ?? ""), null, { timeout: 20000 });
      await page.locator('form[data-testid="login"] button[type="submit"]').click();
      await page.waitForFunction(() => !!document.querySelector('[data-testid="user"]') || /Too many attempts/.test(document.body.textContent), null, { timeout: 15000 });
    });
    if (await page.locator('[data-testid="user"]').count()) return;
  }
  throw new Error("could not sign in");
}

/** Sign in again if an earlier flow left the session signed out. */
export async function ensureSignedIn(h, f) {
  const { page } = h;
  await page.goto(new URL("/app/overview", f.base).href);
  await page.waitForFunction(() => !!document.querySelector('[data-testid="user"], form[data-testid="login"]'));
  if (await page.locator('form[data-testid="login"]').count()) {
    await page.locator('form[data-testid="login"] input[name="username"]').fill("admin");
    await page.locator('form[data-testid="login"] input[name="password"]').fill(f.password);
    await signIn(h);
  }
}

export const onboarding = {
  async full(h, f) {
    const { page } = h;
    assert.ok(f.fixture, "onboarding needs the fake provider fixture");
    await go(h, f, "/app", 'form[data-testid="onboarding"]');
    await h.state("owner form");
    await fill(h, 'form[data-testid="onboarding"] input[name="username"]', "admin");
    await fill(h, 'form[data-testid="onboarding"] input[name="password"]', f.password);
    await fill(h, 'form[data-testid="onboarding"] input[name="confirm"]', f.password);
    await page.locator('form[data-testid="onboarding"] button[type="submit"]').click();
    const step = page.locator(tid("setup-step"));
    const at = async (title) => { await page.waitForFunction((t) => document.querySelector('[data-testid="setup-step"]')?.textContent === t, title); await h.state(title); };
    await at("Appearance");
    await page.locator(tid("setup-continue")).click();
    await at("Discover services");
    await page.getByText("Docker is reachable").first().waitFor();
    await page.locator(tid("setup-continue")).click();
    await at("Select what to include");
    const service = page.getByRole("checkbox", { name: "Service 1", exact: true });
    await service.click();
    await page.waitForFunction(() => [...document.querySelectorAll('[role="checkbox"]')].some((c) => c.getAttribute("aria-checked") === "true"));
    await page.locator(`${tid("setup-selection")} input[id$="-name"]`).first().waitFor();
    await page.locator('button[aria-label="Add an integration"]').click();
    await page.locator('[role="option"][data-value="pihole"]').click();
    await h.state("selection with provider");
    await page.locator(tid("setup-continue")).click();
    await at("Configure connections");
    const pi = '.setup-item:has(input[name="password"])';
    await fill(h, `${pi} input[name="base_url"]`, "http://127.0.0.1:8081/fixture/pihole");
    await fill(h, `${pi} input[name="password"]`, "pihole-fixture-password");
    await page.locator(`${pi} ${tid("setup-test")}`).click();
    await page.locator(`${pi} ${tid("setup-verdict")}`).filter({ hasText: "Connection succeeded" }).waitFor();
    await h.state("connection tested");
    await page.locator(tid("setup-continue")).click();
    await at("Review dashboard");
    await page.locator(tid("setup-create")).click();
    await page.locator(tid("board")).waitFor();
    assert.ok(new URL(page.url()).pathname.startsWith("/app/boards/"), "created board opens");
    await finishTour(h);
    await page.locator(tid("board-tile")).first().waitFor();
    assert.ok(await page.locator(tid("board-tile")).count() >= 2, "board has the selected service and the provider");
    await h.state("new home board");
    f.board = new URL(page.url()).pathname;
    assert.ok(await step.count() === 0);
  },
  async skip(h, f) {
    const { page } = h;
    await go(h, f, "/app", 'form[data-testid="onboarding"]');
    await fill(h, 'form[data-testid="onboarding"] input[name="username"]', "admin");
    await fill(h, 'form[data-testid="onboarding"] input[name="password"]', f.password);
    await fill(h, 'form[data-testid="onboarding"] input[name="confirm"]', f.password);
    await page.locator('form[data-testid="onboarding"] button[type="submit"]').click();
    for (const title of ["Appearance", "Discover services", "Select what to include", "Configure connections"]) {
      await page.waitForFunction((t) => document.querySelector('[data-testid="setup-step"]')?.textContent === t, title);
      await h.state(`${title} (skip)`);
      await page.locator(`${tid("setup-skip")}:not([disabled])`).click();
    }
    await page.waitForFunction(() => document.querySelector('[data-testid="setup-step"]')?.textContent === "Review dashboard");
    await h.state("Review dashboard (skip)");
    await page.locator(tid("setup-create")).click();
    await page.locator(tid("board")).waitFor();
    await finishTour(h);
    await page.locator(tid("board-empty-add")).waitFor();
    await h.state("empty home board");
    f.board = new URL(page.url()).pathname;
  },
};

export const flows = {
  async "login and logout"(h, f) {
    const { page } = h;
    await page.goto(new URL("/app/overview", f.base).href);
    await page.locator(tid("user")).click();
    await page.locator(tid("signout")).waitFor();
    await h.state("account menu");
    await page.locator(tid("signout")).click();
    await page.locator('form[data-testid="login"]').waitFor();
    await h.state("signed out");
    await fill(h, 'form[data-testid="login"] input[name="username"]', "admin");
    await fill(h, 'form[data-testid="login"] input[name="password"]', "not-the-password");
    await h.expecting([/status of 401/, /401 \(Unauthorized\)/], async () => {
      await page.locator('form[data-testid="login"] button[type="submit"]').click();
      await page.locator(tid("login-error")).waitFor();
      await h.state("wrong password");
    });
    // One failed attempt makes the server ask for a short pause (429,
    // retry_after); the form counts it down, then the user tries again.
    await fill(h, 'form[data-testid="login"] input[name="password"]', f.password);
    await signIn(h);
    await h.state("signed in again");
  },
  async overview(h, f) {
    await go(h, f, "/app/overview", `${tid("overview")} .overview-counts`);
    await h.state("overview");
    await go(h, f, "/app/host", tid("host-sample"));
    await h.state("host");
    await go(h, f, "/app/alerts", tid("alerts"));
    await h.state("alerts");
  },
  async containers(h, f) {
    const { page } = h;
    await go(h, f, "/app/containers", tid("row"));
    await h.state("list");
    await page.locator(`input${tid("filter")}`).fill("svc00");
    await page.waitForFunction(() => new URLSearchParams(location.search).get("q") !== null || document.querySelectorAll('[data-testid="row"]').length <= 12);
    await h.state("filtered");
    await page.locator(`input${tid("filter")}`).fill("");
    const row = page.locator(`${tid("row")} a[href^="/app/containers/"]`).first();
    await row.click();
    await page.locator(tid("ctr-tabs")).waitFor();
    await h.state("detail overview");
    for (const tab of ["metrics", "inspect", "events", "processes"]) {
      const url = new URL(page.url()); url.searchParams.set("tab", tab);
      await page.locator(`${tid("ctr-tabs")} a[href*="tab=${tab}"]`).first().click();
      await page.waitForURL((u) => u.searchParams.get("tab") === tab);
      await h.state(`detail ${tab}`);
    }
  },
  async logs(h, f) {
    const { page } = h;
    await go(h, f, "/app/containers/svc001/logs", "[data-log-id]");
    const first = await page.locator("[data-log-id]").count();
    await page.waitForFunction((n) => document.querySelectorAll("[data-log-id]").length > n || document.querySelector('[data-testid="live-status"]')?.dataset.state === "live", first, { timeout: 15000 });
    await h.state("live tail");
    const filter = page.locator(`input${tid("filter")}`);
    await filter.fill("GET");
    await page.waitForFunction(() => (new URLSearchParams(location.search).get("search") ?? "") === "GET");
    await page.locator(tid("log-matches")).waitFor();
    await h.state("filtered");
    await filter.fill("");
    // Phones fold the filters and the download link behind "Filters".
    const toggle = page.locator(".log-filters:visible");
    if (await toggle.count() && !(await page.locator("a[download]:visible").count())) await toggle.click();
    const [download] = await Promise.all([page.waitForEvent("download"), page.locator("a[download]:visible").first().click()]);
    const file = await download.path();
    assert.ok(file, "download completed");
    const { size } = await import("node:fs").then((fs) => fs.statSync(file));
    assert.ok(size > 0, "downloaded log is not empty");
    await h.state("after download");
    await go(h, f, "/app/logs", tid("logs-picker-row"));
    await h.state("logs picker");
    await go(h, f, "/app/logs?c=svc001,svc002,svc003", "[data-log-id]");
    await h.state("multi pane");
  },
  async "history charts"(h, f) {
    const { page } = h;
    await go(h, f, "/app/history", tid("history"));
    await page.locator('[data-testid^="chart-"]:visible').first().waitFor({ timeout: 20000 });
    await h.state("history");
    const chart = page.locator('[data-testid^="chart-"]').first();
    const box = await chart.boundingBox();
    if (box) {
      if (h.profile.touch) await page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
      else await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await h.state("chart read-out");
    }
    await go(h, f, "/app/containers/svc001?tab=metrics", tid("ctr-tabs"));
    // The history charts load when their section scrolls into view; on a
    // phone it starts below the fold, so scroll as a user would.
    // The page grows while its data arrives, so keep the placeholder in view.
    for (let i = 0; i < 20 && !(await page.locator('[data-testid^="chart-"]:visible').count()); i++) {
      await page.locator('[aria-label="Loading charts"]').first().scrollIntoViewIfNeeded({ timeout: 1000 }).catch(() => {});
      await page.waitForTimeout(1000);
    }
    await page.locator('[data-testid^="chart-"]:visible').first().waitFor({ timeout: 5000 });
    await h.state("container metrics charts");
  },
  async boards(h, f) {
    const { page } = h;
    await go(h, f, f.board, tid("board"));
    const tiles = await page.locator(tid("board-tile")).count();
    await page.locator(tid("board-edit")).click();
    await page.locator(tid("board-editor")).waitFor();
    await h.state("editing");
    await page.locator(tid("board-add-section")).click();
    await page.locator(tid("board-add-item")).last().click();
    await page.locator(tid("board-link-form")).waitFor();
    await h.state("picker");
    await fill(h, `${tid("board-link-form")} input[name="title"]`, "Matrix link");
    await fill(h, `${tid("board-link-form")} input[name="url"]`, "https://example.com/matrix");
    await page.locator(`${tid("board-link-form")} button[type="submit"]`).click();
    await page.locator(tid("board-link-form")).waitFor({ state: "detached" });
    await h.state("edited");
    await page.locator(tid("board-save")).click();
    await page.locator(tid("board-editor")).waitFor({ state: "detached" });
    assert.equal(await page.locator(tid("board-tile")).count(), tiles + 1, "Save keeps the new link");
    await h.state("saved");
    await page.locator(tid("board-edit")).click();
    await page.locator(tid("board-editor")).waitFor();
    await page.locator(tid("board-add-section")).click();
    await page.locator(tid("board-cancel")).click();
    const confirm = page.locator(dialog);
    if (await confirm.isVisible().catch(() => false)) { await h.state("discard prompt"); await confirm.getByRole("button", { name: /discard/i }).click(); }
    await page.locator(tid("board-editor")).waitFor({ state: "detached" });
    assert.equal(await page.locator(tid("board-tile")).count(), tiles + 1, "Cancel discards the unsaved section");
    await h.state("cancelled");
  },
  async "catalog to widget"(h, f) {
    const { page } = h;
    await go(h, f, f.board, tid("board"));
    const tiles = await page.locator(tid("board-tile")).count();
    await go(h, f, "/app/settings/integrations", tid("integration-create"));
    await page.locator(tid("integration-create")).click();
    const form = page.locator(tid("integration-form"));
    await form.waitFor();
    await h.state("catalog");
    await form.locator("#provider-search, input[type=search]").first().fill("Pi-hole");
    await form.getByRole("button", { name: "Choose Pi-hole" }).click();
    await form.locator('input[name="base_url"]').waitFor();
    await h.state("provider form");
    await form.locator('input[name="name"]').fill("Matrix Pi-hole");
    await form.locator('input[name="base_url"]').fill("http://127.0.0.1:8081/fixture/pihole");
    await form.locator('input[name="password"]').fill("pihole-fixture-password");
    await page.locator(tid("provider-draft-test")).click();
    await page.locator(tid("provider-draft-verdict")).waitFor();
    await h.state("tested");
    await page.getByRole("button", { name: "Choose widgets" }).click();
    await form.locator('[aria-label="Board"]').click();
    await page.locator('[role="option"]').nth(1).click();
    const sw = form.locator('[role="switch"]').first();
    await sw.waitFor();
    // The provider's widgets start switched on; make sure the first is.
    if (await sw.getAttribute("aria-checked") !== "true") await sw.click();
    await h.state("choose widgets");
    await page.getByRole("button", { name: "Save widgets to board" }).click();
    await form.waitFor({ state: "detached" });
    await h.state("installed");
    await go(h, f, f.board, tid("board-tile"));
    await page.waitForFunction((n) => document.querySelectorAll('[data-testid="board-tile"]').length > n, tiles);
    await h.state("widget on board");
  },
  async settings(h, f) {
    const { page } = h;
    for (const tab of ["general", "containers", "bookmarks", "widgets", "notifications", "appearance", "history", "config", "security", "boards", "integrations", "users"]) {
      await go(h, f, `/app/settings/${tab}`, tid("settings-tabs"));
      await page.waitForTimeout(150);
      await h.state(tab);
    }
    await go(h, f, "/app/settings/general", 'input[name="greeting"]');
    const greeting = page.locator('input[name="greeting"]');
    const before = await greeting.inputValue();
    await greeting.fill("Matrix greeting");
    await page.locator('form button[type="submit"]').filter({ hasText: "Save" }).first().click();
    await page.waitForFunction(() => document.querySelector('form button[type="submit"]')?.disabled);
    await page.reload();
    await greeting.waitFor();
    assert.equal(await greeting.inputValue(), "Matrix greeting", "setting persisted");
    await h.state("saved setting");
    await greeting.fill(before);
    await page.locator('form button[type="submit"]').filter({ hasText: "Save" }).first().click();
    await go(h, f, "/app/account", tid("account-password"));
    await h.state("account");
  },
  async "users and invites"(h, f) {
    const { page } = h;
    await go(h, f, "/app/settings/users", tid("invite-create"));
    await page.locator(tid("invite-create")).click();
    await page.locator(dialog).waitFor();
    await h.state("invite dialog");
    await page.locator(dialog).getByRole("button", { name: "Create invitation" }).click();
    const link = page.locator(tid("invite-link"));
    await link.waitFor();
    await h.state("invite link");
    const url = new URL(await link.inputValue());
    await page.locator(dialog).getByRole("button", { name: "Done" }).click();
    await page.locator(tid("invite-row")).first().waitFor();
    await h.state("invitation listed");
    // Accept in a separate browser context (a new person, no cookie).
    const other = await page.context().browser().newContext(h.contextOptions);
    other.setDefaultTimeout(15000);
    try {
      const p2 = await other.newPage();
      // The invited person's page gets every check too (its own listeners;
      // its states and events go into this flow).
      const h2 = await harness(p2, { engine: h.engine, profileName: h.profileName, profile: h.profile, out: h.out, unit: h.unit, motion: h.motion, results: h.results });
      h2.ctx.flow = h.ctx.flow;
      await guardNetwork(other, (u) => h2.record({ kind: "network", text: `request to a non-local address blocked: ${u}` }));
      await p2.goto(new URL(url.pathname + url.search + url.hash, f.base).href);
      await p2.locator('form[data-testid="invite"]').waitFor();
      await h2.state("invite accept form");
      const name = `matrix-${h.profileName.replace(/[^a-z0-9]/gi, "")}`;
      await p2.locator('form[data-testid="invite"] input[name="username"]').fill(name);
      await p2.locator('form[data-testid="invite"] input[name="password"]').fill(f.password);
      await p2.locator('form[data-testid="invite"] input[name="confirm"]').fill(f.password);
      await p2.locator('form[data-testid="invite"] button[type="submit"]').click();
      await p2.locator(tid("user")).waitFor();
      assert.ok((await p2.locator(tid("user")).getAttribute("aria-label")).includes(name), "invited user signs in");
      await h2.state("invited user signed in");
      const left = await h2.flush();
      if (left.length) h.results.states.push({ engine: h.engine, profile: h.profileName, unit: h.unit, flow: h.ctx.flow, state: "(invited page, after its last state)", url: "", controls: 0, problems: left });
    } finally { await other.close(); }
    await page.reload();
    await page.locator(tid("user-row")).nth(1).waitFor();
    await h.state("new user listed");
  },
  async "command palette"(h, f) {
    const { page } = h;
    await go(h, f, "/app/overview", tid("overview"));
    if (h.profile.touch) await page.locator(tid("palette-open")).click();
    else await page.keyboard.press("Control+k");
    const palette = page.locator(tid("palette"));
    await palette.waitFor();
    await h.state("open");
    await page.keyboard.type("containers");
    await page.waitForTimeout(200);
    await h.state("results");
    await page.keyboard.press("Enter");
    await page.waitForURL((u) => u.pathname.startsWith("/app/containers"));
    await palette.waitFor({ state: "hidden" });
    await h.state("navigated");
    await page.keyboard.press("Control+k");
    await palette.waitFor();
    await page.keyboard.press("Escape");
    await palette.waitFor({ state: "hidden" });
  },
  async "keyboard only"(h, f) {
    const { page } = h;
    await go(h, f, "/app/overview", tid("overview"));
    await page.evaluate(() => { document.activeElement?.blur(); });
    await page.keyboard.press("Tab");
    const skip = await page.evaluate(() => document.activeElement?.classList.contains("skip"));
    assert.ok(skip, "first Tab reaches the skip link");
    // The focused skip link is drawn over the brand by design: no target check here.
    await h.state("skip link", { focus: false, targets: false });
    await page.keyboard.press("Enter");
    // Walk 30 stops: each one visibly focused (the same ring test as every
    // state's focus check, lib.mjs) and on screen.
    const stops = await focusWalk(page, 30);
    const seen = new Set(stops.map((r) => r.key));
    const bad = stops.filter((r) => !r.ring || !r.visible || !r.onScreen).map((r) => `${r.key} ring=${r.ring} focus-visible=${r.visible} on-screen=${r.onScreen}`);
    assert.ok(seen.size >= 8, `Tab reaches distinct controls (${seen.size})`);
    assert.deepEqual(bad, [], `every Tab stop shows its focus: ${bad.join("; ").slice(0, 400)}`);
    await h.state("tab walk");
    await page.keyboard.press("Control+k");
    await page.locator(tid("palette")).waitFor();
    await page.keyboard.type("settings");
    await page.waitForTimeout(150);
    await page.keyboard.press("Enter");
    await page.waitForURL((u) => u.pathname.startsWith("/app/settings"));
    await h.state("palette to settings");
    await go(h, f, "/app/containers", tid("row"));
    await page.locator(`input${tid("filter")}`).focus();
    await page.keyboard.type("svc001");
    await page.keyboard.press("Tab");
    await h.state("filter by keyboard");
    // The account menu opens and closes from the keyboard.
    await page.locator(tid("user")).focus();
    await page.keyboard.press("Enter");
    await page.locator(tid("signout")).waitFor();
    await h.state("account menu by keyboard");
    await page.keyboard.press("Escape");
    await page.locator(tid("signout")).waitFor({ state: "hidden" });
    const back = await page.evaluate(() => document.activeElement?.getAttribute("data-testid"));
    assert.equal(back, "user", "Escape returns focus to the menu button");
  },
  async touch(h, f) {
    const { page } = h;
    if (!h.profile.touch) return "skipped: not a touch profile";
    await go(h, f, "/app/overview", tid("overview"));
    const drawer = page.locator(`${tid("drawer-open")}:visible`);
    if (await drawer.count()) {
      await drawer.first().tap();
      await page.locator("html.side-open").waitFor({ timeout: 5000 });
      await h.state("drawer open");
      await page.locator('#side a[href="/app/containers"]').first().tap();
    } else await page.locator('nav a[href="/app/containers"]:visible').first().tap();
    await page.waitForURL((u) => u.pathname === "/app/containers");
    await page.locator(tid("row")).first().waitFor();
    await h.state("tapped to containers");
    await page.locator(`${tid("row")} a[href^="/app/containers/"]`).first().tap();
    await page.locator(tid("ctr-tabs")).waitFor();
    await h.state("tapped row");
    await page.locator(tid("palette-open")).tap();
    await page.locator(tid("palette")).waitFor();
    await page.keyboard.press("Escape");
    await page.locator(tid("palette")).waitFor({ state: "hidden" });
  },
  async "sse reconnect"(h, f) {
    const { page } = h;
    await go(h, f, "/app/containers", tid("row"));
    const live = page.locator(tid("live-status"));
    await page.waitForFunction(() => document.querySelector('[data-testid="live-status"]')?.dataset.state === "live", null, { timeout: 15000 });
    // Only failures of the event stream itself are expected (the message
    // or its source URL names /api/v1/stream); any other failed request
    // while the network is down still counts.
    await h.expecting([/(ERR_INTERNET_DISCONNECTED|NetworkError|network connection was lost|Failed to load resource|EventSource|ERR_NETWORK|interrupted|can.t establish a connection|connection.*(refused|reset)|Could not connect).*\/api\/v1\/stream\b/is], async () => {
      await page.context().setOffline(true);
      // Offline emulation alone leaves an open EventSource connected in
      // Chromium; the runner restarts the throwaway server to cut it.
      const ctl = `/out/ctl-${new URL(f.base).port}`;
      fs.rmSync(`${ctl}.done`, { force: true });
      fs.writeFileSync(`${ctl}.restart`, "");
      for (let i = 0; i < 100 && !fs.existsSync(`${ctl}.done`); i++) await page.waitForTimeout(200);
      await page.waitForFunction(() => document.querySelector('[data-testid="live-status"]')?.dataset.state === "reconnecting", null, { timeout: 20000 });
      await page.locator(tid("live-stopped")).waitFor({ timeout: 10000 });
      await h.state("network dropped", { axe: true });
      await page.context().setOffline(false);
      await page.waitForFunction(() => document.querySelector('[data-testid="live-status"]')?.dataset.state === "live", null, { timeout: 45000 });
      await page.locator(tid("live-stopped")).waitFor({ state: "detached" });
      assert.equal(await live.getAttribute("data-state"), "live");
      // Engines report the failed stream requests late; still part of the drop.
      await h.state("reconnected");
    });
  },
  async "tablet rotation"(h, f) {
    const { page } = h;
    if (!h.profile.tablet) return "skipped: not a tablet profile";
    const { width, height } = h.profile;
    try {
      for (const [route, ready] of [["/app/containers", tid("row")], [f.board, tid("board")], ["/app/history", tid("history")]]) {
        await go(h, f, route, ready);
        await page.setViewportSize({ width: height, height: width });
        await page.waitForTimeout(400);
        await h.state(`${route.split("/")[2]} rotated`);
        await page.setViewportSize({ width, height });
        await page.waitForTimeout(400);
        await h.state(`${route.split("/")[2]} rotated back`);
      }
      await page.locator(tid("palette-open")).click();
      await page.locator(tid("palette")).waitFor();
      await page.setViewportSize({ width: height, height: width });
      await page.waitForTimeout(400);
      const box = await page.locator(tid("palette")).boundingBox();
      assert.ok(box && box.x >= 0 && box.x + box.width <= height + 1, "open palette stays on screen after rotation");
      await h.state("palette rotated");
      await page.keyboard.press("Escape");
    } finally { await page.setViewportSize({ width, height }); }
  },
};

// The page sweep: every route of the shared fixture, at any viewport.
export async function sweep(h, f, routes) {
  for (const [route, ready] of routes) {
    await go(h, f, route, ready);
    await h.state(route);
  }
}
