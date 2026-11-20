import type { ReactNode } from "react";

export function PageHeader({ title, description, actions, tabs, icon, className = "", testId }: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  tabs?: ReactNode;
  icon?: ReactNode;
  className?: string;
  testId?: string;
}) {
  return <header className={`page-header ${className}`} data-testid={testId}>
    <div className="page-heading">
      {icon}
      <div className="page-heading-copy">
        <h1>{title}</h1>
        {description && <div className="page-description">{description}</div>}
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </div>
    {tabs}
  </header>;
}
