import type { ComponentProps } from "react";

/** A whole card that is one action: a catalog entry, a suggestion, an
 * "Add item" placeholder. Styled by the caller's class on top of
 * `.card-button` (focus ring, pointer, reset). */
export function CardButton({ className = "", type = "button", ...props }: ComponentProps<"button">) {
  return <button {...props} type={type} className={`card-button ${className}`} />;
}
