// React logs, through the built Rust server. No browser or Node on the host.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { CSP_LISTENER, input, launch, q, recorder, sleep } from "./cdp.mjs";
const base = new URL(process.argv[2] || "http://127.0.0.1:3396");
assert.match(base.hostname, /^(localhost|127\.0\.0\.1)$/); assert.match(base.port, /^339[5-9]$/);
assert.ok(process.env.HB_PASSWORD); assert.ok(process.env.OUT);
const out = process.env.OUT; fs.mkdirSync(out, { recursive: true });
const session = await fetch(new URL("/api/v1/session", base)).then((r) => r.json());
const signed = await fetch(new URL(session.owner_exists ? "/api/login" : "/api/v1/onboarding/owner", base), { method: "POST", headers: { Origin: base.origin, "Content-Type": "application/json" }, body: JSON.stringify({ username: "admin", password: process.env.HB_PASSWORD }) });
assert.ok(signed.ok); const cookie = signed.headers.get("set-cookie").split(";")[0], split = cookie.indexOf("=");
const credentials = await signed.json();
const headers = { Cookie: cookie, Origin: base.origin, "Content-Type": "application/json", "X-RH-CSRF": credentials.csrf };
const config = await fetch(new URL("/api/settings/config", base), { headers }).then((r) => r.json());
assert.ok(!/^\[logs\]/m.test(config.text), "fixture expects no explicit logs section");
const configure = async (text, hash) => {
  const r = await fetch(new URL("/api/settings/config", base), { method: "POST", headers, body: JSON.stringify({ text, base_hash: hash }) });
  assert.ok(r.ok, `configure logs ${r.status}: ${await r.text()}`);
};
await configure(config.text + "\n[logs]\ntail = 5000\nring = 5000\n", config.hash);
const picker = await fetch(new URL("/api/v1/logs/picker", base), { headers }).then((r) => r.json());
const group = picker.groups.find((g) => g.project && g.containers.some((c) => c.name === "svc001")); assert.ok(group);
const browser = await launch(); const results = []; const failures = [];
try {
  for (const width of (process.env.WIDTHS || "1440 390").split(/\s+/).map((v) => parseInt(v))) for (const theme of (process.env.SCHEMES || "dark light").split(/\s+/)) {
    const tab = await browser.tab(); const inp = input(tab), rec = recorder(tab, base.origin); inp.touch = width === 390;
    const label = `${width}-${theme}`; const measurements = { label };
    const logRequests = []; tab.on("Network.requestWillBeSent", ({request}) => { if (request.url.includes("/api/logs/stream?")) logRequests.push({url:request.url, at:Date.now()}); });
    const wait = async (expr, message, ms = 12000) => assert.ok(await tab.until(expr, ms), message);
    const clickText = async (text) => { await tab.ev(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${q(text)})?.click()`); await sleep(150); };
    const choose = async (label, value) => {
      await inp.click(`[aria-label="${label}"]`);
      await wait(`!!document.querySelector('[role="option"][data-value="${value}"]')`, `option ${label} ${value}`);
      await inp.click(`[role="option"][data-value="${value}"]`);
    };
    const setQuery = async (text) => { await inp.clear('[data-testid="filter"]'); if (text) await inp.type(text); };
    const setDate = async (id, text) => tab.ev(`{const e=document.getElementById(${q(id)});Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(e,${q(text)});e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));}`);

    
    const load = async (route, selector) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`!!document.querySelector(${q(selector)})`, `render ${route}`);
      await wait('document.documentElement.scrollWidth <= innerWidth + 1', `no horizontal overflow ${label}`);
    };
    try {
      for (const d of ["Page", "Runtime", "Network", "Log"]) await tab.send(d + ".enable");
      await tab.send("Runtime.addBinding", { name: "__hbCsp" }); await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `globalThis.logSources=[]; const OriginalEventSource=EventSource; globalThis.EventSource=class extends OriginalEventSource {constructor(...args){super(...args); if(String(args[0]).includes('/api/logs/stream?')) logSources.push(this);}};` });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem('rh-theme',${q(theme)});` });
      await tab.send("Network.setCookie", { name: cookie.slice(0, split), value: cookie.slice(split + 1), url: base.origin, path: "/", httpOnly: true, sameSite: "Strict" });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width === 390 ? 844 : 900, deviceScaleFactor: 1, mobile: inp.touch });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
      console.log(label, "picker");
      await load("/app/logs", '[data-testid="logs-picker-row"]'); await tab.shot(path.join(out, `${label}-list.png`));
      await inp.click('[data-testid="filter"]'); await inp.type("svc001");
      await wait('document.querySelectorAll("[data-testid=logs-picker-row]").length === 1', "picker filter");
      assert.ok((await tab.ev("location.search")).includes("q=svc001"));
      await inp.key("Enter"); await wait('!!document.querySelector("[data-testid=log-body] [data-log-id]")', "Enter opens first match");
      await wait('document.querySelector("[data-testid=log-pane]").textContent.includes("5,000 / 5,000")', "5000-line ring");
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
      await sleep(1000);
      await tab.ev(`globalThis.logTasks=[]; globalThis.logObserver=new PerformanceObserver(list => logTasks.push(...list.getEntries().map(e=>e.duration))); logObserver.observe({type:'longtask',buffered:false});`);
      await sleep(7000);
      measurements.burstMaxTaskMs = await tab.ev('Math.max(0,...logTasks)');
      await tab.ev('logObserver.disconnect()');
      measurements.mountedRows = await tab.ev('document.querySelectorAll("[data-log-id]").length');
      assert.ok(measurements.mountedRows < 150, "rows are windowed");
      await wait(`(() => { const b=document.querySelector('[data-testid=log-body]'); return b.scrollHeight-b.scrollTop-b.clientHeight < 2; })()`, "follows newest after burst");
      await tab.shot(path.join(out, `${label}-single.png`));
      assert.equal(await tab.ev('!!document.querySelector("#hostile-log,#hostile-img") || !!globalThis.logPwned'), false, "hostile markup creates no elements");
      assert.ok(await tab.ev('document.querySelector("[data-testid=log-body]").textContent.includes("<script")'), "hostile log remains literal text");
      const scrollBox = await inp.box('[data-testid="log-body"]', false);
      if (width === 390) await inp.touchDrag(scrollBox.x, scrollBox.y-40, scrollBox.x, scrollBox.y+40, 16);
      else await tab.send('Input.dispatchMouseEvent', {type:'mouseWheel',x:scrollBox.x,y:scrollBox.y,deltaX:0,deltaY:-4});
      await wait('!!document.querySelector("[data-testid=log-latest]")', "scroll up stops following");
      if (width === 390) { await inp.click('.log-filters'); await sleep(500); }
      const anchor = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), br=b.getBoundingClientRect(); const all=[...b.querySelectorAll('[data-log-id]')]; const r=all.find(r=>r.getBoundingClientRect().top >= br.top-.5) || all.find(r=>r.getBoundingClientRect().bottom > br.top+1); return {id:r.dataset.logId, offset:r.getBoundingClientRect().top-br.top}; })()`);
      for (const label of ['Wrap','Time','Wrap','Time']) {
        await tab.ev(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${q(label)}).click()`);
        await sleep(200);
        const after = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), r=b.querySelector('[data-log-id="${anchor.id}"]'); return r ? r.getBoundingClientRect().top-b.getBoundingClientRect().top : null; })()`);
        assert.ok(after !== null && Math.abs(after-anchor.offset)<4, `${label} keeps reader line: ${after} vs ${anchor.offset}`);
      }
      await tab.ev(`document.querySelector('[data-testid=log-latest]').click()`);
      await wait('!document.querySelector("[data-testid=log-latest]")', "jump resumes following");

      console.log(label, "controls");
      await clickText("Pause"); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "paused"', "pause state");
      const keptId = await tab.ev('document.querySelector("[data-log-id]").dataset.logId');
      const currentConfig = await fetch(new URL('/api/settings/config',base), {headers}).then(r=>r.json());
      await configure(currentConfig.text + `\n# log viewer config event ${label}\n`, currentConfig.hash);
      await sleep(600);
      assert.ok(await tab.ev(`document.querySelector('[data-log-id="${keptId}"]') && document.querySelector('[data-testid=log-pane]').dataset.state === 'paused'`), "config event preserves pause and rows");
      await clickText("Resume");
      await inp.click('[data-testid="filter"]');
      await tab.ev(`globalThis.searchStarted=performance.now()`);
      await inp.type("ERROR"); await wait('Number(document.querySelector("[data-testid=log-matches]")?.textContent.split("/")[1]) > 0', "plain search counter");
      measurements.search5000Ms = await tab.ev('performance.now()-searchStarted');
      measurements.searchMatchMs = await tab.ev('Number(document.querySelector("[data-testid=log-body]").dataset.searchMs)');
      await inp.key("Enter"); await inp.key("Escape");
      assert.ok(await tab.ev('!new URL(location.href).searchParams.has("search")'), "Escape clears URL search");
      await setQuery("ansi red"); await wait('Number(document.querySelector("[data-testid=log-matches]").textContent.split("/")[1])>0', "case-insensitive matches");
      await choose("Show", "0");
      await wait(`[...document.querySelectorAll('[data-log-id]')].length > 0 && [...document.querySelectorAll('[data-log-id]')].every(r=>r.textContent.toLowerCase().includes('ansi red'))`, "only matching");
      await choose("Show", "1");
      await wait(`[...document.querySelectorAll('[data-log-id]')].some(r=>!r.textContent.toLowerCase().includes('ansi red'))`, "matching context");
      await inp.click('[aria-label="Match case"]'); await wait('document.querySelector("[data-testid=log-matches]").textContent === "0/0"', "case-sensitive search");
      await inp.click('[aria-label="Match case"]'); await wait('Number(document.querySelector("[data-testid=log-matches]").textContent.split("/")[1])>0', "case toggle restores matches");
      await inp.click('[aria-label="Show error lines"]'); await wait('!document.querySelector("[data-log-id][data-level=e]")', "level exclusion"); await inp.click('[aria-label="Show error lines"]');
      await choose("Show", "all"); await setQuery("["); await inp.click('[aria-label="Regular expression"]');
      await wait('document.querySelector("[data-testid=filter]").getAttribute("aria-invalid")==="true"', "invalid regex stays safe");
      await inp.click('[aria-label="Regular expression"]'); await setQuery("");
      console.log(label, "ranges");
      for (const preset of ["15m", "1h", "6h", "24h", "start"]) { await choose("Range",preset); await wait(`new URL(location.href).searchParams.get('range')===${q(preset)}`, `range ${preset}`); }
      await choose("Range", "custom"); await clickText("Apply");
      await wait('!!document.querySelector(".log-custom [aria-invalid=true]")', "empty custom range validation");
      await setDate('logs-from', new Date(Date.now()+3600000).toISOString().slice(0,16)); await clickText("Apply"); await wait('document.querySelector(".log-custom").textContent.includes("future")', "future From validation");
      const fromDate = new Date(Date.now()-600000).toISOString().slice(0,16), toDate = new Date(Date.now()-1200000).toISOString().slice(0,16);
      await setDate('logs-from',fromDate); await setDate('logs-to',toDate); await clickText("Apply"); await wait('document.querySelector(".log-custom").textContent.includes("later")', "backward range validation");
      await setDate('logs-to',new Date().toISOString().slice(0,16)); await clickText("Apply"); await wait('new URL(location.href).searchParams.has("until")', "apply custom window"); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "ended"', "closed window ended");
      await choose("Range", ""); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "live"', "return to live");
      await sleep(400);
      await tab.ev(`document.querySelector('[data-testid=log-body] details summary')?.click()`);
      assert.ok(await tab.ev(`!!document.querySelector('[data-testid=log-body] details[open] pre')`), "JSON folds and opens");
      console.log(label, "clipboard and reconnect");
      await tab.send("Browser.grantPermissions", {origin:base.origin, permissions:["clipboardReadWrite","clipboardSanitizedWrite"]});
      const expectedCopy = await tab.ev(`(()=>{const r=document.querySelector('.log-row:not(:has(details))'); r.click(); return r.querySelector('.log-text').textContent;})()`);
      await clickText("Copy line"); assert.equal(await tab.ev('navigator.clipboard.readText()'), expectedCopy, "copy plain line text");
      await clickText("Copy link"); const copiedLink = await tab.ev('navigator.clipboard.readText()'); assert.match(copiedLink, /#t=\d/);
      const beforeReconnect = Date.now(), requestCount = logRequests.length;
      await tab.ev(`logSources.filter(s=>s.readyState===1).at(-1).dispatchEvent(new Event('error'))`);
      await wait(`document.querySelector('[data-testid=log-pane]').dataset.state.includes('retrying')`, "disconnected state");
      await wait(`document.querySelector('[data-testid=log-pane]').dataset.state==='live'`, "reconnect live", 10000);
      const reconnect = logRequests.slice(requestCount).find(r=>new URL(r.url).searchParams.has('since')); assert.ok(reconnect, "reconnect sends cursor"); measurements.reconnectMs = reconnect.at-beforeReconnect;
      assert.ok(measurements.reconnectMs>=1900, "initial reconnect backoff");
      await load(copiedLink, '[data-testid="log-body"] [data-log-id]');
      await wait('!!document.querySelector(".log-current")', "timestamp permalink selects a line");
      const previousLines = await tab.ev('Number(document.querySelector("[data-testid=log-body]").dataset.searchLines)');
      await inp.click('[data-testid="log-older"]'); await wait(`Number(document.querySelector('[data-testid=log-body]').dataset.searchLines)>${previousLines}`, "older lines prepend");
      if (width===390) await inp.click('.log-filters');
      const download = await tab.ev('document.querySelector("a[download]").getAttribute("href")');
      const dl = await fetch(new URL(download, base), { headers }); assert.equal(dl.status, 200); assert.ok(dl.headers.get("content-disposition").includes("attachment")); assert.ok((await dl.text()).includes("<script"));
      console.log(label, "stack and panes");
      await load(`/app/stacks/${encodeURIComponent(group.name)}/logs`, ".log-name"); await wait('document.querySelector(".log-name").textContent.includes("svc")', "member indexes resolve to names"); await tab.shot(path.join(out, `${label}-stack.png`));
      await load("/app/logs?c=svc001,svc002,svc003,svc005", '[data-testid="log-body"] [data-log-id]');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===4', "four panes"); await tab.shot(path.join(out, `${label}-four.png`));
      await tab.ev('document.querySelector("[data-testid=log-pane] header>a[aria-label^=Close]")?.click()');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===3', "close pane in place");
      const leaveAt = Date.now(); await tab.ev('document.querySelector("nav.crumb a").click()'); await wait('!!document.querySelector("[data-testid=logs-picker]")', "route leave"); measurements.leaveAt = leaveAt;
      await sleep(2500);
      const status = await fetch(new URL("/api/logs/status", base), { headers }).then((r) => r.json()); assert.equal(status.viewer_streams, 0, "viewer stream closed after leave"); measurements.streamCountZeroAfterMs = Date.now() - leaveAt;
      if (process.env.LOG_FIXTURES === "1") {
        await load('/app/containers/svc149/logs','[data-testid="log-pane"]'); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "not found"', "not-found pane");
        await load('/app/containers/svc148/logs','[data-testid="log-pane"]'); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "error"', "error pane");
      }
      await load('/app/containers/svc004/logs','[data-testid="log-pane"]'); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "ended"', "stopped container ended");
      await load('/app/containers/svc001/logs','[data-testid="log-body"] [data-log-id]');
      rec.allow(({url,status})=>url.includes('/api/v1/logs/picker')&&status===401 ? 'deliberate session removal' : false);
      await tab.send('Network.deleteCookies',{name:cookie.slice(0,split),url:base.origin});
      await tab.ev(`logSources.filter(s=>s.readyState===1).at(-1).dispatchEvent(new Event('error'))`);
      await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "no access"', "no-access pane");
      await tab.send('Network.setCookie',{name:cookie.slice(0,split),value:cookie.slice(split+1),url:base.origin,path:'/',httpOnly:true,sameSite:'Strict'});
      measurements.consoleErrors = [...rec.console, ...rec.exceptions]; measurements.cspViolations = rec.csp;
      assert.deepEqual(rec.csp, [], "zero CSP violations");
      assert.deepEqual([...rec.console, ...rec.exceptions], [], "zero console errors");
      results.push(measurements);
      assert.ok(measurements.burstMaxTaskMs <= 100, `burst maximum ${measurements.burstMaxTaskMs} ms exceeds 100 ms`);
    } catch (e) { failures.push(`${label}: ${e.stack}`); await tab.shot(path.join(out, `${label}-failure.png`)); }
    finally { if (!results.includes(measurements)) results.push(measurements); fs.writeFileSync(path.join(out, `${label}-recorder.json`), JSON.stringify(rec)); await tab.close(); }
  }
} finally {
  const current = await fetch(new URL("/api/settings/config", base), { headers }).then((r) => r.json());
  await configure(config.text, current.hash);
  browser.quit(); fs.writeFileSync(path.join(out, "results.json"), JSON.stringify({ results, failures }, null, 2)); }
console.log(JSON.stringify({ results, failures }, null, 2)); assert.deepEqual(failures, []);
