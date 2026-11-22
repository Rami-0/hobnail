// One matrix unit: one engine, one profile, one fresh instance.
//   ENGINE=chromium|firefox|webkit PROFILE=<lib.mjs PROFILES key>
//   UNIT=flows   onboarding with every step, then every flow
//   UNIT=skip    onboarding skipping every step, then login and logout
//   UNIT=sweep   the route sweep (owner created through the API)
//   THEME=light|dark MOTION=reduce|no-preference FLOWS="a,b" (optional filter)
// node web/e2e/matrix/run.mjs <base url> [fixture control url]
// Writes $OUT/<engine>-<profile>-<unit>-<theme>.json; exit 1 on any failure.
import fs from "node:fs";
import path from "node:path";
import { PROFILES, assertTarget, contextOptions, harness, launch } from "./lib.mjs";
import { ensureSignedIn, flows, onboarding, sweep, tourStaysClosed } from "./flows.mjs";
import { sweepFixture } from "../sweep-fixture.mjs";

const base = process.argv[2];
assertTarget(base);
const fixture = process.argv[3] || "";
if (fixture && (new URL(fixture).hostname !== "127.0.0.1" || new URL(fixture).port === "3200")) throw new Error("fixture control must be local");
const engine = process.env.ENGINE, profileName = process.env.PROFILE, unit = process.env.UNIT || "flows";
const theme = process.env.THEME || "light", motion = process.env.MOTION || "no-preference";
const profile = PROFILES[profileName];
if (!profile || !["chromium", "firefox", "webkit"].includes(engine)) throw new Error(`bad ENGINE/PROFILE ${engine}/${profileName}`);
const password = process.env.HB_PASSWORD;
if (!password || password.length < 12) throw new Error("HB_PASSWORD missing");
const only = (process.env.FLOWS || "").split(",").map((s) => s.trim()).filter(Boolean);
const out = process.env.OUT;
const tag = `${engine}-${profileName}-${unit}-${theme}${motion === "reduce" ? "-rm" : ""}`;
const results = { engine, profile: profileName, unit, theme, motion, flows: [], states: [], started: new Date().toISOString() };
const f = { base, password, fixture };

const browser = await launch(engine);
results.version = browser.version();
const options = contextOptions(engine, profile, { theme, motion });
const context = await browser.newContext(options);
context.setDefaultTimeout(15000);
const page = await context.newPage();
const h = await harness(page, { engine, profileName, profile, out, unit, motion, results });
h.contextOptions = options;

async function run(name, fn) {
  if (only.length && !only.includes(name) && !name.startsWith("onboarding") && name !== "tour stays closed") return;
  h.ctx.flow = name;
  const t0 = Date.now(), first = results.states.length;
  let error = null, note = null;
  try { note = (await fn(h, f)) || null; } catch (e) { error = String(e?.stack || e).split("\n").slice(0, 4).join(" | ").slice(0, 600); await h.shot(`${name}-FAIL`); }
  const states = results.states.slice(first);
  const problems = states.reduce((n, s) => n + s.problems.length, 0);
  const ok = !error && problems === 0;
  results.flows.push({ flow: name, ok, skipped: !!note && !error && states.length === 0, note, error, states: states.length, problems, ms: Date.now() - t0 });
  console.log(`${ok ? "ok  " : "FAIL"} ${tag} ${name}: ${states.length} states, ${problems} problems${note ? " (" + note + ")" : ""}${error ? "\n     " + error : ""}`);
  return ok;
}

try {
  if (unit === "flows") {
    await run("onboarding: every step", onboarding.full);
    if (!f.board) throw new Error("onboarding failed; the flows need its board");
    // Straight after onboarding, while the page still holds its session.
    await run("tour stays closed", tourStaysClosed);
    for (const [name, fn] of Object.entries(flows)) {
      await ensureSignedIn(h, f).catch(() => {});
      await run(name, fn);
    }
  } else if (unit === "skip") {
    if (await run("onboarding: skip", onboarding.skip)) {
      await run("tour stays closed", tourStaysClosed);
      await run("login and logout", flows["login and logout"]);
    }
  } else if (unit === "sweep") {
    process.env.OUT ||= out;
    const fx = await sweepFixture(new URL(base));
    const split = fx.cookie.indexOf("=");
    await context.addCookies([{ name: fx.cookie.slice(0, split), value: fx.cookie.slice(split + 1), url: new URL(base).origin, httpOnly: true, sameSite: "Strict" }]);
    // The sweep starts on a settled instance: finish onboarding's setup.
    await run("page sweep", (hh) => sweep(hh, f, fx.routes));
    await context.clearCookies();
    await run("login page", async (hh) => { await page.goto(new URL("/app/login", base).href); await page.locator('form[data-testid="login"]').waitFor(); await hh.state("/app/login"); });
  }
} catch (e) {
  console.log(`FAIL ${tag}: ${String(e?.message || e)}`);
  results.fatal = String(e?.message || e);
} finally {
  results.finished = new Date().toISOString();
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, `${tag}.json`), JSON.stringify(results, null, 1));
  await context.close().catch(() => {});
  await browser.close().catch(() => {});
}
process.exitCode = results.fatal || results.flows.some((x) => !x.ok) ? 1 : 0;
