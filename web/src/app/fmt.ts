// The few formatters the shell needs. The pages stream owns the full
// formatting twins (src/format/, checked against Rust fixtures); these
// local copies keep the shell independent of that folder until it exists
// and follow the same rules (util::human_bytes, static/app.js fmtVal).

/** `512 B`, `1.5 KiB`, `120 MiB` (util::human_bytes). */
export function bytes(n: number): string {
  const u = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
  let v = Math.max(0, n);
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${Math.round(n)} B` : `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

/** A chip's whole percent, or a dash. */
export function pct0(v: number | null | undefined): string {
  return v === null || v === undefined || !Number.isFinite(v) ? "—" : `${v.toFixed(0)}%`;
}

/** `14:05` in the browser's time zone, 24 h. */
export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}
