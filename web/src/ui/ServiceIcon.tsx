import { useState } from "react";
import { avatar, iconSource } from "../boards/document.ts";

/** Opt-in same-origin icons, with a letter when disabled, absent or unknown. */
export function ServiceIcon({ name, title, enabled, className = "service-icon" }: {
  name?: string | null; title: string; enabled: boolean; className?: string;
}) {
  const src = iconSource(name, enabled);
  const [failed, setFailed] = useState<string | null>(null);
  return src && failed !== src
    ? <img className={className} src={src} alt="" onError={() => setFailed(src)} />
    : <span className={className} aria-hidden="true">{avatar(title)}</span>;
}
