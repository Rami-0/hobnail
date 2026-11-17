// A tiny History-API router (no router dependency). The pure parts (route
// matching, the /app basename, link-click rules, `next` validation) are in
// paths.ts; this file holds the browser state: the current location as an
// external store, `navigate`, `useRoute`, and one document click listener
// that turns same-origin /app links into client navigations.
import { useSyncExternalStore } from "react";

import { interceptedPath, matchRoute, stripBase, type Route } from "./paths.ts";

export { BASE, appHref, safeNext, type Route } from "./paths.ts";

export interface AppLocation {
  pathname: string;
  search: string;
  hash: string;
  route: Route;
}

const listeners = new Set<() => void>();
let current: AppLocation | null = null;

function read(): AppLocation {
  const { pathname, search, hash } = window.location;
  if (current && current.pathname === pathname && current.search === search && current.hash === hash) {
    return current;
  }
  current = { pathname, search, hash, route: matchRoute(pathname) };
  return current;
}

function notify(): void {
  for (const l of listeners) l();
}

function subscribe(fn: () => void): () => void {
  listeners.add(fn);
  if (listeners.size === 1) window.addEventListener("popstate", notify);
  return () => {
    listeners.delete(fn);
    if (listeners.size === 0) window.removeEventListener("popstate", notify);
  };
}

/**
 * Go to a document path (`/app/login?next=…`). Same-origin only; a path
 * outside /app is a full page load (the old UI lives there).
 */
export function navigate(to: string, opts: { replace?: boolean } = {}): void {
  const u = new URL(to, window.location.href);
  if (u.origin !== window.location.origin || stripBase(u.pathname) === null) {
    window.location.assign(u.href);
    return;
  }
  const target = u.pathname + u.search + u.hash;
  if (opts.replace) window.history.replaceState(null, "", target);
  else window.history.pushState(null, "", target);
  notify();
  if (!u.hash) window.scrollTo(0, 0);
}

/** The current location and route; re-renders on navigation. */
export function useRoute(): AppLocation {
  return useSyncExternalStore(subscribe, read, read);
}

function onLinkClick(e: MouseEvent): void {
  const el = e.target instanceof Element ? e.target.closest("a[href]") : null;
  if (!(el instanceof HTMLAnchorElement)) return;
  const path = interceptedPath({
    button: e.button,
    metaKey: e.metaKey,
    ctrlKey: e.ctrlKey,
    shiftKey: e.shiftKey,
    altKey: e.altKey,
    defaultPrevented: e.defaultPrevented,
    target: el.target,
    hasDownload: el.hasAttribute("download"),
    href: el.href,
    origin: window.location.origin,
    current: window.location.pathname + window.location.search,
  });
  if (path === null) return;
  e.preventDefault();
  navigate(path);
}

/** Left clicks on same-origin /app links navigate in place; everything else is the browser's. */
export function interceptLinks(doc: Document = document): () => void {
  doc.addEventListener("click", onLinkClick);
  return () => doc.removeEventListener("click", onLinkClick);
}
