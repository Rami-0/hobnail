// Built-client CSP sweep. No allowances for violations, console errors or failed requests.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { CSP_LISTENER, input, launch, q, recorder, sleep } from './cdp.mjs';
import { assertSweepTarget, sweepFixture } from './sweep-fixture.mjs';
const EXPECTED_CSP = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const base = new URL(process.argv[2] || 'http://127.0.0.1:3396');
const charts = {
  '/app/containers/svc001?tab=overview': { key: 'cpu', api: '/api/v1/containers/svc001?range=6h' },
  '/app/containers/svc001?tab=metrics': { key: 'cpu', api: '/api/v1/containers/svc001?range=6h' },
  '/app/host': { key: 'cpu.user', api: '/api/v1/host/history?range=1h', metrics: 'metrics' },
  '/app/history': { key: 'cpu', api: '/api/v1/history?range=24h&level=incidents', metrics: 'metrics' },
};

function hasMarker(event) {
  return event.kind === 'container.oom' || event.kind.startsWith('alert.fired')
    || (event.kind === 'container.die' && event.level !== 'info')
    || (event.kind === 'container.health' && /\bunhealthy\b/.test(event.message));
}

async function chartFixture(fixture, chart) {
  const deadline = Date.now() + 35000;
  do {
    const result = await fixture.api(chart.api);
    const points = chart.metrics ? result[chart.metrics].find(metric => metric.key === chart.key)?.points : result.series.cpu;
    if (points?.length) return (result.series ?? result).markers.some(hasMarker);
    await sleep(500);
  } while (Date.now() < deadline);
  assert.fail(`No samples for ${chart.key} from ${chart.api}`);
}

