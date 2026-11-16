// Component-layer behaviour check (DESIGN.md §11.4 to §11.9): loads a real
// page from a running instance, injects dialog / confirm / menu markup and
// drives it through the page's own /static/app.js (window.HB) with real key
// and pointer events. Needs no login and writes nothing to the server.
//   node scripts/ui-check.mjs http://127.0.0.1:3210/
// Exits 1 when a check fails, 2 on bad usage. Headless Chromium from the
// Playwright cache (override with CH=/path/to/chrome).
import { spawn } from "node:child_process";
import os from "node:os";

const url = process.argv[2];
if (!url || !/^https?:\/\//.test(url)) {
  console.error("usage: node scripts/ui-check.mjs <base URL of a test instance>");
  process.exit(2);
}
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
});
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (r.result.exceptionDetails) throw new Error(expression.slice(0, 80) + ": " + JSON.stringify(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text));
  return r.result.result.value;
};
const KEYS = { Escape: 27, Enter: 13, ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35, Tab: 9 };
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

let fails = 0;
const check = (ok, what, got) => { console.log(`${ok ? "ok  " : "FAIL"} ${what}${ok || got === undefined ? "" : ` (got ${JSON.stringify(got)})`}`); if (!ok) fails++; };
const active = () => ev("document.activeElement && (document.activeElement.id || document.activeElement.getAttribute('data-k') || document.activeElement.tagName)");
const isOpen = (id) => ev(`document.getElementById(${JSON.stringify(id)}).open`);

// The fixture: a form dialog (two fields, a secondary "test" action before
// the primary), the confirm dialog, a menu. `window.__saves` records each
// save; `window.__release()` finishes a pending one.
const FIXTURE = `(() => {
  document.body.insertAdjacentHTML("beforeend", \`
  <a id="trig" href="?add=thing" data-dlg="dlg-t">Add thing</a>
  <dialog class="dlg" id="dlg-t" aria-labelledby="dt-h"><form method="post" data-hb novalidate>
   <div class="dlg-h"><h2 id="dt-h">Add thing</h2><button type="button" class="btn icon ghost" data-dlg-close aria-label="Close" id="x-close">x</button></div>
   <div class="dlg-b"><div class="note dlg-note" hidden tabindex="-1"></div>
    <div class="fld"><label for="f-n">Name</label><input class="in" id="f-n" name="name" required maxlength="80"><p class="fld-err" hidden></p></div>
    <div class="fld"><label for="f-u">URL</label><input class="in" id="f-u" name="url" data-v="http"><p class="fld-err" hidden></p></div>
    <label class="cb"><input type="checkbox" id="f-c" name="on" checked> Enabled</label></div>
   <div class="dlg-ft"><div class="dlg-main" style="display:contents"><span class="dlg-msg grow"></span>
     <button type="button" class="btn" data-dlg-close id="cancel">Cancel</button>
     <button type="button" class="btn" data-submit="test" id="test"><span>Save and test</span></button>
     <button type="submit" class="btn primary" id="save"><span>Add thing</span></button></div>
    <div class="dlg-guard" hidden><span>Discard your changes?</span><button type="button" class="btn" data-keep id="keep">Keep editing</button><button type="button" class="btn danger" data-discard id="discard">Discard</button></div></div>
  </form></dialog>
  <dialog class="dlg confirm" id="dlg-confirm"><div class="dlg-h"><h2><span></span></h2></div><div class="dlg-b"><p data-body></p><div data-typed hidden>Type <b></b><input class="in"></div></div>
   <div class="dlg-ft"><span class="grow"></span><button type="button" class="btn" data-cancel id="c-cancel">Cancel</button><button type="button" class="btn primary danger" data-ok><span>Delete</span></button></div></dialog>
  <a id="mt" href="/settings/appearance" data-menu="m1" role="button" aria-haspopup="menu" aria-expanded="false">Theme</a>
  <div class="menu" id="m1" popover role="menu"><button class="mi" role="menuitemradio" aria-checked="false" data-k="m-a">A</button><button class="mi" role="menuitemradio" aria-checked="true" data-k="m-b">B</button><button class="mi" role="menuitemradio" aria-checked="false" data-k="m-c">C</button></div>\`);
  window.__saves = [];
  document.getElementById("dlg-t")._submit = (v, o) => new Promise((done) => { window.__saves.push(o.action); window.__release = () => done({ ok: true, toast: "Added " + v.name }); });
})()`;

