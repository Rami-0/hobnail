// M2b board/integration browser acceptance. Run only against a throwaway
// fake-instance on 3395–3399, through scripts/e2e.sh. No npm dependencies.
import fs from "node:fs";
import path from "node:path";
import { launch, input, recorder, sleep, q, CSP_LISTENER } from "./cdp.mjs";
const base = (process.argv[2] ?? "").replace(/\/+$/, "");
if (!/^https?:\/\//.test(base) || !/^339[5-9]$/.test(new URL(base).port) || !process.env.HB_PASSWORD || !process.env.OUT) throw new Error("usage: HB_PASSWORD=... OUT=... scripts/e2e.sh web/e2e/board-check.mjs http://127.0.0.1:3396");
const PW = process.env.HB_PASSWORD, OUT = process.env.OUT, origin = new URL(base).origin;
fs.mkdirSync(OUT, { recursive: true });
const browser = await launch(); const tab = await browser.tab(); const inp = input(tab); const rec = recorder(tab, origin);
for (const domain of ["Page", "Runtime", "Network", "Log"]) await tab.send(`${domain}.enable`);
await tab.send("Runtime.addBinding", { name: "__hbCsp" });
await tab.send("Page.addScriptToEvaluateOnNewDocument", { source: CSP_LISTENER });
tab.on("Page.javascriptDialogOpening", () => { void tab.send("Page.handleJavaScriptDialog", { accept: true }); });
rec.allow(({ method, status, url }) => method === "PUT" && status === 409 && /\/api\/v1\/boards\//.test(url) ? "intentional concurrent edit" : false);
let checks = 0, failed = 0, boardId, hiddenBoardId, integrationId, owner;
const check = (ok, message) => { checks++; if (!ok) failed++; console.log(`${ok ? "ok" : "FAIL"} ${message}`); if (!ok) throw new Error(message); };
const wait = async (expression, label = expression) => check(await tab.until(expression, 10000), label);
const go = async (route, ready) => { await tab.send("Page.navigate", { url: base + route }); await wait(ready); };
const fill = async (selector, text) => { await inp.click(selector); await inp.key("a", { modifiers: 2 }); await inp.key("Backspace"); await inp.type(text); };
const visible = (test) => `!!document.querySelector('[data-testid="${test}"]')`;
const api = async (method, route, body, session = owner) => {
  const response = await fetch(base + "/api/v1" + route, { method, headers: { "Content-Type": "application/json", Origin: origin, Cookie: session.cookie, "X-RH-CSRF": session.csrf }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  const text = await response.text(); let data; try { data = JSON.parse(text); } catch {}
  return { status: response.status, text, data };
};
const login = async () => { const response = await fetch(base + "/api/login", { method: "POST", headers: { "Content-Type": "application/json", Origin: origin }, body: JSON.stringify({ username: "admin", password: PW }) }); if (!response.ok) throw new Error("API login failed"); return { cookie: response.headers.get("set-cookie").split(";")[0], csrf: (await response.json()).csrf }; };
let boardPuts = 0; const responseTasks = []; const SECRET = "m2-browser-secret-never-return-836291";
const integrationResponses = new Set();
tab.on("Network.requestWillBeSent", ({ request }) => { if (boardId && request.method === "PUT" && new URL(request.url).pathname === `/api/v1/boards/${boardId}`) boardPuts++; });
tab.on("Network.responseReceived", ({ requestId, response }) => { if (/\/api\/v1\/integrations/.test(response.url)) integrationResponses.add(requestId); });
tab.on("Network.loadingFinished", ({ requestId }) => { if (integrationResponses.delete(requestId)) responseTasks.push(tab.send("Network.getResponseBody", { requestId }).then((result) => { const body = result.result?.body ?? ""; if (body.includes(SECRET)) throw new Error("credential leaked in an integration response"); })); });
const edit = async () => { await inp.click('[data-testid="board-edit"]'); await wait(visible("board-editor"), "edit mode opens"); };
const addLink = async (title) => {
  await inp.click('[data-testid="board-add-item"]'); await wait(visible("board-picker"));
  await fill('[data-testid="board-link-form"] input[name="title"]', title); await fill('[data-testid="board-link-form"] input[name="url"]', "https://example.com/" + title);
  await inp.click('[data-testid="board-link-form"] button[type="submit"]'); await wait(`!${visible("board-picker")}`, "link picker closes");
};
try {
  await go("/app", '!!document.querySelector("form")');
  const onboarding = await tab.ev(visible("onboarding")); const form = onboarding ? "onboarding" : "login";
  await fill(`form[data-testid="${form}"] input[name="username"]`, "admin"); await fill(`form[data-testid="${form}"] input[name="password"]`, PW);
  if (onboarding) await fill('form[data-testid="onboarding"] input[name="confirm"]', PW);
  await inp.click(`form[data-testid="${form}"] button[type="submit"]`); await wait(visible("user"), "onboard or sign in through the browser"); owner = await login();
  await go("/app/settings/boards", visible("board-create")); await inp.click('[data-testid="board-create"]'); await wait(visible("board-form"));
  await fill('#board-form input[name="name"]', "M2 browser board"); await inp.click('button[form="board-form"][type="submit"]'); await wait(`!${visible("board-form")}`, "create board closes form");
  boardId = (await api("GET", "/boards")).data.find((board) => board.name === "M2 browser board").id;
  await go(`/app/boards/${boardId}`, visible("board-empty")); check((await api("GET", `/boards/${boardId}`)).data.document.sections.length === 0, "a new board is empty");
  await edit(); await inp.click('[data-testid="board-add-section"]'); await inp.click('[data-testid="board-cancel"]'); await wait(visible("board-empty"), "Cancel discards layout changes");
  check((await api("GET", `/boards/${boardId}/draft`)).data === null, "Cancel deletes server draft");
  await edit(); await inp.click('[data-testid="board-add-section"]'); await addLink("first"); await addLink("second");
  await tab.ev(`document.querySelector('[data-section="0"][data-item="0"] .drag-handle').focus()`); await inp.key("ArrowDown", { modifiers: 1 });
  await wait(`document.querySelector('[data-section="0"][data-item="0"] h3')?.textContent === "second"`, "Alt+Down moves an item");
  await inp.key("ArrowRight", { modifiers: 8 });
  await wait(`document.querySelector('[data-section="0"][data-item="1"] input[id$="desktop"]')?.value === "5"`, "Shift+Right resizes the moved item");
  await inp.click('[data-testid="board-add-item"]'); await wait('!!document.querySelector(".suggestions button")', "suggestions include discovered containers"); await inp.click('.suggestions button'); await wait(`!${visible("board-picker")}`);
  const before = boardPuts; await inp.click('[data-testid="board-save"]'); await wait(`!${visible("board-editor")}`, "Save exits edit mode"); check(boardPuts - before === 1, "Save sends exactly one board PUT");
  await tab.send("Page.reload"); await wait('document.querySelectorAll("[data-testid=board-tile]").length === 3', "reload preserves saved items");
  await edit(); await inp.click('[data-testid="board-add-section"]'); await wait(`document.querySelector('[data-testid="draft-status"]')?.textContent.includes("Draft saved")`, "draft saved to server");
  await inp.click('nav[data-testid="sidebar"] a[href="/app/containers"]'); await wait('!!document.querySelector("[role=dialog][data-open]")', "unsaved navigation asks before leaving"); await inp.click('[role="dialog"][data-open] .dlg-ft button'); await wait(visible("board-editor"), "Keep editing preserves local work");
  await tab.send("Page.reload"); await wait(visible("board-draft"), "reload offers server draft");
  await inp.key("Escape"); await wait(`!${visible("board-draft")}`, "dismiss draft prompt without discarding");
  await inp.click('[data-testid="board-edit"]'); await wait(visible("board-draft"), "Edit offers the preserved draft again");
  await inp.click('[data-testid="draft-resume"]'); await wait(visible("board-editor"), "draft resumes");
  await wait(`document.querySelector('[data-testid="draft-status"]')?.textContent.includes("Draft saved")`);
  // Drafts are per user: another session of this same owner would clear
  // the owner's shared draft when it saves. Use a distinct Admin principal.
  const adminInvite = await api("POST", "/invites", { role: "admin" });
  check(adminInvite.status === 201, "create a separate Admin editing session");
  const adminAccepted = await fetch(base + "/api/v1/invites/accept", { method: "POST", headers: { "Content-Type": "application/json", Origin: origin }, body: JSON.stringify({ token: adminInvite.data.token, username: `editor-${Date.now()}`, password: PW }) });
  const secondSession = { cookie: adminAccepted.headers.get("set-cookie").split(";")[0], csrf: (await adminAccepted.json()).csrf };
  const remote = (await api("GET", `/boards/${boardId}`, undefined, secondSession)).data.document;
  remote.sections.push({ title: "Other session", position: remote.sections.length, items: [] }); check((await api("PUT", `/boards/${boardId}`, remote, secondSession)).status === 200, "second session saves a board version");
  await inp.click('[data-testid="board-save"]'); await wait(`${visible("board-conflict")} && document.querySelector('[data-testid="board-save"]').disabled`, "a real 409 exposes conflict choices and disables Save");
  await tab.send("Page.reload"); await wait(visible("board-draft"), "stale draft survives a reload after conflict");
  await inp.click('[data-testid="draft-resume"]'); await wait(visible("board-conflict"), "recovered stale draft requires explicit rebase");
  await inp.click('[data-testid="board-rebase"]'); await wait(`!document.querySelector('[data-testid="board-save"]').disabled`, "explicit rebase enables Save");
  await inp.click('[data-testid="board-save"]'); await wait(`!${visible("board-editor")}`, "rebased board saves");
  check((await api("GET", `/boards/${boardId}`)).data.document.sections.some((section) => section.title === "Other session"), "rebase preserves the other session's addition");
  for (const width of [1440, 390]) for (const theme of ["dark", "light"]) {
    await tab.send("Emulation.setDeviceMetricsOverride", { width, height: width === 390 ? 844 : 900, deviceScaleFactor: 1, mobile: width === 390 });
    await tab.ev(`document.documentElement.dataset.theme = ${q(theme)}`); await sleep(250);
    check(await tab.ev("document.documentElement.scrollWidth <= innerWidth"), `${width} ${theme}: board has no sideways scroll`); await tab.shot(path.join(OUT, `board-${width}-${theme}.png`));
    await edit(); await sleep(250); check(await tab.ev("document.documentElement.scrollWidth <= innerWidth"), `${width} ${theme}: editor has no sideways scroll`); await tab.shot(path.join(OUT, `editor-${width}-${theme}.png`)); await inp.click('[data-testid="board-cancel"]'); await wait(visible("board-edit"));
  }
  await tab.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await go("/app/settings/integrations", visible("integration-create")); await inp.click('[data-testid="integration-create"]'); await wait(visible("integration-form"));
  await fill('#integration-form input[name="name"]', "M2 HTTP connection"); await fill('#integration-form input[name="base_url"]', "http://127.0.0.1:1"); await fill('#integration-form input[name="secret"]', SECRET);
  await inp.click('button[form="integration-form"][type="submit"]'); await wait(`!${visible("integration-form")}`, "integration saves");
  const integrations = await api("GET", "/integrations"); integrationId = integrations.data.find((item) => item.name === "M2 HTTP connection").id;
  check(!integrations.text.includes(SECRET), "credential never appears in integration list response");
  check(await tab.ev(`!document.documentElement.textContent.includes(${q(SECRET)}) && ![...document.querySelectorAll("input")].some((element) => element.value.includes(${q(SECRET)}))`), "credential does not remain in DOM after Save");
  await inp.click('[data-testid="integration-row"] button'); await wait(visible("integration-form")); check(await tab.ev('document.querySelector("input[name=secret]").value === ""'), "edit credential is never prefilled");
  await fill('#integration-form input[name="base_url"]', "http://127.0.0.1:2"); await inp.click('button[form="integration-form"][type="submit"]'); await wait('document.querySelector("#integration-secret-err")?.textContent.includes("again")', "origin change requires entering credential again");
  const denied = await api("PUT", `/integrations/${integrationId}`, { name: "M2 HTTP connection", kind: "http", base_url: "http://127.0.0.1:2", enabled: true }); check(denied.status === 400 && denied.data.error.field === "secret", "server enforces origin binding on secret field");
  await inp.click('[data-testid="integration-form"] .dlg-ft button'); await inp.click('[data-testid="integration-test"]'); await wait(visible("integration-verdict"), "connection test displays verdict class and message");
  hiddenBoardId = (await api("POST", "/boards", { name: "Outside Viewer scope" })).data.id;
  const invite = await api("POST", "/invites", { role: "viewer", scopes: { boards: [boardId], integrations: [] } }); check(invite.status === 201, "create scoped Viewer invite");
  const accepted = await fetch(base + "/api/v1/invites/accept", { method: "POST", headers: { "Content-Type": "application/json", Origin: origin }, body: JSON.stringify({ token: invite.data.token, username: `viewer-${Date.now()}`, password: PW }) });
  const viewer = { cookie: accepted.headers.get("set-cookie").split(";")[0], csrf: (await accepted.json()).csrf };
  const split = viewer.cookie.indexOf("="); await tab.send("Network.setCookie", { name: viewer.cookie.slice(0, split), value: viewer.cookie.slice(split + 1), url: base, httpOnly: true, sameSite: "Strict", path: "/" });
  await go(`/app/boards/${boardId}`, visible("board")); check(await tab.ev(`!${visible("board-edit")}`), "Viewer sees no board edit controls");
  check((await api("PUT", `/boards/${boardId}`, (await api("GET", `/boards/${boardId}`)).data.document, viewer)).status === 403, "Viewer writes return 403");
  check((await api("GET", "/integrations", undefined, viewer)).data.length === 0, "Viewer integration scope filters results");
  check((await api("GET", "/boards", undefined, viewer)).data.every((board) => board.id === boardId), "Viewer board navigation is scope-filtered");
  rec.allow(({ method, status, url }) => method === "GET" && status === 404 && new URL(url).pathname === `/api/v1/boards/${hiddenBoardId}` ? "intentional out-of-scope board" : false);
  await go(`/app/boards/${hiddenBoardId}`, 'document.querySelector("main h1")?.textContent === "No such page"');
  check(await tab.ev(`!${visible("board-edit")}`), "out-of-scope board renders not found without edit controls");
  await Promise.all(responseTasks); await sleep(250);
  check(rec.csp.length === 0, `zero CSP violations (${rec.csp.length})`); check(rec.console.length === 0, `zero console errors (${rec.console.length})`); check(rec.exceptions.length === 0, `zero uncaught exceptions (${rec.exceptions.length})`); check(rec.failed.length === 0, `zero unexpected HTTP/network failures (${rec.failed.length})`);
} catch (error) { failed++; console.error(error.stack); await tab.shot(path.join(OUT, "failure.png")); }
finally {
  if (integrationId) await api("DELETE", `/integrations/${integrationId}`).catch(() => {});
  if (hiddenBoardId) await api("DELETE", `/boards/${hiddenBoardId}`).catch(() => {});
  if (boardId) await api("DELETE", `/boards/${boardId}`).catch(() => {});
  fs.writeFileSync(path.join(OUT, "result.json"), JSON.stringify({ checks, failed, recorder: rec }, null, 2));
  await tab.close(); browser.quit();
}
console.log(`${checks} checks; ${failed} failures; screenshots and log data: ${OUT}`); process.exitCode = failed ? 1 : 0;
