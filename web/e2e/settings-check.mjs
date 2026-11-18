// Run against a throwaway fake instance through scripts/e2e.sh.
import assert from "node:assert/strict";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder } from "./cdp.mjs";

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
  const result = await call(pathname, "POST", body);
  return { cookie: result.cookie, csrf: ok(result, pathname).csrf };
}
const boot = ok(await call("/api/v1/session"), "read session");
const owner = await credentials(boot.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", { username: "admin", password });
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
    const rec = recorder(tab, base.origin);
    const wait = async (expression, message) => assert.ok(await tab.until(expression, 10000), `${label}: ${message}`);
    const go = async (route, expression) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`location.pathname === ${q(route.split("?")[0])} && (${expression})`, route);
    };
    const fill = async (selector, value) => { await inp.clear(selector); await inp.type(value); };
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
      await tab.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
      for (const [slug, heading] of pages) {
        await go(`/app/settings/${slug}`, text("h2", heading));
        await wait("document.documentElement.scrollWidth <= innerWidth + 1", `${slug} fits the viewport`);
        if (slug === "config") {
          await wait('document.querySelector("textarea").wrap === "soft"', "raw editor wraps");
          await button(".settings-form", "Validate");
          await wait(text('[role="status"]', "Configuration is valid"), "raw validation works");
        }
        await tab.shot(path.join(out, `${label}-${slug}.png`));
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
      await go("/app/overview?add=widget", present(editor));
      await inp.key("Escape");
      await go("/app/alerts?add=channel", present(editor));
      await wait(text(dialog, "Secret source"), "Alerts shares the channel editor");
      await inp.key("Escape");

      if (width === 1440 && scheme === "dark") {
        for (const role of ["viewer", "admin"]) {
          await go("/app/settings/users", present('[data-testid="invite-create"]'));
          await inp.click('[data-testid="invite-create"]');
          if (role === "viewer") {
            await inp.click(dialog + ' button[aria-label="Access"]');
            await inp.click('[role="option"][data-value="limited"]');
            await wait(present(dialog + ' [role="switch"]'), "scope targets show by name");
            await inp.click(dialog + ' [role="switch"]');
          } else {
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
          const userRoute = `/api/v1/users/${account.id}`;
          ok(await call(userRoute + "/role", "PATCH", { role: role === "viewer" ? "admin" : "viewer" }, owner), "change role");
          const changed = ok(await call("/api/v1/session", "GET", undefined, other), "read role change");
          if (changed.user) assert.notEqual(changed.user.role, role);
          ok(await call(userRoute + "/sign-out", "POST", undefined, owner), "force sign-out");
          assert.equal(ok(await call("/api/v1/session", "GET", undefined, other), "read revoked session").user, null);
          ok(await call(userRoute + "/disabled", "PATCH", { disabled: true }, owner), "disable account");
          assert.notEqual((await call("/api/login", "POST", { username, password })).status, 200);
          ok(await call(userRoute, "DELETE", undefined, owner), "delete account");
          assert.ok(!ok(await call("/api/v1/users", "GET", undefined, owner), "list remaining accounts").some((entry) => entry.id === account.id));
        }
      }
      assert.deepEqual([...rec.csp, ...rec.exceptions, ...rec.console, ...rec.failed], [], `${label} browser diagnostics`);
      console.log(`ok ${label}: Settings tabs, Account, Users and signed-in editing`);
    } catch (error) {
      failures.push(`${label}: ${error.stack}`);
      await tab.shot(path.join(out, `${label}-failure.png`));
    } finally {
      await tab.close();
    }
  }
} finally {
  await browser.close();
  for (const id of createdUsers) await call(`/api/v1/users/${id}`, "DELETE", undefined, owner);
  const final = ok(await call("/api/settings/config", "GET", undefined, owner), "read final config");
  if (final.hash !== initial.hash) ok(await call("/api/settings/config", "POST", { base_hash: final.hash, text: initial.text }, owner), "restore config");
  await call(`/api/v1/boards/${board.id}`, "DELETE", undefined, owner);
}
assert.deepEqual(failures, []);