async function load(width, mobile) {
  await send("Emulation.setDeviceMetricsOverride", { width, height: mobile ? 844 : 900, deviceScaleFactor: mobile ? 2 : 1, mobile });
  await send("Page.navigate", { url });
  for (let i = 0; i < 100; i++) { await sleep(100); if (await ev("document.readyState === 'complete' && !!window.HB").catch(() => false)) break; }
  await ev(FIXTURE);
}

await send("Page.enable");
await send("Runtime.enable");
await load(1440, false);
check(await ev("typeof HB.open === 'function' && typeof HB.toast === 'function'"), "window.HB is loaded by /static/app.js");

// Open, focus, URL, Esc on a clean dialog.
await click("#trig");
check(await isOpen("dlg-t"), "a dialog opens from its trigger");
check((await active()) === "f-n", "focus goes to the first field", await active());
check(await ev("location.search.includes('add=thing')"), "the URL carries ?add=");
await key("Escape"); await sleep(300);
check(!(await isOpen("dlg-t")), "Esc closes a clean dialog");
check(await ev("!location.search.includes('add=')"), "closing removes ?add=");
check((await active()) === "trig", "focus returns to the trigger", await active());

// Backdrop: press and release outside the box.
await click("#trig");
await clickAt(5, 450); await sleep(300);
check(!(await isOpen("dlg-t")), "a backdrop click closes a clean dialog");

// Unsaved guard on Esc, on ×, and on a native close request.
await click("#trig"); await typeText("Draft");
await key("Escape");
check(await isOpen("dlg-t") && await ev("!document.querySelector('#dlg-t .dlg-guard').hidden"), "Esc with input shows the guard");
check((await active()) === "keep", "the guard focuses Keep editing", await active());
await key("Escape");
check(await ev("document.querySelector('#dlg-t .dlg-guard').hidden") && (await active()) === "f-n", "Esc again keeps editing");
await click("#x-close");
check(await ev("!document.querySelector('#dlg-t .dlg-guard').hidden"), "× with input shows the guard");
await click("#keep");
await ev("(() => { const d = document.getElementById('dlg-t'); if (d.requestClose) d.requestClose(); else d.dispatchEvent(new Event('cancel', { cancelable: true })); })()"); await sleep(100);
check(await isOpen("dlg-t") && await ev("!document.querySelector('#dlg-t .dlg-guard').hidden"), "a native close request with input shows the guard");
await click("#discard"); await sleep(300);
check(!(await isOpen("dlg-t")), "Discard closes");

// Reopen at once after a close: the late `close` event must not wipe the new state.
await click("#trig"); await ev("HB.close(true); HB.open('dlg-t', { trigger: document.getElementById('trig') })"); await sleep(100);
await typeText("Again"); await key("Escape");
check(await isOpen("dlg-t") && await ev("!document.querySelector('#dlg-t .dlg-guard').hidden"), "after close-and-reopen, Esc still guards the input");
await click("#discard"); await sleep(300);
check(!(await isOpen("dlg-t")), "after close-and-reopen, Discard still closes");
await ev("HB.open('dlg-t'); HB.open('dlg-t')"); await sleep(100);
await click("#cancel"); await sleep(300);
check(!(await isOpen("dlg-t")), "after a reopen through the switch path, Cancel still closes");

// Field errors: every problem at once, the first focused.
await click("#trig");
await click("#f-u"); await typeText("ftp://x"); await key("Enter"); await sleep(100);
const errs = await ev("[...document.querySelectorAll('#dlg-t .fld-err:not([hidden])')].map((e) => e.textContent)");
check(errs.length === 2, "both field errors show at once", errs);
check((await active()) === "f-n", "the first invalid field is focused", await active());
check(await ev("document.querySelector('#dlg-t .dlg-msg').textContent") === "2 fields need attention.", "the footer counts the problems");
check(await ev("__saves.length") === 0, "nothing is saved while invalid");

// Enter submits with the primary; busy state; success closes and toasts.
await typeText("Wiki"); await click("#f-u"); await ev("document.getElementById('f-u').value = ''"); await typeText("https://example.com");
await key("Enter"); await sleep(100);
check(JSON.stringify(await ev("__saves")) === '["save"]', "Enter submits with the primary action", await ev("__saves"));
check(await ev("document.getElementById('save').getAttribute('aria-busy')") === "true", "the used button is busy");
check(await ev("document.getElementById('save').textContent") === "Saving…", "the busy button says Saving…");
check(await ev("document.getElementById('test').disabled && document.getElementById('cancel').disabled"), "every other action is disabled while busy");
check(await ev("document.querySelector('#dlg-t .dlg-b').inert"), "the body is inert while busy");
await key("Escape");
check(await isOpen("dlg-t"), "Esc does nothing while busy");
await ev("__release()"); await sleep(400);
check(!(await isOpen("dlg-t")), "a successful save closes the dialog");
check(await ev("[...document.querySelectorAll('body>.toasts>.toast')].some((t) => t.textContent.includes('Added Wiki'))"), "a toast says what happened");
check(await ev("document.querySelector('body>.toasts').getAttribute('aria-live')") === "polite", "the toast region is polite");
check((await active()) === "trig", "focus returns to the trigger after saving", await active());

