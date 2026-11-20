// The formatting twins against fixtures.json: inputs and the outputs the
// Rust helpers give (written by src/api/tests.rs). Every case is asserted.
import assert from "node:assert/strict";
import { test } from "node:test";

import type { PortBinding } from "../api/gen/PortBinding.ts";
import { ago, bindingLink, bytes, duration, fixed, pct, portLinks, rowLink, statusOf, statusText, urlHostPart, viewerHost, imageLabel, type StatusIn } from "./index.ts";

interface Fixtures {
  human_bytes: { in: number; out: string }[];
  human_duration: { in: number; out: string }[];
  pct: { in: number; out: string }[];
  ago: { at: number; now: number; out: string }[];
  status: (StatusIn & Record<string, unknown>)[];
  binding_link: { case: string; host: string; binding: PortBinding; out: { url: string } | { none: string } }[];
  port_views: { case: string; host: string; ports: PortBinding[]; out: unknown[] }[];
  row_link: { case: string; host: string; link: string | null; ports: PortBinding[]; out: string | null }[];
}

// A variable path: tsc does not resolve it, Node loads it at run time.
const path = "./fixtures.json";
let fx: Fixtures | null = null;
try {
  fx = ((await import(path, { with: { type: "json" } })) as { default: Fixtures }).default;
} catch {
  fx = null;
}
const skip = fx === null;
if (skip) console.log("# format: fixtures.json is missing (src/api/tests.rs writes it); the twin cases are skipped");

const each = <T>(name: string, rows: T[] | undefined, fn: (r: T) => void) =>
  test(`fixtures: ${name} (${rows?.length ?? 0} cases)`, () => {
    if (!rows) return;
    assert.ok(rows.length > 0, `${name}: no cases`);
    for (const r of rows) fn(r);
  });

if (fx) {
  const f = fx;
  each("human_bytes", f.human_bytes, (c) => assert.equal(bytes(c.in), c.out, `bytes(${c.in})`));
  each("human_duration", f.human_duration, (c) => assert.equal(duration(c.in), c.out, `duration(${c.in})`));
  each("pct", f.pct, (c) => assert.equal(pct(c.in), c.out, `pct(${c.in})`));
  each("ago", f.ago, (c) => assert.equal(ago(c.at, c.now), c.out, `ago(${c.at}, ${c.now})`));
  each("status", f.status, (c) => {
    const want = { label: c.label, dot: c.dot, row_exit_code: c.row_exit_code, row_restarts: c.row_restarts, attention: c.attention };
    assert.deepEqual(statusOf(c), want, JSON.stringify(c));
  });
  each("binding_link", f.binding_link, (c) => assert.deepEqual(bindingLink(c.binding, c.host), c.out, `${c.case} @ ${c.host}`));
  each("port_views", f.port_views, (c) => assert.deepEqual(portLinks(c.ports, c.host), c.out, `${c.case} @ ${c.host}`));
  each("row_link", f.row_link, (c) => assert.equal(rowLink({ link: c.link, ports: c.ports }, c.host), c.out, `${c.case} @ ${c.host}`));
}

test("Rust's half-to-even on exact ties", () => {
  assert.equal(fixed(1.25, 1), "1.2");
  assert.equal(fixed(1.35, 1), (1.35).toFixed(1), "not an exact tie in binary");
  assert.equal(fixed(12.5, 0), "12");
  assert.equal(fixed(13.5, 0), "14");
  assert.equal(fixed(0.5, 0), "0");
  assert.equal(fixed(-2.5, 0), "-2");
});

test("hosts and labels", () => {
  assert.equal(viewerHost("[::1]"), "::1");
  assert.equal(viewerHost("example.com"), "example.com");
  assert.equal(urlHostPart("::1"), "[::1]");
  assert.equal(imageLabel(`sha256:${"ab".repeat(32)}`), "untagged abababababab");
  assert.equal(imageLabel("nginx:1.27"), "nginx:1.27");
});

test("the State cell's text", () => {
  const now = 1_800_000_000;
  assert.equal(statusText({ label: "healthy", state: "running", started_at: now - 6 * 86400 - 7200, exit_code: null }, now), "healthy 6d 2h");
  assert.equal(statusText({ label: "stopped", state: "exited", started_at: null, exit_code: 137 }, now), "stopped 137");
  assert.equal(statusText({ label: "created", state: "created", started_at: null, exit_code: null }, now), "created");
});
