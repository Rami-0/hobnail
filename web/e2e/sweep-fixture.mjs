// Shared real-backend fixtures for route-wide CSP and accessibility sweeps.
import assert from 'node:assert/strict';
export function assertSweepTarget(base) {
  assert.match(base.hostname, /^(127\.0\.0\.1|localhost)$/);
  assert.ok(/^339[5-9]$/.test(base.port) || (base.hostname === "127.0.0.1" && base.port === "3300"));
}

export async function sweepFixture(base) {
  assertSweepTarget(base);
  assert.ok(process.env.HB_PASSWORD && process.env.OUT);
  const boot = await fetch(new URL('/api/v1/session', base)).then(r => r.json());
  const response = await fetch(new URL(boot.owner_exists ? '/api/login' : '/api/v1/onboarding/owner', base), {
    method: 'POST', headers: { Origin: base.origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ username: 'admin', password: process.env.HB_PASSWORD }),
  });
  assert.ok(response.ok, `sign in: ${response.status}`);
  const cookie = response.headers.get('set-cookie').split(';')[0];
  const { csrf } = await response.json();
  const headers = { Cookie: cookie, Origin: base.origin, 'Content-Type': 'application/json', 'X-RH-CSRF': csrf };
  const api = async (route, method = 'GET', body) => {
    const result = await fetch(new URL(route, base), { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    assert.ok(result.ok, `${method} ${route}: ${result.status} ${result.ok ? '' : await result.text()}`);
    return result.status === 204 ? null : result.json();
  };
  const board = await api('/api/v1/boards', 'POST', { name: 'Browser sweep board' });
  const { document } = await api(`/api/v1/boards/${board.id}`);
  document.sections = [{ title: 'Links', position: 0, items: [{ position: 0, spans: { mobile: 1, tablet: 3, desktop: 3, rows: 1 }, payload: { kind: 'link', title: 'Example', url: 'https://example.com/' } }] }];
  await api(`/api/v1/boards/${board.id}`, 'PUT', document);
  const routes = [
    ['/app/overview', '[data-testid="board"] [data-testid="hobnail-attention"] .hw-count'],
    ['/app/containers', '[data-testid="row"]'],
    ...['overview', 'metrics', 'inspect', 'events', 'processes'].map(tab => [`/app/containers/svc001?tab=${tab}`, tab === 'inspect' ? '[data-testid="container-inspect"]' : '[data-testid="ctr-tabs"]']),
    ['/app/host', '[data-testid="host-sample"]'], ['/app/history', '[data-testid="history"]'], ['/app/alerts', '[data-testid="alerts"]'],
    ['/app/logs', '[data-testid="logs-picker-row"]'], ['/app/containers/svc001/logs', '[data-log-id]'],
    ['/app/stacks/stack01/logs', '[data-log-id]'], ['/app/logs?c=svc001,svc002,svc003,svc005', '[data-log-id]'],
    [`/app/boards/${board.id}`, '[data-testid="board-tile"]'],
    ...['general','containers','bookmarks','widgets','notifications','appearance','history','config','security','boards','integrations','users'].map(tab => [`/app/settings/${tab}`, '[data-testid="settings-tabs"]']),
    ['/app/account', '[data-testid="account-password"]'],
  ];
  return { cookie, api, board, routes, async installCookie(tab) {
    const split = cookie.indexOf('=');
    await tab.send('Network.setCookie', { name: cookie.slice(0, split), value: cookie.slice(split + 1), url: base.origin, path: '/', httpOnly: true, sameSite: 'Strict' });
  }, async cleanup() { await api(`/api/v1/boards/${board.id}`, 'DELETE'); } };
}