// The secondary action submits with its own name.
await click("#trig"); await typeText("Two"); await click("#test"); await sleep(100);
check(await ev("__saves[__saves.length - 1]") === "test", "a data-submit button submits with its action");
await ev("__release()"); await sleep(400);

// A missing save handler is a developer error, not a fake network message.
await ev("document.getElementById('dlg-t')._submit = null");
errors.length = 0;
await click("#trig"); await typeText("Three"); await key("Enter"); await sleep(100);
check(errors.some((e) => /no submit handler/.test(e)), "a missing save handler is a console error", errors);
check(!(await ev("document.querySelector('#dlg-t .dlg-note').textContent")).includes("server"), "and is not reported as a server problem");
await ev("HB.close(true)"); await sleep(300);
errors.length = 0;

// Confirm: Cancel has focus, Esc cancels only the confirm.
let p = ev("HB.confirm({ title: 'Delete it?', body: 'It can be restored.', ok: 'Delete' })");
await sleep(200);
check((await active()) === "c-cancel", "the confirm focuses Cancel", await active());
await key("Escape");
check((await p) === false, "Esc cancels the confirm");
p = ev("HB.confirm({ title: 'Delete it?', ok: 'Delete' })"); await sleep(200);
await click("#dlg-confirm [data-ok]");
check((await p) === true, "the danger button confirms");

// Menu: opens on the checked item, arrows, Home/End, Esc returns focus.
await click("#mt"); await sleep(200);
check(await ev("document.getElementById('m1').matches(':popover-open')"), "the menu opens as a popover");
check((await active()) === "m-b", "the menu focuses the checked item", await active());
await key("ArrowDown"); check((await active()) === "m-c", "ArrowDown moves to the next item", await active());
await key("ArrowDown"); check((await active()) === "m-a", "ArrowDown wraps", await active());
await key("End"); check((await active()) === "m-c", "End goes to the last item", await active());
await key("Home"); check((await active()) === "m-a", "Home goes to the first item", await active());
await key("Escape"); await sleep(200);
check(!(await ev("document.getElementById('m1').matches(':popover-open')")) && (await active()) === "mt", "Esc closes the menu and returns focus");

