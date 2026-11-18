// Generic accessibility invariants across the built React app.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { input, launch, q, sleep } from './cdp.mjs';
import { sweepFixture } from './sweep-fixture.mjs';
import { MEASURE } from './hit-targets.mjs';
const base = new URL(process.argv[2] || 'http://127.0.0.1:3396');
assert.match(base.hostname, /^(127\.0\.0\.1|localhost)$/);
assert.match(base.port, /^339[5-9]$/);
const onboarding = process.env.ONBOARDING_ONLY === '1';
if (onboarding) assert.equal((await fetch(new URL('/api/v1/session',base)).then(r=>r.json())).owner_exists,false,'onboarding audit needs a fresh instance');
const fixture = onboarding ? { routes: [['/app','form[data-testid="onboarding"]']], board: {id:''}, installCookie: async()=>{}, cleanup: async()=>{} } : await sweepFixture(base);
const browser = await launch(), results = [];
const failures = [];
try {
  for (const width of [1440,390]) {
    const tab = await browser.tab(), inp = input(tab); inp.touch = width === 390;
    for (const domain of ['Page','Runtime','Network']) await tab.send(domain+'.enable');
    await fixture.installCookie(tab);
    await tab.send('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:inp.touch});
    await tab.send('Emulation.setTouchEmulationEnabled',{enabled:inp.touch,maxTouchPoints:5});
    await tab.send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
    const audit = async (route) => {
      const controls = await tab.ev(`${MEASURE}(null)`);
      const problems = await tab.ev(`(() => {
        const problems=[], visible=el=>el.getClientRects().length && getComputedStyle(el).visibility==='visible' && !el.closest('[inert],[aria-hidden="true"]');
        for(const el of document.querySelectorAll('a[href],button,input:not([type=hidden]),textarea,select,[role="switch"],[role="combobox"]')) {
          if(!visible(el)) continue;
          const labelled=el.getAttribute('aria-labelledby')?.split(/\\s+/).map(id=>document.getElementById(id)?.textContent||'').join('');
          const name=el.getAttribute('aria-label')||labelled||Array.from(el.labels||[],l=>l.textContent).join('')||el.textContent||el.title;
          if(!name?.trim()) problems.push('unnamed '+el.outerHTML.slice(0,180));
          if(${width}===390 && el.matches('input,textarea,select') && parseFloat(getComputedStyle(el).fontSize)<16) problems.push('small input '+el.outerHTML.slice(0,100));
        }
        for(const el of document.querySelectorAll('[role="status"],[role="alert"],output,[aria-live]')) {
          const role=el.getAttribute('role')|| (el.tagName==='OUTPUT'?'status':null),live=el.getAttribute('aria-live')||(role==='status'?'polite':role==='alert'?'assertive':null);
          if(!role || !['polite','assertive','off'].includes(live)) problems.push('status semantics '+el.outerHTML.slice(0,100));
        }
        if(document.documentElement.scrollWidth>innerWidth+1) problems.push('sideways scroll '+document.documentElement.scrollWidth);
        for(const el of document.querySelectorAll('*')) {
          if(!visible(el)) continue;
          const style=getComputedStyle(el);
          if(style.transitionDuration.split(',').some(s=>parseFloat(s)>0.001)||style.animationDuration.split(',').some(s=>parseFloat(s)>0.001)) {problems.push('reduced motion '+el.tagName+'.'+el.className);break;}
        }
        return problems;
      })()`);
      const small = controls.filter(c=>width===390?!c.ok44:!c.ok24);
      problems.push(...small.map(c=>`target ${c.sel} ${c.name} ${c.w}x${c.h}`));
      await inp.key('Tab');
      const focus = await tab.ev(`(() => {const el=document.activeElement;if(el===document.body)return false;const s=getComputedStyle(el);return el.matches(':focus-visible') && ((s.outlineStyle!=='none'&&parseFloat(s.outlineWidth)>0)||s.boxShadow!=='none');})()`);
      if(!focus) problems.push('no visible keyboard focus ring');
      results.push({width,route,controls:controls.length,problems});
      failures.push(...problems.map(p=>`${width} ${route}: ${p}`));
      fs.writeFileSync(path.join(process.env.OUT, "progress.json"), JSON.stringify(results,null,2));
      console.log(`${problems.length?'FAIL':'ok'} ${width} ${route}: ${controls.length} controls, ${problems.length} violations`);
    };
    try {
      for(const [route,selector] of fixture.routes) {
        await tab.send('Page.navigate',{url:new URL(route,base).href});
        assert.ok(await tab.until(`!!document.querySelector(${q(selector)})`,15000),route);
        await sleep(600);
        assert.ok(await tab.until(`!document.querySelector('.ch-wait')`,20000), `all charts finish staging on ${route}`);
        await audit(route);
        if(route===`/app/boards/${fixture.board.id}`) {
          await inp.click('[data-testid="board-edit"]'); assert.ok(await tab.until(`!!document.querySelector('[data-testid="board-editor"]')`));
          await audit(route+' edit'); await inp.click('[data-testid="board-cancel"]');
        }
      }
      if (!onboarding) {
        await tab.send('Network.clearBrowserCookies');
        await tab.send('Page.navigate',{url:new URL('/app/login',base).href});
        assert.ok(await tab.until(`!!document.querySelector('[data-testid="login"]')`));await audit('/app/login');
      }
    } catch(error) {failures.push(`${width}: ${error.stack}`);}
    finally {await tab.shot(path.join(process.env.OUT,`${width}-last.png`));await tab.close();}
  }
} finally {browser.quit();await fixture.cleanup();fs.writeFileSync(path.join(process.env.OUT,'results.json'),JSON.stringify({results,failures},null,2));}
for(const failure of failures) console.error('FAIL '+failure);
console.log(`${results.length} audits; ${failures.length} violations`);process.exitCode=failures.length?1:0;
