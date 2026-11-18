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
  for (const width of [1440, 390]) for (const theme of ["dark", "light"]) {
    const tab = await browser.tab(); const inp = input(tab), rec = recorder(tab, base.origin); inp.touch = width === 390;
    const label = `${width}-${theme}`; const measurements = { label };
    const wait = async (expr, message, ms = 12000) => assert.ok(await tab.until(expr, ms), message);
    const clickText = async (text) => { await tab.ev(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${q(text)})?.click()`); await sleep(150); };
    
    const load = async (route, selector) => {
      await tab.send("Page.navigate", { url: new URL(route, base).href });
      await wait(`!!document.querySelector(${q(selector)})`, `render ${route}`);
      await wait('document.documentElement.scrollWidth <= innerWidth + 1', `no horizontal overflow ${label}`);
    };
    try {
      for (const d of ["Page", "Runtime", "Network", "Log"]) await tab.send(d + ".enable");
      await tab.send("Runtime.addBinding", { name: "__hbCsp" }); await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
      await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: `localStorage.setItem('rh-theme',${q(theme)});` });
      await tab.send("Network.setCookie", { name: cookie.slice(0, split), value: cookie.slice(split + 1), url: base.origin, path: "/", httpOnly: true, sameSite: "Strict" });
      await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width === 390 ? 844 : 900, deviceScaleFactor: 1, mobile: inp.touch });
      await tab.send("Emulation.setTouchEmulationEnabled", { enabled: inp.touch, maxTouchPoints: 5 });
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
      await load("/app/logs", '[data-testid="logs-picker-row"]'); await tab.shot(path.join(out, `${label}-list.png`));
      await inp.click('[data-testid="filter"]'); await inp.type("svc001");
      await wait('document.querySelectorAll("[data-testid=logs-picker-row]").length === 1', "picker filter");
      assert.ok((await tab.ev("location.search")).includes("q=svc001"));
      await inp.key("Enter"); await wait('!!document.querySelector("[data-testid=log-body] [data-log-id]")', "Enter opens first match");
      await wait('document.querySelector("[data-testid=log-pane]").textContent.includes("5,000 / 5,000")', "5000-line ring");
      if (width === 390) await tab.send("Emulation.setCPUThrottlingRate", { rate: 4 });
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
      await tab.ev(`{const b=document.querySelector('[data-testid=log-body]'); b.scrollTop-=100;}`);
      await wait('!!document.querySelector("[data-testid=log-latest]")', "scroll up stops following");
      if (width === 390) await inp.click('.log-filters');
      const anchor = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), br=b.getBoundingClientRect(); const r=[...b.querySelectorAll('[data-log-id]')].find(r=>r.getBoundingClientRect().bottom > br.top+1); return {id:r.dataset.logId, offset:r.getBoundingClientRect().top-br.top}; })()`);
      for (const label of ['Wrap','Time','Wrap','Time']) {
        await tab.ev(`Array.from(document.querySelectorAll('button')).find(b=>b.textContent.trim()===${q(label)}).click()`);
        await sleep(200);
        const after = await tab.ev(`(() => { const b=document.querySelector('[data-testid=log-body]'), r=b.querySelector('[data-log-id="${anchor.id}"]'); return r ? r.getBoundingClientRect().top-b.getBoundingClientRect().top : null; })()`);
        assert.ok(after !== null && Math.abs(after-anchor.offset)<4, `${label} keeps reader line: ${after} vs ${anchor.offset}`);
      }
      await tab.ev(`document.querySelector('[data-testid=log-latest]').click()`);
      await wait('!document.querySelector("[data-testid=log-latest]")', "jump resumes following");

      await clickText("Pause"); await wait('document.querySelector("[data-testid=log-pane]").dataset.state === "paused"', "pause state"); await clickText("Resume");
      await inp.click('[data-testid="filter"]');
      await tab.ev(`globalThis.searchStarted=performance.now()`);
      await inp.type("ERROR"); await wait('Number(document.querySelector("[data-testid=log-matches]")?.textContent.split("/")[1]) > 0', "plain search counter");
      measurements.search5000Ms = await tab.ev('performance.now()-searchStarted');
      measurements.searchMatchMs = await tab.ev('Number(document.querySelector("[data-testid=log-body]").dataset.searchMs)');
      await inp.key("Enter"); await inp.key("Escape");
      assert.ok(await tab.ev('!new URL(location.href).searchParams.has("search")'), "Escape clears URL search");
      await tab.ev(`document.querySelector('[data-testid=log-body] details summary')?.click()`);
      assert.ok(await tab.ev(`!!document.querySelector('[data-testid=log-body] details[open] pre')`), "JSON folds and opens");
      const download = await tab.ev('document.querySelector("a[download]").getAttribute("href")');
      const dl = await fetch(new URL(download, base), { headers }); assert.equal(dl.status, 200); assert.ok(dl.headers.get("content-disposition").includes("attachment")); assert.ok((await dl.text()).includes("<script"));
      await load(`/app/stacks/${encodeURIComponent(group.name)}/logs`, ".log-name"); await wait('document.querySelector(".log-name").textContent.includes("svc")', "member indexes resolve to names"); await tab.shot(path.join(out, `${label}-stack.png`));
      await load("/app/logs?c=svc001,svc002,svc003,svc005", '[data-testid="log-body"] [data-log-id]');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===4', "four panes"); await tab.shot(path.join(out, `${label}-four.png`));
      await tab.ev('document.querySelector("[data-testid=log-pane] header>a")?.click()');
      await wait('document.querySelectorAll("[data-testid=log-pane]").length===3', "close pane in place");
      const leaveAt = Date.now(); await tab.ev('document.querySelector("nav.crumb a").click()'); await wait('!!document.querySelector("[data-testid=logs-picker]")', "route leave"); measurements.leaveAt = leaveAt;
      await sleep(2500);
      const status = await fetch(new URL("/api/logs/status", base), { headers }).then((r) => r.json()); assert.equal(status.viewer_streams, 0, "viewer stream closed after leave"); measurements.streamCountZeroAfterMs = Date.now() - leaveAt;
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