// HB.morph (live regions updated in place): for keyed and unkeyed lists,
// with additions, removals, reorders, attribute and text changes and SVG,
// the result is exactly what replacing the HTML would give; kept nodes
// stay the same nodes; focus, an open <details> and a row's highlight
// survive; a focused input is left alone; and all of it again with
// Element.prototype.moveBefore deleted (Safari, older Firefox).
const MORPH = `((noMove) => {
  const saved = Object.getOwnPropertyDescriptor(Element.prototype, "moveBefore");
  if (noMove && saved) delete Element.prototype.moveBefore;
  const out = { hadMoveBefore: !!saved, noMove: typeof Element.prototype.moveBefore !== "function", cases: {} };
  const host = document.createElement("div"); host.id = "mx-host"; document.body.append(host);
  // The full-replace result, then the morph; compare.
  const run = (name, before, after, prep) => {
    host.innerHTML = '<div id="mx" class="r">' + before + "</div>";
    const old = document.getElementById("mx"), ctx = prep ? prep(old) || {} : {};
    const want = old.cloneNode(false); want.innerHTML = after;
    const live = HB.morph(old, after);
    const r = { same: live === old, equal: live.outerHTML === (ctx.want ? ctx.want(want) : want).outerHTML };
    if (!r.equal) r.got = live.outerHTML, r.wanted = (ctx.want ? ctx.want(want) : want).outerHTML;
    if (ctx.after) Object.assign(r, ctx.after(live));
    out.cases[name] = r;
  };
  const K = (k, t, a) => '<li data-key="' + k + '"' + (a || "") + "><a href=\\"/c/" + k + '">' + t + "</a></li>";
  run("keyed: add, remove, reorder, text, attribute",
    "<ul>" + K("a", "A") + K("b", "B") + K("c", "C") + K("d", "D") + "</ul>",
    "<ul>" + K("d", "D2", ' class="x"') + K("e", "E") + K("b", "B") + K("a", "A", ' title="t"') + "</ul>",
    // (A node out of its place is replaced, not moved, unless it holds
    // focus: a moved node counts as a layout shift. "a" stays in place.)
    (old) => { const was = old.querySelector('[data-key="a"]');
      return { after: (l) => ({ kept: l.querySelector('[data-key="a"]') === was, gone: !l.querySelector('[data-key="c"]') }) }; });
  run("keyed: rows in the same order keep their nodes when others go or come",
    "<ul>" + K("a", "A") + K("b", "B") + K("c", "C") + K("d", "D") + "</ul>",
    "<ul>" + K("b", "B") + K("x", "X") + K("d", "D!") + K("c", "C") + "</ul>",
    (old) => { const n = (k) => old.querySelector('[data-key="' + k + '"]'); const was = { b: n("b"), c: n("c") };
      return { after: (l) => ({ kept: ["b", "c"].every((k) => l.querySelector('[data-key="' + k + '"]') === was[k]) }) }; });
  // A ranked list (data-rank): re-used by position, nothing moves; the item
  // holding focus keeps its node and goes to its new rank.
  const RK = (ks) => "<ol data-rank>" + ks.map((k) => K(k, k.toUpperCase() + "!")).join("") + "</ol>";
  run("ranked: re-used by position, nothing moves",
    "<ol data-rank>" + K("a", "A") + K("b", "B") + K("c", "C") + "</ol>", RK(["c", "a", "d", "b"]),
    (old) => { const was = [...old.querySelectorAll("li")];
      return { after: (l) => ({ inPlace: [...l.querySelectorAll("li")].slice(0, 3).every((n, i) => n === was[i]) }) }; });
  run("ranked: the focused item keeps its node at its new rank",
    "<ol data-rank>" + K("a", "A") + K("b", "B") + K("c", "C") + K("d", "D") + "</ol>", RK(["d", "c", "b", "a"]),
    (old) => { const f = old.querySelector('[data-key="b"] a'); f.focus();
      return { after: (l) => ({ focusKept: document.activeElement === f, focusSameRow: f.closest("li").dataset.key === "b" && f.textContent === "B!" }) }; });
  run("ranked: the focused item moves to the top",
    "<ol data-rank>" + K("a", "A") + K("b", "B") + K("c", "C") + "</ol>", RK(["c", "a"]),
    (old) => { const f = old.querySelector('[data-key="c"] a'); f.focus();
      return { after: (l) => ({ focusKept: document.activeElement === f && l.querySelector("li") === f.parentNode }) }; });
  run("unkeyed: by position and tag", "<p>one</p><p>two</p><span>s</span>text<!--c--><b>x</b>", "<p>one!</p><div>d</div>more text<b class=\\"y\\">x</b><i>new</i>");
  run("mixed keyed and unkeyed", '<h3>T</h3><ul><li id="i1">1</li><li>plain</li><li data-c="z">z</li></ul><p>end</p>', '<h3>T2</h3><ul><li data-c="z">z!</li><li id="i1" hidden>1</li><li>plain 2</li><li>more</li></ul>');
  run("svg: attributes, children, namespace",
    '<svg viewBox="0 0 10 10" class="i"><rect width="3" height="2"></rect><use href="#i-x"></use></svg>',
    '<svg viewBox="0 0 20 10" class="i"><rect width="7" height="2" data-key="r"></rect><circle r="2"></circle><use href="#i-y"></use></svg>',
    () => ({ after: (l) => ({ svgNs: [...l.querySelectorAll("svg, svg *")].every((e) => e.namespaceURI === "http://www.w3.org/2000/svg") }) }));
  run("focus stays on a link whose row moves",
    "<ul>" + K("a", "A") + K("b", "B") + K("c", "C") + "</ul>",
    "<ul>" + K("c", "C") + K("b", "B!") + K("a", "A") + "</ul>",
    (old) => { const f = old.querySelector('[data-key="b"] a'); f.focus(); return { after: () => ({ focusKept: document.activeElement === f, focusText: document.activeElement.textContent }) }; });
  run("focus stays on a link whose row is moved to the top",
    "<ul>" + K("a", "A") + K("b", "B") + K("c", "C") + "</ul>",
    "<ul>" + K("c", "C") + K("a", "A") + K("b", "B") + "</ul>",
    (old) => { const f = old.querySelector('[data-key="c"] a'); f.focus(); return { after: () => ({ focusKept: document.activeElement === f }) }; });
  run("a removed focused row: focus goes to the same link",
    '<ul><li>' + '<a href="/c/x">X</a></li></ul>',
    '<ul><li data-key="x"><a href="/c/x">X</a></li></ul>',
    (old) => { old.querySelector("a").focus(); return { after: (l) => ({ focusSameHref: document.activeElement.getAttribute("href") === "/c/x" && l.contains(document.activeElement) }) }; });
  run("an open <details> stays open",
    '<details data-key="d"><summary>S</summary><p>body</p></details>',
    '<details data-key="d"><summary>S2</summary><p>body</p></details>',
    (old) => { old.querySelector("details").open = true; return { want: (w) => { w.querySelector("details").setAttribute("open", ""); return w; }, after: (l) => ({ open: l.querySelector("details").open }) }; });
  run("a row's highlight survives the next morph",
    '<ul data-flash><li data-key="a" class="rl">A</li></ul>',
    '<ul data-flash><li data-key="a" class="rl">A2</li></ul>',
    (old) => { old.querySelector("li").classList.add("row-new"); return { want: (w) => { w.querySelector("li").classList.add("row-new"); return w; }, after: (l) => ({ flash: l.querySelector("li").classList.contains("row-new") }) }; });
  run("a focused input is left alone",
    '<p>a</p><input id="mx-in" value="typed">',
    '<p>b</p><input id="mx-in" value="server" class="z">',
    (old) => { old.querySelector("input").focus(); return { want: (w) => { const i = w.querySelector("input"); i.setAttribute("value", "typed"); i.removeAttribute("class"); return w; }, after: () => ({ focusKept: document.activeElement.id === "mx-in" }) }; });
  run("a waiting region is made busy",
    '<div data-wait="">x</div>', '<div data-wait="">y</div>',
    () => ({ want: (w) => { w.firstChild.setAttribute("aria-busy", "true"); return w; } }));
  host.remove();
  if (noMove && saved) Object.defineProperty(Element.prototype, "moveBefore", saved);
  return out;
})`;
for (const noMove of [false, true]) {
  const m = await ev(`${MORPH}(${noMove})`);
  const tag = noMove ? " (moveBefore deleted)" : "";
  if (noMove) check(m.noMove, "moveBefore is deleted for the second run", m);
  for (const [name, r] of Object.entries(m.cases)) {
    const extra = Object.entries(r).filter(([k]) => !["same", "equal", "got", "wanted", "focusText"].includes(k));
    check(r.same && r.equal && extra.every(([, v]) => v === true), `morph: ${name}${tag}`, r);
  }
}

