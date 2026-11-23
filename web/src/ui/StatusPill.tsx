import type { ReactNode } from "react";

export type StatusTone = "ok" | "warn" | "bad" | "off" | "info";

/** A status word with a dot (never colour alone): "Running", "Stale". */
export function StatusPill({ tone, children, title }: { tone: StatusTone; children: ReactNode; title?: string }) {
  return <span className={`status-pill status-${tone}`} title={title}><span className="status-dot" aria-hidden="true" />{children}</span>;
}
