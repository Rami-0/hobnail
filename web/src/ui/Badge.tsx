import type { ComponentProps } from "react";

export type BadgeVariant = "default" | "secondary" | "outline" | "success" | "warning" | "destructive" | "info";

export function Badge({ variant = "outline", className = "", ...props }: ComponentProps<"span"> & { variant?: BadgeVariant }) {
  return <span {...props} className={`badge badge-${variant} ${className}`} />;
}

export const statusBadge = (dot: string): BadgeVariant =>
  dot === "ok" ? "success" : dot === "warn" ? "warning" : dot === "bad" ? "destructive" : "outline";
