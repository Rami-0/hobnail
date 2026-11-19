// Run against a throwaway fake instance through scripts/e2e.sh.
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, sleep } from "./cdp.mjs";

const base = new URL(process.argv[2] ?? "http://127.0.0.1:3396");
assert.match(base.hostname, /^(127\.0\.0\.1|localhost)$/);
assert.match(base.port, /^339[5-9]$/);
const password = process.env.HB_PASSWORD;
const out = process.env.OUT;
assert.ok(password && out, "set HB_PASSWORD and OUT");

async function call(route, method = "GET", body, session) {
  const response = await fetch(new URL(route, base), {
    method,
    headers: {
      Origin: base.origin, "Content-Type": "application/json",
      ...(session ? { Cookie: session.cookie, "X-RH-CSRF": session.csrf } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  const result = { status: response.status, text, data: undefined, cookie: response.headers.get("set-cookie")?.split(";")[0] };
  try { result.data = JSON.parse(text); } catch { /* Settings gate refusals are text. */ }
  return result;
}

function ok(result, action) {
  assert.ok(result.status >= 200 && result.status < 300, `${action}: HTTP ${result.status}`);
  return result.data;
}
async function credentials(pathname, body) {
  let result = await call(pathname, "POST", body);
  for (let attempt = 0; result.status === 429 && attempt < 3; attempt++) {
    await sleep(Math.min(30, Math.max(1, Number(result.data?.retry_after ?? 2))) * 1000);
    result = await call(pathname, "POST", body);
  }
  return { cookie: result.cookie, csrf: ok(result, pathname).csrf };
}
const boot = ok(await call("/api/v1/session"), "read session");
const owner = await credentials(boot.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", { username: "admin", password });
const cssInitial = ok(await call("/api/settings/appearance", "GET", undefined, owner), "read CSS");
const initial = ok(await call("/api/settings/config", "GET", undefined, owner), "read config");
const run = Date.now().toString(36);
const board = ok(await call("/api/v1/boards", "POST", { name: `Settings scope ${run}` }, owner), "create scope target");

const browser = await launch();
const pages = [
  ["general", "Page"], ["containers", "Hidden containers"], ["bookmarks", "Bookmarks"],
  ["widgets", "Custom widgets"], ["notifications", "Alert rules"], ["appearance", "Custom CSS"],
  ["history", "Database"], ["config", "Raw editor"], ["security", "Recent sign-ins"],
  ["boards", "Boards"], ["integrations", "Integrations"], ["users", "Invitations"],
];
const present = (selector) => `!!document.querySelector(${q(selector)})`;
const text = (selector, value) => `Array.from(document.querySelectorAll(${q(selector)})).some((el) => el.textContent.includes(${q(value)}))`;
const dialog = '[role="dialog"][data-open]';
const editor = '[data-testid="settings-edit"]';
const failures = [];
const createdUsers = [];
try {
  for (const width of [1440, 390]) for (const scheme of ["dark", "light"]) {
    const label = `${width}-${scheme}`;
    const tab = await browser.tab();
    const inp = input(tab);
    inp.touch = width < 700;
    const rec = recorder(tab, base.origin);
    const wait = async (expression, message) => assert.ok(await tab.until(expression, 10000), `${label}: ${message}`);
    const go = async (route, expression) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`location.pathname === ${q(route.split("?")[0])} && (${expression})`, route.startsWith("/app/invite/") ? "invitation page" : route);
    };
    const fill = async (selector, value) => {
      await inp.clear(selector);
      await tab.send("Input.insertText", { text: value });
      await tab.ev('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
    };
    const button = async (selector, caption) => {
      const marked = await tab.ev(`(() => {
        const el = Array.from(document.querySelectorAll(${q(selector + " button")})).find((button) => button.textContent.trim() === ${q(caption)});
        if (!el) return false;
        el.setAttribute("data-check-action", "");
        return true;
      })()`);
      assert.ok(marked, `missing button ${caption}`);
      await inp.click("[data-check-action]");
      await tab.ev('document.querySelectorAll("[data-check-action]").forEach((el) => el.removeAttribute("data-check-action"))');
    };
    const selectContainer = async (name) => {
      await wait(`Array.from(document.querySelectorAll('[data-testid="settings-containers"] label.cb')).some(el=>el.textContent.trim()===${q(name)})`, `container selection ${name} is loaded`);
      await tab.ev(`Array.from(document.querySelectorAll('[data-testid="settings-containers"] label.cb')).find(el=>el.textContent.trim()===${q(name)}).querySelector('[role="switch"]').setAttribute('data-container-check','')`);
      await inp.click('[data-container-check]');
      await tab.ev(`document.querySelector('[data-container-check]')?.removeAttribute('data-container-check')`);
    };
    const cookie = async (session) => {
      const equals = session.cookie.indexOf("=");
      await tab.send("Network.setCookie", { name: session.cookie.slice(0, equals), value: session.cookie.slice(equals + 1),
        url: base.origin, httpOnly: true, sameSite: "Strict", path: "/" });
    };
    try {
      for (const domain of ["Page", "Network", "Runtime", "Log"]) await tab.send(domain + ".enable");
      await tab.send("Runtime.addBinding", { name: "__hbCsp" });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      rec.allow(({ status, url }) => [401, 403, 409, 422].includes(status) && url.includes("/api/settings/")
        ? "expected Settings refusal" : false);
      await cookie(owner);
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width === 390 ? 844 : 900, deviceScaleFactor: 1, mobile: width === 390 });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
      for (const [slug, heading] of pages) {
        await go(`/app/settings/${slug}`, text("h2", heading));
        await wait("document.documentElement.scrollWidth <= innerWidth + 1", `${slug} fits the viewport`);
        await tab.shot(path.join(out, `${label}-${slug}.png`));
        if (slug === "config") {
          await wait('document.querySelector("textarea").wrap === "soft"', "raw editor wraps");
          await button(".settings-form", "Validate");
          await wait(text('[role="status"]', "Configuration is valid"), "raw validation works");
        }
      }
      await go("/app/settings/config", present('textarea[name="text"]'));
      const raw = 'textarea[name="text"]';
      const original = await tab.ev(`document.querySelector(${q(raw)}).value`);
      await inp.click(raw);
      await tab.ev(`{const editor=document.querySelector(${q(raw)});editor.setSelectionRange(editor.value.length,editor.value.length);}`);
      await inp.key("Tab");
      await wait(`document.activeElement===document.querySelector(${q(raw)}) && document.querySelector(${q(raw)}).value.endsWith('  ')`, "Tab indents the raw editor");
      await inp.key("Escape"); await inp.key("Tab");
      await wait(`document.activeElement!==document.querySelector(${q(raw)})`, "Escape releases Tab for focus navigation");
      await fill(raw, original + `\n# Keyboard save ${label}\n`);
      await inp.key("s", { modifiers: width===1440 ? 2 : 4 });
      await wait(text('[data-testid="toast"]', "Saved revision"), "Ctrl or Command S saves raw config");
      await fill(raw, original + '\n[[widgets]]\nid = "settings-command-refusal"\ntype = "command"\ncommand = ["true"]\n');
      await button(".settings-form", "Validate");
      await wait(text('[role="alert"]', "command widgets can only be added"), "command widget guard reason is visible");
      if (original.includes("settings-bound.example")) {
        await fill(raw, original.replace("settings-bound.example", "settings-other.example"));
        await button(".settings-form", "Validate");
        await wait(text('[role="alert"]', "would send header secrets to a new destination"), "destination guard reason is visible");
      }
      await go("/app/account", present('[data-testid="account-password"]'));
      await tab.shot(path.join(out, `${label}-account.png`));
      await go("/app/settings/bookmarks?add=bookmark", present(editor));
      await inp.click(editor + ' button[type="submit"]');
      await wait(`document.querySelectorAll(${q(editor + " [aria-invalid=true]")}).length >= 2`, "all missing fields are reported together");
      const name = `Bookmark ${run} ${label}`;
      await fill(editor + ' input[name="name"]', name);
      await inp.key("Escape");
      await wait(text(dialog, "Discard your changes?"), "escape guards input");
      await button(dialog, "Keep editing");
      await fill(editor + ' input[name="url"]', "https://example.com/settings-check");
      await inp.click(editor + ' button[type="submit"]');
      await wait(`!${present(editor)} && ${text(".settings-row", name)}`, "save updates the collection");
      await inp.click(`button[aria-label=${q("Delete " + name)}]`);
      await button(dialog, "Delete");
      await wait(`!${text(".settings-row", name)}`, "delete updates the collection");
      await button("body", "Undo");
      await wait(text(".settings-row", name), "Undo restores the bookmark");
      await inp.click(`button[aria-label=${q("Edit " + name)}]`);
      await wait(present(editor), "bookmark edit opens");
      await fill(editor + ' input[name="description"]', "Edited in the browser");
      await inp.key("Enter", { modifiers: width===1440 ? 2 : 4 });
      await wait(`!${present(editor)}`, "bookmark edit saves");
      const marks = ok(await call("/api/settings/bookmarks", "GET", undefined, owner), "read bookmarks for ordering");
      ok(await call("/api/settings/bookmarks", "POST", { base_hash: marks.hash, action: "add", item: { name: `Order target ${label}`, url: "https://example.com/order" } }, owner), "seed reorder target");
      await go("/app/settings/bookmarks", text(".settings-row", `Order target ${label}`));
      const beforeOrder = await tab.ev(`[...document.querySelectorAll('[data-testid="settings-bookmark-list"] .settings-row h3')].map(e=>e.textContent)`);
      await inp.click(`button[aria-label=${q("Move " + name + " down")}]`);
      await wait(`[...document.querySelectorAll('[data-testid="settings-bookmark-list"] .settings-row h3')].findIndex(e=>e.textContent===${q(name)}) === ${beforeOrder.indexOf(name)+1}`, "bookmark moves down");
      await go("/app/overview?add=widget", present(editor));
      await inp.key("Escape");
      await go("/app/alerts?add=channel", present(editor));
      await wait(text(dialog, "Secret source"), "Alerts shares the channel editor");
      await inp.key("Escape");

      if (scheme === "dark") {
        await go("/app/settings/general", present('input[name="greeting"]'));
        await fill('input[name="search_url"]', "https://example.com/search");
        await button(".settings-form", "Save");
        await wait(`document.querySelector('input[name="search_url"]').getAttribute('aria-invalid')==='true'`, "General rejects a search URL without its query placeholder");
        await fill('input[name="search_url"]', "https://example.com/search?q={q}");
        const greeting = `Hello from ${label}`;
        await fill('input[name="greeting"]', greeting);
        await inp.click('[data-testid="settings-tabs"] a[href="/app/settings/history"]');
        await wait(present('[data-testid="settings-leave"]'), "dirty form guards tab navigation");
        await button('[data-testid="settings-leave"]', "Keep editing");
        await wait(`!document.querySelector('[data-testid="settings-leave"][data-open]') && location.pathname === "/app/settings/general"`, "keeping edits retains the General form");
        const before = ok(await call("/api/settings/general", "GET", undefined, owner), "read current page settings");
        ok(await call("/api/settings/general", "POST", { base_hash: before.hash, values: { title: "Concurrent title" } }, owner), "save a concurrent change");
        await button(".settings-form", "Save");
        await wait(text(".settings-form", "Reload and keep my input"), "409 offers a rebase");
        await button(".settings-form", "Reload and keep my input");
        await wait(`document.querySelector('input[name="greeting"]').value === ${q(greeting)} && !(${text(".settings-form", "Reload and keep my input")})`, "rebase retains input");
        await button(".settings-form", "Save");
        await wait(text('[data-testid="toast"]', "Saved revision"), "general save announces its revision");
        const saved = ok(await call("/api/settings/general", "GET", undefined, owner), "read rebased settings");
        assert.equal(saved.page.greeting, greeting);
        assert.equal(saved.page.title, "Concurrent title");
        ok(await call("/api/settings/general", "POST", { base_hash: saved.hash, values: { title: before.page?.title ?? null, greeting: before.page?.greeting ?? null } }, owner), "restore page defaults");

        await fill('input[name="greeting"]', "Retry preserved input");
        await tab.send("Network.clearBrowserCookies");
        const allowExpired = rec.allow(({ method, url, status }) => new URL(url).pathname === "/api/settings/general" && method === "POST" && status === 401 ? "intentional expired session" : false);
        await button(".settings-form", "Save");
        await wait(`document.querySelector('.settings-form a[target="_blank"][href^="/app/login"]')?.textContent.includes("Sign in")`, "401 offers sign-in in a new tab");
        await button(".settings-form", "Retry");
        await wait(text(".settings-form", "Sign in as the same account"), "retry explains missing session");
        await cookie(await credentials("/api/login", { username: "admin", password }));
        await button(".settings-form", "Retry");
        await wait(`!document.querySelector('.settings-form a[target="_blank"][href^="/app/login"]')`, "same-account retry clears sign-in prompt");
        allowExpired();
        const retried = ok(await call("/api/settings/general", "GET", undefined, owner), "read retried settings");
        assert.equal(retried.page.greeting, "Retry preserved input");

        await go("/app/settings/containers", present('[data-testid="settings-containers"]'));
        await selectContainer('svc001');
        await selectContainer('svc002');
        await wait(text('[aria-label="Bulk edit"]', "2 selected"), "bulk bar counts its selection");
        await fill('#settings-bulk-group', `Browser bulk ${width}`);
        await button('[aria-label="Bulk edit"]', "Set group");
        await wait(`!${present('[aria-label="Bulk edit"]')}`, "bulk group save clears selection");
        const bulkSaved = ok(await call("/api/settings/containers", "GET", undefined, owner), "read bulk overrides");
        assert.equal(bulkSaved.overrides.svc001.group, `Browser bulk ${width}`);
        assert.equal(bulkSaved.overrides.svc002.group, `Browser bulk ${width}`);
        for (const name of ['svc001','svc002']) await selectContainer(name);
        await button('[aria-label="Bulk edit"]', "Remove overrides"); await button(dialog, "Remove overrides");
        await wait(`!${present(dialog)}`, "bulk removal confirms and saves");
        let groupState = ok(await call("/api/settings/containers", "GET", undefined, owner), "read group order");
        for (const name of [`browser-group-a-${width}`, `browser-group-b-${width}`]) {
          groupState = ok(await call("/api/settings/groups", "POST", { base_hash: groupState.hash, action: "add", item: { name } }, owner), "seed ordered group");
        }
        await go("/app/settings/containers", text('.settings-row', `browser-group-b-${width}`));
        await inp.click(`button[aria-label=${q(`Move browser-group-b-${width} up`)}]`);
        await wait(`[...document.querySelectorAll('[data-testid="settings-group-list"] h3')].findIndex(e=>e.textContent===${q(`browser-group-b-${width}`)}) < [...document.querySelectorAll('[data-testid="settings-group-list"] h3')].findIndex(e=>e.textContent===${q(`browser-group-a-${width}`)})`, "group order is saved");
        await go("/app/settings/containers?add=pattern", present(editor));
        await fill(editor + ' input[name="pattern"]', "settings-check-missing*");
        await wait(text(editor + ' [role="status"]', "0 matching containers"), "exclude dialog shows a match count");
        await button(editor, "Save");
        await wait(text(".settings-row", "settings-check-missing*"), "pattern is saved");
        await inp.click('button[aria-label="Delete settings-check-missing*"]');
        await button(dialog, "Delete");
        await wait(`!${text(".settings-row", "settings-check-missing*")}`, "pattern deletion refreshes the list");
        await go("/app/settings/containers?edit=override:svc001", present('[data-testid="edit-dialog"]'));
        await wait(text('[data-testid="edit-dialog"]', "Save"), "Settings reuses the override editor");
        await inp.key("Escape");

        await go("/app/settings/widgets?add=widget", present(editor));
        const widget = `browser-widget-${width}`;
        await fill(editor + ' input[name="id"]', widget);
        await fill(editor + ' input[name="url"]', "http://127.0.0.1:8081/fixture/session");
        await button(editor, "Add field");
        await wait(text(editor, "Path in the response"), "widget supports named fields");
        await fill(editor + ' .settings-pair input[id$="-label"]', "owner");
        await fill(editor + ' .settings-pair input[id$="-value"]', "owner_exists");
        await fill(editor + ' input[name="template"]', "Owner exists: {{owner}}");
        await button(editor, "Fetch now");
        await wait(present(editor + " .note"), "widget fetch shows its result");
        await button(editor, "Save");
        await wait(text(".settings-row", widget), "custom API widget is saved");

        const widgetsState = ok(await call("/api/settings/widgets", "GET", undefined, owner), "read widget order");
        ok(await call("/api/settings/widgets", "POST", { base_hash: widgetsState.hash, action: "add", item: { id: `browser-order-${width}`, type: "custom-api", url: "http://127.0.0.1:8081/fixture/session", fields: { owner: "owner_exists" }, template: "Order target" } }, owner), "seed widget order target");
        await go("/app/settings/widgets", text('.settings-row', `browser-order-${width}`));
        await inp.click(`button[aria-label=${q(`Move browser-order-${width} up`)}]`);
        await wait(`[...document.querySelectorAll('[data-testid="settings-widget-list"] h3')].findIndex(e=>e.textContent===${q(`browser-order-${width}`)}) < [...document.querySelectorAll('[data-testid="settings-widget-list"] h3')].findIndex(e=>e.textContent===${q(widget)})`, "widget order is saved");
        await go("/app/overview", text(`#w-${widget}`, "Owner exists: true"));
        const notifications = ok(await call("/api/settings/notifications", "GET", undefined, owner), "read notification fixtures");
        const channel = `browser-refused-${width}`;
        const addedChannel = ok(await call("/api/settings/channels", "POST", { base_hash: notifications.hash, action: "add", item: { name: channel, type: "webhook", url: "http://127.0.0.1:1/refused", timeout: "1s" } }, owner), "seed refused channel");
        ok(await call("/api/settings/alerts", "POST", { base_hash: addedChannel.hash, action: "add", item: { name: `browser-rule-${width}`, type: "http", url: "http://127.0.0.1:1/refused", enabled: false, channels: [channel] } }, owner), "seed channel reference");
        await go("/app/settings/notifications", text('.settings-row', channel));
        await tab.ev(`Array.from(document.querySelectorAll('[data-testid="settings-channel-list"] .settings-row')).find(row=>row.querySelector('h3')?.textContent===${q(channel)}).setAttribute('data-channel-check','')`);
        await button('[data-channel-check]', "Send test");
        await wait(text('[data-channel-check] [role="alert"]', "connection refused"), "refused notification test is shown as an error");
        await inp.click(`button[aria-label=${q('Delete '+channel)}]`); await button(dialog,"Delete");
        await wait(text(dialog, "rule"), "a referenced channel cannot be deleted");
        await inp.key("Escape");
        await go("/app/settings/appearance", present('textarea[name="css"]'));
        await fill('textarea[name="css"]', "body{\n}}");
        await wait(text('[data-testid="css-syntax"]', "line 2: a } without"), "CSS syntax warning gives the offending line");
        await fill('textarea[name="css"]', width === 1440 ? ":root{--border-2:#123456}" : ":root{--border-2:#345678}");
        await wait(text('[role="alert"]', "--ctl-border"), "CSS warns about a retired token");
        await button(".settings-form", "Save");
        await wait(text(".settings-form", "Previous version"), "CSS retains its previous version");

        await go("/app/settings/config", text("h2", "Revisions"));
        await wait(text(".settings-row", "bookmarks"), "revisions are listed");
        await button(".settings-row", "Diff");
        await wait(text(dialog, "removed"), "revision diff is shown");
        await inp.key("Escape");
        await button(".settings-row", "View");
        await wait(present(dialog + " pre"), "revision text is shown");
        await inp.key("Escape");

        await button(".settings-row:nth-child(2)", "Restore…");
        await wait(text(dialog, "Replace the current config"), "restore opens a revision confirmation");
        await button(dialog, "Restore revision");
        await wait(`!${present(dialog)}`, "revision restore completes");
        await go("/app/settings/history", present('a[download]'));
        assert.equal((await call("/api/settings/history/export", "GET", undefined, owner)).status, 200);
        await button(".settings-section", "Clear history…");
        await wait(`document.querySelector(${q(dialog + " .dlg-ft button")}).disabled`, "history clear requires a typed phrase");
        await fill('#clear-history-phrase', "clear history");
        await button(dialog, "Clear history");
        await wait(`!${present(dialog)} && ${text('[data-testid="toast"]', "History cleared")}`, "typed phrase actually clears history");
        await go("/app/settings/security", present('[data-testid="account-sessions"]'));
        await credentials("/api/login", { username: "admin", password });
        await go("/app/settings/security", present('[data-testid="account-sessions"] .settings-row'));
        const sessionsBefore = await tab.ev(`document.querySelectorAll('[data-testid="account-sessions"] .settings-row').length`);
        await button('[data-testid="account-sessions"]', "Sign out session");
        await button(dialog, "Sign out");
        await wait(`document.querySelectorAll('[data-testid="account-sessions"] .settings-row').length===${sessionsBefore-1}`, "Security revokes one other session");
      }

      if (width === 1440 && scheme === "dark") {
        for (const role of ["viewer", "admin"]) {
          await go("/app/settings/users", present('[data-testid="invite-create"]'));
          await inp.click('[data-testid="invite-create"]');
          if (role === "viewer") {
            await inp.click(dialog + ' button[aria-label="Access"]');
            await inp.click('[role="option"][data-value="limited"]');
            await wait(present(dialog + ' [role="switch"]'), "scope targets show by name");
            await inp.click(dialog + ` [role="switch"][aria-label=${q(board.name)}]`);
          } else {
            await wait(present(dialog + ' button[aria-label="Role"]'), "invitation role control is ready");
            await inp.click(dialog + ' button[aria-label="Role"]');
            await inp.click('[role="option"][data-value="admin"]');
          }
          await button(dialog, "Create invitation");
          const linkSelector = '[data-testid="invite-link"]';
          await wait(present(linkSelector), "show one-time invite link");
          const link = await tab.ev(`document.querySelector(${q(linkSelector)}).value`);
          const inviteRoute = new URL(link).pathname;
          await button(dialog, "Done");
          await wait(`!${present(linkSelector)}`, "closing clears the link");
          const username = `${role}-${run}`;
          await go(inviteRoute, present('[data-testid="invite"]'));
          for (const [field, value] of [["username", username], ["password", password], ["confirm", password]]) {
            await fill(`form[data-testid="invite"] input[name="${field}"]`, value);
          }
          await inp.click('form[data-testid="invite"] button[type="submit"]');
          await wait(text('[data-testid="user"]', username), "invited user signs in");
          const other = await credentials("/api/login", { username, password });
          const raw = await call("/api/settings/config", "GET", undefined, other);
          assert.equal(raw.status, role === "viewer" ? 403 : 200);
          if (role === "viewer") {
            assert.equal((await call("/api/settings/general", "POST", { values: { title: "Refused" } }, other)).status, 403);
          } else {
            await go("/app/settings/general", text("h2", "Page"));
          }
          await go("/app/settings/users", present('[data-testid="settings-locked"]'));
          assert.equal((await call("/api/v1/users", "GET", undefined, other)).status, 403);
          await cookie(owner);
          const accounts = ok(await call("/api/v1/users", "GET", undefined, owner), "list accounts");
          const account = accounts.find((entry) => entry.name === username);
          assert.ok(account, "accepted account is listed");
          createdUsers.push(account.id);
          assert.equal(account.role, role);
          assert.equal(role === "viewer" ? account.scopes.boards.length : account.scopes, role === "viewer" ? 1 : null);
          await go("/app/settings/users", text('[data-testid="user-row"]', username));
          const row = `[data-testid="user-row"][data-user-id="${account.id}"]`;
          await button(row, "Change role");
          await wait(present(dialog + ' button[aria-label="Role"]'), "account role control is ready");
          await inp.click(dialog + ' button[aria-label="Role"]');
          await inp.click(`[role="option"][data-value="${role === "viewer" ? "admin" : "viewer"}"]`);
          await button(dialog, "Change role");
          await wait(`!${present(dialog)}`, "role editor closes");
          const changed = ok(await call("/api/v1/session", "GET", undefined, other), "read role change");
          if (changed.user) assert.notEqual(changed.user.role, role);
          await button(row, "Disable");
          await button(dialog, "Disable account");
          await wait(text(row, "Disabled"), "account becomes disabled");
          assert.equal(ok(await call("/api/v1/session", "GET", undefined, other), "read disabled session").user, null);
          await button(row, "Enable");
          await button(dialog, "Enable account");
          await wait(text(row, "Active"), "account becomes active");
          const enabled = await credentials("/api/login", { username, password });
          await button(row, "Force sign-out");
          await button(dialog, "Force sign-out");
          await wait(`!${present(dialog)}`, "force sign-out closes");
          assert.equal(ok(await call("/api/v1/session", "GET", undefined, enabled), "read revoked session").user, null);
          await button(row, "Delete");
          await button(dialog, "Delete account");
          await wait(`!${present(row)}`, "deleted account leaves the list");
          assert.ok(!ok(await call("/api/v1/users", "GET", undefined, owner), "list remaining accounts").some((entry) => entry.id === account.id));
        }
        await go("/app/account", present('[data-testid="account-password"]'));
        const otherOwner = await credentials("/api/login", { username: "admin", password });
        const newPassword = password + "-changed";
        const form = '[data-testid="account-password"]';
        for (const [field, value] of [["current", password], ["new", newPassword], ["confirm", newPassword]]) {
          await fill(`${form} input[name="${field}"]`, value);
        }
        await button(form, "Change password");
        await wait(text('[data-testid="toast"]', "Password changed"), "password form succeeds");
        assert.equal(ok(await call("/api/v1/session", "GET", undefined, owner), "current owner session").user.name, "admin");
        assert.equal(ok(await call("/api/v1/session", "GET", undefined, otherOwner), "other owner session").user, null);
        ok(await call("/api/v1/account/password", "POST", { current: newPassword, new: password }, owner), "restore fixture password");
      }
      assert.deepEqual([...rec.csp, ...rec.exceptions, ...rec.console, ...rec.failed], [], `${label} browser diagnostics`);
      console.log(`ok ${label}: Settings tabs, Account, Users and signed-in editing`);
    } catch (error) {
      failures.push(`${label}: ${error.stack}`);
      if (!(await tab.ev(present('[data-testid="invite-link"]')))) await tab.shot(path.join(out, `${label}-failure.png`));
    } finally {
      await tab.close();
    }
  }
} finally {
  browser.quit();
  for (const id of createdUsers) await call(`/api/v1/users/${id}`, "DELETE", undefined, owner);
  const final = ok(await call("/api/settings/config", "GET", undefined, owner), "read final config");
  if (final.hash !== initial.hash) ok(await call("/api/settings/config", "POST", { base_hash: final.hash, text: initial.text }, owner), "restore config");
  const css = ok(await call("/api/settings/appearance", "GET", undefined, owner), "read final CSS");
  if (css.hash !== cssInitial.hash) ok(await call("/api/settings/appearance", "POST", { base_hash: css.hash, css: cssInitial.css }, owner), "restore CSS");
  await call(`/api/v1/boards/${board.id}`, "DELETE", undefined, owner);
}
assert.deepEqual(failures, []);
