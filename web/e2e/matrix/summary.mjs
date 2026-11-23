// Summarise a matrix output directory: node summary.mjs <dir>
// Prints the engine x profile x flow table, problem counts by kind, and the
// distinct problems (deduplicated across states), then RESULT: PASS|FAIL.
// FAIL also when a unit of expected.txt (written by e2e-matrix.sh: the plan
// times the engines) has no result, or when there is no result at all.
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
const files = fs.readdirSync(dir).filter((f) => f.endsWith(".json"));
const runs = files.map((f) => JSON.parse(fs.readFileSync(path.join(dir, f), "utf8")));
const expectedFile = path.join(dir, "expected.txt");
const expected = fs.existsSync(expectedFile) ? fs.readFileSync(expectedFile, "utf8").split("\n").filter(Boolean) : null;
const missing = expected ? expected.filter((tag) => !files.includes(`${tag}.json`)) : [];
runs.sort((a, b) => `${a.engine}${a.unit}${a.profile}${a.theme}`.localeCompare(`${b.engine}${b.unit}${b.profile}${b.theme}`));
const kinds = {}, distinct = new Map();
let flowsOk = 0, flowsFail = 0, states = 0;
console.log("engine     unit   profile   theme  motion        flow                         result  states problems");
for (const r of runs) {
  if (r.fatal && !r.flows.length) console.log(`${r.engine.padEnd(10)} ${r.unit.padEnd(6)} ${r.profile.padEnd(9)} FATAL ${r.fatal}`);
  for (const fl of r.flows) {
    fl.ok ? flowsOk++ : flowsFail++;
    console.log(`${r.engine.padEnd(10)} ${r.unit.padEnd(6)} ${r.profile.padEnd(9)} ${r.theme.padEnd(6)} ${r.motion.padEnd(13)} ${fl.flow.padEnd(28)} ${(fl.ok ? (fl.skipped ? "n/a" : "pass") : "FAIL").padEnd(7)} ${String(fl.states).padStart(6)} ${String(fl.problems).padStart(8)}${fl.error ? "  " + fl.error.slice(0, 160) : ""}`);
  }
  for (const s of r.states) {
    states++;
    for (const p of s.problems) {
      kinds[p.kind] = (kinds[p.kind] || 0) + 1;
      const key = `${p.kind}: ${String(p.detail).replace(/\d{2,}x\d{2,}|\b\d{3,4}\b/g, "#")}`.slice(0, 220);
      const d = distinct.get(key) || { n: 0, where: new Set() };
      d.n++; d.where.add(`${r.engine}/${r.profile}`);
      distinct.set(key, d);
    }
  }
}
const excluded = runs.flatMap((r) => (r.excluded || []).map((x) => ({ ...x, where: `${r.engine}/${r.profile}/${r.unit}` })));
console.log(`\nruns ${runs.length}${expected ? ` of ${expected.length} expected` : ""}, flows ${flowsOk + flowsFail} (${flowsOk} pass, ${flowsFail} fail), states checked ${states}`);
console.log("problems by kind:", JSON.stringify(kinds));
// Named checks a flow counts itself (results.checks), summed per engine.
const checks = {};
for (const r of runs) for (const [k, n] of Object.entries(r.checks || {})) (checks[k] ??= {})[r.engine] = (checks[k][r.engine] || 0) + n;
for (const [k, by] of Object.entries(checks)) console.log(`check "${k}": ${Object.entries(by).map(([e, n]) => `${e} ${n}`).join(", ")}`);
console.log("\ndistinct problems:");
for (const [k, d] of [...distinct].sort((a, b) => b[1].n - a[1].n)) console.log(`${String(d.n).padStart(5)}  ${k}  [${[...d.where].slice(0, 6).join(" ")}${d.where.size > 6 ? " …" : ""}]`);
// Engine messages set aside by lib.mjs (screenshotProbe, leftPage), by reason.
const reasons = new Map();
for (const x of excluded) { const d = reasons.get(x.reason) || { n: 0, units: new Set(), texts: new Set() }; d.n++; d.units.add(x.where); d.texts.add(x.text.replace(/127\.0\.0\.1:\d+/g, "127.0.0.1:#").replace(/-[A-Za-z0-9_]{8}\.(css|js)/g, "-#.$1").slice(0, 110)); reasons.set(x.reason, d); }
console.log(`\nexcluded engine messages: ${excluded.length}`);
for (const [reason, d] of reasons) console.log(`  ${d.n}  ${reason} (${d.units.size} units): ${[...d.texts].slice(0, 6).join(" | ")}`);
for (const tag of missing) console.log(`MISSING ${tag}: no result`);
console.log(`\nRESULT: ${flowsFail || missing.length || !runs.length || runs.some((r) => r.fatal) ? "FAIL" : "PASS"}`);
