// The sidebar (src/ui/shell.rs layout, tree_inner; static/app.js tree):
// the pages, the stack tree, and the foot (live indicator, shortcuts,
// version). Pages of this client are client links; every other page links
// to the current interface (a full page load) and says so.
//
// Stack tree: every stack with its running/total count, a problem dot, its
// 24 h strip and (compose projects only) its merged logs; under it each
// container with its status dot and logs. A stack's name opens it in the
// Containers table; the chevron folds it, remembered per browser (rh-tree,
// as the old UI); the stack of the open container stays open. The filter
// text is kept while moving between pages until cleared (rh-tree-q); Enter
// opens the first match, Down moves into the list.
import { useAlertCount } from "./useAlertCount.ts";
import {
  BellIcon,
  BoxIcon,
  CircleAlertIcon,
  FunnelIcon,
  HistoryIcon,
  HouseIcon,
  KeyboardIcon,
  LayoutGridIcon,
  PanelLeftIcon,
  ScrollTextIcon,
  ServerIcon,
  SlidersHorizontalIcon,
  XIcon,
} from "lucide-react";
import { memo, startTransition, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";

import { useSession } from "../api/session.ts";
import { useBootstrap, useContainers, useStrips } from "../live/index.ts";
import { ServiceIcon } from "../ui/ServiceIcon.tsx";
import { HealthStrip } from "../ui/Sparkline.tsx";
import { CHEVRON_RIGHT_ICON, LOGS_ICON } from "./glyphs.tsx";
import { LiveIndicator } from "./Live.tsx";
import { local, readFolded, readTreeQuery, session, writeFolded, writeTreeQuery } from "./prefs.ts";
import { appHref, containerHref, type Route } from "./router.ts";
import { logsHref } from "./paths.ts";
import { filterTree, treeOf, treeTotals, type TreeCtr, type TreeStack } from "./tree.ts";

const stackLogsUrl = (name: string) => `/app/stacks/${encodeURIComponent(name)}/logs`;

export function Brand({ site }: { site: string | null }) {
  return (
    <a className="brand" href={appHref("/")} title={site ? `Hobnail · ${site}` : "Hobnail"}>
      <img className="logo" src="/icon.svg" alt="" width="24" height="24" />
      <span className="brand-name">
        Hobnail
        {site && <span className="brand-site">{site}</span>}
      </span>
    </a>
  );
}

function NavLink({
  href,
  label,
  icon,
  current,
  extra,
}: {
  href: string;
  label: string;
  icon: ReactNode;
  current?: boolean;
  extra?: ReactNode;
}) {
  return (
    <a
      className="nav-item"
      href={href}
      data-rtip={label}
      aria-current={current ? "page" : undefined}
    >
      {icon}
      <span className="nav-label">
        {label}
      </span>
      {extra}
    </a>
  );
}

function Dot({ c }: { c: TreeCtr }) {
  const pulse = c.label === "starting" || c.label === "restarting";
  return <span className={`dot ${c.dot}${pulse ? " pulse" : ""}`} role="img" aria-label={c.label} title={c.label} />;
}

/**
 * How many of `total` items to render: a few at once, then a slice per
 * task in a transition (React yields between them), so a long tree never
 * makes one long task at load.
 */
function useProgressive(total: number, first: number, slice: number): number {
  const [limit, setLimit] = useState(first);
  useEffect(() => {
    if (limit >= total) return;
    const t = window.setTimeout(() => startTransition(() => setLimit((l) => l + slice)), 0);
    return () => window.clearTimeout(t);
  }, [limit, total, slice]);
  return limit;
}

interface StackItemProps {
  stack: TreeStack;
  ctrs: TreeCtr[];
  active: string | null;
  folded: boolean;
  filtering: boolean;
  scoped: boolean;
  strip: number[] | undefined;
  onFold: (stack: string, folded: boolean) => void;
}

/** A new list rebuilds the tree's objects: a stack re-renders only when what it shows changed. */
function sameStack(a: StackItemProps, b: StackItemProps): boolean {
  return (
    a.active === b.active &&
    a.folded === b.folded &&
    a.filtering === b.filtering &&
    a.scoped === b.scoped &&
    a.strip === b.strip &&
    a.onFold === b.onFold &&
    JSON.stringify(a.stack) === JSON.stringify(b.stack) &&
    JSON.stringify(a.ctrs) === JSON.stringify(b.ctrs)
  );
}

const StackItem = memo(function StackItem({
  stack,
  ctrs,
  active,
  folded,
  filtering,
  scoped,
  strip,
  onFold,
}: {
  stack: TreeStack;
  ctrs: TreeCtr[];
  active: string | null;
  folded: boolean;
  filtering: boolean;
  scoped: boolean;
  strip: number[] | undefined;
  onFold: (stack: string, folded: boolean) => void;
}) {
  const hasActive = active !== null && ctrs.some((c) => c.name === active);
  const open = filtering || hasActive || !folded;
  return (
    <details
      className="stack"
      data-testid="tree-stack"
      data-stack={stack.name}
      open={open}
      onToggle={(e) => {
        const now = e.currentTarget.open;
        if (now !== open && !filtering) onFold(stack.name, !now);
      }}
    >
      <summary className={hasActive ? "cur" : undefined}>
        {CHEVRON_RIGHT_ICON}
        <a className="stack-name" href={`${appHref("/containers")}?stack=${encodeURIComponent(stack.name)}`} title={stack.title}>
          {stack.title}
        </a>
        {stack.trouble && <span className="dot bad" role="img" aria-label="problem in this stack" />}
        <span
          className={stack.running === stack.total ? "cnt num all-up" : "cnt num"}
          data-testid="tree-count"
          title={`${stack.running} of ${stack.total} running`}
        >
          {stack.running}/{stack.total}
        </span>
        {!scoped && stack.project && (
          <a className="ia logs" href={stackLogsUrl(stack.name)} title="Logs of the whole stack (current interface)" aria-label={`Logs of the stack ${stack.title}`}>
            {LOGS_ICON}
          </a>
        )}
        {strip && strip.length > 0 && <HealthStrip codes={strip} label={stack.title} />}
      </summary>
      {/* A folded stack's items are drawn when it opens (fewer nodes at load). */}
      {open && (
      <ul>
        {ctrs.map((c) => (
          <li key={c.name} className={c.running ? "ctr" : "ctr stopped"}>
            <a href={containerHref(c.name)} title={c.name} aria-current={active === c.name ? "page" : undefined}>
              <Dot c={c} />
              <span className="nm">{c.title}</span>
              {!c.running && <span className="sr"> ({c.label})</span>}
            </a>
            {!scoped && <a className="ia logs" href={logsHref(c.name)} title="Logs" aria-label={`Logs of ${c.title}`}>
              {LOGS_ICON}
            </a>}
          </li>
        ))}
      </ul>
      )}
    </details>
  );
}, sameStack);

/** The container links the filter leaves on screen, in order. */
function treeHits(t: HTMLElement): HTMLAnchorElement[] {
  return Array.from(t.querySelectorAll<HTMLAnchorElement>("li.ctr > a:first-child")).filter((a) => a.offsetParent !== null);
}

function Tree({ active, filterRef }: { active: string | null; filterRef: Ref<HTMLInputElement> }) {
  const b = useBootstrap();
  // Deferred: the tree renders in a transition (sliced), after the page.
  const { data: liveData, error } = useContainers();
  const data = useDeferredValue(liveData, null);
  const strips = useStrips();
  const [q, setQ] = useState(() => readTreeQuery(session()));
  const [folded, setFolded] = useState(() => readFolded(local()));
  const listRef = useRef<HTMLDivElement>(null);
  const stacks = useMemo(() => treeOf(data), [data]);
  const shown = useMemo(() => filterTree(stacks, q), [stacks, q]);
  // The page's content comes first: the tree follows a few stacks per task.
  const limit = useProgressive(shown.length, 0, 3);
  const onFold = useCallback((name: string, f: boolean) => setFolded(writeFolded(local(), name, f)), []);
  const totals = treeTotals(stacks);
  const filtering = q.trim() !== "";
  // The open container's item is looked for once the whole tree is drawn.
  const loaded = data !== null && limit >= shown.length;

  // The open container's item in view (NAV-24), once per page.
  useEffect(() => {
    const t = listRef.current;
    if (!t || !loaded || active === null) return;
    const a = t.querySelector<HTMLElement>(`li.ctr > a[href="${CSS.escape(containerHref(active))}"]`);
    if (!a || !t.clientHeight) return;
    const r = a.getBoundingClientRect();
    const box = t.getBoundingClientRect();
    if (r.top < box.top || r.bottom > box.bottom) t.scrollTop += r.top - box.top - box.height / 3;
  }, [active, loaded]);

  const onFilterKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      const a = listRef.current && treeHits(listRef.current)[0];
      if (a) {
        e.preventDefault();
        a.click();
      }
    } else if (e.key === "ArrowDown") {
      const a = listRef.current && treeHits(listRef.current)[0];
      if (a) {
        e.preventDefault();
        a.focus();
      }
    }
  };
  // Up and Down move through the containers; Up from the first goes back to the filter.
  useEffect(() => {
    const t = listRef.current;
    if (!t) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      const a = e.target;
      if (!(a instanceof HTMLAnchorElement) || !a.matches("li.ctr > a:first-child")) return;
      const list = treeHits(t);
      const i = list.indexOf(a);
      e.preventDefault();
      if (e.key === "ArrowUp" && i === 0) document.getElementById("tree-filter")?.focus();
      else list[i + (e.key === "ArrowDown" ? 1 : -1)]?.focus();
    };
    t.addEventListener("keydown", onKey);
    return () => t.removeEventListener("keydown", onKey);
  }, [listRef]);

  if (b && !b.features.docker) {
    return (
      <div className="tree-wrap">
        <div className="tree" id="tree">
          <p className="tree-note">Docker discovery is off.</p>
        </div>
      </div>
    );
  }
  const dockerError = data?.docker_error ?? b?.docker_error ?? null;
  return (
    <div className="tree-wrap">
      <div className="tree-head">
        <span className="label">Stacks</span>
        <span className="tree-cnt num" title={`${totals.running} of ${totals.total} containers running`}>
          <b>{totals.running}</b>/{totals.total}
        </span>
      </div>
      <div className="tree-filter">
        <FunnelIcon aria-hidden="true" />
        <input
          ref={filterRef}
          id="tree-filter"
          type="search"
          placeholder="Filter containers"
          aria-label="Filter stacks and containers"
          autoComplete="off"
          spellCheck={false}
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            writeTreeQuery(session(), e.target.value);
          }}
          onKeyDown={onFilterKey}
        />
      </div>
      <div className="tree" id="tree" ref={listRef}>
        {dockerError ? (
          <p className="tree-note bad" title={dockerError}>
            <CircleAlertIcon aria-hidden="true" />
            Docker unreachable
          </p>
        ) : !data ? (
          <p className="tree-note">{error ? `Could not load the containers: ${error}` : "Connecting to Docker…"}</p>
        ) : null}
        {(limit >= shown.length ? shown : shown.slice(0, limit)).map(({ stack, ctrs }) => (
          <StackItem
            key={stack.name}
            stack={stack}
            ctrs={ctrs}
            active={active}
            folded={!!folded[stack.name]}
            filtering={filtering}
            scoped={!!b?.scoped}
            strip={strips?.stacks[stack.name]}
            onFold={onFold}
          />
        ))}
        {data && !dockerError && stacks.length === 0 && <p className="tree-note">No containers.</p>}
        {filtering && stacks.length > 0 && shown.length === 0 && <p className="tree-note">No match.</p>}
      </div>
    </div>
  );
}

