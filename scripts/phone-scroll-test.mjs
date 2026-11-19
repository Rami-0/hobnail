// Phone scroll-stability check (regression test for the "page keeps
// scrolling" bug). Mimics iOS Safari (no CSS scroll anchoring) at 390 px and
// injects live updates through the page's real SSE listeners.
//   node scripts/phone-scroll-test.mjs http://127.0.0.1:3396/
// Expect: grow_above.moved_px 0; during_touch.swaps_while_touching 0.
import { spawn } from "node:child_process";
import os from "node:os";
const url = process.argv[2];
const CH = `${os.homedir()}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const port = 9300 + Math.floor(Math.random() * 500);
const ch = spawn(CH, ["--no-sandbox", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${port}`, "--window-size=390,844", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 50; i++) { try { await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(100); } }
const t = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map();
ws.addEventListener("message", (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } });
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const ev = async (expression) => { const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }); if (r.result.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails)); return r.result.result.value; };
await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
await send("Page.enable");
// Capture the page's EventSource so the test can feed it updates.
await send("Page.addScriptToEvaluateOnNewDocument", { source: `
  window.__es = [];
  const O = window.EventSource;
  window.EventSource = function (u) { const e = new O(u); window.__es.push(e); return e; };
  window.EventSource.prototype = O.prototype;` });
await send("Page.navigate", { url });
await sleep(3000);
// iOS Safari has no scroll anchoring: turn it off in Chromium too.
await ev(`(() => { const s = document.createElement('style'); s.textContent = '*{overflow-anchor:none !important}'; document.head.appendChild(s); })()`);

// The reader's landmark: the "Top consumers" section (unique id), below the
// host section that the injected update makes taller.
const reading = `(() => { const el = document.getElementById('ov-top');
  return { text: el.id, top: Math.round(el.getBoundingClientRect().top), scrollY: Math.round(scrollY) }; })()`;
const toReader = `window.scrollTo(0, document.getElementById('ov-top').getBoundingClientRect().top + scrollY - 200)`;
// A live update that makes the host section (above the reader) 300 px taller.
const inject = (extra) => `(() => {
  const old = document.getElementById('ov-sys');
  const html = old.outerHTML.replace('>', '><div class="__grow" style="height:${extra}px"></div>');
  window.__es[0].dispatchEvent(new MessageEvent('frag', { data: 'sys\\n' + html }));
  return !!old; })()`;

const results = {};
// 1) Reader scrolled down, idle; a section above grows.
await ev(toReader);
await sleep(2000); // past the 1.5 s interaction hold
const a0 = await ev(reading);
await ev(inject(300));
await sleep(200);
const grown = await ev(`!!document.querySelector('#ov-sys .__grow')`);
const a1 = await ev(reading);
results.grow_above = { injected_block_present: grown, before: a0, after: a1, moved_px: a1 && a0 ? a1.top - a0.top : null };

// 2) Finger on the screen for 4 s: updates must wait, the text must not move.
await ev(toReader);
await sleep(2000);
const b0 = await ev(reading);
await ev(`(() => { window.__swaps = 0; const mo = new MutationObserver((ms) => ms.forEach((m) => { window.__swaps += m.addedNodes.length; }));
  mo.observe(document.getElementById('ov-sys').parentElement, { childList: true }); window.__mo = mo; })()`);
await ev(`window.dispatchEvent(new TouchEvent('touchstart', { touches: [] }))`); // finger down first
await ev(inject(150));
for (let i = 0; i < 8; i++) { await ev(`window.dispatchEvent(new TouchEvent('touchmove', { touches: [] }))`); await sleep(500); }
const swapsWhileTouching = await ev("window.__swaps");
const b_during = await ev(reading);
await sleep(2500);
const swapsAfter = await ev("window.__swaps");
const b1 = await ev(reading);
results.during_touch = { swaps_while_touching: swapsWhileTouching, text_moved_while_touching_px: b_during.top - b0.top, swaps_after_release: swapsAfter - swapsWhileTouching, text_moved_after_px: b1.top - b0.top, same_text: b0.text === b1.text };

console.log(JSON.stringify(results, null, 1));
ch.kill(); process.exit(0);