if (process.env.ONBOARDING_ONLY === '1') {
  assertSweepTarget(base);
  assert.equal((await fetch(new URL('/api/v1/session',base)).then(r=>r.json())).owner_exists,false,'onboarding sweep needs a fresh instance');
  const browser = await launch(), evidence = [];
  try {
    for (const width of [1440,390]) {
      const tab = await browser.tab(), rec = recorder(tab,base.origin);
      try {
        for (const domain of ['Page','Runtime','Network','Log']) await tab.send(domain+'.enable');
        await tab.send('Runtime.addBinding',{name:'__hbCsp'});
        await tab.send('Page.addScriptToEvaluateOnNewDocument',{source:CSP_LISTENER});
        await tab.send('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:width===390});
        const response=await fetch(new URL('/app',base)), csp=response.headers.get('content-security-policy')||'';
        assert.equal(csp,EXPECTED_CSP,'complete document CSP');
        await tab.send('Page.navigate',{url:new URL('/app',base).href});
        assert.ok(await tab.until(`!!document.querySelector('[data-testid="onboarding"]')`));await sleep(500);
        assert.deepEqual([...rec.csp,...rec.console,...rec.exceptions,...rec.failed],[]);
        evidence.push({width,csp,rec});console.log(`ok ${width} onboarding`);
      } finally {await tab.close();}
    }
  } finally {browser.quit();fs.writeFileSync(path.join(process.env.OUT,'results.json'),JSON.stringify(evidence,null,2));}
  process.exit(0);
}
const fixture = await sweepFixture(base), browser = await launch(), results = [];
let failures = 0;
try {
  for (const width of [1440,390]) {
    const tab = await browser.tab(), inp = input(tab), rec = recorder(tab, base.origin);
    inp.touch = width === 390;
    for (const domain of ['Page','Runtime','Network','Log']) await tab.send(domain+'.enable');
    await tab.send('Runtime.addBinding', { name: '__hbCsp' });
    await tab.send('Page.addScriptToEvaluateOnNewDocument', { source: CSP_LISTENER });
    await fixture.installCookie(tab);
    await tab.send('Emulation.setDeviceMetricsOverride', { width, height: 900, deviceScaleFactor: 1, mobile: inp.touch });
    const theme = width === 390 ? 'dark' : 'light';
    await tab.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: theme }] });
    const visit = async (route, selector) => {
      const response = await fetch(new URL(route, base), { headers: { Cookie: fixture.cookie } });
      const csp = response.headers.get('content-security-policy') || '';
      assert.equal(csp,EXPECTED_CSP,'complete document CSP');
      const chart = charts[route];
      const markers = chart && await chartFixture(fixture, chart);
      await tab.send('Page.navigate', { url: new URL(route, base).href });
      assert.ok(await tab.until(`!!document.querySelector(${q(selector)})`,15000), route);
      if (chart) {
        const figure = `[data-testid="chart-${chart.key}"]`;
        const placeholder = '.charts:has(.ch-wait)';
        assert.ok(await tab.until(`!!document.querySelector(${q(figure + ',' + placeholder)})`,15000), `chart grid on ${route}`);
        await tab.ev(`(document.querySelector(${q(figure)}) ??
          document.querySelector(${q(placeholder)})).scrollIntoView({block:"start"})`);
        assert.ok(await tab.until(`!!document.querySelector(${q(figure)})`,15000), `chart on ${route}`);
        await tab.ev(`document.querySelector(${q(figure)}).scrollIntoView({block:"center"})`);
        assert.ok(await tab.until(`!!document.querySelector(${q(figure + ' .ch-canvas svg.recharts-surface')})`,20000), `SVG plot on ${route}`);
        if (markers) assert.ok(await tab.until(`!!document.querySelector(${q(figure + ' .ch-marks button[aria-label]')})`), `marker control on ${route}`);
      }
      await sleep(500);
      console.log(`ok ${width} ${route}`); results.push({ width, route, csp, plot: chart?.key, markers });
    };
    const open = async (selector, ready) => {
      if (!await inp.box(selector)) return false;
      await inp.click(selector);
      assert.ok(await tab.until(ready,8000), `open ${selector}`);
      await sleep(150); await inp.key('Escape'); await sleep(150); return true;
    };
    try {
      for (const [route,selector] of fixture.routes) {
        await visit(route,selector);
        if (route.includes('/settings/')) {
          const selects = await tab.ev(`Array.from(document.querySelectorAll('main [role="combobox"]')).map(el=>el.id).filter(Boolean)`);
          for (const id of selects) await open(`[id=${q(id)}]`, `!!document.querySelector('[role="listbox"]')`);
        }
      }
      for (const [route, selector] of [
        ['/app/settings/bookmarks?add=bookmark','[data-testid="settings-edit"]'],
        ['/app/settings/containers?add=group','[data-testid="settings-edit"]'],
        ['/app/settings/containers?add=pattern','[data-testid="settings-edit"]'],
        ['/app/settings/containers?edit=override:svc001','[data-testid="edit-dialog"]'],
        ['/app/settings/widgets?add=widget','[data-testid="settings-edit"]'],
        ['/app/settings/notifications?add=channel','[data-testid="settings-edit"]'],
        ['/app/settings/notifications?add=rule','[data-testid="settings-edit"]'],
      ]) await visit(route,selector);
      await visit('/app/settings/boards','[data-testid="board-create"]');
      assert.ok(await open('[data-testid="board-create"]', `!!document.querySelector('[data-testid="board-form"]')`));
      await visit('/app/settings/integrations','[data-testid="integration-create"]');
      assert.ok(await open('[data-testid="integration-create"]', `!!document.querySelector('[data-testid="integration-form"]')`));
      await visit('/app/settings/users','[data-testid="invite-create"]');
      assert.ok(await open('[data-testid="invite-create"]', `!!document.querySelector('[role="dialog"][data-open]')`));
      await visit(`/app/boards/${fixture.board.id}`,'[data-testid="board-edit"]');
      await inp.click('[data-testid="board-edit"]');
      assert.ok(await tab.until(`!!document.querySelector('[data-testid="board-editor"]')`));
      assert.ok(await open('[data-testid="board-add-item"]', `!!document.querySelector('[data-testid="board-picker"]')`));
      await inp.click('[data-testid="board-cancel"]');
      await open('[data-testid="user"]', `!!document.querySelector('[role="menu"]')`);
      await inp.key('k',{modifiers:2}); assert.ok(await tab.until(`!!document.querySelector('[data-testid="palette"]')`)); await inp.key('Escape');
      await inp.key('?'); assert.ok(await tab.until(`!!document.querySelector('[data-testid="shortcuts"]')`)); await inp.key('Escape');
      await tab.send('Network.clearBrowserCookies'); await visit('/app/login','form[data-testid="login"]');
      await sleep(300);
      assert.deepEqual([...rec.csp,...rec.console,...rec.exceptions,...rec.failed],[]);
    } catch(error) { failures++; console.error(`FAIL ${width}: ${error.stack}`); await tab.shot(path.join(process.env.OUT,`${width}-failure.png`)); }
    finally { fs.writeFileSync(path.join(process.env.OUT,`${width}-recorder.json`),JSON.stringify(rec,null,2)); await tab.close(); }
  }
} finally { browser.quit(); await fixture.cleanup(); fs.writeFileSync(path.join(process.env.OUT,'results.json'),JSON.stringify({results,failures},null,2)); }
console.log(`${results.length} route visits; ${failures} failed profiles`); process.exitCode = failures ? 1 : 0;
