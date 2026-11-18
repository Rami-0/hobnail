// Pure path logic of the client router: no DOM, no React, so Node's test
// runner can load it (paths.test.ts). router.ts wires it to the History API.

/** Every client route lives under this path; the server sends index.html for it. */
export const BASE = "/app";

export type Route =
  | { name: "home" }
  | { name: "login" }
  | { name: "invite"; token: string }
  | { name: "containers" }
  | { name: "history" }
  | { name: "overview" }
  | { name: "host" }
  | { name: "alerts" }
  | { name: "board"; id: string }
  | { name: "settings"; tab: "boards" | "integrations" }
  /** One container's page; `ctr` is the decoded container name. */
  | { name: "container"; ctr: string }
  | { name: "not_found"; path: string };

/** The route's parameters by name (`{ name: "web-1" }` on /app/containers/web-1). */
export function routeParams(r: Route): Record<string, string> {
  return r.name === "container" ? { name: r.ctr } : r.name === "board" ? { id: r.id } : {};
}

/** `/app/login` -> `/login`, `/app` and `/app/` -> `/`; null outside the base. */
export function stripBase(pathname: string): string | null {
  if (pathname === BASE) return "/";
  if (!pathname.startsWith(BASE + "/")) return null;
  return pathname.slice(BASE.length) || "/";
}

/** A client path (`/login`) as a document path (`/app/login`). */
export function appHref(path: string): string {
  if (path === "/" || path === "") return BASE;
  return BASE + (path.startsWith("/") ? path : "/" + path);
}

/** Which page a document path shows. Trailing slashes do not matter. */
export function matchRoute(pathname: string): Route {
  const p = stripBase(pathname);
  if (p === null) return { name: "not_found", path: pathname };
  const clean = p.length > 1 ? p.replace(/\/+$/, "") : p;
  switch (clean) {
    case "/":
      return { name: "home" };
    case "/login":
      return { name: "login" };
    case "/alerts":
      return { name: "alerts" };
    case "/host":
      return { name: "host" };
    case "/overview":
      return { name: "overview" };
    case "/history":
      return { name: "history" };
    case "/containers":
      return { name: "containers" };
  }
  const board = /^\/boards\/([a-f0-9]{32})$/.exec(clean);
  if (board?.[1]) return { name: "board", id: board[1] };
  if (clean === "/settings/boards" || clean === "/settings/integrations") return { name: "settings", tab: clean.endsWith("integrations") ? "integrations" : "boards" };
  const invite = /^\/invite\/([a-f0-9]{64})$/.exec(clean);
  if (invite?.[1]) return { name: "invite", token: invite[1] };
  const m = /^\/containers\/([^/]+)$/.exec(clean);
  if (m?.[1]) {
    let ctr: string;
    try {
      ctr = decodeURIComponent(m[1]);
    } catch {
      return { name: "not_found", path: pathname };
    }
    // eslint-disable-next-line no-control-regex
    if (ctr !== "" && !ctr.includes("/") && !/[\u0000-\u001f\u007f]/.test(ctr)) return { name: "container", ctr };
  }
  return { name: "not_found", path: pathname };
}

/** The client path of a container's page (`/app/containers/<name>`). */
export function containerHref(name: string): string {
  return `${BASE}/containers/${encodeURIComponent(name)}`;
}

/** Routes reserved for the Logs milestone. Timeline fragments are milliseconds. */
export function logsHref(name: string, at: number | null = null): string {
  const root = `${containerHref(name)}/logs`;
  return at === null ? root : `${root}#t=${Math.max(0, at) * 1000}`;
}

/** Search parameters as a patch: a value sets the key, null or "" removes it, other keys stay. */
export type SearchPatch = Record<string, string | null>;

/**
 * The query string (`?a=1`, or "" for none) after applying `next` to
 * `current`: a URLSearchParams replaces the whole query, a record patches it.
 */
export function nextSearch(current: string, next: URLSearchParams | SearchPatch): string {
  let p: URLSearchParams;
  if (next instanceof URLSearchParams) p = new URLSearchParams(next);
  else {
    p = new URLSearchParams(current);
    for (const [k, v] of Object.entries(next)) {
      if (v === null || v === "") p.delete(k);
      else p.set(k, v);
    }
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

/** Longest `next` the server accepts (auth.rs MAX_NEXT). */
const MAX_NEXT = 2048;

// Rust's `char::is_control` (Unicode Cc): C0, DEL and C1.
// eslint-disable-next-line no-control-regex
const CONTROL_OR_SPACE = /[\u0000-\u001f\u007f-\u009f ]/;

/** auth.rs `local_path_ok`: a rooted path, not `//host`, no backslash. */
function localPathOk(s: string): boolean {
  return !CONTROL_OR_SPACE.test(s) && s.startsWith("/") && !s.startsWith("//") && !s.includes("\\");
}

/**
 * Where to go after signing in. Mirrors `auth::safe_next` (ASCII only, no
 * control characters or spaces, rooted, not `//`, no backslash, the
 * percent-decoded form passing the same rules) and additionally keeps the
 * client inside itself: only `/app`, `/app/…`, `/app?…` and `/app#…`.
 * Anything else, or a missing value, gives `/app`.
 */
export function safeNext(next: string | null | undefined): string {
  if (!next || next.length > MAX_NEXT || !/^\p{ASCII}*$/u.test(next) || !localPathOk(next)) {
    return BASE;
  }
  let decoded: string;
  try {
    decoded = decodeURIComponent(next);
  } catch {
    return BASE;
  }
  if (!localPathOk(decoded)) return BASE;
  const inApp = (s: string) =>
    s === BASE || s.startsWith(BASE + "/") || s.startsWith(BASE + "?") || s.startsWith(BASE + "#");
  if (!inApp(next) || !inApp(decoded)) return BASE;
  // Dot segments: `/app/../settings` would resolve outside the client.
  if (new URL(next, "http://h.invalid").pathname.split("/")[1] !== BASE.slice(1)) return BASE;
  return next;
}

/** What a click handler knows about a click on a link. */
export interface LinkClick {
  button: number;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  defaultPrevented: boolean;
  /** The anchor's `target` attribute ("" when absent). */
  target: string;
  hasDownload: boolean;
  /** The anchor's resolved `href`. */
  href: string;
  /** `location.origin`. */
  origin: string;
  /** `location.pathname + location.search`: a link that only changes the hash stays the browser's (skip links, in-page anchors). */
  current: string;
}

/**
 * The client path + query + hash to navigate to when the router should take
 * over a link click, else null (the browser handles it: modifier keys, other
 * buttons, new windows, downloads, other origins, paths outside /app,
 * in-page anchors).
 */
export function interceptedPath(c: LinkClick): string | null {
  if (c.defaultPrevented || c.button !== 0) return null;
  if (c.metaKey || c.ctrlKey || c.shiftKey || c.altKey) return null;
  if (c.hasDownload || (c.target !== "" && c.target !== "_self")) return null;
  let u: URL;
  try {
    u = new URL(c.href);
  } catch {
    return null;
  }
  if (u.origin !== c.origin) return null;
  if (stripBase(u.pathname) === null) return null;
  if (u.hash !== "" && u.pathname + u.search === c.current) return null;
  return u.pathname + u.search + u.hash;
}
