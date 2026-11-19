// Formatting twins of the server's helpers (src/util.rs `human_bytes`,
// `human_duration`; the display fixtures; src/read_model `status_label`,
// `port_views`, `binding_link`). Pure: no DOM, no clock (callers pass `now`).
// fixtures.json (written by a Rust test) holds inputs and the outputs the
// server gives; format.test.ts asserts the same outputs here.
import type { ContainerRow } from "../api/gen/ContainerRow.ts";
import type { PortBinding } from "../api/gen/PortBinding.ts";

/**
 * `v.toFixed(d)`, but an exact tie rounds half to even, as Rust's
 * `format!("{v:.d$}")` does (`(1.25).toFixed(1)` is "1.3"; Rust says "1.2").
 */
export function fixed(v: number, d: number): string {
  if (!Number.isFinite(v)) return String(v);
  const s = Math.abs(v).toFixed(Math.min(100, d + 60));
  const dot = s.indexOf(".");
  const tail = s.slice(dot + 1 + d);
  if (tail[0] === "5" && /^50*$/.test(tail)) {
    // Exactly half way: keep the even neighbour.
    const kept = d === 0 ? s.slice(0, dot) : s.slice(0, dot + 1 + d);
    const last = Number(kept[kept.length - 1]);
    const down = (v < 0 ? "-" : "") + kept;
    if (last % 2 === 0) return down;
    return v.toFixed(d);
  }
  return v.toFixed(d);
}

const UNITS = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] as const;

/** util::human_bytes: "512 B", "1.5 KiB", "120 MiB". Negative or fractional input is clamped and floored like `as u64`. */
export function bytes(n: number): string {
  const whole = Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0;
  let v = whole;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  if (i === 0) return `${whole} B`;
  return `${v >= 100 ? fixed(v, 0) : fixed(v, 1)} ${UNITS[i]}`;
}

/** ui::fmt::pct: "12%", "0.4%" (one decimal below 10), "—" when not a number. */
export function pct(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "—";
  return v < 10 ? `${fixed(v, 1)}%` : `${fixed(v, 0)}%`;
}

