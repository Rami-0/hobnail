// Settings form check (DESIGN.md §11.6, wave 2): drives every Settings form
// on a running test instance, signed in, with real key and pointer events:
// the Bookmarks, Notifications and Widgets lists and their dialogs; Hidden
// containers, Groups, container overrides and the bulk bar; General;
// Appearance (default theme, custom.css); History and data (Recording,
// Sample interval, Clear history up to the confirm: that one request is
// answered by the page, so the instance's history survives); Config file
// (raw editor, revision viewer, Restore); Change password and Sign out
// other sessions (up to the confirm). Each form gets a no-change round trip
// and, where a dialog, confirm, switch or arrow starts a write, an outside
// write meanwhile (the base-hash rule). It adds, edits, moves and deletes
// entries named "ui-check-*" and puts the config file and custom.css back
// byte for byte at the end, also when a check fails. Signs in once (the login is rate
// limited: never run it in a loop with a wrong password). Channel tests go
// to a mock receiver only (scripts/mock-receiver): MOCK_RECEIVER is its
// address from here, MOCK_FROM_APP its address from the instance. With
// CONFIG_FILE (the instance's config file, on this machine) and RICH_CONFIG
// (a config with every kind of entry) it also saves every entry of that
// config unchanged and checks that nothing is written.
//   HB_PASSWORD=... node scripts/settings-check.mjs http://127.0.0.1:3210/
// CHECK_WRONG_PASSWORD=1 also sends one wrong current password to Change
// password (it counts as a failed login).
// Exits 1 when a check fails, 2 on bad usage. Headless Chromium from the
// Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";
import fs from "node:fs";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const PW = process.env.HB_PASSWORD;
const MOCK = (process.env.MOCK_RECEIVER || "http://127.0.0.1:3299").replace(/\/+$/, "");
const MOCK_IN = (process.env.MOCK_FROM_APP || "http://hobnail-dev-mock:9999").replace(/\/+$/, "");
if (!/^https?:\/\//.test(url) || !PW) {
  console.error("usage: HB_PASSWORD=<password> [MOCK_RECEIVER=…] [MOCK_FROM_APP=…] node scripts/settings-check.mjs <base URL of a test instance>");
  process.exit(2);
}
const ORIGIN = new URL(url).origin;

