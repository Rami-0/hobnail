// The tab: title "(<problems>) <page> · <site> · Hobnail" and the favicon
// with a red count (static/app.js badge). The favicon is built from
// /icon.svg as a data: URL (CSP `img-src 'self' data:`); nothing is parsed
// as HTML.
import { useEffect } from "react";

import { badgedTitle } from "../live/reducer.ts";

let iconText: Promise<string | null> | null = null;
let baseHref: string | null = null;

function loadIcon(): Promise<string | null> {
  iconText ??= fetch("/icon.svg", { credentials: "same-origin" })
    .then((r) => (r.ok ? r.text() : null))
    .catch(() => null);
  return iconText;
}

/** The badge SVG: the mark at 24 × 24 with a red count in its corner. Exported for tests. */
export function badgeSvg(icon: string, n: number): string | null {
  const t = n > 9 ? "9+" : String(Math.max(0, Math.floor(n)));
  const at = icon.indexOf("<svg");
  if (at < 0) return null;
  const inner = `${icon.slice(0, at)}<svg x="0" y="0" width="24" height="24"${icon.slice(at + 4)}`;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">${inner}` +
    `<circle cx="18" cy="6" r="7" fill="#ef4444"/>` +
    `<text x="18" y="9" font-family="Arial,sans-serif" font-size="${t.length > 1 ? 7 : 9}" font-weight="700" text-anchor="middle" fill="#fff">${t}</text></svg>`
  );
}

async function setFavicon(n: number): Promise<void> {
  const link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (!link) return;
  baseHref ??= link.getAttribute("href") ?? "/icon.svg";
  if (n <= 0) {
    link.setAttribute("href", baseHref);
    return;
  }
  const icon = await loadIcon();
  const svg = icon && badgeSvg(icon, n);
  if (svg) link.setAttribute("href", `data:image/svg+xml,${encodeURIComponent(svg)}`);
}

/** Keep the tab's title and favicon in step with the page and the problem count. */
export function useTabTitle(page: string, site: string | null, problems: number): void {
  useEffect(() => {
    const base = [page, site, "Hobnail"].filter(Boolean).join(" · ");
    document.title = badgedTitle(base, problems);
  }, [page, site, problems]);
  useEffect(() => {
    void setFavicon(problems);
  }, [problems]);
}