/** util::human_duration: "3d 4h", "5h 2m", "7m", "12s". */
export function duration(secs: number): string {
  const s = Number.isFinite(secs) ? Math.max(0, Math.floor(secs)) : 0;
  const d = Math.floor(s / 86_400);
  const h = Math.floor(s / 3600) % 24;
  const m = Math.floor(s / 60) % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

/** ui::fmt::ago: "just now" under 10 s, else "3m ago". A time in the future is "just now". */
export function ago(at: number, now: number): string {
  const d = Math.max(0, Math.floor(now) - Math.floor(at));
  return d < 10 ? "just now" : `${duration(d)} ago`;
}

/** "15 Nov, 02:14" in the browser's zone and language (not a twin: the server writes UTC). */
export function localTime(at: number): string {
  return new Date(at * 1000).toLocaleString([], {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
}

/** "14:02" (or "15 Nov 14:02" for a window longer than a day) in the browser's zone. */
export function clockTime(at: number, withDate = false): string {
  const d = new Date(at * 1000);
  const hm = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  return withDate ? `${d.toLocaleDateString([], { day: "numeric", month: "short" })} ${hm}` : hm;
}

/**
 * The State cell's text: the status word, the exit code of a stopped
 * container, and for how long (`healthy 6d 2h`, `stopped 1`, `created`).
 * Running states count from `started_at`; a stopped container says nothing
 * about time (the API has no finish time).
 */
export function statusText(row: Pick<ContainerRow, "label" | "state" | "started_at" | "exit_code">, now: number): string {
  const parts: string[] = [row.label];
  if (row.exit_code !== null && row.exit_code !== 0 && row.state === "exited") parts.push(String(row.exit_code));
  const running = row.state === "running" || row.state === "paused" || row.state === "restarting";
  if (running && row.started_at !== null && row.started_at > 0) parts.push(duration(now - row.started_at));
  return parts.join(" ");
}

// ---- status (src/read_model/mod.rs status_label, dot_for, CardView::attention) ----

export interface StatusIn {
  state: string;
  health: string | null;
  /** Docker's exit code (any state). */
  exit_code: number | null;
  /** `die` events in the last hour. */
  die_events_last_hour: number;
}

export interface StatusOut {
  label: string;
  dot: "ok" | "warn" | "bad" | "off";
  /** The row's exit code: non-zero and stopped only. */
  row_exit_code: number | null;
  /** The row's restarts: die events in the last hour when 2 or more. */
  row_restarts: number;
  attention: { severity: "bad" | "warn"; reason: string } | null;
}

/** The status word, dot and attention the server gives a container (a twin; the server sends them). */
export function statusOf(c: StatusIn): StatusOut {
  const h = c.health;
  let label: string;
  if (c.state === "running") label = h === "unhealthy" ? "unhealthy" : h === "starting" ? "starting" : h === "healthy" ? "healthy" : "running";
  else if (c.state === "restarting" || c.state === "paused" || c.state === "created") label = c.state;
  else if (c.state === "exited") label = "stopped";
  else label = "dead";
  let dot: StatusOut["dot"] = "off";
  if (c.state === "running") dot = h === "unhealthy" ? "bad" : h === "starting" ? "warn" : "ok";
  else if (c.state === "paused" || c.state === "restarting") dot = "warn";
  const rowExit = c.exit_code !== null && c.exit_code !== 0 && c.state === "exited" ? c.exit_code : null;
  const restarts = c.die_events_last_hour >= 2 ? c.die_events_last_hour : 0;
  let attention: StatusOut["attention"] = null;
  if (label === "unhealthy" || label === "restarting" || label === "dead") attention = { severity: "bad", reason: label };
  else if (label === "starting" || label === "paused") attention = { severity: "warn", reason: label };
  else if (rowExit !== null) attention = { severity: "warn", reason: `stopped, exit ${rowExit}` };
  else if (restarts > 0) attention = { severity: "warn", reason: `${restarts} exits in the last hour` };
  return { label, dot, row_exit_code: rowExit, row_restarts: restarts, attention };
}

// ---- ports (src/read_model/ports.rs) ----

/** Container ports that are never a web UI (databases, mail, queues, DNS). */
const NON_HTTP_PORTS = new Set([
  5432, 3306, 6379, 27017, 1025, 25, 587, 465, 110, 143, 993, 995, 1110, 11211, 5672, 9092, 2181, 53,
]);

function parseV4(s: string): number[] | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(s);
  if (!m) return null;
  const o = m.slice(1).map((x) => (x.length > 1 && x.startsWith("0") ? NaN : Number(x)));
  return o.every((x) => x >= 0 && x <= 255) ? o : null;
}

function v6Part(h: string): number[] | null {
  if (h === "") return [];
  const out: number[] = [];
  const bits = h.split(":");
  for (let i = 0; i < bits.length; i++) {
    const b = bits[i] ?? "";
    if (i === bits.length - 1 && b.includes(".")) {
      const v4 = parseV4(b);
      if (!v4) return null;
      out.push(((v4[0] ?? 0) << 8) | (v4[1] ?? 0), ((v4[2] ?? 0) << 8) | (v4[3] ?? 0));
    } else {
      if (!/^[0-9a-fA-F]{1,4}$/.test(b)) return null;
      out.push(parseInt(b, 16));
    }
  }
  return out;
}

/** Eight 16-bit groups, or null when `s` is not an IPv6 literal Rust's parser accepts. */
function parseV6(s: string): number[] | null {
  if (!s.includes(":") || !/^[0-9a-fA-F:.]+$/.test(s)) return null;
  const halves = s.split("::");
  if (halves.length > 2) return null;
  const a = v6Part(halves[0] ?? "");
  const b = halves.length === 2 ? v6Part(halves[1] ?? "") : [];
  if (!a || !b) return null;
  if (halves.length === 1) return a.length === 8 ? a : null;
  const fill = 8 - a.length - b.length;
  if (fill < 1) return null;
  return [...a, ...Array<number>(fill).fill(0), ...b];
}

function isUnspecified(ip: string): boolean {
  if (ip === "") return true;
  const v4 = parseV4(ip);
  if (v4) return v4.every((x) => x === 0);
  const v6 = parseV6(ip);
  return !!v6 && v6.every((x) => x === 0);
}

function isLoopback(ip: string): boolean {
  if (ip === "localhost") return true;
  const v4 = parseV4(ip);
  if (v4) return v4[0] === 127;
  const v6 = parseV6(ip);
  return !!v6 && v6.slice(0, 7).every((x) => x === 0) && v6[7] === 1;
}

/** Bracket an IPv6 literal for a URL. */
export function urlHostPart(h: string): string {
  return h.includes(":") && !h.startsWith("[") ? `[${h}]` : h;
}

/** `location.hostname` as the server's `request_host` sees it (IPv6 without brackets). */
export function viewerHost(hostname: string): string {
  return hostname.startsWith("[") && hostname.endsWith("]") ? hostname.slice(1, -1) : hostname;
}

function pickBinding(bindings: [string, number][], host: string): [string, number] | null {
  const any = bindings.find(([ip]) => isUnspecified(ip));
  if (any) return [host, any[1]];
  const same = bindings.find(([ip]) => ip === host);
  if (same) return [same[0], same[1]];
  if (isLoopback(host)) {
    const lo = bindings.find(([ip]) => isLoopback(ip));
    if (lo) return [host, lo[1]];
  }
  const other = bindings.find(([ip]) => !isLoopback(ip));
  return other ? [other[0], other[1]] : null;
}

export interface PortLink {
  /** Host port shown on the badge. */
  label: string;
  /** Link, for HTTP-ish ports reachable from the viewer. */
  url: string | null;
  /** Published on loopback only. */
  local: boolean;
  /** "8125 → 8025/tcp on 100.64.0.10, 127.0.0.1". */
  detail: string;
  proto: string;
}

const dedup = <T>(v: T[]): T[] => v.filter((x, i) => i === 0 || x !== v[i - 1]);

/** view::port_views: one badge per published container port. */
export function portLinks(ports: readonly PortBinding[], host: string): PortLink[] {
  const keys: [number, string][] = [];
  for (const p of ports) {
    if (!keys.some(([c, pr]) => c === p.container_port && pr === p.proto)) keys.push([p.container_port, p.proto]);
  }
  return keys.map(([cport, proto]) => {
    const bindings: [string, number][] = ports
      .filter((p) => p.container_port === cport && p.proto === proto)
      .map((p) => [p.host_ip, p.host_port]);
    const chosen = pickBinding(bindings, host);
    const nonHttp = proto !== "tcp" || NON_HTTP_PORTS.has(cport);
    const scheme = cport === 443 || cport === 8443 ? "https" : "http";
    const label = String(chosen ? chosen[1] : (bindings[0]?.[1] ?? ""));
    const ips = dedup(bindings.map(([ip]) => (ip === "" ? "*" : ip)));
    const hostPorts = dedup(bindings.map(([, p]) => String(p)));
    return {
      url: chosen && !nonHttp ? `${scheme}://${urlHostPart(chosen[0])}:${chosen[1]}` : null,
      local: chosen === null,
      detail: `${hostPorts.join(", ")} → ${cport}/${proto} on ${ips.join(", ")}`,
      label,
      proto,
    };
  });
}

/** view::binding_link: the link of one host binding, or why there is none. */
export function bindingLink(p: PortBinding, host: string): { url: string } | { none: "loopback only" | "not a web port" } {
  if (p.proto !== "tcp" || NON_HTTP_PORTS.has(p.container_port)) return { none: "not a web port" };
  let h: string;
  if (isUnspecified(p.host_ip)) h = host;
  else if (isLoopback(p.host_ip)) {
    if (!isLoopback(host)) return { none: "loopback only" };
    h = host;
  } else h = p.host_ip;
  const scheme = p.container_port === 443 || p.container_port === 8443 ? "https" : "http";
  return { url: `${scheme}://${urlHostPart(h)}:${p.host_port}` };
}

/** The row's main link: the configured template with `{host}` filled in, else the first port link. */
export function rowLink(row: Pick<ContainerRow, "link" | "ports">, host: string): string | null {
  if (row.link !== null && row.link !== "") return row.link.split("{host}").join(urlHostPart(host));
  for (const p of portLinks(row.ports, host)) if (p.url) return p.url;
  return null;
}

/** An untagged image as words: "untagged abababababab" (OV-26). */
export function imageLabel(image: string): string {
  return image.startsWith("sha256:") ? `untagged ${image.slice(7, 19)}` : image;
}
