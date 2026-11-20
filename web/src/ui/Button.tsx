// Busy (§11.4): `aria-busy` and `aria-disabled`, not `disabled`, so the
// button keeps focus while its action runs; clicks are ignored meanwhile.
// Touch target: the height is `--ctl-h`, which tokens.css raises to 44 px
// on touch screens and narrow windows.
import { Button as SdkButton } from "@ui-sdk/react/button";
import type { ButtonHTMLAttributes, MouseEvent, ReactNode, Ref } from "react";

export type ButtonVariant = "default" | "secondary" | "outline" | "ghost" | "destructive" | "link" | "primary";
export type ButtonSize = "sm" | "default" | "lg" | "icon";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  leadingIcon?: ReactNode;
  trailingIcon?: ReactNode;
  /** The action this button started is running: spinner, no further clicks. */
  busy?: boolean;
  /** An icon before the label (hidden while busy, the spinner takes its place). */
  icon?: ReactNode;
  /** React 19 passes `ref` as a prop. */
  ref?: Ref<HTMLButtonElement>;
}

export function Button({
  variant = "outline",
  size = "default",
  loading = false,
  busy = loading,
  leadingIcon,
  trailingIcon,
  icon,
  type = "button",
  className,
  onClick,
  children,
  ...rest
}: ButtonProps) {
  const cls = ["btn", variant === "primary" ? "btn-default" : `btn-${variant}`, size === "default" ? "" : `btn-${size}`, className ?? ""].filter(Boolean).join(" ");
  const click = (e: MouseEvent<HTMLElement>) => {
    if (busy) {
      e.preventDefault();
      return;
    }
    onClick?.(e as MouseEvent<HTMLButtonElement>);
  };
  return (
    <SdkButton
      {...rest}
      type={type}
      className={cls}
      aria-busy={busy || undefined}
      aria-disabled={busy || rest["aria-disabled"] || undefined}
      onClick={click}
    >
      {!busy && (leadingIcon ?? icon)}
      {children}
      {!busy && trailingIcon}
    </SdkButton>
  );
}
