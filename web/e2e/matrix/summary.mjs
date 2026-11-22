// Summarise a matrix output directory: node summary.mjs <dir>
// Prints the engine x profile x flow table, problem counts by kind, and the
// distinct problems (deduplicated across states), then RESULT: PASS|FAIL.
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
const runs = fs.readdirSync(dir).filter((f) => f.endsWith(".json")).map((f) => JSON.parse(fs.readFileSync(path.join(dir, f), "utf8")));
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
console.log(`\nruns ${runs.length}, flows ${flowsOk + flowsFail} (${flowsOk} pass, ${flowsFail} fail), states checked ${states}`);
console.log("problems by kind:", JSON.stringify(kinds));
console.log("\ndistinct problems:");
for (const [k, d] of [...distinct].sort((a, b) => b[1].n - a[1].n)) console.log(`${String(d.n).padStart(5)}  ${k}  [${[...d.where].slice(0, 6).join(" ")}${d.where.size > 6 ? " …" : ""}]`);
console.log(`\nRESULT: ${flowsFail || runs.some((r) => r.fatal) ? "FAIL" : "PASS"}`);
