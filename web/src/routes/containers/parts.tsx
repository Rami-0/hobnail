// Small pieces both container pages use: the service icon (src/ui/icons.rs
// `render`) and the status dot (never colour alone: the dot has a name).
import type { CSSProperties } from "react";

const SAFE = /^[A-Za-z0-9._-]+$/;

/** ui::is_safe_name: a file or dashboard-icons name the server serves. */
export function isSafeIconName(n: string): boolean {
  return n.length > 0 && n.length < 100 && !n.startsWith(".") && SAFE.test(n);
}

function avatarLetter(label: string): string {
  const c = [...label].find((x) => /[A-Za-z0-9]/.test(x));
  return c ? c.toUpperCase() : "?";
}

/** A stable avatar hue for each name. */
export function avatarHue(label: string): number {
  let h = 7;
  for (const b of new TextEncoder().encode(label)) h = (Math.imul(h, 31) + b) >>> 0;
  return h % 360;
}

/** The icon of a container or stack: a served file, a dashboard icon, or a letter avatar. */
export function ServiceIcon({ name, label }: { name: string | null; label: string }) {
  const n = name?.trim() ?? "";
  if (n && !n.startsWith("builtin:") && isSafeIconName(n)) {
    const src = /\.(svg|png)$/.test(n) ? `/icons/${n}` : `/icons/di/${n}.svg?l=${avatarLetter(label)}`;
    return <img className="icon" src={src} alt="" loading="lazy" width={20} height={20} />;
  }
  const first = [...label].find((x) => /\p{L}|\p{N}/u.test(x));
  return (
    <span className="icon avatar" style={{ "--h": avatarHue(label) } as CSSProperties} aria-hidden="true">
      {first ? first.toUpperCase() : "?"}
    </span>
  );
}

const DOT_WORD: Record<string, string> = { ok: "ok", warn: "warning", bad: "problem", off: "not running" };

export function StatusDot({ dot }: { dot: string }) {
  const d = dot in DOT_WORD ? dot : "off";
  return (
    <>
      <span className={`dot ${d}`} aria-hidden="true" />
      <span className="sr">{DOT_WORD[d]}: </span>
    </>
  );
}

export { containerHref, logsHref } from "../../app/paths.ts";