export interface SidebarProps {
  route: Route;
  site: string | null;
  /** Over the page (rail opened at 700–1100 px, or the phone drawer). */
  open: boolean;
  /** Under 700 px: a drawer, out of the tab order while closed. */
  phone: boolean;
  wide: boolean;
  rail: boolean;
  onToggle: () => void;
  onClose: () => void;
  /** The rail's "Stacks" item: expand, focus the filter (NAV-27). */
  onStacks: () => void;
  onKeys: () => void;
  sideRef: Ref<HTMLElement>;
  filterRef: Ref<HTMLInputElement>;
}

export function Sidebar({
  route: r,
  site,
  open,
  phone,
  wide,
  rail,
  onToggle,
  onClose,
  onStacks,
  onKeys,
  sideRef,
  filterRef,
}: SidebarProps) {
  const b = useBootstrap();
  const s = useSession();
  const active = r.name === "container" ? r.ctr : null;
  const firing = useAlertCount();
  const build = s.session?.build;
  const railLabel = wide ? (rail ? "Expand sidebar" : "Collapse sidebar") : open ? "Close sidebar" : "Open sidebar";
  return (
    <aside
      id="side"
      className="side"
      aria-label="Sidebar"
      ref={sideRef}
      inert={phone && !open}
    >
      <div className="side-head">
        <Brand site={site} />
        <button
          type="button"
          className="icon-btn rail-toggle"
          aria-label={railLabel}
          title={railLabel}
          aria-pressed={wide ? rail : open}
          onClick={onToggle}
        >
          <PanelLeftIcon aria-hidden="true" />
        </button>
        <button type="button" className="icon-btn side-close" aria-label="Close menu" title="Close" onClick={onClose}>
          <XIcon aria-hidden="true" />
        </button>
      </div>
      <nav className="nav" aria-label="Main" data-testid="sidebar">
        <NavLink href={appHref("/")} label="Home" icon={<HouseIcon aria-hidden="true" />} current={r.name === "home" || (r.name === "overview" && !b?.default_board)} />
        <NavLink
          href={appHref("/containers")}
          label="Containers"
          icon={<BoxIcon aria-hidden="true" />}
          current={r.name === "containers" || r.name === "container"}
        />
        {b?.boards.map((board) => <NavLink key={board.id} href={`/app/boards/${board.id}`} label={board.name}
          icon={<ServiceIcon name={board.icon} title={board.name} enabled={b.remote_icons} />} current={r.name === "board" && r.id === board.id} />)}
        {b && b.role !== "viewer" && <NavLink href="/app/settings/boards" label="Settings" icon={<SlidersHorizontalIcon aria-hidden="true" />} current={r.name === "settings"} />}
        {b && !b.scoped && <>
        <NavLink href="/app/overview" label="Overview" current={r.name === "overview"} icon={<LayoutGridIcon aria-hidden="true" />} />
        <NavLink href="/app/host" label="Host" current={r.name === "host"} icon={<ServerIcon aria-hidden="true" />} />
        <NavLink href="/app/logs" label="Logs" icon={<ScrollTextIcon aria-hidden="true" />} />
        <NavLink href="/app/history" label="History" current={r.name === "history"} icon={<HistoryIcon aria-hidden="true" />} />
        <NavLink
          href="/app/alerts"
          label="Alerts"
          current={r.name === "alerts"}
          icon={<BellIcon aria-hidden="true" />}
          extra={
            firing > 0 ? (
              <span className="nav-badge" title="Alerts firing">
                {firing}
                <span className="sr"> firing</span>
              </span>
            ) : null
          }
        />
        </>}
        <button type="button" className="nav-item rail-only" data-rtip="Stacks" onClick={onStacks}>
          <FunnelIcon aria-hidden="true" />
          <span className="nav-label">Stacks and containers</span>
        </button>
      </nav>
      <Tree active={active} filterRef={filterRef} />
      <div className="side-foot">
        <LiveIndicator />
        <button type="button" className="linklike keys-btn" title="Keyboard shortcuts (?)" aria-label="Keyboard shortcuts" onClick={onKeys}>
          <KeyboardIcon aria-hidden="true" />
        </button>
        {build && (
          <span className="ver" title={`built ${build.time}`}>
            v{build.version} · <code>{build.sha}</code>
          </span>
        )}
      </div>
    </aside>
  );
}
