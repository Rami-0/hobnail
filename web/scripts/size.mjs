// Bundle budgets (docs/plans/react-port.md 4.9), checked after the build;
// exit 1 on a miss. KB = 1000 B. "gzip" is the `.gz` copy compress.mjs
// wrote, or the raw size when there is none.
//   first route (index.html + every JS/CSS it references directly,
//     modulepreload links included)                 <= 180 KB gzip
//   any other JS/CSS chunk                           <=  60 KB gzip
//     charts vendor chunk ("recharts"/"charts-vendor") <= 110 KB gzip
//   whole dist: <= 1.3 MB raw (non-.gz files) and <= 400 KB gzip
//   npm run size
import { readdir, readFile, stat } from "node:fs/promises";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const KB = 1000;
const BUDGET = {
  firstRoute: 180 * KB,
  chunk: 60 * KB,
  charts: 110 * KB,
  distRaw: 1300 * KB,
  distGzip: 400 * KB,
};
const DIST = fileURLToPath(new URL("../dist/", import.meta.url));

async function* walk(dir) {
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* walk(p);
    else if (e.isFile()) yield p;
  }
}

let indexHtml;
try {
  indexHtml = await readFile(join(DIST, "index.html"), "utf8");
} catch {
  console.error(`size: ${DIST}index.html not found; run the build first`);
  process.exit(1);
}

const files = new Map(); // rel path -> { raw, gzip }
for await (const p of walk(DIST)) {
  if (p.endsWith(".gz")) continue;
  const rel = relative(DIST, p).split(sep).join("/");
  const raw = (await stat(p)).size;
  const gzip = await stat(`${p}.gz`).then((s) => s.size, () => raw);
  files.set(rel, { raw, gzip });
}

// Direct references of index.html: <script src>, <link rel=stylesheet|modulepreload href>.
const first = new Set(["index.html"]);
for (const tag of indexHtml.match(/<(script|link)\b[^>]*>/gi) ?? []) {
  const attr = (n) => tag.match(new RegExp(`\\s${n}\\s*=\\s*["']?([^"'\\s>]+)`, "i"))?.[1];
  const isLink = /^<link/i.test(tag);
  const rel = (attr("rel") ?? "").toLowerCase();
  if (isLink && !/\b(stylesheet|modulepreload)\b/.test(rel)) continue;
  const url = isLink ? attr("href") : attr("src");
  if (!url || /^[a-z]+:|^\/\//i.test(url)) continue;
  const path = url.split(/[?#]/)[0].replace(/^\.?\//, "");
  if (!files.has(path)) {
    // Served by the Rust server at runtime (e.g. /custom.css); not part of
    // the bundle. A missing hashed asset is a broken build.
    if (path.startsWith("assets/")) {
      console.error(`size: index.html references ${url}, which is not in dist/`);
      process.exit(1);
    }
    continue;
  }
  first.add(path);
}

const isChunk = (f) => /\.(m?js|css)$/i.test(f);
const isCharts = (f) => /recharts|charts-vendor/i.test(f);
const misses = [];
const rows = [];
let firstGzip = 0;
let totalRaw = 0;
let totalGzip = 0;
for (const [f, { raw, gzip }] of [...files].sort((a, b) => a[0].localeCompare(b[0]))) {
  totalRaw += raw;
  totalGzip += gzip;
  let note = "";
  if (first.has(f)) {
    firstGzip += gzip;
    note = "first route";
  } else if (isChunk(f)) {
    const limit = isCharts(f) ? BUDGET.charts : BUDGET.chunk;
    note = `${isCharts(f) ? "charts chunk" : "chunk"} <= ${limit / KB} KB`;
    if (gzip > limit) {
      note += "  OVER";
      misses.push(`${f}: ${fmt(gzip)} gzip > ${limit / KB} KB`);
    }
  }
  rows.push([f, raw, gzip, note]);
}
check("first route", firstGzip, BUDGET.firstRoute, "gzip");
check("dist", totalRaw, BUDGET.distRaw, "raw");
check("dist", totalGzip, BUDGET.distGzip, "gzip");

function fmt(n) {
  return `${(n / KB).toFixed(1)} KB`;
}
function check(what, n, limit, kind) {
  if (n > limit) misses.push(`${what}: ${fmt(n)} ${kind} > ${limit / KB} KB`);
}

const w = Math.max(5, ...rows.map((r) => r[0].length));
const line = (a, b, c, d = "") =>
  console.log(`${a.padEnd(w)}  ${String(b).padStart(9)}  ${String(c).padStart(9)}  ${d}`);
line("file", "raw B", "gzip B");
for (const r of rows) line(...r);
line("total", totalRaw, totalGzip, `budget ${BUDGET.distRaw / KB} KB raw / ${BUDGET.distGzip / KB} KB gzip`);
line("first route", "", firstGzip, `budget ${BUDGET.firstRoute / KB} KB gzip (${[...first].join(", ")})`);
if (misses.length) {
  console.error(`\nsize: ${misses.length} budget miss(es):\n  ${misses.join("\n  ")}`);
  process.exit(1);
}
console.log("\nsize: all budgets met");