// Reduced motion: every duration token is 0 and the dialog has no transition.
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
await ev("HB.open('dlg-t')"); await sleep(50);
check(await ev("getComputedStyle(document.documentElement).getPropertyValue('--dur-3').trim()") === "0ms", "reduced motion sets the duration tokens to 0");
check(await ev("getComputedStyle(document.getElementById('dlg-t')).transitionDuration.split(',').every((d) => parseFloat(d) === 0)"), "reduced motion: the dialog does not animate");
await ev("HB.close(true)");
await send("Emulation.setEmulatedMedia", { features: [] });

// Phone: the dialog is a bottom sheet with 44 px controls and 16 px text.
await load(390, true);
await ev("HB.open('dlg-t')"); await sleep(400);
const sheet = await ev("(() => { const r = document.getElementById('dlg-t').getBoundingClientRect(), i = document.getElementById('f-n'), s = document.getElementById('save').getBoundingClientRect(); return { w: Math.round(r.width), bottom: Math.round(r.bottom), vh: innerHeight, inH: i.offsetHeight, fs: Math.round(parseFloat(getComputedStyle(i).fontSize)), saveBottom: Math.round(s.bottom) }; })()");
check(sheet.w === 390 && sheet.bottom === sheet.vh, "phone: the dialog is a full-width bottom sheet", sheet);
check(sheet.inH === 44 && sheet.fs === 16, "phone: inputs are 44 px high with 16 px text", sheet);
check(sheet.saveBottom <= sheet.vh, "phone: the primary button is in view", sheet);

check(errors.length === 0, "no script errors on the page", errors);
console.log(fails ? `${fails} check(s) failed` : "all checks passed");
ws.close(); ch.kill(); process.exit(fails ? 1 : 0);
