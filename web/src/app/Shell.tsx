// Application shell with navigation, status, account controls and toasts.
//
// Sidebar by width: over 1100 px it is in the page and its
// toggle collapses it to a rail (remembered, rh-side); from 700 to 1100 px
// it is a rail the toggle opens over the page; under 700 px it is a drawer
// that only the bottom bar's "More" opens. Open over the page, everything
// behind is inert, Esc and Back close it (Forward opens it again), focus
// goes into it and back to the opener; a closed drawer is out of the tab
// order. `<html data-side="rail">` and `<html class="side-open">` drive the
// CSS.
import { BoxIcon, BellIcon, ChevronRightIcon, EllipsisIcon, LayoutGridIcon, RefreshCwIcon, ScrollTextIcon, SearchIcon } from "lucide-react";
import { lazy, useCallback, useDeferredValue, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";

import { useSession } from "../api/session.ts";
import { problemCount, useBootstrap, useContainers, useHiddenProblems, useNewBuild } from "../live/index.ts";
import { Banner } from "../ui/Note.tsx";
import { Button } from "../ui/Button.tsx";
import { LoadBoundary } from "../ui/LoadBoundary.tsx";
import { Account } from "./Account.tsx";
import { isTyping, keyAction, moveRow, type GoPage, type KeyState } from "./keys.ts";
import { StalePill } from "./Live.tsx";
import { interceptedPath, logsHref } from "./paths.ts";
import { settingsHref, local, readRail, writeRail } from "./prefs.ts";
import { appHref, navigate, useRoute } from "./router.ts";
import { pageFilter } from "./page-filter.ts";
import { openDialogs, subscribeDialogs } from "../ui/layers.ts";
import { getToasts, subscribeToasts } from "../ui/toast.ts";

const Palette = lazy(() => import("./Palette.tsx"));
const Shortcuts = lazy(() => import("./Shortcuts.tsx").then((m) => ({ default: m.Shortcuts })));
function hasToasts() {
  return getToasts().shown.length > 0;
}

function ShellToasts() {
  const visible = useSyncExternalStore(subscribeToasts, hasToasts, hasToasts);
  // Dialog saves can release a toast in the same commit that closes the modal.
  const dialogs = useSyncExternalStore(subscribeDialogs, openDialogs, openDialogs);
  return <LoadBoundary fallback={null} failed={null}>{visible || dialogs > 0 ? <LazyToaster /> : <output className="toasts" aria-live="polite" />}</LoadBoundary>;
}

const LazyToaster = lazy(() => import("../ui/Toaster.tsx").then((m) => ({ default: m.Toaster })));
import { Sidebar } from "./Sidebar.tsx";
import { applyDefaultTheme } from "./theme.ts";
import { ThemeButton } from "./ThemeButton.tsx";
import { useTabTitle } from "./title.ts";
import { useAlertCount } from "./useAlertCount.ts";
import { Vitals } from "./Vitals.tsx";

const WIDE = "(min-width:1101px)";
const PHONE = "(max-width:699px)";

function useMedia(q: string): boolean {
  const [on, set] = useState(() => window.matchMedia(q).matches);
  useEffect(() => {
    const m = window.matchMedia(q);
    const f = () => set(m.matches);
    f();
    m.addEventListener("change", f);
    return () => m.removeEventListener("change", f);
  }, [q]);
  return on;
}

/** Before the first render: the remembered rail. */
export function applyStoredRail(): void {
  if (readRail(local())) document.documentElement.dataset.side = "rail";
}

const GO_HREF: Record<GoPage, (owner?: boolean, scoped?: boolean) => string> = {
  overview: () => "/app/overview",
  containers: () => appHref("/containers"),
  host: () => "/app/host",
  logs: () => "/app/logs",
  history: () => "/app/history",
  alerts: () => "/app/alerts",
  settings: (owner, scoped) => `/app${settingsHref(local(), owner, scoped)}`,
};

const visible = (el: Element) => el instanceof HTMLElement && el.offsetParent !== null && !el.closest("[hidden]");

/** Rows j/k move through: the page's rows that open something (NAV-60). */
function rows(): HTMLElement[] {
  const marked = Array.from(document.querySelectorAll<HTMLElement>('main [data-testid="row"]'));
  const list = marked.length ? marked : Array.from(document.querySelectorAll<HTMLElement>("main tbody tr, main ul.rows > li"));
  return list.filter((r) => visible(r) && r.querySelector("a[href]"));
}

function moveKeyRow(dir: 1 | -1): void {
  const list = rows();
  if (!list.length) return;
  let i = list.findIndex((r) => r.classList.contains("kfocus"));
  if (i < 0) i = list.findIndex((r) => r.contains(document.activeElement));
  for (const r of list) r.classList.remove("kfocus");
  const r = list[moveRow(i, dir, list.length)];
  if (!r) return;
  r.classList.add("kfocus");
  r.querySelector<HTMLElement>("a[href]")?.focus({ preventScroll: true });
  r.scrollIntoView({ block: "nearest" });
}

/** The logs page for the focused row or open container. */
function logsTarget(current: string | null): string | null {
  const a = document.activeElement;
  const row = document.querySelector<HTMLElement>(".kfocus[data-name]") ?? (a instanceof Element ? a.closest<HTMLElement>("[data-name]") : null);
  let name = row?.dataset.name ?? null;
  if (!name && a instanceof HTMLAnchorElement) {
    const m = /^\/app\/containers\/([^/?#]+)$/.exec(new URL(a.href).pathname);
    if (m?.[1]) {
      try {
        name = decodeURIComponent(m[1]);
      } catch {
        name = null;
      }
    }
  }
  name ??= current;
  return name ? logsHref(name) : null;
}

export function Shell({ title, children }: { title: string; children: ReactNode }) {
  const loc = useRoute();
  const session = useSession();
  useEffect(() => {
    const sheet = document.createElement("link");
    sheet.rel = "stylesheet";
    sheet.href = "/custom.css";
    document.head.append(sheet);
    return () => sheet.remove();
  }, []);
  const b = useBootstrap();
  const ctrs = useContainers();
  const hidden = useHiddenProblems();
  const newBuild = useNewBuild();
  const wide = useMedia(WIDE);
  const phone = useMedia(PHONE);
  const [rail, setRail] = useState(() => document.documentElement.dataset.side === "rail");
  const [open, setOpenState] = useState(false);
  const [palette, setPalette] = useState(false);
  const [keys, setKeys] = useState(false);
  const sideRef = useRef<HTMLElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const drawer = useRef({ pushed: false, popping: false, from: null as HTMLElement | null });
  // The sidebar is drawn in a second step (a transition), after the top bar
  // and the page: two smaller tasks instead of one long one on a slow phone.
  const full = useDeferredValue(true, false);
  const settled = session.status !== "loading";
  // #root[data-ready="1"]: drawn, sidebar included, with a known session.
  useEffect(() => {
    if (full && settled) document.getElementById("root")?.setAttribute("data-ready", "1");
  }, [full, settled]);

  // ---- tab title and favicon ----
  const firing = useAlertCount();
  const problems = hidden ?? Math.max(problemCount(ctrs.data), firing);
  useTabTitle(title, b?.title ?? null, problems);

  // ---- the owner's default theme, for a browser that has not chosen ----
  const themeDefault = b?.theme_default;
  useEffect(() => {
    if (themeDefault) applyDefaultTheme(themeDefault);
  }, [themeDefault]);

  // ---- sidebar state on <html> ----
  useEffect(() => {
    const root = document.documentElement;
    if (rail) root.dataset.side = "rail";
    else delete root.dataset.side;
  }, [rail]);
  useEffect(() => {
    document.documentElement.classList.toggle("side-open", open);
  }, [open]);

  const openRef = useRef(false);
  const setOpen = useCallback((on: boolean, opts: { from?: HTMLElement | null; entry?: boolean; viaBack?: boolean } = {}) => {
    if (openRef.current === on) return;
    openRef.current = on;
    setOpenState(on);
    const d = drawer.current;
    if (on) {
      d.from = opts.from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
      if (opts.entry) d.pushed = true;
      else {
        try {
          window.history.pushState({ rhDrawer: 1 }, "");
          d.pushed = true;
        } catch {
          d.pushed = false;
        }
      }
      window.setTimeout(() => sideRef.current?.querySelector<HTMLElement>(".nav-item")?.focus(), 0);
      return;
    }
    if (opts.viaBack) d.pushed = false;
    else if (d.pushed) {
      d.pushed = false;
      d.popping = true;
      try {
        window.history.back();
      } catch {
        d.popping = false;
      }
    }
    const from = d.from;
    window.setTimeout(() => {
      if (from?.isConnected && !window.matchMedia(WIDE).matches) from.focus({ preventScroll: true });
    }, 0);
  }, []);

  // Back closes the drawer; Forward onto its entry opens it again.
  useEffect(() => {
    const onPop = (e: PopStateEvent) => {
      const d = drawer.current;
      if (d.popping) {
        d.popping = false;
        return;
      }
      if (document.documentElement.classList.contains("side-open")) setOpen(false, { viaBack: true });
      else if ((e.state as { rhDrawer?: number } | null)?.rhDrawer && !window.matchMedia(WIDE).matches) setOpen(true, { entry: true });
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [setOpen]);

  // Wide again: nothing is over the page.
  useEffect(() => {
    if (wide) setOpen(false);
  }, [wide, setOpen]);

  // Leaving through a link in the open drawer replaces its history entry,
  // so Back from the next page comes straight back here.
  useEffect(() => {
    if (!full) return;
    const side = sideRef.current;
    if (!side) return;
    const onClick = (e: globalThis.MouseEvent) => {
      const d = drawer.current;
      if (!d.pushed || !(e.target instanceof Element)) return;
      const a = e.target.closest("a[href]");
      if (!(a instanceof HTMLAnchorElement) || e.defaultPrevented || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || a.target) return;
      e.preventDefault();
      d.pushed = false;
      openRef.current = false;
      setOpenState(false);
      const path = interceptedPath({
        button: 0,
        metaKey: false,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
        defaultPrevented: false,
        target: "",
        hasDownload: a.hasAttribute("download"),
        href: a.href,
        origin: window.location.origin,
        current: window.location.pathname + window.location.search,
      });
      if (path) navigate(path, { replace: true });
      else window.location.replace(a.href);
    };
    side.addEventListener("click", onClick);
    return () => side.removeEventListener("click", onClick);
  }, [full]);

  const toggle = () => {
    if (window.matchMedia(WIDE).matches) {
      const next = !rail;
      writeRail(local(), next);
      setRail(next);
    } else setOpen(!open);
  };

  const stacks = () => {
    if (window.matchMedia(WIDE).matches) {
      if (rail) {
        writeRail(local(), false);
        setRail(false);
      }
    } else setOpen(true);
    window.setTimeout(() => filterRef.current?.focus(), 0);
  };

  // ---- the keyboard map ----
  const keyState = useRef<KeyState>({ gAt: 0 });
  const current = loc.route.name === "container" || loc.route.name === "container_logs" ? loc.route.ctr : null;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if (e.key === "Escape" && document.documentElement.classList.contains("side-open")) {
        setOpen(false);
        return;
      }
      const a = document.activeElement;
      const typing =
        a instanceof HTMLElement && isTyping(a.tagName, a instanceof HTMLInputElement ? a.type : "", a.isContentEditable);
      const blocked = !!document.querySelector('[role="dialog"][data-open]:not([data-testid="palette"])') || document.documentElement.classList.contains("side-open");
      const r = keyAction(e, keyState.current, Date.now(), typing, blocked);
      if (palette && r.action?.kind !== "palette") return;
      keyState.current = r.state;
      const act = r.action;
      if (!act) return;
      switch (act.kind) {
        case "go":
          if (act.page === "settings" && b?.role === "viewer") return;
          if (b?.scoped && act.page !== "containers" && act.page !== "logs" && !(act.page === "settings" && b.role === "admin")) return;
          e.preventDefault();
          navigate(GO_HREF[act.page](b?.role === "owner", b?.scoped));
          return;
        case "palette":
          e.preventDefault();
          setPalette((shown) => !shown);
          return;
        case "help":
          e.preventDefault();
          setKeys(true);
          return;
        case "slash": {
          const f = pageFilter();
          if (f) {
            e.preventDefault();
            f.focus();
            f.select();
          } else {
            e.preventDefault();
            setPalette(true);
          }
          return;
        }
        case "row":
          e.preventDefault();
          moveKeyRow(act.dir);
          return;
        case "logs": {
          const u = logsTarget(current);
          if (u) {
            e.preventDefault();
            navigate(u);
          }
        }
      }
    };
    const onFocus = (e: FocusEvent) => {
      for (const r of document.querySelectorAll(".kfocus")) if (e.target instanceof Node && !r.contains(e.target)) r.classList.remove("kfocus");
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("focusin", onFocus);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("focusin", onFocus);
    };
  }, [b?.scoped, b?.role, current, palette, setOpen]);

  // ---- banners ----
  const dockerError = b?.features.docker ? (ctrs.data?.docker_error ?? b.docker_error) : null;
  const anyCtr = !!ctrs.data?.groups.some((g) => g.containers.length > 0);
  const overPage = open && !wide;
  const r = loc.route.name;

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <div className="app">
        {full ? (
          <Sidebar
            route={loc.route}
            site={b?.title ?? null}
            open={open}
            phone={phone}
            wide={wide}
            rail={rail}
            onToggle={toggle}
            onClose={() => setOpen(false)}
            onStacks={stacks}
            onKeys={() => setKeys(true)}
            sideRef={sideRef}
            filterRef={filterRef}
          />
        ) : (
          <aside id="side" className="side" aria-label="Sidebar" ref={sideRef} />
        )}
        <div className="scrim" onClick={() => setOpen(false)} aria-hidden="true" />
        <div className="col" inert={overPage}>
          <header className="top">
            <div className="crumbs">
              {r === "container" || r === "container_logs" ? (
                <nav className="crumb" aria-label="Breadcrumb">
                  <a href="/app/containers">Containers</a>
                  <ChevronRightIcon aria-hidden="true" />
                  <span className="title mono">{loc.route.ctr}</span>
                </nav>
              ) : <span className="title">{title}</span>}
            </div>
            <Button variant="outline" className="cmd" data-testid="palette-open" aria-label="Search or jump to…" aria-keyshortcuts="Control+k Meta+k" aria-haspopup="dialog" aria-expanded={palette} onClick={() => setPalette(true)}>
              <SearchIcon aria-hidden="true" />
              <span className="cmd-t">Search or jump to…</span>
              <kbd className="cmd-k" />
            </Button>
            {b && !b.scoped && !phone && <Vitals />}
            <ThemeButton />
            <Button variant="ghost" type="button" className="icon-btn pwa-reload" aria-label="Reload this page" title="Reload" onClick={() => window.location.reload()}>
              <RefreshCwIcon aria-hidden="true" />
            </Button>
            <Account />
          </header>
          <main id="main" tabIndex={-1}>
            {newBuild && (
              <Banner
                kind="info"
                testId="new-build"
                action={
                  <Button className="sm" onClick={() => window.location.reload()}>
                    Reload
                  </Button>
                }
              >
                <strong>A new version is available</strong> — reload to use it.
              </Banner>
            )}
            {session.session?.user && b?.config_error && (
              <Banner kind="bad" testId="config-banner">
                <strong>Config error. Still using the last good config.</strong> <code className="wrap">{b.config_error}</code>
              </Banner>
            )}
            {dockerError && (
              <Banner kind="bad" testId="docker-banner">
                <strong>Hobnail cannot reach Docker.</strong> Is the Docker socket mounted and readable?
                {anyCtr && " Containers show their last known state."}
                <details className="dk-raw">
                  <summary>Details</summary>
                  <code className="wrap">{dockerError}</code>
                </details>
              </Banner>
            )}
            {children}
          </main>
        </div>
      </div>
      <nav className="bbar" aria-label="Quick navigation" inert={overPage}>
        <a className="bb-i" href={!b?.scoped ? "/app/overview" : "/app"} aria-current={r === "overview" || r === "home" ? "page" : undefined}>
          <LayoutGridIcon aria-hidden="true" />
          <span>Overview</span>
        </a>
        <a className="bb-i" href={appHref("/containers")} aria-current={r === "containers" || r === "container" ? "page" : undefined}>
          <BoxIcon aria-hidden="true" />
          <span>Containers</span>
        </a>
        <a className="bb-i" href="/app/logs" aria-current={["logs", "container_logs", "stack_logs"].includes(r) ? "page" : undefined}>
          <ScrollTextIcon aria-hidden="true" />
          <span>
            Logs
          </span>
        </a>
        {b && !b.scoped && <>
        <a className="bb-i" href="/app/alerts" aria-current={r === "alerts" ? "page" : undefined}>
          <BellIcon aria-hidden="true" />
          <span>
            Alerts
          </span>
          {firing > 0 && <b className="bb-n">{firing}</b>}
        </a>
        </>}
        <Button variant="ghost"
          type="button"
          className="bb-i"
          data-testid="drawer-open"
          aria-controls="side"
          aria-current={r === "host" || r === "history" || r === "settings" ? "page" : undefined}
          aria-expanded={open}
          onClick={(e) => setOpen(true, { from: e.currentTarget })}
        >
          <EllipsisIcon aria-hidden="true" />
          <span>More</span>
        </Button>
      </nav>
      {palette && <LoadBoundary fallback={null} failed={null}><Palette onClose={() => setPalette(false)} onHelp={() => setKeys(true)} /></LoadBoundary>}
      {keys && <LoadBoundary fallback={null} failed={null}><Shortcuts open={keys} onClose={() => setKeys(false)} /></LoadBoundary>}
      <StalePill />
      <ShellToasts />
    </>
  );
}
