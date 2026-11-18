// A tiny History-API router (no router dependency). The pure parts (route
// matching, the /app basename, link-click rules, `next` validation) are in
// paths.ts; this file holds the browser state: the current location as an
// external store, `navigate`, `useRoute`, and one document click listener
// that turns same-origin /app links into client navigations.
import { useSyncExternalStore } from "react";

import { interceptedPath, matchRoute, nextSearch, routeParams, stripBase, type Route, type SearchPatch } from "./paths.ts";

export { BASE, appHref, containerHref, safeNext, type Route, type SearchPatch } from "./paths.ts";

export interface AppLocation {
  pathname: string;
  /** Same as `pathname` (the document path, `/app/containers/web`). */
  path: string;
  /** The query, parsed. Shared by every reader of this location: do not mutate it (use setSearch). */
  search: URLSearchParams;
  /** The raw query string, `?…` or "". */
  query: string;
  hash: string;
  route: Route;
  /** Route parameters (`{ name }` on a container page). */
  params: Record<string, string>;
}

const listeners = new Set<() => void>();
let current: AppLocation | null = null;

function read(): AppLocation {
  const { pathname, search, hash } = window.location;
  if (current && current.pathname === pathname && current.query === search && current.hash === hash) {
    return current;
  }
  const route = matchRoute(pathname);
  current = { pathname, path: pathname, search: new URLSearchParams(search), query: search, hash, route, params: routeParams(route) };
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
export function navigate(to: string, opts: { replace?: boolean; keepScroll?: boolean } = {}): void {
  const u = new URL(to, window.location.href);
  if (u.origin !== window.location.origin || stripBase(u.pathname) === null) {
    window.location.assign(u.href);
    return;
  }
  const target = u.pathname + u.search + u.hash;
  // Same page, another query (a filter, a range, a tab of the same path): the reader keeps their place.
  const samePage = u.pathname === window.location.pathname;
  if (opts.replace) window.history.replaceState(null, "", target);
  else window.history.pushState(null, "", target);
  notify();
  if (!u.hash && !opts.keepScroll && !samePage) window.scrollTo(0, 0);
}

/**
 * Change the query of the current page: a URLSearchParams replaces it, a
 * record patches it (null or "" removes a key). Pushes a history entry
 * unless `replace` (filters and sort should replace; a dialog URL pushes).
 */
export function setSearch(next: URLSearchParams | SearchPatch, opts: { replace?: boolean } = {}): void {
  const { pathname, search, hash } = window.location;
  const q = nextSearch(search, next);
  if (q === search) return;
  navigate(pathname + q + hash, { replace: opts.replace ?? false, keepScroll: true });
}

export interface RouteState extends AppLocation {
  setSearch: typeof setSearch;
}

/** The current location and route, plus `setSearch`; re-renders on navigation. */
export function useRoute(): RouteState {
  const loc = useSyncExternalStore(subscribe, read, read);
  return withSet(loc);
}

const withSetCache = new WeakMap<AppLocation, RouteState>();
function withSet(loc: AppLocation): RouteState {
  let r = withSetCache.get(loc);
  if (!r) {
    r = { ...loc, setSearch };
    withSetCache.set(loc, r);
  }
  return r;
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
