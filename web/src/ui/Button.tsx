// Button: the app's only button primitive (DESIGN.md §11.4). Hand-written for
// Phase 0; when UI SDK is installed this wraps its button entry instead and
// keeps these props.
//
// Busy (§11.4): `aria-busy` and `aria-disabled`, not `disabled`, so the
// button keeps focus while its action runs; clicks are ignored meanwhile.
// Touch target: the height is `--ctl-h`, which tokens.css raises to 44 px
// on touch screens and narrow windows.
import type { ButtonHTMLAttributes, MouseEvent, ReactNode } from "react";

export type ButtonVariant = "primary" | "secondary" | "ghost";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  /** The action this button started is running: spinner, no further clicks. */
  busy?: boolean;
  /** An icon before the label (hidden while busy, the spinner takes its place). */
  icon?: ReactNode;
}

export function Button({
  variant = "secondary",
  busy = false,
  icon,
  type = "button",
  className,
  onClick,
  children,
  ...rest
}: ButtonProps) {
  const cls = ["btn", variant === "secondary" ? "" : variant, className ?? ""].filter(Boolean).join(" ");
  const click = (e: MouseEvent<HTMLButtonElement>) => {
    if (busy) {
      e.preventDefault();
      return;
    }
    onClick?.(e);
  };
  return (
    <button
      {...rest}
      type={type}
      className={cls}
      aria-busy={busy || undefined}
      aria-disabled={busy || rest["aria-disabled"] || undefined}
      onClick={click}
    >
      {icon}
      {children}
    </button>
  );
}
