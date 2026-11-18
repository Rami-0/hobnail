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
async function saveTrace(tab, label) {
  const finished=tab.once('Tracing.tracingComplete',30000); await tab.send('Tracing.end'); const result=await finished;
  if (result?.stream) { let data=''; for (;;) { const chunk=await tab.send('IO.read',{handle:result.stream,size:4<<20}); if(chunk.error) break; data+=chunk.result.base64Encoded?Buffer.from(chunk.result.data,'base64').toString():chunk.result.data; if(chunk.result.eof) break; } await tab.send('IO.close',{handle:result.stream}); fs.writeFileSync(path.join(out,`${label}-trace.json`),data); }
  const cpu=await tab.send('Profiler.stop'); fs.writeFileSync(path.join(out,`${label}-cpu.json`),JSON.stringify(cpu.result.profile));
}
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
      const values = label === "Show" ? ["all","0","1","3","10"] : ["","15m","1h","6h","24h","start","custom"];
      await inp.key("Home");
      for (let i=0; i<values.indexOf(value); i++) await inp.key("ArrowDown");
      await inp.key("Enter");
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
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `globalThis.logSources=[]; const OriginalEventSource=EventSource; globalThis.EventSource=class extends OriginalEventSource {constructor(...args){super(...args); if(String(args[0]).includes('/api/logs/stream?')) {logSources.push(this);this.wireCount=0;this.addEventListener('l',e=>{const lines=JSON.parse(e.data);this.wireCount+=lines.length;this.receivedT=Math.max(this.receivedT||0,...lines.map(l=>l.t));});}}};` });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem('rh-theme',${q(theme)});` });
      await tab.send("Network.setCookie", { name: cookie.slice(0, split), value: cookie.slice(split + 1), url: base.origin, path: "/", httpOnly: true, sameSite: "Strict" });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width === 390 ? 844 : 900, deviceScaleFactor: 1, mobile: inp.touch });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
      console.log(label, "picker");
      await load("/app/logs", '[data-testid="logs-picker-row"]'); await tab.shot(path.join(out, `${label}-list.png`));
      for (const name of ['svc001','svc002','svc003','svc005']) await inp.click(`[data-testid="logs-picker-row"]:has(a[href$="/${name}/logs"]) [role="switch"]`);
      await wait(`new URL(location.href).searchParams.get('sel')?.split(',').length===4`, "picker ticks stay in URL");
      assert.ok(await tab.ev(`[...document.querySelectorAll('[role=switch]')].filter(b=>b.getAttribute('aria-checked')!=='true').every(b=>b.disabled || b.getAttribute('aria-disabled')==='true')`), "fifth tick is disabled");
      await inp.click('.logs-picker-toolbar .btn'); await wait('document.querySelectorAll("[data-testid=log-pane]").length===4', "ticks open side by side");
      await tab.ev('history.back()'); await wait('document.querySelectorAll("[data-testid=logs-picker] [role=switch][aria-checked=true]").length===4', "Back restores ticks");
      await inp.key("/"); await wait('document.activeElement === document.querySelector("[data-testid=filter]")', "slash focuses picker filter"); await inp.type("svc001");
      await wait('document.querySelectorAll("[data-testid=logs-picker-row]").length === 1', "picker filter");
      assert.ok((await tab.ev("location.search")).includes("q=svc001"));
      await inp.key("Enter"); await wait('!!document.querySelector("[data-testid=log-body] [data-log-id]")', "Enter opens first match");
      await wait('document.querySelector("[data-testid=log-pane]").textContent.includes("5,000 / 5,000")', "5000-line ring");
      await wait(`[...document.querySelectorAll('.log-level-count')].reduce((n,e)=>n+Number(e.textContent),0)===5000`, "closed panes release their counts");
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
      await sleep(1000);
      if(process.env.TRACE_LOGS==='1') { await tab.send('Profiler.enable'); await tab.send('Profiler.start'); await tab.send('Tracing.start',{transferMode:'ReturnAsStream',traceConfig:{includedCategories:['devtools.timeline','disabled-by-default-devtools.timeline','toplevel','blink.user_timing','__metadata']}}); }
      measurements.burstObserveStartedAt=Date.now();
      await tab.ev(`globalThis.logTasks=[]; globalThis.logObserver=new PerformanceObserver(list => logTasks.push(...list.getEntries().map(e=>e.duration))); logObserver.observe({type:'longtask',buffered:false});`);
      await sleep(7000);
      measurements.burstMaxTaskMs = await tab.ev('Math.max(0,...logTasks)');
      measurements.burstObserveEndedAt=Date.now();
      measurements.wireLines = await tab.ev('Math.max(...logSources.map(s=>s.wireCount))');
      assert.ok(measurements.wireLines>=6500, "initial 5000 plus the 1500-line burst arrived");
      await tab.ev('logObserver.disconnect()');
      if(process.env.TRACE_LOGS==='1') await saveTrace(tab,label);
      if(process.env.PROFILE_ONLY==='1') { measurements.profilingOnly=true; results.push(measurements); assert.ok(measurements.burstMaxTaskMs<=100,`profile burst maximum ${measurements.burstMaxTaskMs} ms`); continue; }

      measurements.mountedRows = await tab.ev('document.querySelectorAll("[data-log-id]").length');
      assert.ok(measurements.mountedRows < 150, "rows are windowed");
      await wait(`(() => { const b=document.querySelector('[data-testid=log-body]'); return b.scrollHeight-b.scrollTop-b.clientHeight < 2; })()`, "follows newest after burst");
      if(width===390) assert.ok(await tab.ev('document.querySelector("[data-testid=log-container-head] h1").getBoundingClientRect().height<40'), "phone container identity has room for its short title");
      await tab.shot(path.join(out, `${label}-single.png`));
      assert.equal(await tab.ev('!!document.querySelector("#hostile-log,#hostile-img") || !!globalThis.logPwned'), false, "hostile markup creates no elements");
      assert.ok(await tab.ev('document.querySelector("[data-testid=log-body]").textContent.includes("<script")'), "hostile log remains literal text");
      const scrollBox = await inp.box('[data-testid="log-body"]', false);
      if (width === 390) await inp.touchDrag(scrollBox.x, scrollBox.y-40, scrollBox.x, scrollBox.y+40, 16);
      else await tab.send('Input.dispatchMouseEvent', {type:'mouseWheel',x:scrollBox.x,y:scrollBox.y,deltaX:0,deltaY:-4});
      await wait('!!document.querySelector("[data-testid=log-latest]")', "scroll up stops following");
      // Move well clear of the physical bottom clamp for geometry checks,
      // as the behavioral reference does; the tiny gesture above tests follow.
      if (width===390) { for(let drag=0;drag<3;drag++) await inp.touchDrag(scrollBox.x,scrollBox.y-100,scrollBox.x,scrollBox.y+100,16); }
      else await tab.send('Input.dispatchMouseEvent',{type:'mouseWheel',x:scrollBox.x,y:scrollBox.y,deltaX:0,deltaY:-900});
      await sleep(250);

      if (width === 390) { await inp.click('.log-filters'); await sleep(500); }
      const anchor = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), br=b.getBoundingClientRect(); const all=[...b.querySelectorAll('[data-log-id]')]; const r=all.find(r=>r.getBoundingClientRect().top >= br.top-.5) || all.find(r=>r.getBoundingClientRect().bottom > br.top+1); return {id:r.dataset.logId, offset:r.getBoundingClientRect().top-br.top}; })()`);
      for (const label of ['Wrap','Time','Wrap','Time']) {
        await tab.ev(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${q(label)}).click()`);
        await sleep(200);
        const after = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), r=b.querySelector('[data-log-id="${anchor.id}"]'); return r ? r.getBoundingClientRect().top-b.getBoundingClientRect().top : null; })()`);
        if (after === null || Math.abs(after-anchor.offset)>=4) console.log("anchor drift", anchor, await tab.ev(`(()=>{const b=document.querySelector('[data-testid=log-body]'),edge=b.getBoundingClientRect().top;return {top:b.scrollTop,height:b.clientHeight,total:b.scrollHeight,rows:[...b.querySelectorAll('[data-log-id]')].map(r=>({id:r.dataset.logId,y:r.getBoundingClientRect().top-edge,h:r.offsetHeight}))}})()`));
        assert.ok(after !== null && Math.abs(after-anchor.offset)<4, `${label} keeps reader line: ${after} vs ${anchor.offset}`);
      }
      await tab.send('Emulation.setDeviceMetricsOverride',{width:width===390?410:1360,height:width===390?844:900,deviceScaleFactor:1,mobile:inp.touch}); await sleep(350);
      const resizedOffset = await tab.ev(`(()=>{const b=document.querySelector('[data-testid=log-body]'),r=b.querySelector('[data-log-id="${anchor.id}"]');return r?r.getBoundingClientRect().top-b.getBoundingClientRect().top:null})()`);
      assert.ok(resizedOffset!==null && Math.abs(resizedOffset-anchor.offset)<4, "resize preserves reader line");
      await tab.send('Emulation.setDeviceMetricsOverride',{width,height:width===390?844:900,deviceScaleFactor:1,mobile:inp.touch}); await sleep(350);
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
      await wait('CSS.highlights.has("rh-log-search")', "visible search text is highlighted without replacing nodes");
      await wait('Number(document.querySelector("[data-testid=log-matches]").textContent.split("/")[0])>1', "new search selects its newest result");
      const newestMatch=await tab.ev('Number(document.querySelector("[data-testid=log-matches]").textContent.split("/")[0])');
      await inp.key("Enter"); await wait(`Number(document.querySelector('[data-testid=log-matches]').textContent.split('/')[0])===${newestMatch-1}`, "Enter selects the older match");
      await inp.key("Enter",{modifiers:8}); await wait(`Number(document.querySelector('[data-testid=log-matches]').textContent.split('/')[0])===${newestMatch}`, "Shift Enter selects the newer match");
      await inp.key("Escape");
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
      await wait('document.querySelector("[data-testid=log-body]").dataset.searchLines === "5000"', "live tail is fully loaded");
      await setQuery("JSON request"); await wait('!!document.querySelector(".log-current details summary")', "seek a JSON row");
      await sleep(200);
      await tab.ev(`document.querySelector('.log-current details summary')?.click()`);
      assert.ok(await tab.ev(`!!document.querySelector('[data-testid=log-body] details[open] pre')`), "JSON folds and opens");
      await setQuery("");
      console.log(label, "clipboard and reconnect");
      await tab.send("Browser.grantPermissions", {origin:base.origin, permissions:["clipboardReadWrite","clipboardSanitizedWrite"]});
      const expectedCopy = await tab.ev(`(()=>{const r=document.querySelector('.log-row:not(:has(details))'); r.click(); return r.querySelector('.log-text').textContent;})()`);
      await clickText("Copy line"); assert.equal(await tab.ev('navigator.clipboard.readText()'), expectedCopy, "copy plain line text");
      await clickText("Copy link"); const copiedLink = await tab.ev('navigator.clipboard.readText()'); assert.match(copiedLink, /#t=\d/);
      await tab.ev('document.querySelector("[data-testid=log-body]").focus()'); await inp.key("Enter");
      await wait('document.activeElement?.textContent==="Copy line"', "pane Enter focuses Copy");
      const keyboardLine=await tab.ev('document.querySelector(".log-selected").dataset.logId');
      await inp.key("ArrowUp"); await wait(`document.querySelector('.log-selected')?.dataset.logId!==${q(keyboardLine)}`, "line actions move to the previous line");
      await inp.key("ArrowDown"); await wait(`document.querySelector('.log-selected')?.dataset.logId===${q(keyboardLine)}`, "line actions move to the next line");
      await inp.key("Escape"); await wait('document.activeElement===document.querySelector("[data-testid=log-body]")&&!document.querySelector(".log-actions")', "line action Escape returns to the pane");
      await inp.key("End"); await wait('!document.querySelector("[data-testid=log-latest]")', "End resumes following");
      await inp.key("/"); await wait('document.activeElement===document.querySelector("[data-testid=filter]")', "viewer slash focuses search");
      await clickText("Pause"); await wait('document.querySelector("[data-testid=log-pane]").textContent.includes("waiting")', "pause keeps incoming text queued", 6000);
      const receivedCursor = await tab.ev('logSources.filter(s=>s.readyState===1).at(-1).receivedT');
      const beforeReconnect = Date.now(), requestCount = logRequests.length;
      await tab.ev(`logSources.filter(s=>s.readyState===1).at(-1).dispatchEvent(new Event('error'))`);
      await wait(`document.querySelector('[data-testid=log-pane]').dataset.state.includes('retrying')`, "disconnected state");
      await clickText("Resume");
      await wait(`document.querySelector('[data-testid=log-pane]').dataset.state==='live'`, "reconnect live", 10000);
      const reconnect = logRequests.slice(requestCount).find(r=>new URL(r.url).searchParams.has('since')); assert.ok(reconnect, "reconnect sends cursor"); assert.ok(Number(new URL(reconnect.url).searchParams.get("since")) > receivedCursor/1000, "cursor advances beyond queued received text"); measurements.reconnectMs = reconnect.at-beforeReconnect;
      assert.ok(measurements.reconnectMs>=1900, "initial reconnect backoff");
      await load(copiedLink, '[data-testid="log-body"] [data-log-id]');
      await wait('!!document.querySelector(".log-current")', "timestamp permalink selects a line");
      const previousLines = await tab.ev('Number(document.querySelector("[data-testid=log-body]").dataset.searchLines)');
      await inp.click('[data-testid="log-older"]'); await wait(`Number(document.querySelector('[data-testid=log-body]').dataset.searchLines)>${previousLines}`, "older lines prepend");
      if (width===390) await inp.click('.log-filters');
      const download = await tab.ev('document.querySelector("a[download]").getAttribute("href")');
      const dl = await fetch(new URL(download, base), { headers }); assert.equal(dl.status, 200); assert.ok(dl.headers.get("content-disposition").includes("attachment")); assert.ok((await dl.text()).includes("<script"));
      await load(`/app/containers/svc001/logs?search=%5B&re=1`, '[data-testid="filter"]');
      await wait('document.querySelector(".log-note")?.textContent.includes("plain text")', "linked regex requires opt-in");
      assert.equal(await tab.ev('document.querySelector("[data-testid=filter]").getAttribute("aria-invalid")'), 'false');
      await load(`/app/containers/svc001/logs#t=${Date.now()+60000}`, '[data-testid="log-pane"]');
      await wait('document.querySelector("[data-testid=log-pane]").textContent.includes("still to come")', "future timestamp keeps live tail");
      console.log(label, "stack and panes");
      await load(`/app/stacks/${encodeURIComponent(group.name)}/logs`, ".log-name"); await wait('document.querySelector(".log-name").textContent.includes("svc")', "member indexes resolve to names"); await wait('document.querySelector("[data-testid=log-body]").dataset.searchLines==="5000"', "merged stack reaches its ring cap",25000); await wait('(()=>{const b=document.querySelector("[data-testid=log-body]");return b.scrollHeight-b.scrollTop-b.clientHeight<2})()', "stack follows the newest line"); await tab.shot(path.join(out, `${label}-stack.png`));
      await load("/app/logs?c=svc001,svc002,svc003,svc005", '[data-testid="log-body"] [data-log-id]');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===4', "four panes");
      await wait('[...document.querySelectorAll("[data-testid=log-body]")].every(b=>b.dataset.searchLines==="5000")', "all panes loaded", 25000);
      await wait(`[...document.querySelectorAll('.log-level-count')].reduce((n,e)=>n+Number(e.textContent),0)===20000`, "four-pane level counts equal retained lines");
      await tab.ev(`globalThis.fourOld=logSources.filter(s=>s.readyState===1 && s.url.includes('c=svc001')).at(-1);fourOld.dispatchEvent(new Event('error'))`);
      await wait(`logSources.some(s=>s!==fourOld&&s.readyState===1&&s.url.includes('c=svc001'))`, "four-pane cursor reconnect", 10000);
      measurements.fourBurstObserveStartedAt=Date.now();
      await tab.ev(`globalThis.fourTasks=[];globalThis.fourObserver=new PerformanceObserver(l=>fourTasks.push(...l.getEntries().map(e=>e.duration)));fourObserver.observe({type:'longtask',buffered:false});`);
      await sleep(7200); measurements.fourBurstMaxTaskMs=await tab.ev('Math.max(0,...fourTasks)'); measurements.fourBurstObserveEndedAt=Date.now();
      await tab.ev('fourObserver.disconnect()');
      assert.ok(await tab.ev(`logSources.filter(s=>s.readyState===1&&s.url.includes('c=svc001')).at(-1).wireCount>=1500`), "four-pane burst arrived");
      const fourShot = path.join(out, `${label}-four.png`);
      if (width===390) {
        const metrics=await tab.send('Page.getLayoutMetrics'), size=metrics.result.cssContentSize;
        const capture=await tab.send('Page.captureScreenshot',{format:'png',captureBeyondViewport:true,clip:{x:0,y:0,width,height:size.height,scale:1}});
        fs.writeFileSync(fourShot,Buffer.from(capture.result.data,'base64'));
      } else await tab.shot(fourShot);
      await setQuery("ERROR");
      await wait(`[...document.querySelectorAll('[data-testid=log-matches]')].length===4 && [...document.querySelectorAll('[data-testid=log-matches]')].every(e=>Number(e.textContent.split('/')[1])>0)`, "search across all panes");
      await wait('document.querySelectorAll(".log-current").length===1', "new search selects one newest match across panes");
      await wait('document.querySelectorAll(".log-current").length===1', "a new search selects one newest match globally");
      await inp.key("Enter"); await wait('document.querySelectorAll(".log-current").length===1', "one global search selection");
      const firstMatch = await tab.ev('Number(document.querySelector(".log-current").dataset.timestamp)');
      await inp.key("Enter"); await sleep(200);
      assert.ok(await tab.ev('Number(document.querySelector(".log-current").dataset.timestamp)') <= firstMatch, "older match steps chronologically across panes");
      await tab.ev(`globalThis.retainedSources=logSources.filter(s=>s.readyState===1&&!s.url.includes('c=svc001'))`);
      await tab.ev('document.querySelector("[data-testid=log-pane] header>a[aria-label^=Close]")?.click()');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===3', "close pane in place");
      assert.equal(await tab.ev('new URL(location.href).searchParams.get("search")'), "ERROR", "close retains shared search state");
      assert.ok(await tab.ev('retainedSources.length===3&&retainedSources.every(s=>s.readyState===1)'), "remaining panes keep their streams");
      const leaveAt = Date.now(); await tab.ev('document.querySelector("nav.crumb a").click()'); await wait('!!document.querySelector("[data-testid=logs-picker]")', "route leave"); measurements.leaveAt = leaveAt;
      await sleep(2500);
      const status = await fetch(new URL("/api/logs/status", base), { headers }).then((r) => r.json()); assert.equal(status.viewer_streams, 0, "viewer stream closed after leave"); measurements.streamCountZeroAfterMs = Date.now() - leaveAt;
      if (process.env.LOG_FIXTURES === "1") {
        await load('/app/containers/svc149/logs','[data-testid="log-pane"]'); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "not found"', "not-found pane");
        await load('/app/containers/svc148/logs','[data-testid="log-pane"]'); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "error"', "error pane");
      }
      await load('/app/containers/svc001/logs', '[data-testid="log-body"] [data-log-id]');
      await tab.ev(`window.dispatchEvent(new PageTransitionEvent('pagehide'))`); await sleep(1800);
      assert.equal((await fetch(new URL('/api/logs/status',base),{headers}).then(r=>r.json())).viewer_streams,0,"pagehide closes the upstream stream");
      const previousSources=await tab.ev('logSources.length');
      await tab.ev(`window.dispatchEvent(new PageTransitionEvent('pageshow'))`);
      await wait(`logSources.length>${previousSources} && document.querySelector('[data-testid=log-pane]').dataset.state==='live'`, "pageshow resumes the log stream");
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
      assert.ok(measurements.fourBurstMaxTaskMs <= 100, `four-pane burst maximum ${measurements.fourBurstMaxTaskMs} ms exceeds 100 ms`);
      assert.ok(measurements.burstMaxTaskMs <= 100, `burst maximum ${measurements.burstMaxTaskMs} ms exceeds 100 ms`);
    } catch (e) { console.log(label, String(e), await tab.ev(`({width:innerWidth,scroll:scrollX,documentWidth:document.documentElement.scrollWidth,toolbar:document.querySelector('.log-toolbar')?.outerHTML,controls:[...document.querySelectorAll('.log-filter-controls,.log-filters')].map(e=>({class:e.className,display:getComputedStyle(e).display,rect:e.getBoundingClientRect().toJSON()}))})`).catch(()=>null)); failures.push(`${label}: ${e.stack}`); await tab.shot(path.join(out, `${label}-failure.png`)); }
    finally { if (!results.includes(measurements)) results.push(measurements); fs.writeFileSync(path.join(out, `${label}-recorder.json`), JSON.stringify(rec)); await tab.close(); }
  }
} finally {
  const current = await fetch(new URL("/api/settings/config", base), { headers }).then((r) => r.json());
  await configure(config.text, current.hash);
  browser.quit(); fs.writeFileSync(path.join(out, "results.json"), JSON.stringify({ results, failures }, null, 2)); }
console.log(JSON.stringify({ results, failures }, null, 2)); assert.deepEqual(failures, []);
