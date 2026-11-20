// Release-age audit of the lock file (owner decision D3): every package in
// package-lock.json must have been published at least MIN_DAYS days ago.
// npm's min-release-age gates resolution; this re-checks what is locked.
//   scripts/web.sh node scripts/ages.mjs [min days, default 7]
import { readFile } from "node:fs/promises";

const MIN_DAYS = Number(process.argv[2] ?? 7);
const lock = JSON.parse(await readFile(new URL("../package-lock.json", import.meta.url), "utf8"));
const now = Date.now();
const seen = new Map();
for (const [path, p] of Object.entries(lock.packages)) {
  if (!path) continue;
  const name = p.name ?? path.slice(path.lastIndexOf("node_modules/") + 13);
  seen.set(`${name}@${p.version}`, { name, version: p.version });
}
let bad = 0;
const rows = [];
for (const { name, version } of seen.values()) {
  // The full document: the abbreviated one has no per-version times.
  const r = await fetch(`https://registry.npmjs.org/${name.replace("/", "%2F")}`);
  const meta = await r.json();
  const t = meta.time?.[version];
  const days = t ? (now - Date.parse(t)) / 86_400_000 : NaN;
  // D2's owner override applies only to these reviewed SDK releases.
  const sdkException = ["@ui-sdk/react", "@ui-sdk/core"].includes(name)
    && version === "0.1.0-beta.10";
  const ok = days >= MIN_DAYS || sdkException;
  if (!ok) bad++;
  rows.push(`${sdkException ? "SDK" : ok ? "ok " : "NEW"} ${days.toFixed(1).padStart(7)} d  ${name}@${version}  ${t ?? "no publish time"}`);
}
rows.sort();
console.log(rows.join("\n"));
console.log(`${seen.size} packages, ${bad} younger than ${MIN_DAYS} days`);
process.exit(bad ? 1 : 0);
