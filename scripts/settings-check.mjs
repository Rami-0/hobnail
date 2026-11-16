// Settings dialog check (DESIGN.md §11.6, wave 2): drives the real
// Bookmarks list and its dialog on a running test instance, signed in, and
// the Change password form, with real key and pointer events. It adds,
// edits, moves and deletes bookmarks named "ui-check-*" and removes every
// one of them again at the end, also when a check fails. Signs in once
// (the login is rate limited: never run it in a loop with a wrong password).
//   HB_PASSWORD=... node scripts/settings-check.mjs http://127.0.0.1:3210/
// CHECK_WRONG_PASSWORD=1 also sends one wrong current password to Change
// password (it counts as a failed login).
// Exits 1 when a check fails, 2 on bad usage. Headless Chromium from the
// Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";

const url = (process.argv[2] || "").replace(/\/+$/, "");
const PW = process.env.HB_PASSWORD || process.argv[3];
if (!/^https?:\/\//.test(url) || !PW) {
  console.error("usage: HB_PASSWORD=<password> node scripts/settings-check.mjs <base URL of a test instance>");
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
async function restore() {
  await cleanup();
  const now = (await api("GET", "config")).data;
  if (now.hash !== start.hash) await api("POST", "config", { base_hash: now.hash, text: start.text });
  return (await api("GET", "config")).data.hash === start.hash;
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
setTimeout(async () => { console.log("FAIL the run timed out"); await restore().catch(() => {}); ch.kill(); process.exit(1); }, 300000).unref();
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
  check((await toasts()).some((x) => /^Not undone: the config file changed since the delete/.test(x)), "Undo is refused when the file changed since", await toasts());

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
  await load("/settings/security");
  check(await ev("document.querySelector('#f-password').method === 'post' && document.querySelector('#f-password [type=submit]').disabled && getComputedStyle(document.querySelector('#f-password .needs-js')).display !== 'none'"), "no JS: Change password posts, is disabled and says why");
  await send("Emulation.setScriptExecutionDisabled", { value: false });

  check(errors.length === 0, "no script errors on the page", errors);
} catch (e) {
  check(false, "the run finished", String(e && e.stack || e));
} finally {
  const back = await restore();
  check(back, "clean-up: the config file is back at its starting hash");
  const after = (await bookmarks()).bookmarks.map((b) => b.item.name);
  check(JSON.stringify(after) === JSON.stringify(initial), "clean-up: the bookmarks are as they were", after);
  console.log(fails ? `${fails} check(s) failed` : "all checks passed");
  ws.close(); ch.kill(); process.exit(fails ? 1 : 0);
}
