// Event words and links for the container page's timeline (pure).
// src/ui/pages/mod.rs `event_label` and src/ui/chart.rs marker links.

const LABELS: Record<string, string> = {
  "app.start": "Hobnail started",
  "container.start": "started",
  "container.die": "exited",
  "container.stop": "stopped",
  "container.health": "health",
  "container.oom": "out of memory",
  "alert.fired": "alert fired",
  "alert.resolved": "alert resolved",
  "alert.delivery_failed": "delivery failed",
  "alert.test": "test sent",
  "auth.login": "signed in",
  "auth.login_failed": "sign-in failed",
  "auth.password": "password changed",
  "auth.password_changed": "password changed",
  "config.saved": "config saved",
  "config.restored": "config restored",
};

/** An event kind in plain words (OV-05); unknown kinds: the last part, underscores as spaces. */
export function eventLabel(kind: string): string {
  return LABELS[kind] ?? (kind.split(".").pop() ?? kind).replace(/_/g, " ");
}

/** Pill colour of an event level. */
export function levelClass(level: string): "" | "warn" | "bad" {
  return level === "error" ? "bad" : level === "warn" ? "warn" : "";
}

/** The old History timeline around an event, its row highlighted (chart.rs markers_from_events). */
export function historyHref(id: number, at: number, level: string): string {
  const all = level === "info" ? "&events=all" : "";
  return `/app/history?from=${at - 1800}&to=${at + 1800}${all}&before=${id + 1}#ev-${id}`;
}
