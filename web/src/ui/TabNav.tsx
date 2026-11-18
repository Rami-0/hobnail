// TabNav (DESIGN.md §11.9 "page tabs are links"): a row of links, one per
// URL, the current one `aria-current="page"`. On a phone the row scrolls
// and keeps the current tab in view (OV-36). Links are plain anchors: the
// router takes over /app links, any other URL is a full page load.
import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

export interface TabItem {
  id: string;
  label: ReactNode;
  href: string;
  /** A count or icon after the label. */
  extra?: ReactNode;
}

export interface TabNavProps {
  /** The nav landmark's name ("Container"). */
  label: string;
  items: TabItem[];
  /** The id of the current tab. */
  current: string;
  testId?: string;
}

const move = (event: KeyboardEvent<HTMLAnchorElement>) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const links = [...(event.currentTarget.closest("nav")?.querySelectorAll<HTMLAnchorElement>("a.tab") ?? [])];
    const current = links.findIndex((link) => link === event.currentTarget);
    if (current < 0) return;
    const offset = event.key === "ArrowRight" ? 1 : -1;
    const next = event.key === "Home" ? 0 : event.key === "End" ? links.length - 1 : (current + offset + links.length) % links.length;
    event.preventDefault();
    links[next]?.focus();
  };

export function TabNav({ label, items, current, testId }: TabNavProps) {
  const ref = useRef<HTMLElement>(null);
  const ids = items.map((t) => t.id).join("\n");
  useEffect(() => {
    const nav = ref.current;
    const at = ids.split("\n").indexOf(current);
    const cur = at < 0 ? null : nav?.querySelectorAll<HTMLElement>("a.tab")[at];
    if (!nav || !cur || nav.scrollWidth <= nav.clientWidth) return;
    const r = cur.getBoundingClientRect();
    const b = nav.getBoundingClientRect();
    nav.scrollLeft += r.left - b.left - (nav.clientWidth - cur.offsetWidth) / 2;
  }, [current, ids]);
  return (
    <nav ref={ref} className="tabs" aria-label={label} data-testid={testId}>
      {items.map((t) => (
        <a key={t.id} className="tab" href={t.href} onKeyDown={move} aria-current={t.id === current ? "page" : undefined}>
          {t.label}
          {t.extra}
        </a>
      ))}
    </nav>
  );
}