// ---- one sign-in, then the API for setup and clean-up ----
const lr = await fetch(url + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: ORIGIN }, body: JSON.stringify({ password: PW }) });
if (lr.status !== 200) { console.error(`sign-in failed: HTTP ${lr.status}`); process.exit(1); }
const cookie = lr.headers.get("set-cookie").split(";")[0];
const token = (await lr.json()).csrf;
const api = async (method, path, body) => {
  const r = await fetch(url + "/api/settings/" + path, { method, headers: { Cookie: cookie, "X-RH-CSRF": token, Origin: ORIGIN, "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  return { status: r.status, data: await r.json().catch(() => ({})) };
};
const bookmarks = async () => (await api("GET", "bookmarks")).data;
async function cleanup() {
  for (let i = 0; i < 20; i++) {
    const d = await bookmarks();
    const b = d.bookmarks.find((x) => /^ui-check-/.test(x.item.name || ""));
    if (!b) return;
    await api("POST", "bookmarks", { base_hash: d.hash, action: "delete", id: b.id });
  }
}
await cleanup();
const initial = (await bookmarks()).bookmarks.map((b) => b.item.name);
// The file as it was: put back byte for byte at the end (a raw save of the
// same text), so the run leaves the config at its starting hash.
const start = (await api("GET", "config")).data;
const startCss = (await api("GET", "appearance")).data;
async function restore() {
  await cleanup();
  const now = (await api("GET", "config")).data;
  if (now.hash !== start.hash) await api("POST", "config", { base_hash: now.hash, text: start.text });
  const css = (await api("GET", "appearance")).data;
  if (css.hash !== startCss.hash) await api("POST", "appearance", { base_hash: css.hash, css: startCss.css });
  return (await api("GET", "config")).data.hash === start.hash && (await api("GET", "appearance")).data.hash === startCss.hash;
}
// Outside writes (another tab, a hand edit), by name: never by position.
const idOf = (d, name) => d.bookmarks.find((b) => b.item.name === name).id;
async function outside(action, name, item) {
  const d = await bookmarks();
  const body = { base_hash: d.hash, action };
  if (name) body.id = idOf(d, name);
  if (item) body.item = item;
  const r = await api("POST", "bookmarks", body);
  if (r.status !== 200) throw new Error(`outside ${action} ${name}: HTTP ${r.status} ${JSON.stringify(r.data)}`);
}
const itemOf = async (name) => { const d = await bookmarks(); const b = d.bookmarks.find((x) => x.item.name === name); return b && b.item; };

// ---- the browser (DevTools protocol, as scripts/ui-check.mjs) ----
const CH = process.env.CH || `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const ch = spawn(CH, ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${port}`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 50; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(100); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map(); const errors = [];
ws.addEventListener("message", (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === "Runtime.exceptionThrown") errors.push(d.params.exceptionDetails.text);
  if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") errors.push(d.params.args.map((a) => a.value).join(" "));
  // A "Leave site?" prompt would block every later step: leave, and say so.
  if (d.method === "Page.javascriptDialogOpening") { errors.push(`unexpected ${d.params.type} prompt`); ws.send(JSON.stringify({ id: ++id, method: "Page.handleJavaScriptDialog", params: { accept: true } })); }
});
// A run that hangs still puts the config back.
setTimeout(async () => { console.log("FAIL the run timed out"); await restore().catch(() => {}); ch.kill(); process.exit(1); }, 600000).unref();
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
const KEYS = { Escape: 27, Enter: 13, Tab: 9 };
async function key(k) {
  const base = { key: k, code: k, windowsVirtualKeyCode: KEYS[k], nativeVirtualKeyCode: KEYS[k] };
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  if (k === "Enter") await send("Input.dispatchKeyEvent", { type: "char", ...base, text: "\r" });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
  await sleep(60);
}
async function typeText(s) { await send("Input.insertText", { text: s }); await sleep(30); }
async function clickAt(x, y) {
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  await sleep(80);
}
async function click(sel) {
  const r = await ev(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); e.scrollIntoView({ block: "center" }); const b = e.getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()`);
  await clickAt(r[0], r[1]);
}
async function clear(sel) { await click(sel); await ev(`document.querySelector(${JSON.stringify(sel)}).select()`); await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 }); await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 }); }
const until = async (expr, ms = 4000) => { for (let i = 0; i < ms / 50; i++) { if (await ev(expr).catch(() => false)) return true; await sleep(50); } return false; };

let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };
const active = () => ev("document.activeElement && (document.activeElement.id || document.activeElement.getAttribute('data-key') || document.activeElement.textContent.trim().slice(0, 30))");
const isOpen = (i) => ev(`document.getElementById(${JSON.stringify(i)}).open`);
const toasts = () => ev("[...document.querySelectorAll('body>.toasts>.toast')].map((t) => t.textContent.trim())");
const rows = () => ev("[...document.querySelectorAll('#bm-list tr[data-key]')].map((r) => r.querySelector('.linkbtn').textContent)");
const rowSel = (name) => `(() => [...document.querySelectorAll('#bm-list tr[data-key]')].find((r) => r.querySelector('.linkbtn').textContent === ${JSON.stringify(name)}))()`;

async function load(path, width = 1440, mobile = false) {
  await send("Emulation.setDeviceMetricsOverride", { width, height: mobile ? 844 : 900, deviceScaleFactor: mobile ? 2 : 1, mobile });
  await send("Page.navigate", { url: url + path });
  for (let i = 0; i < 100; i++) { await sleep(100); if (await ev("document.readyState === 'complete' && !!window.HB").catch(() => false)) break; }
  await sleep(150);
}

try {
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Network.enable");
  await send("Network.setCookie", { name: cookie.split("=")[0], value: cookie.split("=")[1], url });
  await load("/settings/bookmarks");
  check(JSON.stringify(await rows()) === JSON.stringify(initial), "the list is rendered by the server, as in the file", await rows());
  check(await ev("!document.querySelector('#toasts') && !document.querySelector('.toasts:not(body>.toasts)')"), "one toast system (no #toasts region)");

  // ---- add: errors at once, server check, busy, toast, row in place ----
  await ev("window.scrollTo(0, 0)");
  await click("#bm-add");
  check(await isOpen("dlg-bookmark"), "Add bookmark opens the dialog");
  check((await active()) === "bm-name", "focus goes to Name", await active());
  check(await ev("location.search === '?add=bookmark'"), "the URL is ?add=bookmark");
  await key("Enter"); await sleep(100);
  const errs = await ev("[...document.querySelectorAll('#dlg-bookmark .fld-err:not([hidden])')].map((e) => e.textContent)");
  check(errs.length === 2, "Enter on an empty form shows both errors at once", errs);
  check(await ev("document.querySelector('#dlg-bookmark .dlg-msg').textContent") === "2 fields need attention.", "the footer counts them");
  await typeText("ui-check-a");
  await click("#bm-url"); await typeText("not a url"); await key("Enter"); await sleep(100);
  check(await ev("document.getElementById('bm-url-e').textContent") === "Use a link that starts with http:// or https://.", "a bad link is caught on the URL field");
  // The server checks too: skip the browser rule once.
  await ev("document.getElementById('bm-url').removeAttribute('data-v')");
  await clear("#bm-url"); await typeText("javascript:alert(1)"); await key("Enter");
  await until("document.getElementById('bm-url-e').textContent !== ''");
  check(await ev("document.getElementById('bm-url-e').textContent") === "Use a link that starts with http:// or https://.", "the server's 422 lands on the URL field");
  check(await isOpen("dlg-bookmark"), "the dialog stays open with the input");
  await ev("document.getElementById('bm-url').setAttribute('data-v', 'http')");
  await clear("#bm-url"); await typeText("https://ui-check.example/a");
  const sy = await ev("scrollY");
  // Hold the next write for a moment so the busy state can be seen.
  await ev("(() => { const f = window.fetch; window.fetch = (u, o) => (window.__hold && o && o.method === 'POST' ? new Promise((r) => setTimeout(r, 700)).then(() => f(u, o)) : f(u, o)); window.__hold = true; })()");
  await key("Enter");
  check(await ev("document.querySelector('#dlg-bookmark [type=submit]').getAttribute('aria-busy')") === "true", "Save is busy while the request runs");
  check(await ev("document.querySelector('#dlg-bookmark .dlg-b').inert"), "the form is inert while busy");
  await until("!document.getElementById('dlg-bookmark').open");
  await ev("window.__hold = false");
  await sleep(300);
  check(!(await isOpen("dlg-bookmark")), "a successful add closes the dialog");
  check((await toasts()).some((x) => /^Added “ui-check-a” · revision #\d+/.test(x)), "the toast says what happened", await toasts());
  check((await rows()).at(-1) === "ui-check-a", "the new row is in the list without a reload", await rows());
  check(await ev(`${rowSel("ui-check-a")}.classList.contains('row-new')`), "the new row flashes");
  check(await ev("scrollY") === sy, "the scroll position is kept");
  check(await ev("location.search === ''"), "closing removes ?add=");
  check(await ev("document.activeElement.matches('#bm-add')"), "focus returns to Add bookmark", await active());

  // ---- edit: filled, guard, save in place, focus back on the row ----
  const newKey = await ev(`${rowSel("ui-check-a")}.dataset.key`);
  await ev(`${rowSel("ui-check-a")}.querySelector('.linkbtn').click()`); await sleep(300);
  check(await ev("document.getElementById('bm-name').value") === "ui-check-a" && await ev("document.getElementById('bm-url').value") === "https://ui-check.example/a", "edit opens filled from the row");
  check(await ev("document.querySelector('#dlg-bookmark h2').textContent") === "Edit bookmark “ui-check-a”", "the title names the bookmark");
  check(await ev(`location.search === '?edit=' + encodeURIComponent(${JSON.stringify(newKey)})`) || await ev(`location.search === '?edit=${newKey}'`), "the URL is ?edit=<key>", await ev("location.search"));
  await click("#bm-description"); await typeText("first desc");
  await key("Escape");
  check(await ev("!document.querySelector('#dlg-bookmark .dlg-guard').hidden"), "Esc with input asks first");
  await click("#dlg-bookmark [data-keep]");
  await clickAt(5, 450); await sleep(150);
  check(await isOpen("dlg-bookmark") && await ev("!document.querySelector('#dlg-bookmark .dlg-guard').hidden"), "a backdrop click with input asks first");
  await click("#dlg-bookmark [data-keep]");
  check(await ev("document.getElementById('bm-description').value") === "first desc", "Keep editing keeps the input");
  await key("Enter");
  await until("!document.getElementById('dlg-bookmark').open"); await sleep(300);
  check(await ev(`${rowSel("ui-check-a")}.textContent.includes('first desc')`), "the edited row shows the change in place");
  check((await toasts()).some((x) => /^Saved “ui-check-a”/.test(x)), "the toast says Saved", await toasts());
  check((await active()) === newKey, "focus returns to the edited row", await active());

  // ---- a stale token (new sign-in elsewhere) is replaced from a same-origin GET ----
  await ev("document.querySelector('meta[name=rh-csrf]').content = 'stale'");
  await ev(`${rowSel("ui-check-a")}.querySelector('.linkbtn').click()`); await sleep(250);
  await click("#bm-description"); await typeText(" 2"); await key("Enter");
  await until("!document.getElementById('dlg-bookmark').open");
  check(!(await isOpen("dlg-bookmark")) && await ev("document.querySelector('meta[name=rh-csrf]').content !== 'stale'"), "an outdated CSRF token is refreshed from the page and the save goes through");

  // ---- 409: the file changed underneath; reload and keep the input ----
  await ev(`${rowSel("ui-check-a")}.querySelector('.linkbtn').click()`); await sleep(250);
  await click("#bm-description"); await typeText(" 3");
  let d = await bookmarks();
  await api("POST", "bookmarks", { base_hash: d.hash, action: "add", item: { name: "ui-check-ext", url: "https://ui-check.example/ext" } });
  await key("Enter");
  await until("!document.querySelector('#dlg-bookmark .dlg-note').hidden");
  check(/changed on disk/.test(await ev("document.querySelector('#dlg-bookmark .dlg-note').textContent")), "a stale save is reported inside the dialog", await ev("[document.getElementById('dlg-bookmark').open, document.querySelector('#dlg-bookmark .dlg-note').textContent, document.querySelector('#set-banners').textContent]"));
  check((await active()) === "" || await ev("document.activeElement.classList.contains('dlg-note')"), "focus moves to the note", await active());
  check(await ev("document.getElementById('bm-description').value").then((v) => v.endsWith(" 3")), "the input is kept");
  await click("#dlg-bookmark .dlg-note .btn.primary");
  await until("/Reloaded/.test(document.querySelector('#dlg-bookmark .dlg-note').textContent)");
  check((await rows()).includes("ui-check-ext"), "Reload and keep my input refreshed the list behind");
  await key("Enter");
  await until("!document.getElementById('dlg-bookmark').open");
  check(!(await isOpen("dlg-bookmark")), "saving again after the reload works");

  // ---- one-click reorder: one revision per step ----
  const before = await rows();
  const i = before.indexOf("ui-check-a");
  await ev(`${rowSel("ui-check-a")}.querySelector('[data-move=down]').focus()`);
  await ev(`${rowSel("ui-check-a")}.querySelector('[data-move=down]').click()`);
  await until(`[...document.querySelectorAll('#bm-list tr[data-key]')].map((r) => r.querySelector('.linkbtn').textContent).indexOf('ui-check-a') === ${i + 1}`);
  check((await rows()).indexOf("ui-check-a") === i + 1, "↓ moves the row one step at once", await rows());
  check((await toasts()).some((x) => /^Moved “ui-check-a” down · revision #/.test(x)), "the move is saved as a revision", await toasts());
  check(await ev(`document.activeElement.closest('tr') === ${rowSel("ui-check-a")}`), "focus follows the moved row");

  // ---- delete through the confirm; Undo restores; delete from the dialog ----
  await click(`#bm-list tr[data-key="bookmark:${i + 1}"] [data-menu]`); await sleep(200);
  check(await ev("document.getElementById('m-bm').matches(':popover-open')"), "⋯ opens the row menu");
  await click("#m-bm [data-row-act=delete]"); await sleep(250);
  check(await isOpen("dlg-confirm") && await ev("document.querySelector('#dlg-confirm h2').textContent") === "Delete the bookmark “ui-check-a”?", "Delete… asks with the shared confirm");
  check(await ev("document.activeElement.hasAttribute('data-cancel')"), "the confirm focuses Cancel");
  await click("#dlg-confirm [data-ok]");
  await until("![...document.querySelectorAll('#bm-list .linkbtn')].some((a) => a.textContent === 'ui-check-a')");
  check(!(await rows()).includes("ui-check-a"), "the row is removed", await rows());
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Deleted “ui-check-a”/.test(t.textContent))");
  check(await ev("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Deleted “ui-check-a”/.test(t.textContent) && t.querySelector('button').textContent === 'Undo')"), "the toast offers Undo");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].find((t) => /Deleted “ui-check-a”/.test(t.textContent)).querySelector('button').click()");
  await until("[...document.querySelectorAll('#bm-list .linkbtn')].some((a) => a.textContent === 'ui-check-a')");
  check((await rows()).indexOf("ui-check-a") === i + 1, "Undo restores the row in its place", await rows());
  check((await toasts()).some((x) => /^Restored “ui-check-a”/.test(x)), "the toast says Restored", await toasts());
  await ev(`${rowSel("ui-check-a")}.querySelector('.linkbtn').click()`); await sleep(250);
  await click("#dlg-bookmark [data-act=dlg-delete]"); await sleep(200);
  check(await isOpen("dlg-confirm") && await isOpen("dlg-bookmark"), "Delete… inside the dialog asks on top of it");
  await key("Escape"); await sleep(150);
  check(!(await isOpen("dlg-confirm")) && await isOpen("dlg-bookmark"), "Esc cancels only the confirm");
  await click("#dlg-bookmark [data-act=dlg-delete]"); await sleep(200);
  await click("#dlg-confirm [data-ok]");
  await until("![...document.querySelectorAll('#bm-list .linkbtn')].some((a) => a.textContent === 'ui-check-a')");
  check(!(await isOpen("dlg-bookmark")) && !(await rows()).includes("ui-check-a"), "deleting from the dialog closes it and removes the row");

  // Undo is refused when the file changed since the delete.
  d = await bookmarks();
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await ev(`${rowSel("ui-check-ext")}.querySelector('[data-menu]').click()`); await sleep(200);
  await click("#m-bm [data-row-act=delete]"); await sleep(200);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Deleted “ui-check-ext”/.test(t.textContent))");
  d = await bookmarks();
  await api("POST", "bookmarks", { base_hash: d.hash, action: "add", item: { name: "ui-check-later", url: "https://ui-check.example/later" } });
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].find((t) => /Deleted “ui-check-ext”/.test(t.textContent)).querySelector('button').click()");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not undone/.test(t.textContent))");
  check((await toasts()).some((x) => /^Not undone: the config file changed since/.test(x)), "Undo is refused when the file changed since", await toasts());

  // ---- the base-hash rule: nothing writes to another bookmark ----
  for (const n of ["x1", "x2", "x3"]) await outside("add", null, { name: `ui-check-${n}`, url: `https://ui-check.example/${n}` });
  await load("/settings/bookmarks");
  const nameIs = (n) => `${rowSel(n)}.querySelector('.linkbtn')`;
  // 1. An outside change while an untouched edit dialog is open: nothing is
  //    refreshed under it, the save is refused, Reload-and-keep re-finds x2.
  await ev(`${nameIs("ui-check-x2")}.click()`); await sleep(250);
  await outside("delete", "ui-check-x1"); await sleep(1500);
  check(await ev("document.querySelector('#dlg-bookmark .dlg-note').hidden") && (await rows()).includes("ui-check-x1"), "an outside change does not refresh the list under an open dialog");
  check(await ev("!document.querySelector('#set-banners [data-b]')"), "and no page banner on top of the dialog");
  await click("#bm-description"); await typeText("x2 desc"); await key("Enter");
  await until("!document.querySelector('#dlg-bookmark .dlg-note').hidden");
  check(/changed on disk/.test(await ev("document.querySelector('#dlg-bookmark .dlg-note').textContent")), "the save from the old position is refused (409)");
  await click("#dlg-bookmark .dlg-note .btn.primary");
  await until("/Reloaded/.test(document.querySelector('#dlg-bookmark .dlg-note').textContent)");
  await click("#dlg-bookmark [type=submit]");
  await until("!document.getElementById('dlg-bookmark').open");
  check((await itemOf("ui-check-x2")).description === "x2 desc" && !(await itemOf("ui-check-x3")).description, "Reload and keep re-finds the same bookmark: x2 changed, x3 untouched", [await itemOf("ui-check-x2"), await itemOf("ui-check-x3")]);
  check(await ev("!document.querySelector('#set-banners [data-b]')"), "no stale page banner after the save");
  // 3. The entry changed meanwhile: it cannot be re-found, Save is disabled.
  await ev(`${nameIs("ui-check-x2")}.click()`); await sleep(250);
  await outside("edit", "ui-check-x2", { name: "ui-check-x2", url: "https://ui-check.example/x2", description: "changed outside" });
  await sleep(800);
  await click("#bm-description"); await typeText(" mine"); await key("Enter");
  await until("!document.querySelector('#dlg-bookmark .dlg-note').hidden");
  await click("#dlg-bookmark .dlg-note .btn.primary");
  await until("/cannot be saved/.test(document.querySelector('#dlg-bookmark .dlg-note').textContent)");
  check(await ev("document.querySelector('#dlg-bookmark [type=submit]').disabled"), "an entry that changed meanwhile cannot be saved onto: Save is disabled");
  await click("#dlg-bookmark .dlg-note .btn"); await sleep(300);
  check(!(await isOpen("dlg-bookmark")) && (await itemOf("ui-check-x2")).description === "changed outside", "Discard leaves the outside change alone");
  await ev(`${nameIs("ui-check-x2")}.click()`); await sleep(250);
  check(await ev("!document.querySelector('#dlg-bookmark [type=submit]').disabled"), "Save is enabled again on the next open");
  await key("Escape"); await sleep(250);
  // 2. An outside change while the delete confirm is open: refused, nothing removed.
  const countBefore = (await bookmarks()).bookmarks.length;
  await ev(`${rowSel("ui-check-x3")}.querySelector('[data-menu]').click()`); await sleep(200);
  await click("#m-bm [data-row-act=delete]"); await sleep(250);
  await outside("delete", "ui-check-x2"); await sleep(1500);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted/.test(t.textContent))");
  check((await toasts()).some((x) => /^Not deleted: the config file changed/.test(x)), "a delete confirmed after an outside change is refused", await toasts());
  check(!!(await itemOf("ui-check-x3")) && (await bookmarks()).bookmarks.length === countBefore - 1, "nothing else was deleted");
  // The same from Delete… inside an open edit dialog.
  await load("/settings/bookmarks");
  await ev(`${nameIs("ui-check-x3")}.click()`); await sleep(250);
  await click("#dlg-bookmark [data-act=dlg-delete]"); await sleep(200);
  await outside("add", null, { name: "ui-check-x4", url: "https://ui-check.example/x4" }); await sleep(800);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted/.test(t.textContent))");
  check(!!(await itemOf("ui-check-x3")), "Delete… in the dialog after an outside change is refused too");
  // 6. A hand-written bookmark with an unchanged non-http URL stays editable.
  const cfg = (await api("GET", "config")).data;
  await api("POST", "config", { base_hash: cfg.hash, text: cfg.text + '\n[[bookmarks]]\nname = "ui-check-files"\nurl = "/files"\n' });
  await load("/settings/bookmarks");
  await ev(`${nameIs("ui-check-files")}.click()`); await sleep(250);
  await click("#bm-description"); await typeText("hand-written"); await key("Enter");
  await until("!document.getElementById('dlg-bookmark').open");
  check((await itemOf("ui-check-files")).description === "hand-written" && (await itemOf("ui-check-files")).url === "/files", "a bookmark with an unchanged non-http URL can be edited");

  // =================================================================
  // ---- Notifications: channels (wave 2b) ----
  const notif = async () => (await api("GET", "notifications")).data;
  const chItem = async (n) => { const c = (await notif()).channels.find((x) => x.id === n); return c && c.item; };
  const ruItem = async (n) => { const r = (await notif()).rules.find((x) => x.id === n); return r && r.item; };
  async function outsideApi(path, body) {
    const what = path === "widgets" ? "widgets" : "notifications";
    const d = (await api("GET", what)).data;
    const r = await api("POST", path, { base_hash: d.hash, ...body });
    if (r.status !== 200) throw new Error(`outside ${path} ${body.action}: HTTP ${r.status} ${JSON.stringify(r.data)}`);
  }
  const vis = (sel) => `(() => { const e = document.querySelector(${JSON.stringify(sel)}); return !!e && !e.closest('[hidden]') && e.offsetParent !== null; })()`;
  const note = (dlg) => ev(`document.querySelector('#${dlg} .dlg-note').textContent`);
  const noteShown = (dlg) => `!document.querySelector('#${dlg} .dlg-note').hidden`;
  const chRows = () => ev("[...document.querySelectorAll('#ch-list tr[data-key]')].map((r) => r.dataset.key.slice(8))");
  const ruRows = () => ev("[...document.querySelectorAll('#ru-list tr[data-key]')].map((r) => r.dataset.key.slice(5))");
  const mock = async () => { try { return await (await fetch(MOCK + "/_received")).json(); } catch { return null; } };
  await load("/settings/notifications");
  check(JSON.stringify(await chRows()) === JSON.stringify((await notif()).channels.map((c) => c.id)), "channels: the list is rendered by the server, as in the file", await chRows());
  check(JSON.stringify(await ruRows()) === JSON.stringify((await notif()).rules.map((c) => c.id)), "rules: the list is rendered by the server, as in the file", await ruRows());
  await click("#ch-add");
  check(await isOpen("dlg-channel") && (await active()) === "ch-name" && await ev("location.search === '?add=channel'"), "Add channel opens the dialog on Name, at ?add=channel");
  await key("Enter"); await sleep(100);
  let es = await ev("[...document.querySelectorAll('#dlg-channel .fld-err:not([hidden])')].map((e) => e.closest('.fld').querySelector('input').name)");
  check(JSON.stringify(es) === JSON.stringify(["name", "url_env"]), "channel: Enter on an empty form shows both errors at once", es);
  await typeText("ui-check-hook");
  await click("#ch-url-env"); await typeText("lower_case");
  await click("#dlg-channel details summary"); await click("#ch-timeout"); await typeText("abc");
  await click("#dlg-channel details summary"); // closed again: its error must still be found
  await click("#ch-name"); await key("Enter"); await sleep(100);
  es = await ev("[...document.querySelectorAll('#dlg-channel .fld-err:not([hidden])')].map((e) => e.textContent)");
  check(es.length === 2 && es.includes("Capital letters, digits and _ only, like RH_WEBHOOK_URL.") && es.includes("Use a duration like 30s, 5m or 2h."), "channel: a variable name and a duration are checked together, in plain words", es);
  // Only the timeout wrong, in the closed More options: it is opened.
  await clear("#ch-url-env"); await typeText("RH_WEBHOOK_URL");
  if (await ev("document.querySelector('#dlg-channel details').open")) await click("#dlg-channel details summary");
  await click("#ch-name"); await key("Enter"); await sleep(100);
  check(await ev("document.querySelector('#dlg-channel details').open") && (await active()) === "ch-timeout", "an error in the closed More options opens it and focuses the field", await active());
  await clear("#ch-timeout");
  // Slack, then Telegram: the fields follow the type; Plain URL goes away.
  await click("#dlg-channel input[name=src][value=plain]");
  check(await ev(vis("#ch-url")) && !(await ev(vis("#ch-url-env"))), "Plain URL shows the URL field instead of the variable");
  await click("#dlg-channel input[name=type][value=slack]");
  check(await ev(vis("#ch-webhook-url-env")) && !(await ev(vis("#ch-url"))) && !(await ev(vis("#dlg-channel input[name=src][value=plain]"))) && await ev("document.querySelector('#dlg-channel input[name=src][value=env]').checked"), "Slack: no Plain URL, the source falls back to a variable");
  await click("#dlg-channel input[name=src][value=file]");
  await click("#ch-webhook-url-file"); await typeText("/etc/passwd"); await key("Enter"); await sleep(100);
  check(/^A file inside .*secrets\//.test(await ev("document.getElementById('ch-webhook-url-file-e').textContent")), "a file outside data/secrets/ is caught on its field", await ev("document.getElementById('ch-webhook-url-file-e').textContent"));
  await click("#dlg-channel input[name=type][value=telegram]");
  check(await ev(vis("#ch-token-file")) && await ev(vis("#ch-chat-id")), "Telegram: token (same source) and chat id");
  // Back to a webhook with a plain URL at the mock receiver: save and test.
  await click("#dlg-channel input[name=type][value=webhook]");
  await click("#dlg-channel input[name=src][value=plain]");
  await click("#ch-url"); await typeText(MOCK_IN + "/webhook");
  const mockBefore = (await mock() || []).length;
  await click("#dlg-channel [data-submit=test]");
  await until(noteShown("dlg-channel"), 8000);
  const tn = await note("dlg-channel");
  check(/^Saved \(revision #\d+\)\. Test sent to “ui-check-hook”/.test(tn), "Save and send test: saved, then tested, the result in the dialog note", tn);
  check(await isOpen("dlg-channel") && await ev("document.querySelector('#dlg-channel h2').textContent") === "Edit channel “ui-check-hook”" && await ev("document.querySelector('#dlg-channel [type=submit]').textContent.trim()") === "Save changes", "the dialog stays open, now editing the saved channel");
  check((await chRows()).includes("ui-check-hook"), "the new channel is in the list behind");
  check(((await mock()) || []).length > mockBefore, "the mock receiver got the test");
  check((await chItem("ui-check-hook")).url === MOCK_IN + "/webhook" && !("url_env" in (await chItem("ui-check-hook"))), "only the chosen source is in the file");
  check(await ev("document.querySelectorAll('#dlg-channel [type=submit]').length") === 1 && await ev("document.querySelector('#dlg-channel [data-submit]').type") === "button", "the primary is the only submit button (Enter never tests)");
  await click("#ch-name"); await key("Enter");
  await until("!document.getElementById('dlg-channel').open");
  check((await active()) === "channel:ui-check-hook", "Enter saves (no changes) and focus returns to the row", await active());
  // A duplicate name: the server's 422 lands on Name.
  await click("#ch-add"); await typeText("slack");
  await click("#ch-url-env"); await typeText("RH_X"); await key("Enter");
  await until("document.getElementById('ch-name-e').textContent !== ''");
  check(/already exists/.test(await ev("document.getElementById('ch-name-e').textContent")), "a duplicate name is refused on the Name field");
  await key("Escape"); await sleep(100); await click("#dlg-channel [data-discard]"); await sleep(250);
  // Send test from the row: one click.
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await click('#ch-list tr[data-key="channel:ui-check-hook"] [data-test]');
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Test sent to “ui-check-hook”/.test(t.textContent))", 8000);
  check(/Test sent/.test(await ev("document.querySelector('#ch-list tr[data-key=\"channel:ui-check-hook\"] .test-res').textContent")), "Send test in the row: one click, the result in the row and a toast");

  // ---- rules: add (every channel ticked), checks across fields, switch ----
  await click("#ru-add");
  check(await ev("[...document.querySelectorAll('#dlg-rule input[name=channels]')].every((c) => c.checked)") && await ev("document.querySelectorAll('#dlg-rule input[name=channels]').length") === (await notif()).channels.length, "a new rule notifies every channel");
  await typeText("ui-check-rule");
  await ev("(() => { const s = document.getElementById('ru-type'); s.value = 'host'; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  check(await ev(vis("#ru-metric")) && await ev(vis("#ru-disk")) && !(await ev(vis("#ru-containers"))), "type Host metric shows Metric and Disk, hides Containers");
  await ev("(() => { const s = document.getElementById('ru-metric'); s.value = 'memory'; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  check(!(await ev(vis("#ru-disk"))), "Disk only for the disk metric");
  await click("#ru-name"); await key("Enter"); await sleep(100);
  check(await ev("document.getElementById('ru-above-e').textContent") === "Set exactly one of: Above or Below.", "exactly one of Above or Below, in plain words", await ev("document.getElementById('ru-above-e').textContent"));
  await click("#ru-above"); await typeText("95");
  await ev("[...document.querySelectorAll('#dlg-rule input[name=channels]')].forEach((c) => { c.checked = c.value === 'ui-check-hook'; })");
  await key("Enter");
  await until("!document.getElementById('dlg-rule').open");
  check((await ruRows()).includes("ui-check-rule") && JSON.stringify((await ruItem("ui-check-rule")).channels) === '["ui-check-hook"]' && (await ruItem("ui-check-rule")).above === 95, "the rule is added with the ticked channel", await ruItem("ui-check-rule"));
  check((await active()) === "ru-add", "focus returns to Add rule", await active());
  const swSel = '#ru-list tr[data-key="rule:ui-check-rule"] [data-toggle]';
  await sleep(400); // the dialog's closing transition
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())"); // they sit over the last rows
  await click(swSel);
  await until(`!document.querySelector('${swSel}').checked && [...document.querySelectorAll('body>.toasts>.toast')].some((t) => /switched off/.test(t.textContent))`);
  check((await ruItem("ui-check-rule")).enabled === false, "the row switch turns the rule off at once (saved)");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await click(swSel);
  await until(`document.querySelector('${swSel}').checked`);
  await sleep(300);
  check(!("enabled" in (await ruItem("ui-check-rule"))), "and on again (the key goes: on is the default)");

  // ---- a channel rename that rules follow; a delete a rule blocks ----
  await ev(`document.querySelector('#ch-list tr[data-key="channel:ui-check-hook"] .linkbtn').click()`); await sleep(250);
  await clear("#ch-name"); await typeText("ui-check-hook2"); await key("Enter");
  await until(noteShown("dlg-channel"));
  check(/The rule “ui-check-rule” sends to “ui-check-hook”\. Saving renames it to “ui-check-hook2”/.test(await note("dlg-channel")) && await isOpen("dlg-channel"), "a rename warns first and names the rule", await note("dlg-channel"));
  await click("#dlg-channel [type=submit]");
  await until("!document.getElementById('dlg-channel').open");
  check(!!(await chItem("ui-check-hook2")) && JSON.stringify((await ruItem("ui-check-rule")).channels) === '["ui-check-hook2"]', "Save again: the channel is renamed and the rule follows it");
  check(await ev(`!!document.querySelector('#ru-list tr[data-key="rule:ui-check-rule"]') && document.querySelector('#ru-list tr[data-key="rule:ui-check-rule"]').textContent.includes('ui-check-hook2')`), "the rule list behind shows the new name");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await click('#ch-list tr[data-key="channel:ui-check-hook2"] [data-menu]'); await sleep(200);
  await click("#m-ch [data-row-act=delete]"); await sleep(250);
  check(!(await isOpen("dlg-confirm")) && (await toasts()).some((x) => /^The channel “ui-check-hook2” is used by the rule “ui-check-rule”\./.test(x)), "deleting a channel a rule uses says which rule, without asking", await toasts());

  // ---- the base-hash rule, channels ----
  await outsideApi("channels", { action: "add", item: { name: "ui-check-c2", type: "webhook", url: MOCK_IN + "/webhook" } });
  await load("/settings/notifications");
  // 1. An outside change while an edit dialog is open.
  await ev(`document.querySelector('#ch-list tr[data-key="channel:ui-check-c2"] .linkbtn').click()`); await sleep(250);
  await outsideApi("channels", { action: "add", item: { name: "ui-check-c3", type: "webhook", url: MOCK_IN + "/webhook" } }); await sleep(1200);
  check(await ev(noteShown("dlg-channel")) === false && !(await chRows()).includes("ui-check-c3"), "channels: an outside change does not refresh the list under an open dialog");
  await click("#dlg-channel details summary"); await click("#ch-timeout"); await typeText("20s"); await key("Enter");
  await until(noteShown("dlg-channel"));
  check(/changed on disk/.test(await note("dlg-channel")), "channels: the save is refused (409)");
  await click("#dlg-channel .dlg-note .btn.primary");
  await until("/Reloaded/.test(document.querySelector('#dlg-channel .dlg-note').textContent)");
  check(await ev("document.getElementById('ch-timeout').value") === "20s", "channels: Reload and keep keeps the input");
  await key("Enter");
  await until("!document.getElementById('dlg-channel').open");
  check((await chItem("ui-check-c2")).timeout === "20s" && !("timeout" in (await chItem("ui-check-c3"))), "channels: re-found and saved onto the same channel only");
  // The entry changed meanwhile: cannot be saved onto.
  await ev(`document.querySelector('#ch-list tr[data-key="channel:ui-check-c2"] .linkbtn').click()`); await sleep(250);
  await outsideApi("channels", { action: "edit", id: "ui-check-c2", item: { name: "ui-check-c2", type: "webhook", url: MOCK_IN + "/webhook", timeout: "30s" } }); await sleep(800);
  await click("#ch-url"); await typeText("x"); await key("Enter");
  await until(noteShown("dlg-channel"));
  await click("#dlg-channel .dlg-note .btn.primary");
  await until("/cannot be saved/.test(document.querySelector('#dlg-channel .dlg-note').textContent)");
  check(await ev("document.querySelector('#dlg-channel [type=submit]').disabled && document.querySelector('#dlg-channel [data-submit]').disabled"), "channels: a channel changed meanwhile cannot be saved onto (Save and Save and send test disabled)");
  await click("#ch-url");
  for (const type of ["rawKeyDown", "keyUp"]) await send("Input.dispatchKeyEvent", { type, key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, modifiers: 2 });
  await sleep(600);
  check((await chItem("ui-check-c2")).timeout === "30s" && /cannot be saved/.test(await note("dlg-channel")), "Ctrl+Enter does not save past a disabled Save");
  await click("#dlg-channel .dlg-note .btn"); await sleep(300);
  check((await chItem("ui-check-c2")).timeout === "30s", "channels: Discard leaves the outside change alone");
  // 2. An outside change while the delete confirm is open.
  await click('#ch-list tr[data-key="channel:ui-check-c3"] [data-menu]'); await sleep(200);
  await click("#m-ch [data-row-act=delete]"); await sleep(250);
  await outsideApi("channels", { action: "edit", id: "ui-check-c2", item: { name: "ui-check-c2", type: "webhook", url: MOCK_IN + "/webhook", timeout: null } }); await sleep(800);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted/.test(t.textContent))");
  check(!!(await chItem("ui-check-c3")), "channels: a delete confirmed after an outside change is refused");
  // Delete with Undo.
  await load("/settings/notifications");
  await click('#ch-list tr[data-key="channel:ui-check-c3"] [data-menu]'); await sleep(200);
  await click("#m-ch [data-row-act=delete]"); await sleep(250);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Deleted “ui-check-c3”/.test(t.textContent))");
  check(!(await chRows()).includes("ui-check-c3"), "channels: delete through the confirm removes the row");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].find((t) => /Deleted “ui-check-c3”/.test(t.textContent)).querySelector('button').click()");
  await until("[...document.querySelectorAll('#ch-list tr[data-key]')].some((r) => r.dataset.key === 'channel:ui-check-c3')");
  check(!!(await chItem("ui-check-c3")), "channels: Undo restores it");

  // ---- the base-hash rule, rules ----
  await outsideApi("alerts", { action: "add", item: { name: "ui-check-r2", type: "host", metric: "disk", above: 99, channels: [] } });
  await load("/settings/notifications");
  // 1. Edit dialog open, outside change, reload and keep.
  await ev(`document.querySelector('#ru-list tr[data-key="rule:ui-check-r2"] .linkbtn').click()`); await sleep(250);
  await outsideApi("alerts", { action: "add", item: { name: "ui-check-r3", type: "host", metric: "disk", above: 98, channels: [] } }); await sleep(1000);
  await clear("#ru-above"); await typeText("97"); await key("Enter");
  await until(noteShown("dlg-rule"));
  check(/changed on disk/.test(await note("dlg-rule")), "rules: a save after an outside change is refused (409)");
  await click("#dlg-rule .dlg-note .btn.primary");
  await until("/Reloaded/.test(document.querySelector('#dlg-rule .dlg-note').textContent)");
  await key("Enter");
  await until("!document.getElementById('dlg-rule').open");
  check((await ruItem("ui-check-r2")).above === 97 && (await ruItem("ui-check-r3")).above === 98, "rules: reload-and-keep saves onto the same rule only");
  // 2. Confirm open, outside change.
  await click('#ru-list tr[data-key="rule:ui-check-r3"] [data-menu]'); await sleep(200);
  await click("#m-ru [data-row-act=delete]"); await sleep(250);
  await outsideApi("alerts", { action: "edit", id: "ui-check-r2", item: { name: "ui-check-r2", type: "host", metric: "disk", above: 96, channels: [] } }); await sleep(800);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted/.test(t.textContent))");
  check(!!(await ruItem("ui-check-r3")), "rules: a delete confirmed after an outside change is refused");
  // 3. The row switch: its base is taken at the click; an outside change
  //    that lands before its write is answered 409 (the write is held back
  //    here so the outside change comes first).
  await load("/settings/notifications");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await ev("(() => { const f = window.fetch; window.fetch = (u, o) => (window.__hold && o && o.method === 'POST' ? new Promise((r) => setTimeout(r, 1500)).then(() => f(u, o)) : f(u, o)); window.__hold = true; })()");
  await ev(`document.querySelector('#ru-list tr[data-key="rule:ui-check-r3"] [data-toggle]').click()`);
  await outsideApi("alerts", { action: "edit", id: "ui-check-r2", item: { name: "ui-check-r2", type: "host", metric: "disk", above: 95, channels: [] } });
  await ev("window.__hold = false");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not switched/.test(t.textContent))");
  check(!("enabled" in (await ruItem("ui-check-r3"))) && await ev(`document.querySelector('#ru-list tr[data-key="rule:ui-check-r3"] [data-toggle]').checked`), "rules: a switch based on an older file is refused and flips back");

  // =================================================================
  // ---- Widgets (wave 2b) ----
  const wdata = async () => (await api("GET", "widgets")).data;
  const wItem = async (id) => { const w = (await wdata()).widgets.find((x) => x.id === id); return w && w.item; };
  const wRows = () => ev("[...document.querySelectorAll('#w-list tr[data-key]')].map((r) => r.dataset.key.slice(7))");
  await load("/settings/widgets");
  check(JSON.stringify(await wRows()) === JSON.stringify((await wdata()).widgets.filter((w) => !w.locked).map((w) => w.id)), "widgets: the list is rendered by the server (custom-api only)", await wRows());
  check(await ev("!document.querySelector('#w-locked button, #w-locked input, #w-locked a[data-dlg]')"), "command widgets stay read-only (no controls)");
  await click("#w-add");
  check(await isOpen("dlg-widget") && (await active()) === "wf-id", "Add widget opens on Id");
  await click("#wf-t-display"); await sleep(100);
  await click("#wf-template"); await key("Enter"); await sleep(150);
  es = await ev("[...document.querySelectorAll('#dlg-widget .fld-err:not([hidden])')].map((e) => e.id)");
  check(es.length === 2 && es.includes("wf-id-e") && es.includes("wf-url-e"), "widget: both errors at once, found in a tab that was not shown", es);
  check(await ev("document.getElementById('wf-t-req').getAttribute('aria-selected')") === "true" && (await active()) === "wf-id", "the tab with the first error is selected and its field focused");
  await typeText("ui-check-w");
  await click("#wf-url"); await typeText(MOCK_IN + "/_received");
  await click("#wf-t-fields"); await sleep(100);
  await click('#dlg-widget [name="fk.0"]'); await typeText("count");
  await click('#dlg-widget [name="fv.0"]'); await typeText("length");
  await click("#dlg-widget [data-kv-add=field]");
  check(await ev("document.activeElement.name") === "fk.1", "Add field adds a row and focuses it");
  await typeText("first"); await click('#dlg-widget [name="fv.1"]'); await typeText("0.kind");
  await click("#dlg-widget [data-act=wf-preview]");
  await until("!document.getElementById('wf-preview').hidden && !/Fetching/.test(document.getElementById('wf-preview').textContent)", 8000);
  const pv = await ev("({ text: document.getElementById('wf-preview').textContent, got: [...document.querySelectorAll('#dlg-widget .kv-got')].map((g) => g.textContent) })");
  check(/OK/.test(pv.text) && pv.got.every((g) => g.startsWith("→ ")), "Fetch now shows the result and what each path found, in the dialog", pv);
  check(!/"kind"\s*:/.test(pv.text), "Fetch now shows extracted values only, never the raw response");
  await click("#wf-t-display"); await sleep(100);
  await click("#wf-template"); await typeText("{{count}} received");
  await key("Enter");
  await until("!document.getElementById('dlg-widget').open");
  const wi = await wItem("ui-check-w");
  check(!!wi && wi.fields.count === "length" && wi.fields.first === "0.kind" && wi.template === "{{count}} received", "the widget is added with its fields and template", wi);
  check((await wRows()).at(-1) === "ui-check-w" && (await active()) === "w-add", "the row is in the list; focus returns to Add widget", await active());
  // Reorder with a command widget in between: the full order is sent.
  const allBefore = (await wdata()).widgets.map((w) => w.id);
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-w"] [data-move=up]').click()`);
  await until(`[...document.querySelectorAll('#w-list tr[data-key]')].map((r) => r.dataset.key).indexOf('widget:ui-check-w') === ${(await wRows()).length - 2}`);
  const allAfter = (await wdata()).widgets.map((w) => w.id);
  check(allAfter.length === allBefore.length && allAfter.includes("procs") && allAfter.indexOf("ui-check-w") < allBefore.indexOf("ui-check-w"), "↑ moves the widget past its neighbour; the command widget stays", [allBefore, allAfter]);
  // A rule that watches it: rename warns and the rule follows; delete is blocked.
  await outsideApi("alerts", { action: "add", item: { name: "ui-check-wr", type: "widget", widget: "ui-check-w", field: "count", above: 1000, channels: [] } });
  await load("/settings/widgets");
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-w"] .linkbtn').click()`); await sleep(250);
  await clear("#wf-id"); await typeText("ui-check-w2"); await key("Enter");
  await until(noteShown("dlg-widget"));
  check(/The rule “ui-check-wr” watches the widget “ui-check-w”/.test(await note("dlg-widget")), "widget rename warns first and names the rule", await note("dlg-widget"));
  await click("#dlg-widget [type=submit]");
  await until("!document.getElementById('dlg-widget').open");
  check(!!(await wItem("ui-check-w2")) && (await ruItem("ui-check-wr")).widget === "ui-check-w2", "Save again: renamed, and the rule watches the new id");
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  await click('#w-list tr[data-key="widget:ui-check-w2"] [data-menu]'); await sleep(200);
  await click("#m-w [data-row-act=delete]"); await sleep(250);
  check(!(await isOpen("dlg-confirm")) && (await toasts()).some((x) => /^The widget “ui-check-w2” is used by the rule “ui-check-wr”\./.test(x)), "deleting a widget a rule watches says which rule", await toasts());
  // The base-hash rule, widgets.
  await outsideApi("widgets", { action: "add", item: { id: "ui-check-w3", type: "custom-api", url: MOCK_IN + "/_received" } });
  await load("/settings/widgets");
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-w3"] .linkbtn').click()`); await sleep(250);
  await outsideApi("widgets", { action: "add", item: { id: "ui-check-w4", type: "custom-api", url: MOCK_IN + "/_received" } }); await sleep(1000);
  check(!(await wRows()).includes("ui-check-w4"), "widgets: nothing refreshes under an open dialog");
  await click("#wf-title"); await typeText("W3"); await key("Enter");
  await until(noteShown("dlg-widget"));
  check(/changed on disk/.test(await note("dlg-widget")), "widgets: the save is refused (409)");
  await click("#dlg-widget .dlg-note .btn.primary");
  await until("/Reloaded/.test(document.querySelector('#dlg-widget .dlg-note').textContent)");
  await key("Enter");
  await until("!document.getElementById('dlg-widget').open");
  check((await wItem("ui-check-w3")).title === "W3" && !(await wItem("ui-check-w4")).title, "widgets: reload-and-keep saves onto the same widget only");
  await click('#w-list tr[data-key="widget:ui-check-w4"] [data-menu]'); await sleep(200);
  await click("#m-w [data-row-act=delete]"); await sleep(250);
  await outsideApi("widgets", { action: "edit", id: "ui-check-w3", item: { id: "ui-check-w3", type: "custom-api", url: MOCK_IN + "/_received", title: null } }); await sleep(800);
  await click("#dlg-confirm [data-ok]");
  await until("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /Not deleted/.test(t.textContent))");
  check(!!(await wItem("ui-check-w4")), "widgets: a delete confirmed after an outside change is refused");
  // System tiles: one switch that saves.
  await load("/settings/widgets");
  const sysWas = (await wdata()).system.enabled;
  await click("#sys-tiles");
  await until(`[...document.querySelectorAll('body>.toasts>.toast')].some((t) => /System tiles/.test(t.textContent))`);
  check((await wdata()).system.enabled === !sysWas, "System tiles saves when switched");
  await click("#sys-tiles"); await sleep(600);
  check((await wdata()).system.enabled === sysWas, "and back");

  // ---- review fixes: an edit never removes what the form does not show ----
  await outsideApi("channels", { action: "add", item: { name: "ui-check-keep", type: "webhook", url: MOCK_IN + "/webhook", timeout: "7s", headers: { "X-Plain": "keepvalue-1" }, body: '{"text": "{{message}}"}' } });
  await outsideApi("widgets", { action: "add", item: { id: "ui-check-wk", type: "custom-api", url: MOCK_IN + "/_received", method: "post", body: '{"q":1}', output: "json", headers: { "X-Plain": "keepvalue-2" }, template: "{{_status}}" } });
  await load("/settings/notifications");
  check(!(await ev("document.documentElement.outerHTML.includes('keepvalue')")), "header values are never in the page");
  // Every settings read fails from here: a save does not need one.
  await ev("(() => { const f = window.fetch; window.__reads = 0; window.fetch = (u, o) => { if (window.__failReads && String(u).includes('/api/settings/') && (!o || !o.method || o.method === 'GET')) { window.__reads++; return Promise.resolve(new Response('{}', { status: 500 })); } if (o && o.method === 'POST' && String(u).includes('widgets/preview')) window.__previews = (window.__previews || 0) + 1; return f(u, o); }; window.__failReads = true; })()");
  await ev(`document.querySelector('#ch-list tr[data-key="channel:ui-check-keep"] .linkbtn').click()`); await sleep(250);
  await click("#dlg-channel details summary"); await clear("#ch-timeout"); await typeText("8s"); await key("Enter");
  await until("!document.getElementById('dlg-channel').open");
  let ki = await chItem("ui-check-keep");
  check(ki.timeout === "8s" && ki.headers && ki.headers["X-Plain"] === "keepvalue-1" && /message/.test(ki.body || ""), "kept keys survive an edit (headers and body stay), with every settings read failing", ki);
  await load("/settings/widgets");
  await ev("(() => { const f = window.fetch; window.__previews = 0; window.fetch = (u, o) => { if (String(u).includes('/api/settings/widgets') && (!o || !o.method || o.method === 'GET')) return Promise.resolve(new Response('{}', { status: 500 })); if (o && o.method === 'POST' && String(u).includes('widgets/preview')) window.__previews++; return f(u, o); }; })()");
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-wk"] .linkbtn').click()`); await sleep(250);
  await click("#wf-title"); await typeText("Kept"); await key("Enter");
  await until("!document.getElementById('dlg-widget').open");
  ki = await wItem("ui-check-wk");
  check(ki.title === "Kept" && ki.method === "post" && ki.body === '{"q":1}' && ki.output === "json" && ki.headers["X-Plain"] === "keepvalue-2", "a widget edit keeps method as typed, body, output and headers", ki);
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-wk"] .linkbtn').click()`); await sleep(250);
  await click("#dlg-widget [data-act=wf-preview]");
  await until("!document.getElementById('wf-preview').hidden && !/Fetching/.test(document.getElementById('wf-preview').textContent)");
  check(/^Not fetched: the widget's headers or body could not be read/.test(await ev("document.getElementById('wf-preview').textContent")) && await ev("window.__previews") === 0, "a failed read: Fetch now says so and fetches nothing (never without the headers)", await ev("document.getElementById('wf-preview').textContent"));
  await key("Escape"); await sleep(250);
  // A field-name error from the server lands on the Fields list (its tab selected).
  await load("/settings/widgets");
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-wk"] .linkbtn').click()`); await sleep(250);
  await click("#wf-t-fields");
  await click('#dlg-widget [name="fk.0"]'); await typeText("bad name");
  await click('#dlg-widget [name="fv.0"]'); await typeText("x");
  await click("#wf-t-req"); await click("#wf-title"); await key("Enter");
  await until("document.getElementById('wf-fields-e').textContent !== ''");
  check(/^Field name "bad name"/.test(await ev("document.getElementById('wf-fields-e').textContent")) && await ev("document.getElementById('wf-t-fields').getAttribute('aria-selected')") === "true", "a bad field name is shown under the fields, the Fields tab selected", await ev("document.getElementById('wf-fields-e').textContent"));
  await click("#dlg-widget [data-act=wf-preview]");
  await until("!document.getElementById('wf-preview').hidden && !/Fetching/.test(document.getElementById('wf-preview').textContent)");
  check(/fix the highlighted field/.test(await ev("document.getElementById('wf-preview').textContent")), "Fetch now reports the same error on the field, not raw text", await ev("document.getElementById('wf-preview').textContent"));
  await key("Escape"); await sleep(100);
  if (await ev("!document.querySelector('#dlg-widget .dlg-guard').hidden")) await click("#dlg-widget [data-discard]");
  await sleep(250);

  // ---- the new dialogs on a phone ----
  await load("/settings/notifications", 390, true);
  await ev("document.querySelector('#ch-add').click()"); await sleep(400);
  const pm = await ev("(() => { const r = document.getElementById('dlg-channel').getBoundingClientRect(), i = document.getElementById('ch-name'), s = document.querySelector('#dlg-channel [type=submit]').getBoundingClientRect(), t = document.querySelector('#dlg-channel [data-submit]').getBoundingClientRect(), c = document.querySelector('#dlg-channel .choice label').getBoundingClientRect(); return { w: Math.round(r.width), bottom: Math.round(r.bottom), vh: innerHeight, inH: i.offsetHeight, fs: Math.round(parseFloat(getComputedStyle(i).fontSize)), saveH: Math.round(s.height), saveBottom: Math.round(s.bottom), testH: Math.round(t.height), choiceH: Math.round(c.height) }; })()");
  check(pm.w === 390 && pm.bottom === pm.vh && pm.saveBottom <= pm.vh, "phone: the channel dialog is a bottom sheet with Save in view", pm);
  check(pm.inH === 44 && pm.fs === 16 && pm.saveH === 44 && pm.testH === 44 && pm.choiceH >= 38, "phone: 16 px inputs, 44 px buttons", pm);
  await key("Escape"); await sleep(250);
  await load("/settings/widgets", 390, true);
  await ev(`document.querySelector('#w-list tr[data-key="widget:ui-check-w3"] .linkbtn').click()`); await sleep(400);
  const wm = await ev("(() => { const s = document.querySelector('#dlg-widget [type=submit]').getBoundingClientRect(); return { saveBottom: Math.round(s.bottom), vh: innerHeight, sx: document.getElementById('dlg-widget').scrollWidth <= 390 }; })()");
  check(wm.saveBottom <= wm.vh && wm.sx, "phone: the widget dialog keeps Save in view and does not scroll sideways", wm);
  await key("Escape"); await sleep(250);

  // ---- app.js failed to load: the no-JavaScript presentation, no errors ----
  await send("Network.setBlockedURLs", { urls: ["*/static/app.js*"] });
  errors.length = 0;
  await load("/settings/security");
  await sleep(300);
  check(await ev("getComputedStyle(document.querySelector('#f-password .needs-js')).display !== 'none' && document.querySelector('#f-password [type=submit]').disabled"), "without app.js the note shows and Save stays disabled");
  check(errors.length === 0, "without app.js settings.js throws nothing", errors);
  await send("Network.setBlockedURLs", { urls: [] });

  // ---- session gone: the note offers a sign-in, the input stays, Save works after ----
  await load("/settings/bookmarks");
  await click("#bm-add"); await typeText("ui-check-b");
  await click("#bm-url"); await typeText("https://ui-check.example/b");
  await send("Network.deleteCookies", { name: cookie.split("=")[0], url });
  await key("Enter");
  await until("!document.querySelector('#dlg-bookmark .dlg-note').hidden");
  check(/session expired/.test(await ev("document.querySelector('#dlg-bookmark .dlg-note').textContent")) && await ev("document.querySelector('#dlg-bookmark .dlg-note .btn').textContent") === "Sign in (new tab)", "401: a note with Sign in (new tab)");
  check(await ev("document.getElementById('bm-name').value") === "ui-check-b", "401: the input is kept");
  await send("Network.setCookie", { name: cookie.split("=")[0], value: cookie.split("=")[1], url });
  await click("#dlg-bookmark [type=submit]");
  await until("!document.getElementById('dlg-bookmark').open");
  check(!(await isOpen("dlg-bookmark")) && (await rows()).includes("ui-check-b"), "after signing in again, Save goes through");

  // ---- a deep link opens the dialog on load ----
  const k = await ev(`${rowSel("ui-check-b")}.dataset.key`);
  await load(`/settings/bookmarks?edit=${k}`);
  check(await isOpen("dlg-bookmark") && await ev("document.getElementById('bm-name').value") === "ui-check-b", "?edit=<key> opens the filled dialog on load");
  check(await ev("getComputedStyle(document.querySelector('.dlg-page')).display") === "none", "with the script the page form is hidden");
  await key("Escape"); await sleep(200);
  check(await ev("location.search === ''"), "closing a landed dialog removes ?edit=");

  // ---- Change password: every problem at once, nothing in the URL ----
  await load("/settings/security");
  check(await ev("!document.querySelector('#f-password [type=submit]').disabled"), "Change password is enabled once the script runs");
  await click("#pw-current"); await key("Enter"); await sleep(100);
  check(await ev("[...document.querySelectorAll('#f-password .fld-err:not([hidden])')].length") === 3, "an empty password form shows three errors at once");
  await typeText(" not the password ");
  await click("#pw-new"); await typeText("short"); await click("#pw-repeat"); await typeText("other"); await key("Enter"); await sleep(100);
  const pe = await ev("[...document.querySelectorAll('#f-password .fld-err:not([hidden])')].map((e) => e.textContent)");
  check(pe.length === 2 && pe.includes("Use at least 8 characters.") && pe.includes("Does not match the new password."), "short and mismatching are shown together", pe);
  check(await ev("HB.values(document.getElementById('f-password')).current") === " not the password ", "passwords are not trimmed");
  // One wrong current password counts against the sign-in rate limit and is
  // recorded as a failed login: only with CHECK_WRONG_PASSWORD=1.
  if (process.env.CHECK_WRONG_PASSWORD === "1") {
    await ev("document.getElementById('pw-new').value = document.getElementById('pw-repeat').value = 'a new password 1'");
    await key("Enter");
    await until("document.getElementById('pw-current-e').textContent !== ''", 6000);
    check(await ev("document.getElementById('pw-current-e').textContent") === "The current password is wrong.", "a wrong current password is reported on its field");
  }
  check(await ev("location.href === location.origin + '/settings/security'"), "no password ever reaches the URL", await ev("location.href"));

  // ---- phone: bottom sheet above the bottom bar, 16 px inputs, 44 px controls ----
  await load("/settings/bookmarks", 390, true);
  await ev("document.querySelector('#bm-add').click()"); await sleep(400);
  const m = await ev("(() => { const r = document.getElementById('dlg-bookmark').getBoundingClientRect(), i = document.getElementById('bm-url'), s = document.querySelector('#dlg-bookmark [type=submit]').getBoundingClientRect(); return { w: Math.round(r.width), bottom: Math.round(r.bottom), vh: innerHeight, inH: i.offsetHeight, fs: Math.round(parseFloat(getComputedStyle(i).fontSize)), saveH: Math.round(s.height), saveBottom: Math.round(s.bottom) }; })()");
  check(m.w === 390 && m.bottom === m.vh, "phone: a full-width bottom sheet", m);
  check(m.inH === 44 && m.fs === 16 && m.saveH === 44, "phone: 44 px inputs and buttons with 16 px text", m);
  check(m.saveBottom <= m.vh, "phone: Save is in view", m);
  await key("Escape"); await sleep(250);
  await ev("HB.toast('ui-check toast')"); await sleep(250);
  const tb = await ev("(() => { const t = document.querySelector('body>.toasts>.toast:last-child').getBoundingClientRect(), b = document.getElementById('bbar').getBoundingClientRect(); return { toast: Math.round(t.bottom), bar: Math.round(b.top) }; })()");
  check(tb.toast <= tb.bar, "phone: the toast sits above the bottom bar", tb);

  // ---- without JavaScript: readable values, saving disabled with the reason ----
  await send("Emulation.setScriptExecutionDisabled", { value: true });
  await load(`/settings/bookmarks?edit=${k}`);
  const nj = await ev("(() => { const p = document.querySelector('.dlg-page'); return { shown: getComputedStyle(p).display !== 'none', name: p.querySelector('[name=name]').value, save: p.querySelector('[type=submit]').disabled, note: /Saving needs JavaScript/.test(p.textContent), post: p.querySelector('form').method }; })()").catch(() => null);
  // Script execution is off for the page, but Runtime.evaluate still runs.
  check(nj && nj.shown && nj.name === "ui-check-b" && nj.save && nj.note && nj.post === "post", "no JS: ?edit= is a filled page form, Save disabled, with the reason", nj);
  for (const [path, field, value] of [["/settings/notifications?edit=channel:ui-check-c2", "url", MOCK_IN + "/webhook"], ["/settings/notifications?edit=rule:ui-check-r2", "above", "95"], ["/settings/widgets?edit=widget:ui-check-w3", "url", MOCK_IN + "/_received"]]) {
    await load(path);
    const pj = await ev(`(() => { const p = document.querySelector('.dlg-page'); return { shown: getComputedStyle(p).display !== 'none', value: p.querySelector('[name=${field}]').value, save: p.querySelector('[type=submit]').disabled, note: /Saving needs JavaScript\. .*You can read the values here/.test(p.textContent), post: p.querySelector('form').getAttribute('method') }; })()`).catch((e) => String(e));
    check(pj && pj.shown && pj.value === value && pj.save && pj.note && pj.post === "post", `no JS: ${path.split("?")[1]} is a filled page form, Save disabled, with the reason`, pj);
  }
  await load("/settings/security");
  check(await ev("document.querySelector('#f-password').method === 'post' && document.querySelector('#f-password [type=submit]').disabled && getComputedStyle(document.querySelector('#f-password .needs-js')).display !== 'none'"), "no JS: Change password posts, is disabled and says why");
  await send("Emulation.setScriptExecutionDisabled", { value: false });

  // ---- round trip: Save with no change writes nothing, for every entry ----
  // Optional: with CONFIG_FILE (the test instance's config file on this
  // machine) and RICH_CONFIG (a config with every kind of entry: bound
  // channels and widgets, headers, bodies, every rule type), that file is
  // put in place as a hand edit, every entry is opened and saved unchanged,
  // and the run's clean-up writes the starting file back.
  if (process.env.CONFIG_FILE && process.env.RICH_CONFIG) {
    const rich = fs.readFileSync(process.env.RICH_CONFIG, "utf8");
    fs.writeFileSync(process.env.CONFIG_FILE, rich);
    const richHash = (await api("GET", "config")).data.hash;
    let bad = [];
    for (const [path, list, dlg] of [["/settings/notifications", "ch-list", "dlg-channel"], ["/settings/notifications", "ru-list", "dlg-rule"], ["/settings/widgets", "w-list", "dlg-widget"]]) {
      await load(path);
      const n = await ev(`document.querySelectorAll('#${list} tr[data-key]').length`);
      for (let i = 0; i < n; i++) {
        await ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
        const k = await ev(`document.querySelectorAll('#${list} tr[data-key]')[${i}].dataset.key`);
        await ev(`document.querySelectorAll('#${list} tr[data-key]')[${i}].querySelector('.linkbtn').click()`); await sleep(250);
        await click(`#${dlg} [type=submit]`);
        await until(`!document.getElementById('${dlg}').open`, 3000);
        const t = await toasts(), h = (await api("GET", "config")).data.hash;
        if (await isOpen(dlg) || h !== richHash || !t.includes("No changes")) {
          bad.push({ k, open: await isOpen(dlg), note: await note(dlg), toast: t, changed: h !== richHash });
          if (await isOpen(dlg)) { await key("Escape"); await sleep(100); }
          if (h !== richHash) fs.writeFileSync(process.env.CONFIG_FILE, rich);
          await load(path);
        }
      }
    }
    check(!bad.length, "round trip: every channel, rule and widget of the rich config saves unchanged with \"No changes\"", bad);
    // One field changed: only that line changes.
    await load("/settings/notifications");
    const keyOf = async (list, name) => ev(`[...document.querySelectorAll('#${list} tr[data-key]')].find((r) => r.dataset.key.endsWith(':' + ${JSON.stringify(name)})).dataset.key`);
    for (const [list, dlg, name, sel, val] of [["ch-list", "dlg-channel", "tg2", "#ch-timeout", "9s"], ["ru-list", "dlg-rule", "logs", "#ru-count", "4"]]) {
      const k = await keyOf(list, name);
      await ev(`document.querySelector('#${list} tr[data-key="${k}"] .linkbtn').click()`); await sleep(250);
      if (sel === "#ch-timeout") await click(`#${dlg} details summary`);
      await clear(sel); await typeText(val); await key("Enter");
      await until(`!document.getElementById('${dlg}').open`);
      const now = (await api("GET", "config")).data.text;
      const a = rich.split("\n"), b = now.split("\n");
      const gone = a.filter((l) => !b.includes(l)), added = b.filter((l) => !a.includes(l));
      check(gone.length <= 1 && added.length === 1 && added[0].includes(val), `one field changed on ${name}: only its line changes`, { gone, added });
      fs.writeFileSync(process.env.CONFIG_FILE, rich); await load("/settings/notifications");
    }
  }

  {
  // ======================================================================
  // Wave 2c: Containers and groups, General, Appearance, History and data,
  // Config file, Security. Every form: values rendered by the server, all
  // errors at once, a no-change round trip ("No changes", file hash
  // unchanged), and the base-hash rule where a dialog, confirm, switch or
  // arrow starts a write (an outside write meanwhile gives a 409, never a
  // write onto a moved file).
  // ======================================================================
  const fileHash = async () => (await api("GET", "config")).data.hash;
  const fileText = async () => (await api("GET", "config")).data.text;
  // An outside change that moves the file but no value (a hand edit).
  let outsideN = 0;
  async function outsideTouch() {
    const c = (await api("GET", "config")).data;
    const r = await api("POST", "config", { base_hash: c.hash, text: c.text + `\n# ui-check outside ${++outsideN}\n` });
    if (r.status !== 200) throw new Error(`outside touch: HTTP ${r.status} ${JSON.stringify(r.data)}`);
  }
  const hold = (on) => ev(on ? "(() => { if (!window.__held) { const f = window.fetch; window.fetch = (u, o) => (window.__hold && o && o.method === 'POST' ? new Promise((r) => setTimeout(r, 900)).then(() => f(u, o)) : f(u, o)); window.__held = true; } window.__hold = true; })()" : "window.__hold = false");
  const clearToasts = () => ev("[...document.querySelectorAll('body>.toasts>.toast')].forEach((t) => t.remove())");
  const lastToast = async () => (await toasts()).at(-1) || "";
  const noteOf = (sel) => ev(`(document.querySelector(${JSON.stringify(sel)} + ' .dlg-note') || {}).textContent || ''`);
  async function ctrlS() {
    const b = { key: "s", code: "KeyS", windowsVirtualKeyCode: 83, nativeVirtualKeyCode: 83, modifiers: 2 };
    await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...b });
    await send("Input.dispatchKeyEvent", { type: "keyUp", ...b });
    await sleep(80);
  }
  const rowKey = (list, text) => ev(`(() => { const r = [...document.querySelectorAll('#${list} tr[data-key]')].find((r) => r.querySelector('[data-item]').textContent.trim() === ${JSON.stringify(text)}); return r ? r.dataset.key : null; })()`);
  const openRow = async (list, key) => { await ev(`document.querySelector('#${list} tr[data-key="${key}"] [data-dlg]').click()`); await sleep(250); };
  const menuDelete = async (list, key, menu) => {
    await click(`#${list} tr[data-key="${key}"] [data-menu]`); await sleep(150);
    await click(`#${menu} [data-row-act=delete]`); await sleep(200);
  };
  const confirmOpen = () => isOpen("dlg-confirm");

  // ---- Containers and groups ----
  await load("/settings/containers");
  check(await ev("document.querySelectorAll('#set-main [type=submit]').length === 0 && !document.querySelector('#f-exclude,#f-groups,#f-override,#ex-new,#grp-new')"), "containers: no staged list, no inline editor, no Save outside a dialog");
  // Hidden containers: add saves at once, live match line, duplicate on the field.
  let h0 = await fileHash();
  await click("#ex-add");
  check(await isOpen("dlg-exclude") && (await active()) === "ex-pattern" && await ev("location.search === '?add=exclude'"), "pattern: Add opens the dialog on Pattern, ?add=exclude");
  await key("Enter"); await sleep(100);
  check(await ev("document.getElementById('ex-pattern-e').textContent") === "Required.", "pattern: empty is Required on the field");
  await typeText("ui-check-*");
  check(/Matches no container now/.test(await ev("document.getElementById('ex-match').textContent")), "pattern: a live line says what it matches", await ev("document.getElementById('ex-match').textContent"));
  await key("Enter");
  await until("!document.getElementById('dlg-exclude').open"); await sleep(300);
  check((await fileText()).includes('"ui-check-*"'), "pattern: Add writes at once (no separate Save)");
  check((await toasts()).some((x) => /^Added “ui-check-\*” · revision #\d+/.test(x)), "pattern: toast says what happened", await toasts());
  check(await ev("document.activeElement.id") === "ex-add", "pattern: focus back on Add pattern", await active());
  await click("#ex-add"); await typeText("ui-check-*"); await key("Enter");
  await until("document.getElementById('ex-pattern-e').textContent !== ''");
  check(await ev("document.getElementById('ex-pattern-e').textContent") === "This pattern is already in the list.", "pattern: a duplicate is refused on the field");
  await key("Escape");
  check(await ev("!document.querySelector('#dlg-exclude .dlg-guard').hidden"), "pattern: Esc with input asks first");
  await click("#dlg-exclude [data-discard]"); await sleep(200);
  // No-change round trip and the 409 retarget on an edit.
  let kk = await rowKey("ex-list", "ui-check-*");
  h0 = await fileHash(); await clearToasts();
  await openRow("ex-list", kk); await key("Enter");
  await until("!document.getElementById('dlg-exclude').open"); await sleep(200);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "pattern: saving unchanged writes nothing and says No changes");
  await openRow("ex-list", kk);
  await clear("#ex-pattern"); await typeText("ui-check-x*");
  await outsideTouch();
  await key("Enter"); await until(`!document.querySelector('#dlg-exclude .dlg-note').hidden`);
  check(/changed on disk/.test(await noteOf("#dlg-exclude")), "pattern: an outside write meanwhile is a 409 in the dialog");
  await click("#dlg-exclude .dlg-note .btn.primary"); await sleep(400);
  check(/Save again/.test(await noteOf("#dlg-exclude")), "pattern: Reload and keep my input re-finds the same pattern");
  await key("Enter"); await until("!document.getElementById('dlg-exclude').open"); await sleep(300);
  check((await fileText()).includes('"ui-check-x*"') && !(await fileText()).includes('"ui-check-*"'), "pattern: the edit lands on the same pattern");
  // Delete: the one confirm, then Undo; a delete whose file moved is refused.
  kk = await rowKey("ex-list", "ui-check-x*");
  await menuDelete("ex-list", kk, "m-ex");
  check(await confirmOpen() && /ui-check-x\*/.test(await ev("document.getElementById('dlg-cf-t').textContent")), "pattern: delete asks with HB.confirm (no native confirm)");
  await outsideTouch();
  await click("#dlg-confirm [data-ok]"); await sleep(500);
  check((await toasts()).some((x) => /^Not deleted: the config file changed/.test(x)) && (await fileText()).includes('"ui-check-x*"'), "pattern: a confirm based on a moved file deletes nothing", await toasts());
  await clearToasts();
  kk = await rowKey("ex-list", "ui-check-x*");
  await menuDelete("ex-list", kk, "m-ex"); await click("#dlg-confirm [data-ok]"); await sleep(600);
  check(!(await fileText()).includes('"ui-check-x*"') && (await toasts()).some((x) => /^Removed the pattern/.test(x)), "pattern: delete after the confirm", await toasts());
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].at(-1).querySelector('button.btn.sm.ghost:not(.icon)').click()"); await sleep(700);
  check((await fileText()).includes('"ui-check-x*"') && !!(await rowKey("ex-list", "ui-check-x*")), "pattern: Undo brings it back", await toasts());

  // Groups: add with a switch, edit, no change, one-click arrows, delete.
  await click("#grp-add");
  check(await isOpen("dlg-group") && (await active()) === "gr-name", "group: Add opens the dialog on Name");
  check(await ev("!document.querySelector('#dlg-group [name=columns], #dlg-group [name=layout]')"), "group: no Columns or Layout (§11.12)");
  await typeText("ui-check-g"); await click("#gr-title"); await typeText("UI check"); await click("#gr-collapsed"); await key("Enter");
  await until("!document.getElementById('dlg-group').open"); await sleep(300);
  let t = await fileText();
  check(/name = "ui-check-g"\ntitle = "UI check"\ncollapsed = true/.test(t), "group: Add writes name, title and collapsed at once", t.slice(-120));
  kk = "group:ui-check-g"; h0 = await fileHash(); await clearToasts();
  await openRow("grp-list", kk); await key("Enter"); await until("!document.getElementById('dlg-group').open"); await sleep(200);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "group: saving unchanged writes nothing and says No changes");
  await openRow("grp-list", kk); await clear("#gr-title"); await typeText("UI check 2"); await key("Enter");
  await until("!document.getElementById('dlg-group').open"); await sleep(300);
  check((await fileText()).includes('title = "UI check 2"') && await ev("document.activeElement.dataset.key") === kk, "group: edit saves in place, focus back on the row");
  const order = async () => (await fileText()).split("\n").filter((l) => /^name = /.test(l)).join("|");
  const before = await order();
  await click(`#grp-list tr[data-key="${kk}"] [data-move=up]`); await sleep(700);
  check((await order()) !== before && (await toasts()).some((x) => /^Moved “ui-check-g” up/.test(x)), "group: one click on ↑ saves the new order", await toasts());
  // Arrow: the base is taken at the click; an outside write while it is sent is a 409.
  const o2 = await order();
  await hold(true); await click(`#grp-list tr[data-key="${kk}"] [data-move=down]`); await outsideTouch(); await sleep(1300); await hold(false);
  check((await order()) === o2 && (await toasts()).some((x) => /^Not moved/.test(x)), "group: an arrow based on a moved file moves nothing", await toasts());
  await menuDelete("grp-list", kk, "m-grp"); await click("#dlg-confirm [data-ok]"); await sleep(600);
  check(!(await fileText()).includes("ui-check-g"), "group: delete after the confirm");

  // Container overrides: the dialog with all eight fields, errors, save,
  // Remove overrides with the confirm and Undo; ?name= opens the dialog.
  const free = await ev("(() => { const r = [...document.querySelectorAll('#ov-list tr[data-key]')].find((r) => !r.dataset.ov); return r ? r.dataset.key : null; })()");
  const has = await ev("(() => { const r = [...document.querySelectorAll('#ov-list tr[data-key]')].find((r) => r.dataset.ov); return r ? r.dataset.key : null; })()");
  check(!!free && !!has, "overrides: rows rendered by the server, with and without overrides");
  const cname = free.slice("override:".length);
  await openRow("ov-list", free);
  check(await isOpen("dlg-override") && await ev("[...new Set([...document.querySelectorAll('#dlg-override [name]')].map((c) => c.name))].join()") === "name,group,icon,url,description,hide,order,size", "override: one dialog with today's eight fields");
  check(await ev("document.querySelector('#dlg-override [data-act=dlg-delete]').hidden"), "override: no Remove for a container without overrides");
  await typeText("ui-check name"); await click("#ov-url"); await typeText("not a url"); await click("#ov-order"); await typeText("1.5"); await key("Enter"); await sleep(100);
  const oe = await ev("[...document.querySelectorAll('#dlg-override .fld-err:not([hidden])')].map((e) => e.textContent)");
  check(oe.length === 2 && oe.some((x) => /^Use a whole number/.test(x)) && oe.includes("Use a link that starts with http:// or https://."), "override: URL and Order errors at once, on their fields", oe);
  await clear("#ov-url"); await clear("#ov-order"); await typeText("2"); await key("Enter");
  await until("!document.getElementById('dlg-override').open"); await sleep(300);
  t = await fileText();
  check(t.includes(`name = "ui-check name"\norder = 2`), "override: only the changed fields are written", t.slice(-160));
  h0 = await fileHash(); await clearToasts();
  await openRow("ov-list", free); await key("Enter"); await until("!document.getElementById('dlg-override').open"); await sleep(200);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "override: saving unchanged writes nothing and says No changes");
  await openRow("ov-list", has); await key("Enter"); await until("!document.getElementById('dlg-override').open"); await sleep(200);
  check((await fileHash()) === h0, "override: an override from the file saves unchanged byte for byte");
  await openRow("ov-list", free);
  check(await ev("!document.querySelector('#dlg-override [data-act=dlg-delete]').hidden"), "override: Remove overrides… once there are some");
  await click("#dlg-override [data-act=dlg-delete]"); await sleep(200);
  check(await confirmOpen(), "override: Remove overrides asks with HB.confirm");
  await click("#dlg-confirm [data-ok]"); await sleep(700);
  check(!(await fileText()).includes("ui-check name") && (await toasts()).some((x) => /^Removed the overrides of/.test(x)), "override: removed after the confirm, with a toast", await toasts());
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].at(-1).querySelector('button.btn.sm.ghost:not(.icon)').click()"); await sleep(700);
  check((await fileText()).includes("ui-check name"), "override: Undo brings them back");
  await load(`/settings/containers?name=${encodeURIComponent(cname)}`); await sleep(300);
  check(await isOpen("dlg-override") && await ev("document.getElementById('ov-name').value") === "ui-check name" && await ev("location.search.startsWith('?edit=override')"), "override: the container page's ?name= link opens the dialog");
  await key("Escape"); await sleep(200);
  // Bulk bar: in view, Set group with Undo, Remove asks, base taken at the click.
  await click(`#ov-list tr[data-key="${free}"] [data-sel]`);
  check(await ev("!document.getElementById('ov-bulk').hidden && document.getElementById('ov-bulk-n').textContent === '1 selected' && getComputedStyle(document.getElementById('ov-bulk')).position === 'sticky'"), "bulk: the bar appears with the count and stays in view");
  h0 = await fileHash();
  await click("#ov-bulk-group"); await typeText("ui-check-bulk"); await click("[data-bulk=group]"); await sleep(700);
  check((await fileText()).includes('group = "ui-check-bulk"') && (await toasts()).some((x) => /^Moved 1 container/.test(x)), "bulk: Set group writes, with a toast", await toasts());
  await ev("[...document.querySelectorAll('body>.toasts>.toast')].at(-1).querySelector('button.btn.sm.ghost:not(.icon)').click()"); await sleep(700);
  check((await fileHash()) === h0, "bulk: Undo puts the file back as it was");
  await click("[data-bulk=remove]"); await sleep(200);
  check(await confirmOpen(), "bulk: Remove overrides asks with HB.confirm");
  await click("#dlg-confirm [data-cancel]"); await sleep(200);
  check((await fileHash()) === h0, "bulk: Cancel writes nothing");
  await hold(true); await click("[data-bulk=hide]"); await outsideTouch(); await sleep(1300); await hold(false);
  check((await toasts()).some((x) => /^Not changed: the config file changed/.test(x)) && !(await fileText()).includes("hide = true"), "bulk: an action based on a moved file writes nothing", await toasts());

  // ---- General ----
  await load("/settings/general");
  const page0 = (await api("GET", "general")).data.page || {};
  check(await ev("document.getElementById('gen-greeting').value") === (page0.greeting || "") && await ev("!document.querySelector('#f-general [name=columns],#f-general [name=layout],#f-general [name=theme]')"), "general: values from the file; no Theme, Columns or Layout");
  h0 = await fileHash(); await clearToasts();
  await click("#f-general [type=submit]"); await sleep(300);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "general: saving unchanged writes nothing and says No changes");
  await ev("document.getElementById('gen-title').value = 'x'.repeat(81)");
  await clear("#gen-search-url"); await typeText("https://search.example/?q="); await key("Enter"); await sleep(150);
  const ge = await ev("[...document.querySelectorAll('#f-general .fld-err:not([hidden])')].map((e) => e.textContent)");
  check(ge.length === 2 && ge.includes("At most 80 characters.") && ge.includes("Put {q} where the search words go."), "general: errors at once, on their fields", ge);
  await clear("#gen-title"); await typeText("ui-check-title");
  await clear("#gen-search-url"); await typeText(page0.search_url || "https://duckduckgo.com/?q={q}");
  const tBefore = await fileText();
  await hold(true); await key("Enter");
  check(await ev("document.querySelector('#f-general [type=submit]').getAttribute('aria-busy')") === "true" && await ev("document.querySelector('#f-general [data-hb-body]').inert"), "general: Save is busy and the form inert while saving");
  await until("document.querySelector('#f-general [type=submit]').getAttribute('aria-busy') !== 'true'", 3000); await hold(false); await sleep(200);
  const tAfter = await fileText();
  const gone = tBefore.split("\n").filter((l) => !tAfter.split("\n").includes(l)), added = tAfter.split("\n").filter((l) => !tBefore.split("\n").includes(l));
  check(gone.length <= 1 && added.length === 1 && added[0] === 'title = "ui-check-title"', "general: only the changed field is written", { gone, added });
  check(await ev("document.querySelector('.brand-site').textContent") === "ui-check-title" && await ev("document.title.includes('ui-check-title')"), "general: the sidebar and the tab title follow");
  await clear("#gen-greeting"); await typeText("ui-check greet");
  await outsideTouch(); await key("Enter");
  await until("!document.querySelector('#f-general .dlg-note').hidden");
  check(/changed on disk/.test(await noteOf("#f-general")), "general: an outside write is a 409 note in the form, input kept");
  await click("#f-general .dlg-note .btn.primary"); await sleep(500);
  check(/Save again/.test(await noteOf("#f-general")), "general: Reload and keep my input moves on (nothing it shows changed)");
  await key("Enter"); await sleep(600);
  check((await fileText()).includes('greeting = "ui-check greet"'), "general: saved on top of the new file");

  // ---- Appearance ----
  await load("/settings/appearance");
  const css0 = (await api("GET", "appearance")).data;
  check(await ev("document.getElementById('css-text').value") === css0.css, "appearance: custom.css rendered exactly as saved");
  await ev("(() => { const s = document.getElementById('th-theme'); s.value = 'light'; s.dispatchEvent(new Event('change', { bubbles: true })); })()"); await sleep(600);
  check((await fileText()).includes('theme = "light"') && (await toasts()).some((x) => /^Default theme: Light/.test(x)), "appearance: the default theme saves when changed", await toasts());
  await hold(true);
  await ev("(() => { const s = document.getElementById('th-theme'); s.value = 'dark'; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await outsideTouch(); await sleep(1300); await hold(false); await sleep(300);
  check(!(await fileText()).includes('theme = "dark"') && (await toasts()).some((x) => /^Not changed/.test(x)) && await ev("document.getElementById('th-theme').value") === "light", "appearance: a theme change based on a moved file writes nothing and goes back", await toasts());
  await ev("(() => { const s = document.getElementById('th-theme'); s.value = ''; s.dispatchEvent(new Event('change', { bubbles: true })); })()"); await sleep(600);
  check(!(await fileText()).includes("theme ="), "appearance: Default removes the key");
  await clearToasts();
  await click("#f-css [type=submit]"); await sleep(300);
  check((await toasts()).includes("No changes") && (await api("GET", "appearance")).data.hash === css0.hash, "appearance: custom.css saved unchanged: No changes, nothing written");
  await click("#css-text"); await ev("(() => { const t = document.getElementById('css-text'); t.setSelectionRange(t.value.length, t.value.length); })()");
  await typeText("\n:root { --ctl: #8a8a8a }\n"); await click("#f-css [type=submit]"); await sleep(600);
  check(/--ctl-border/.test(await ev("document.getElementById('css-warn').textContent")) && (await toasts()).some((x) => /^Saved custom\.css/.test(x)), "appearance: a retired token name gets the warning with the new name", await ev("document.getElementById('css-warn').textContent"));
  await typeText("/* more */");
  const cnow = (await api("GET", "appearance")).data;
  await api("POST", "appearance", { base_hash: cnow.hash, css: cnow.css + "/* outside */" });
  await click("#f-css [type=submit]"); await until("!document.querySelector('#f-css .dlg-note').hidden");
  await click("#f-css .dlg-note .btn.primary"); await sleep(500);
  check(await ev("document.querySelector('#f-css [type=submit]').disabled") && /cannot be saved/.test(await noteOf("#f-css")), "appearance: custom.css changed elsewhere: Save stays off, input kept");
  await click("#f-css .dlg-note .btn.primary"); await sleep(500);
  check(await ev("!document.querySelector('#f-css [type=submit]').disabled && document.getElementById('css-text').value.endsWith('/* outside */')"), "appearance: Discard loads the current custom.css");

  // ---- History and data ----
  await load("/settings/history");
  await click("#hi-enabled"); await sleep(600);
  check((await fileText()).includes("enabled = false") && (await toasts()).some((x) => /^Recording off/.test(x)), "history: Recording saves when switched", await toasts());
  await click("#hi-sample-interval"); await ev("document.getElementById('hi-sample-interval').select()"); await typeText("abc"); await key("Enter"); await sleep(150);
  check(await ev("document.getElementById('hi-sample-interval-e').textContent") === "Use a duration like 30s, 5m or 2h.", "history: a bad interval is caught on the field with the unit hint");
  await ev("document.getElementById('hi-sample-interval').select()"); await typeText("1s"); await key("Enter");
  await until("document.getElementById('hi-sample-interval-e').textContent !== ''");
  check(await ev("document.getElementById('hi-sample-interval-e').textContent") === "Use a value from 5s to 5m.", "history: the server's range error lands on the field");
  await ev("document.getElementById('hi-sample-interval').select()"); await typeText("20s"); await key("Enter"); await sleep(600);
  check((await fileText()).includes('sample_interval = "20s"'), "history: the interval saves after the switch wrote (no 409 for the page's own save)");
  h0 = await fileHash(); await clearToasts();
  await click("#f-history [type=submit]"); await sleep(300);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "history: saving unchanged writes nothing and says No changes");
  // Clear history: confirm with the typed phrase; the final request is
  // answered by the page (mocked), so the dev history survives.
  const rows0 = (await api("GET", "history")).data.db.event_rows;
  await ev("(() => { const f = window.fetch; window.fetch = (u, o) => (String(u).endsWith('/history/clear') ? Promise.resolve(new Response('{\"ok\":true}', { status: 200, headers: { 'Content-Type': 'application/json' } })) : f(u, o)); })()");
  await click("#clr-btn"); await sleep(200);
  check(await confirmOpen() && await ev("!document.querySelector('#dlg-confirm [data-typed]').hidden && document.querySelector('#dlg-confirm [data-ok]').disabled"), "history: Clear history asks with the typed phrase; OK is off until typed");
  await typeText("Clear History");
  check(await ev("!document.querySelector('#dlg-confirm [data-ok]').disabled"), "history: the phrase enables OK");
  await click("#dlg-confirm [data-ok]"); await sleep(500);
  check((await toasts()).some((x) => /^History cleared/.test(x)), "history: the (mocked) clear reports in a toast", await toasts());
  check((await api("GET", "history")).data.db.event_rows >= rows0, "history: the dev history is untouched");

  // ---- Config file ----
  await load("/settings/config");
  check(await ev("document.getElementById('raw-text').value") === (await fileText()), "config: the editor holds the file exactly");
  h0 = await fileHash(); await clearToasts();
  await click("#raw-text"); await ctrlS(); await sleep(300);
  check((await toasts()).includes("No changes") && (await fileHash()) === h0, "config: Ctrl+S unchanged: No changes, nothing written");
  check(await ev("document.activeElement.id") === "raw-text", "config: after Ctrl+S the caret is back in the editor", await active());
  await ev("(() => { const t = document.getElementById('raw-text'); t.setSelectionRange(t.value.length, t.value.length); })()");
  await typeText("# ui-check raw\n"); await ctrlS(); await sleep(800);
  const rawSaved = { end: (await fileText()).slice(-40), toast: (await toasts()).some((x) => /^Saved config\.toml · revision #\d+/.test(x)), status: await ev("document.getElementById('raw-status').textContent") };
  check(rawSaved.end.endsWith("# ui-check raw\n") && rawSaved.toast && /Valid/.test(rawSaved.status), "config: Ctrl+S saves, the status line stays", rawSaved);
  check(/raw edit/.test(await ev("document.querySelector('#rev-list tr[data-rev]').textContent")), "config: the new revision is listed at once");
  await click("#rev-list tr[data-rev] [data-rev-act=view]"); await sleep(500);
  check(await isOpen("dlg-rev") && /ui-check raw/.test(await ev("document.getElementById('dlg-rev-b').textContent")), "config: View opens the revision in a dialog");
  await key("Escape"); await sleep(200);
  check(!(await isOpen("dlg-rev")), "config: Esc closes the revision viewer (SET-03)");
  await click("#rev-list tr[data-rev]:nth-child(2) [data-rev-act=diff]"); await sleep(500);
  check(await isOpen("dlg-rev") && /ui-check raw/.test(await ev("document.getElementById('dlg-rev-b').textContent")), "config: Diff opens in the dialog");
  await key("Escape"); await sleep(200);
  await click("#rev-list tr[data-rev]:nth-child(2) [data-rev-act=restore]"); await sleep(200);
  check(await confirmOpen(), "config: Restore asks with HB.confirm");
  await outsideTouch();
  await click("#dlg-confirm [data-ok]"); await sleep(600);
  check((await toasts()).some((x) => /^Not restored/.test(x)), "config: a restore based on a moved file writes nothing", await toasts());
  await ev("(() => { const t = document.getElementById('raw-text'); t.setSelectionRange(0, 0); })()");
  await typeText("# ui-check stale\n"); await outsideTouch(); await ctrlS();
  await until("!document.querySelector('#f-raw .dlg-note').hidden");
  await click("#f-raw .dlg-note .btn.primary"); await sleep(500);
  check(await ev("document.querySelector('#f-raw [type=submit]').disabled") && await ev("document.getElementById('raw-text').value.startsWith('# ui-check stale')"), "config: changed elsewhere: Save stays off, the text stays");
  await click("#f-raw .dlg-note .btn.primary"); await sleep(500);
  check(await ev("document.getElementById('raw-text').value") === (await fileText()) && await ev("!document.querySelector('#f-raw [type=submit]').disabled"), "config: Discard loads the current file");

  // ---- Security: sign out others asks first ----
  await load("/settings/security");
  await click("[data-act=sign-out-others]"); await sleep(200);
  check(await confirmOpen(), "security: Sign out other sessions asks with HB.confirm");
  await click("#dlg-confirm [data-cancel]"); await sleep(150);

  // ---- without JavaScript: the converted tabs show their values ----
  await send("Emulation.setScriptExecutionDisabled", { value: true });
  for (const [path, expr] of [
    ["/settings/general", "document.getElementById('gen-title').value === 'ui-check-title' && document.querySelector('#f-general [type=submit]').disabled"],
    [`/settings/containers?edit=override:${encodeURIComponent(cname)}`, "document.querySelector('.dlg-page [name=name]').value === 'ui-check name' && document.querySelector('.dlg-page [type=submit]').disabled"],
    ["/settings/history", "document.getElementById('hi-sample-interval').value === '20s' && document.getElementById('hi-enabled').disabled"],
    ["/settings/config", "document.getElementById('raw-text').value.length > 100 && document.querySelector('#f-raw [type=submit]').disabled"],
    ["/settings/appearance", "document.getElementById('css-text').value.includes('--ctl') && document.getElementById('th-theme').disabled"],
  ]) {
    await load(path);
    check(await ev(`(${expr}) && /Saving needs JavaScript/.test(document.body.textContent) && getComputedStyle(document.querySelector('.needs-js')).display !== 'none'`).catch(() => false), `no JS: ${path} shows its values, saving off with the reason`);
  }
  await send("Emulation.setScriptExecutionDisabled", { value: false });

  // ---- phone: the override dialog is a bottom sheet; 16 px inputs on page forms ----
  await load("/settings/general", 390, true);
  const gp = await ev("(() => { const i = document.getElementById('gen-title'); return { fs: Math.round(parseFloat(getComputedStyle(i).fontSize)), h: i.offsetHeight, w: document.documentElement.scrollWidth }; })()");
  check(gp.fs === 16 && gp.h === 44 && gp.w === 390, "phone: page-form inputs are 16 px and 44 px high, no sideways scroll", gp);
  await load("/settings/containers", 390, true);
  await ev(`document.querySelector('#ov-list tr[data-key="${free}"] [data-dlg]').click()`); await sleep(400);
  const pm = await ev("(() => { const r = document.getElementById('dlg-override').getBoundingClientRect(), s = document.querySelector('#dlg-override [type=submit]').getBoundingClientRect(); return { w: Math.round(r.width), bottom: Math.round(r.bottom), vh: innerHeight, save: Math.round(s.bottom), pw: document.documentElement.scrollWidth }; })()");
  check(pm.w === 390 && pm.bottom === pm.vh && pm.save <= pm.vh && pm.pw === 390, "phone: the override dialog is a bottom sheet with Save in view", pm);
  await key("Escape"); await sleep(200);
  await send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  }

  check(errors.length === 0, "no script errors on the page", errors);
} catch (e) {
  check(false, "the run finished", String(e && e.stack || e));
} finally {
  const back = await restore();
  check(back, "clean-up: the config file and custom.css are back at their starting hashes");
  const after = (await bookmarks()).bookmarks.map((b) => b.item.name);
  check(JSON.stringify(after) === JSON.stringify(initial), "clean-up: the bookmarks are as they were", after);
  console.log(fails ? `${fails} check(s) failed` : "all checks passed");
  ws.close(); ch.kill(); process.exit(fails ? 1 : 0);
}
