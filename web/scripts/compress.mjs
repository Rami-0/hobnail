// Gzip copies for the embedded client (docs/plans/react-port.md 4.3): after
// `vite build`, write `<file>.gz` (level 9) next to every compressible file
// in dist/, kept only when smaller. build.rs embeds both; the server sends
// the copy when the request accepts gzip. No runtime compression.
//   npm run build   (vite build && node scripts/compress.mjs)
import { readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync, constants } from "node:zlib";

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));
const COMPRESSIBLE = /\.(js|mjs|css|html|svg|json|webmanifest|txt|map|wasm)$/i;

async function* files(dir) {
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* files(p);
    else if (e.isFile()) yield p;
  }
}

const rows = [];
let raw = 0;
let gz = 0;
for await (const p of files(DIST)) {
  if (p.endsWith(".gz")) continue;
  if (!COMPRESSIBLE.test(p)) continue;
  const buf = await readFile(p);
  const z = gzipSync(buf, { level: constants.Z_BEST_COMPRESSION });
  const keep = z.length < buf.length;
  if (keep) await writeFile(`${p}.gz`, z);
  else await rm(`${p}.gz`, { force: true }); // a stale copy from an earlier build
  raw += buf.length;
  gz += keep ? z.length : buf.length;
  rows.push([relative(DIST, p), buf.length, keep ? z.length : null]);
}
rows.sort((a, b) => a[0].localeCompare(b[0]));
const w = Math.max(4, ...rows.map((r) => r[0].length));
console.log(`${"file".padEnd(w)}  ${"raw".padStart(9)}  ${"gzip".padStart(9)}`);
for (const [f, r, z] of rows) {
  console.log(`${f.padEnd(w)}  ${String(r).padStart(9)}  ${(z === null ? "kept raw" : String(z)).padStart(9)}`);
}
console.log(`${"total".padEnd(w)}  ${String(raw).padStart(9)}  ${String(gz).padStart(9)}  (${rows.length} files)`);
