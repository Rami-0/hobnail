import { useState, type CSSProperties } from "react";
import { avatar, iconSource } from "../boards/document.ts";
import { brandColor, useBrand } from "./brand.ts";

/** A stable hue for a name, so a letter avatar keeps its colour. */
export function nameHue(label: string): number {
  let h = 7;
  for (const c of label.toLowerCase()) h = (Math.imul(h, 31) + (c.codePointAt(0) ?? 0)) >>> 0;
  return h % 360;
}

/** Two letters for several words, else one: "Home Assistant" → "HA", "qBittorrent" → "Q". */
export function initials(title: string): string {
  const words = title.trim().split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w));
  if (words.length > 1) return words.slice(0, 2).map((w) => avatar(w)).join("");
  return avatar(Array.from(words[0] ?? title).find((c) => /[\p{L}\p{N}]/u.test(c)) ?? title);
}

/** Vendored brand icons on a plate in their brand colour; otherwise opt-in
 * same-origin icons, with a letter when disabled, absent or unknown.
 * `tint` gives the letter fallback a colour derived from the title and two
 * letters, for tiles and catalogs where icons stand side by side. */
export function ServiceIcon({ name, title, enabled, className = "service-icon", tint = false }: {
  name?: string | null; title: string; enabled: boolean; className?: string; tint?: boolean;
}) {
  const brand = useBrand(name);
  const src = iconSource(name, enabled || !!brand);
  const [failed, setFailed] = useState<string | null>(null);
  if (brand && src && failed !== src) {
    return <span className={`${className} brand-icon${brand.mono ? " brand-mono" : ""}`} style={{ "--brand": brandColor(brand.color) } as CSSProperties} aria-hidden="true">
      <img src={src} alt="" onError={() => setFailed(src)} />
    </span>;
  }
  if (src && failed !== src) return <img className={`${className}${tint ? " icon-tint-img" : ""}`} src={src} alt="" onError={() => setFailed(src)} />;
  return tint
    ? <span className={`${className} icon-tint`} style={{ "--hue": nameHue(title) } as CSSProperties} aria-hidden="true">{initials(title)}</span>
    : <span className={className} aria-hidden="true">{avatar(title)}</span>;
}
