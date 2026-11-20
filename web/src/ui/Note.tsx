// Messages: Note (inside a panel or dialog, not tied to one field) and
// Banner (page-level, above the content: Docker unreachable, config error,
// new version). Kind sets colour and icon, and the role: "bad" and "warn"
// are alerts (announced at once), "ok" and "info" polite status. State is
// never colour alone: the icon and the words carry it (DESIGN.md §11.2).
import { CircleAlertIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon } from "lucide-react";
import type { ReactNode } from "react";

export type MessageKind = "ok" | "warn" | "bad" | "info";

const ICON: Record<MessageKind, ReactNode> = {
  ok: <CircleCheckIcon aria-hidden="true" />,
  warn: <TriangleAlertIcon aria-hidden="true" />,
  bad: <CircleAlertIcon aria-hidden="true" />,
  info: <InfoIcon aria-hidden="true" />,
};

const role = (k: MessageKind) => (k === "bad" || k === "warn" ? "alert" : "status");

export interface NoteProps {
  kind: MessageKind;
  children: ReactNode;
  /** Buttons or links under the text ("Reload and keep my input", "Sign in (new tab)", "Retry"). */
  action?: ReactNode;
  testId?: string;
  id?: string;
}

export function Note({ kind, children, action, testId, id }: NoteProps) {
  return (
    <div className={`note ${kind}`} role={role(kind)} data-testid={testId} id={id} tabIndex={-1}>
      {ICON[kind]}
      <div>
        <div>{children}</div>
        {action && <div className="acts">{action}</div>}
      </div>
    </div>
  );
}

export interface BannerProps {
  kind: MessageKind;
  children: ReactNode;
  action?: ReactNode;
  testId?: string;
}

export function Banner({ kind, children, action, testId }: BannerProps) {
  return (
    <div className={`banner ${kind}`} role={role(kind)} data-testid={testId}>
      {ICON[kind]}
      <div>
        {children}
        {action && <span className="banner-act">{action}</span>}
      </div>
    </div>
  );
}

export interface EmptyStateProps {
  icon: ReactNode;
  title: ReactNode;
  children?: ReactNode;
  /** "good": nothing wrong (a green mark); "bad": something failed. */
  tone?: "good" | "bad";
  /** Buttons or links under the text. */
  actions?: ReactNode;
  compact?: boolean;
  testId?: string;
}

export function EmptyState({ icon, title, children, tone, actions, compact, testId }: EmptyStateProps) {
  const cls = ["empty", tone ?? "", compact ? "compact" : ""].filter(Boolean).join(" ");
  return (
    <div className={cls} data-testid={testId}>
      <div className="empty-ic">{icon}</div>
      <h3>{title}</h3>
      {children && <div className="empty-p">{children}</div>}
      {actions && <div className="acts">{actions}</div>}
    </div>
  );
}
