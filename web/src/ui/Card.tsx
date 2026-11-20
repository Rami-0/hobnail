import type { ComponentProps, ReactNode } from "react";

export function Card({ className = "", ...props }: ComponentProps<"section">) {
  return <section {...props} className={`panel ${className}`} />;
}

export function CardHeader({ title, description, action }: { title: ReactNode; description?: ReactNode; action?: ReactNode }) {
  return <header className="card-header">
    <div><h2>{title}</h2>{description && <p>{description}</p>}</div>
    {action}
  </header>;
}

export function CardContent(props: ComponentProps<"div">) {
  return <div className="card-content" {...props} />;
}

export function CardFooter(props: ComponentProps<"footer">) {
  return <footer className="card-footer" {...props} />;
}

export function Separator() {
  return <hr className="separator" />;
}
