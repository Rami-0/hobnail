// Source guard, run by `npm run check`. Fails (exit 1) when src/ contains
// something the CSP or the project rules forbid (web/AGENTS.md):
// - HTML injection or eval: dangerouslySetInnerHTML, .innerHTML (also
//   outerHTML / insertAdjacentHTML), eval(, new Function(, document.write;
// - a UI primitive library imported outside src/ui/ (the wrapper folder;
//   only src/main.tsx may import @ui-sdk/core/web.css);
// - recharts imported outside src/charts/;
// - src/charts/ reached other than by a lazy `import("…/charts/…")` from
//   a chart-bearing route (the charts chunk is never on the first route;
//   `import type` is fine, it is erased).
// Comments are not exempt: rewording a comment is cheaper than an
// allow-list that hides a real use.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const src = join(root, "src");

const BANNED = [
  [/dangerouslySetInnerHTML/, "dangerouslySetInnerHTML"],
  [/\.(inner|outer)HTML\b/, ".innerHTML / .outerHTML"],
  [/\binsertAdjacentHTML\b/, "insertAdjacentHTML"],
  [/\beval\s*\(/, "eval("],
  [/\bnew\s+Function\s*\(/, "new Function("],
  [/\bdocument\s*\.\s*write(ln)?\b/, "document.write"],
];

// Libraries that ship interactive primitives; only src/ui/ may import them.
const UI_LIBS = [
  "@ui-sdk/",
  "@base-ui/",
  "@base-ui-components/",
  "@radix-ui/",
  "radix-ui",
  "react-aria",
  "react-aria-components",
  "@react-aria/",
  "@ark-ui/",
  "@headlessui/",
  "@ariakit/",
  "ariakit",
  "@mui/",
  "@chakra-ui/",
  "@mantine/",
];

const IMPORT = /(?:\bfrom\s*|\bimport\s*\(?\s*)["']([^"']+)["']/g;
// Static imports and re-exports (not `import type` / `export type`), and dynamic ones.
const STATIC = /^\s*(?:import|export)\s+(?!type\b)(?:[^;]*?\bfrom\s*)?["']([^"']+)["']/gm;
const DYNAMIC = /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g;
const chartsDir = join(src, "charts");
const inCharts = (file, spec) => spec.startsWith(".") && (resolve(dirname(file), spec) + sep).startsWith(chartsDir + sep);

function* files(dir) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) yield* files(p);
    else if (/\.(ts|tsx|js|jsx|mjs|css|html)$/.test(e.name)) yield p;
  }
}

const problems = [];
for (const file of files(src)) {
  const rel = relative(root, file).split(sep).join("/");
  const text = readFileSync(file, "utf8");
  const lines = text.split("\n");
  lines.forEach((line, i) => {
    for (const [re, what] of BANNED) {
      if (re.test(line)) problems.push(`${rel}:${i + 1}: ${what} is not allowed (CSP, web/AGENTS.md)`);
    }
  });
  for (const m of text.matchAll(IMPORT)) {
    const spec = m[1];
    const line = text.slice(0, m.index).split("\n").length;
    const isUi = UI_LIBS.some((l) => (l.endsWith("/") ? spec.startsWith(l) : spec === l || spec.startsWith(l + "/")));
    // The one exception: UI SDK's stylesheet is imported once at the entry
    // (decision rule 4).
    const entryCss = spec === "@ui-sdk/core/web.css" && rel === "src/main.tsx";
    if (isUi && !entryCss && !rel.startsWith("src/ui/")) {
      problems.push(`${rel}:${line}: import "${spec}" only from src/ui/ (the wrapper folder)`);
    }
    if ((spec === "recharts" || spec.startsWith("recharts/")) && !rel.startsWith("src/charts/")) {
      problems.push(`${rel}:${line}: import "recharts" only from src/charts/`);
    }
  }
  if (!rel.startsWith("src/charts/")) {
    for (const m of text.matchAll(STATIC)) {
      if (!inCharts(file, m[1])) continue;
      const line = text.slice(0, m.index).split("\n").length;
      problems.push(`${rel}:${line}: "${m[1]}" is the charts chunk: load it with a lazy import() from a chart-bearing route`);
    }
    for (const m of text.matchAll(DYNAMIC)) {
      if (!inCharts(file, m[1]) || /^src\/routes\/(container|history|overview|host)\//.test(rel)) continue;
      const line = text.slice(0, m.index).split("\n").length;
      problems.push(`${rel}:${line}: the charts chunk is loaded only from a chart-bearing route`);
    }
  }
}

if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`guard: ${problems.length} problem(s)`);
  process.exit(1);
}
console.log("guard: ok");
