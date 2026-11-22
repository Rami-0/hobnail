import { Badge, statusBadge } from "../../ui/Badge.tsx";
import { TextInput } from "../../ui/Field.tsx";
import { PageHeader } from "../../ui/PageHeader.tsx";
// /app/containers: the dense, filterable, sortable table of every container
//
// The URL holds the view (q, state, stack, sort, dir, group; replaced, not
// pushed); sort, direction and grouping are remembered per browser. Live
// numbers arrive through useContainers(): the rows are memoised and do not
// re-render on a stats tick; the CPU and memory cells read their own row
// from a small store (liveCells.ts) and the sparklines share one scale per
// column. Nothing re-sorts while focus or the pointer is in the table.
// 150 rows stay cheap: the first rows render at once and the rest follow
// in small slices, heavy cells (strip, sparklines) wait until their row is
// near the screen, and phone rows use `content-visibility: auto`.
import {
  ArrowDownIcon,
  ArrowUpIcon,
  BoxIcon,
  CircleAlertIcon,
  CircleCheckIcon,
  LayersIcon,
  ListIcon,
  RefreshCwIcon,
  SearchIcon,
} from "lucide-react";
import {
  memo,
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import type { ContainerRow } from "../../api/gen/ContainerRow.ts";
import type { GroupRow } from "../../api/gen/GroupRow.ts";
import { setSearch, useRoute } from "../../app/router.ts";
import { bytes, imageLabel, pct, portLinks, rowLink, statusText, viewerHost, duration } from "../../format/index.ts";
import { stats, useBootstrap, useContainers, useNow, useStatsMap, useStrips } from "../../live/index.ts";
import { Button } from "../../ui/Button.tsx";
import { EmptyState } from "../../ui/Note.tsx";
import { HealthStrip, Sparkline } from "../../ui/Sparkline.tsx";
import { Select } from "../../ui/Select.tsx";
import { CHEVRON_DOWN_ICON, EXTERNAL_ICON, LOGS_ICON } from "../../app/glyphs.tsx";
import { LiveCells } from "./liveCells.ts";
import { containerHref, logsHref, ServiceIcon, StatusDot } from "./parts.tsx";
import { useNear } from "./useNear.ts";
import {
  counts,
  flipDir,
  isClosed,
  isDesc,
  isGrouped,
  keepOrder,
  noneWhy,
  orderForPin,
  parseView,
  remember,
  select,
  sortBy,
  SORTS,
  uptime,
  viewSearch,
  type Remembered,
  type Section,
  type SortKey,
  type StateFilter,
  type View,
} from "./view.ts";
import "./containers.css";

const VIEW_KEY = "rh-app-ctr-view";
/** Folded stacks in the container list. */
const FOLD_KEY = "rh-collapsed";

function load<T>(k: string, d: T): T {
  try {
    const v = localStorage.getItem(k);
    return v === null ? d : (JSON.parse(v) as T);
  } catch {
    return d;
  }
}
function save(k: string, v: unknown): void {
  try {
    localStorage.setItem(k, JSON.stringify(v));
  } catch {
    /* private mode: not remembered */
  }
}

/** Every container's live numbers, for the cells. */
function statReadings() {
  return Array.from(stats.names(), (name) => {
    const s = stats.get(name);
    return { name, cpu: s.cpu_pct, mem: s.mem_bytes };
  });
}

/** Rows rendered on the first frame; the rest follow in slices of this size, one per task. */
const FIRST = 10;
const SLICE = 10;
const NO_ROWS: ContainerRow[] = [];

function useProgressive(total: number): number {
  const [limit, setLimit] = useState(FIRST);
  useEffect(() => {
    if (limit >= total) return;
    // A transition: React renders the slice in yielding chunks.
    let tick = 0;
    const frame = requestAnimationFrame(() => {
      tick = window.setTimeout(() => startTransition(() => setLimit((l) => l + SLICE)), 0);
    });
    return () => { cancelAnimationFrame(frame); window.clearTimeout(tick); };
  }, [limit, total]);
  return limit;
}

export default function ContainersPage() {
  const loc = useRoute();
  // The list as a deferred value: its first arrival (and any later change)
  // renders in a transition, which React slices into short tasks.
  const { data: liveData, error } = useContainers();
  const [tableReady, setTableReady] = useState(false);
  useEffect(() => {
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => startTransition(() => setTableReady(true)));
    });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); };
  }, []);
  const data = useDeferredValue(tableReady ? liveData : null, null);
  const boot = useBootstrap();
  const strips = useStrips();
  const host = viewerHost(window.location.hostname);

  // ---- the view: the URL holds it (or, with no view keys, what this browser remembers) ----
  const [remembered, setRemembered] = useState(() => load<Remembered | null>(VIEW_KEY, null));
  const view = useMemo(() => parseView(loc.query, remembered), [loc.query, remembered]);
  const canonical = viewSearch(view);
  // Remembered sort and grouping go into the URL too (replaced: a view is not a history step).
  useEffect(() => {
    if (loc.query.replace(/^\?/, "") !== canonical) setSearch(new URLSearchParams(canonical), { replace: true });
  }, [loc.query, canonical]);
  const setView = useCallback(
    (f: (v: View) => View) => {
      const next = f(parseView(window.location.search, remembered));
      const r = remember(next);
      save(VIEW_KEY, r);
      setRemembered(r);
      setSearch(new URLSearchParams(viewSearch(next)), { replace: true });
    },
    [remembered],
  );
  // The filter's own text (the URL keeps it trimmed); follows the URL when that changes elsewhere.
  const [qText, setQText] = useState(view.q);
  const [qFromUrl, setQFromUrl] = useState(view.q);
  if (qFromUrl !== view.q) {
    setQFromUrl(view.q);
    if (qText.trim() !== view.q) setQText(view.q);
  }

  // ---- folded stacks ----
  const [folded, setFolded] = useState<Record<string, boolean>>(() => {
    const v = load<unknown>(FOLD_KEY, {});
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, boolean>) : {};
  });
  const toggleFold = useCallback((k: string, closed: boolean) => {
    setFolded((f) => {
      const n = { ...f, [k]: closed };
      save(FOLD_KEY, n);
      return n;
    });
  }, []);

  // ---- the order: recomputed, unless the reader is in the table ----
  type Snapshot = { key: string; sections: Section[] };
  const [inTable, setInTable] = useState<{ focus: boolean; pointer: boolean; snapshot: Snapshot | null }>({
    focus: false, pointer: false, snapshot: null,
  });
  const frozen = inTable.focus || inTable.pointer;
  const groups = useMemo<GroupRow[]>(() => data?.groups ?? [], [data]);
  const now = useNow();
  // Live numbers live in the stats store, not in the view: only a sort by
  // CPU or memory follows them (and not while the reader is in the table).
  const byNumber = view.sort === "cpu" || view.sort === "mem";
  const live = useStatsMap(byNumber && !frozen);
  const fresh = useMemo(
    () => select(groups, view, now, byNumber ? (n: string) => (frozen ? stats.get(n) : live.get(n) ?? stats.get(n)) : undefined),
    [groups, view, now, byNumber, frozen, live],
  );
  // The order on screen when the reader came in; a change of the view
  // itself (a sort click) is the reader's own and starts a new one.
  const committed = useRef<Snapshot>(null);
  const [pin, setPin] = useState<{ key: string; keys: string[] } | null>(null);
  if (frozen && (!pin || pin.key !== canonical)) {
    setPin({ key: canonical, keys: orderForPin(canonical, fresh, inTable.snapshot) });
  }
  if (!frozen && pin) setPin(null);
  const sections = useMemo(
    () => (frozen && pin && pin.key === canonical ? keepOrder(fresh, pin.keys) : fresh),
    [frozen, pin, canonical, fresh],
  );
  useLayoutEffect(() => {
    committed.current = { key: canonical, sections };
  }, [canonical, sections]);

  const enter = (kind: "focus" | "pointer") => {
    const snapshot = committed.current;
    setInTable((s) => s[kind] ? s : { ...s, [kind]: true, snapshot: s.focus || s.pointer ? s.snapshot : snapshot });
  };

  // ---- live cells: fed by the stats store, outside React; only the cells
  // whose numbers changed re-render ----
  const [cells] = useState(() => {
    const lc = new LiveCells();
    lc.update(statReadings(), stats.version());
    return lc;
  });
  useEffect(() => {
    const feed = () => cells.notify(cells.update(statReadings(), stats.version()));
    feed();
    return stats.subscribeAll(feed);
  }, [cells]);

  const c = useMemo(() => counts(groups), [groups]);
  const total = c.all;
  const grouped = isGrouped(view);
  let shown = 0;
  let hiddenInFolded = 0;
  for (const sec of sections) {
    if (sec.group && grouped && isClosed(sec.group, view, folded)) hiddenInFolded += sec.rows.length;
    else shown += sec.rows.length;
  }
  const matched = shown + hiddenInFolded;
  const limit = useProgressive(matched);
  // What each section renders now: the first rows at once, the rest in slices.
  const slices: { section: Section; closed: boolean; rows: ContainerRow[] }[] = [];
  let budget = limit;
  for (const sec of sections) {
    const closed = !!sec.group && grouped && isClosed(sec.group, view, folded);
    const take = closed ? 0 : Math.max(0, Math.min(sec.rows.length, budget));
    budget -= take;
    slices.push({ section: sec, closed, rows: take === 0 ? NO_ROWS : take === sec.rows.length ? sec.rows : sec.rows.slice(0, take) });
  }

  // ---- keyboard: the shell's map does j/k, l and / (it finds the rows by
  // data-testid="row" and the filter by data-testid="filter"); here, the
  // arrows move between rows while focus is in one, as j/k do ----
  const filterRef = useRef<HTMLInputElement>(null);
  const tableRef = useRef<HTMLTableElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      const here = document.activeElement instanceof Element ? document.activeElement.closest('tr[data-testid="row"]') : null;
      const table = tableRef.current;
      if (!here || !table?.contains(here)) return;
      const rs = Array.from(table.querySelectorAll<HTMLTableRowElement>('tr[data-testid="row"]'));
      const r = rs[rs.indexOf(here as HTMLTableRowElement) + (e.key === "ArrowDown" ? 1 : -1)];
      if (!r) return;
      e.preventDefault();
      for (const x of rs) x.classList.remove("kfocus");
      r.classList.add("kfocus");
      r.querySelector<HTMLAnchorElement>("a.nm")?.focus({ preventScroll: true });
      r.scrollIntoView({ block: "nearest" });
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  const stackOptions = useMemo(
    () => [
      { value: "", label: `All stacks (${groups.length})` },
      ...groups.map((g) => ({ value: g.name, label: `${g.title} (${g.containers.length})` })),
    ],
    [groups],
  );

  const clear = () => setView((v) => ({ ...v, q: "", state: "", stack: "" }));
  const desc = isDesc(view);

  let body;
  if (boot && !boot.features.docker) {
    body = (
      <EmptyState icon={<BoxIcon aria-hidden="true" />} title="Docker discovery is off">
        Set <code>docker.enabled = true</code> in the config to list containers.
      </EmptyState>
    );
  } else if (!data) {
    body = error ? (
      <EmptyState icon={<CircleAlertIcon aria-hidden="true" />} title="Could not load the containers" tone="bad">
        {String(error)}
      </EmptyState>
    ) : (
      <output className="muted ctbl-loading">Loading containers…</output>
    );
  } else if (data.fetched_at === 0 && !data.docker_error) {
    body = (
      <EmptyState icon={<RefreshCwIcon aria-hidden="true" />} title="Connecting to Docker…">
        The list appears as soon as the first snapshot arrives.
      </EmptyState>
    );
  } else if (total === 0 && data.docker_error) {
    body = (
      <EmptyState icon={<CircleAlertIcon aria-hidden="true" />} title="No containers to show" tone="bad" testId="docker-unreachable">
        Docker cannot be reached ({data.docker_error}) and no earlier list is known.
      </EmptyState>
    );
  } else if (total === 0) {
    body = (
      <EmptyState icon={<BoxIcon aria-hidden="true" />} title="No containers yet">
        Hobnail lists the containers Docker runs on this host. Hidden containers and excluded names are not listed.
      </EmptyState>
    );
  } else {
    const why = noneWhy(view);
    body = (
      <div
        className="tablewrap"
        onFocus={() => enter("focus")}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setInTable((s) => ({ ...s, focus: false }));
        }}
        onPointerEnter={(e) => {
          if (e.pointerType === "mouse") enter("pointer");
        }}
        onPointerLeave={() => setInTable((s) => (s.pointer ? { ...s, pointer: false } : s))}
      >
        <table ref={tableRef} className={grouped ? "tbl ctbl grouped" : "tbl ctbl"} data-testid="containers">
          <caption className="count" role="status" aria-live="polite" data-testid="showing">
            Showing <b className="num">{shown}</b> of <span className="num">{total}</span> containers
            {hiddenInFolded > 0 ? ` · ${hiddenInFolded} in collapsed stacks` : ""}
          </caption>
          <thead>
            <tr>
              <SortTh view={view} k="name" label="Name" cls="c-name" setView={setView} />
              <SortTh view={view} k="stack" label="Stack" cls="c-stack" setView={setView} />
              <th className="c-image" scope="col">
                Image
              </th>
              <SortTh view={view} k="state" label="State" cls="c-state" setView={setView} />
              <SortTh view={view} k="uptime" label="Uptime" cls="c-up c-num" setView={setView} />
              <th className="c-hist" scope="col" title="Health over the last 24 hours">
                24 h
              </th>
              <SortTh view={view} k="cpu" label="CPU" cls="c-num c-cpu" setView={setView} />
              <SortTh view={view} k="mem" label="Memory" cls="c-num c-mem" setView={setView} />
              <th className="c-ports" scope="col">
                Ports
              </th>
              <th className="c-act" scope="col">
                <span className="sr">Actions</span>
              </th>
            </tr>
          </thead>
          {slices.map(({ section, closed, rows }) => (
            <SectionBody
              key={section.group?.name ?? "flat"}
              section={section}
              closed={closed}
              rows={rows}
              strip={section.group ? strips?.stacks[section.group.name] : undefined}
              stripStep={strips?.step}
              rowStrips={strips?.containers}
              now={now}
              host={host}
              cells={cells}
              onFold={toggleFold}
            />
          ))}
          {matched === 0 && (
            <tbody className="none">
              <tr>
                <td colSpan={10}>
                  {why === "stopped" ? (
                    <EmptyState compact tone="good" icon={<CircleCheckIcon aria-hidden="true" />} title="Nothing is stopped">
                      Every container is running.
                    </EmptyState>
                  ) : why === "problem" ? (
                    <EmptyState compact tone="good" icon={<CircleCheckIcon aria-hidden="true" />} title="No problems">
                      Nothing is unhealthy, restarting or dead.
                    </EmptyState>
                  ) : (
                    <EmptyState
                      compact
                      icon={<SearchIcon aria-hidden="true" />}
                      title="No containers match"
                      testId="no-match"
                      actions={
                        <Button className="sm" onClick={clear}>
                          Clear filters
                        </Button>
                      }
                    >
                      Try another name or state.
                    </EmptyState>
                  )}
                </td>
              </tr>
            </tbody>
          )}
        </table>
      </div>
    );
  }

  const chip = (label: string, f: StateFilter, n: number) => (
    <Button variant="ghost"
      type="button"
      data-state={f || "all"}
      aria-pressed={view.state === f}
      onClick={() => setView((v) => ({ ...v, state: f }))}
    >
      {label} <span className="n num">{n}</span>
    </Button>
  );

  return (
    <div className="page wide ctr-page ct">
      <PageHeader title="Containers" description="Search and group containers, and check their current state and resource usage." />
      <search className="bar">
        <div className="search-in">
          <SearchIcon aria-hidden="true" />
          <TextInput
            ref={filterRef}
            type="search"
            data-testid="filter"
            value={qText}
            onChange={(e) => {
              const q = e.target.value;
              setQText(q);
              setView((v) => ({ ...v, q: q.trim() }));
            }}
            onKeyDown={(e: ReactKeyboardEvent<HTMLInputElement>) => {
              // Enter opens the first match; Down moves into the rows.
              if (e.key === "Enter") {
                const a = tableRef.current?.querySelector<HTMLAnchorElement>('tr[data-testid="row"] a.nm');
                if (a) {
                  e.preventDefault();
                  a.click();
                }
              } else if (e.key === "ArrowDown") {
                const a = tableRef.current?.querySelector<HTMLAnchorElement>('tr[data-testid="row"] a.nm');
                if (a) {
                  e.preventDefault();
                  a.focus();
                }
              }
            }}
            placeholder="Filter by name, stack or image"
            autoComplete="off"
            spellCheck={false}
            aria-label="Filter containers"
          />
        </div>
        <fieldset className="seg" data-testid="state-filter">
          <legend className="sr">State</legend>
          {chip("All", "", c.all)}
          {chip("Running", "running", c.running)}
          {chip("Stopped", "stopped", c.stopped)}
          {chip("Problems", "problem", c.problem)}
        </fieldset>
        <Select
          label="Stack"
          hideLabel
          className="stack-sel"
          data-testid="stack-filter"
          value={view.stack}
          onChange={(stack) => setView((v) => ({ ...v, stack }))}
          options={
            view.stack && !groups.some((g) => g.name === view.stack)
              ? [...stackOptions, { value: view.stack, label: `${view.stack} (0)` }]
              : stackOptions
          }
        />
        <div className="sortctl">
          <Select
            label="Sort by"
            hideLabel
            data-testid="sort-select"
            value={view.sort}
            onChange={(s) => setView((v) => ({ ...v, sort: s as SortKey, dir: null }))}
            options={SORTS.map((s) => ({ value: s.key, label: `Sort: ${s.label}` }))}
          />
          <Button
            size="icon" variant="ghost"
            data-testid="sort-dir"
            aria-label={desc ? "Descending; switch to ascending" : "Ascending; switch to descending"}
            icon={desc ? <ArrowDownIcon aria-hidden="true" /> : <ArrowUpIcon aria-hidden="true" />}
            onClick={() => setView(flipDir)}
          />
        </div>
        <fieldset className="seg" data-testid="grouping">
          <legend className="sr">Layout</legend>
          <Button variant="ghost" type="button" data-group="stack" aria-pressed={grouped} onClick={() => setView((v) => ({ ...v, group: "stack" }))}>
            <LayersIcon aria-hidden="true" />
            <span>Stacks</span>
          </Button>
          <Button variant="ghost" type="button" data-group="flat" aria-pressed={!grouped} onClick={() => setView((v) => ({ ...v, group: "flat" }))}>
            <ListIcon aria-hidden="true" />
            <span>Flat</span>
          </Button>
        </fieldset>
      </search>
      {body}
    </div>
  );
}

function SortTh({
  view,
  k,
  label,
  cls,
  setView,
}: {
  view: View;
  k: SortKey;
  label: string;
  cls: string;
  setView: (f: (v: View) => View) => void;
}) {
  const on = view.sort === k;
  const desc = isDesc(view);
  return (
    <th className={cls} scope="col" data-sort={k} aria-sort={on ? (desc ? "descending" : "ascending") : "none"}>
      <Button variant="ghost" type="button" className="sort-b" onClick={() => setView((v) => sortBy(v, k))}>
        {label}
        {on && desc ? (
          <ArrowDownIcon className="sort-i" aria-hidden="true" />
        ) : (
          <ArrowUpIcon className={on ? "sort-i" : "sort-i off"} aria-hidden="true" />
        )}
      </Button>
    </th>
  );
}

interface SectionProps {
  section: Section;
  closed: boolean;
  rows: ContainerRow[];
  strip: number[] | undefined;
  stripStep: number | undefined;
  rowStrips: Record<string, number[]> | undefined;
  now: number;
  host: string;
  cells: LiveCells;
  onFold: (k: string, closed: boolean) => void;
}

/**
 * Rows not drawn yet hold their place (an empty row as tall as they will
 * be), so the next slice fills it and nothing below moves (no layout shift).
 */
function Waiting({ n, grouped = false }: { n: number; grouped?: boolean }) {
  if (n <= 0) return null;
  return <tr className="row-wait" aria-hidden="true" style={{ "--n": n } as CSSProperties}>
    <td colSpan={grouped ? 9 : 10}><span className="sr">Loading</span></td>
  </tr>;
}

const SectionBody = memo(function SectionBody({ section, closed, rows, strip, stripStep, rowStrips, now, host, cells, onFold }: SectionProps) {
  const g = section.group;
  const waiting = closed ? 0 : section.rows.length - rows.length;
  if (!g) {
    return (
      <tbody className="flat">
        {rows.map((r) => (
          <Row key={r.name} row={r} strip={rowStrips?.[r.name]} stripStep={stripStep} now={now} host={host} cells={cells} />
        ))}
        <Waiting n={waiting} />
      </tbody>
    );
  }
  const all = g.containers.length;
  return (
    <tbody className={closed ? "grp closed" : "grp"}>
      <tr className="grp-h" data-testid="group" data-stack={g.name}>
        <th colSpan={9} scope="rowgroup">
          <Button variant="ghost"
            type="button"
            className="grp-t"
            aria-expanded={!closed}
            title={`${g.running} running${g.stopped ? ` · ${g.stopped} stopped` : ""}`}
            onClick={() => onFold(g.name, !closed)}
          >
            {CHEVRON_DOWN_ICON}
            <span className="grp-n" title={g.title}>
              {g.title}
            </span>
            <span className="cnt num">
              {g.running}/{all}
            </span>
            {strip && (
              <span className="grp-strip">
                <HealthStrip codes={strip} label={g.title} step={stripStep} />
              </span>
            )}
          </Button>
        </th>
      </tr>
      {!closed &&
        rows.map((r) => (
          <Row key={r.name} row={r} strip={rowStrips?.[r.name]} stripStep={stripStep} now={now} host={host} cells={cells} />
        ))}
      <Waiting n={waiting} grouped />
    </tbody>
  );
});

interface RowProps {
  row: ContainerRow;
  strip: number[] | undefined;
  stripStep: number | undefined;
  now: number;
  host: string;
  cells: LiveCells;
}

/** Fields a row shows apart from the live numbers (those are the cells' own). */
function sameRow(a: RowProps, b: RowProps): boolean {
  if (a.strip !== b.strip || a.host !== b.host || a.cells !== b.cells || a.stripStep !== b.stripStep) return false;
  return a.row === b.row && Math.floor(a.now / 60) === Math.floor(b.now / 60);
}

const Row = memo(function Row({ row: r, strip, stripStep, now, host, cells }: RowProps) {
  const ref = useRef<HTMLTableRowElement>(null);
  const { seen: near, visible } = useNear(ref);
  const href = containerHref(r.name);
  const ports = portLinks(r.ports, host);
  const open = rowLink(r, host);
  const running = r.state === "running";
  const up = uptime(r, now);
  return (
    <tr ref={ref} className={running ? "row" : "row stopped"} data-testid="row" data-name={r.name}>
      <td className="c-name">
        <StatusDot dot={r.dot} />
        <ServiceIcon name={r.icon} label={r.title} />
        <a className="nm" href={href} title={r.name}>
          <span className="nm-t">{r.title}</span>
          {r.title !== r.name && <span className="sub mono">{r.name}</span>}
        </a>
        {r.restarts > 0 && <Badge variant="warning">{r.restarts}× exit/1h</Badge>}
      </td>
      <td className="c-stack" data-l="Stack">
        <a href={`/app/containers?stack=${encodeURIComponent(r.stack)}`}>{r.stack}</a>
      </td>
      <td className="c-image" data-l="Image">
        <span className="mono img" title={r.image}>
          {imageLabel(r.image)}
        </span>
      </td>
      <td className="c-state" data-l="State" title={statusText(r, now)}>
        <Badge variant={statusBadge(r.dot)}>{r.label}</Badge>
        {r.exit_code !== null && r.exit_code !== 0 && (
          <>
            {" "}
            <span className="mono exit" title="exit code">
              {r.exit_code}
            </span>
          </>
        )}
      </td>
      <td className="c-up c-num" data-l="Uptime">
        {up === null ? <span className="muted">—</span> : <span className="num">{duration(up)}</span>}
      </td>
      <td className="c-hist" data-l="24 h">
        {near ? <HealthStrip codes={strip ?? []} label={r.title} step={stripStep} /> : null}
      </td>
      <ResourceCells name={r.name} running={running} near={near} visible={visible} cells={cells} />
      <td className="c-ports" data-l="Ports">
        {ports.slice(0, 3).map((p) =>
          p.url ? (
            <a key={p.label + p.proto} className="port" href={p.url} target="_blank" rel="noopener noreferrer" title={p.detail}>
              {p.label}
            </a>
          ) : p.local ? (
            <span key={p.label + p.proto} className="port local" title={`${p.detail} (loopback only)`}>
              {p.label} <small>local</small>
            </span>
          ) : (
            <span key={p.label + p.proto} className="port plain" title={`${p.detail} (not a web address)`}>
              {p.label} <small>{p.proto}</small>
            </span>
          ),
        )}
        {ports.length > 3 && (
          <a className="port more" href={href} title="More ports on the details page">
            +{ports.length - 3}
          </a>
        )}
      </td>
      <td className="c-act">
        <div className="ra">
          {<a className="btn sm" href={logsHref(r.name)} title="Logs (l)" aria-label={`Logs of ${r.title}`} data-testid="logs">
            {LOGS_ICON}
            Logs
          </a>}
          {open ? (
            <a className="ia" href={open} target="_blank" rel="noopener noreferrer" title="Open service" aria-label={`Open ${r.title}`} data-testid="open">
              {EXTERNAL_ICON}
            </a>
          ) : (
            <span className="ia" aria-hidden="true" />
          )}
        </div>
      </td>
    </tr>
  );
}, sameRow);

function ResourceCells({ name, running, near, visible, cells }: {
  name: string;
  running: boolean;
  near: boolean;
  visible: boolean;
  cells: LiveCells;
}) {
  const sub = useCallback((fn: () => void) => (visible && running ? cells.subscribe(name, fn) : unsubscribeNothing), [cells, name, visible, running]);
  const cell = useSyncExternalStore(sub, () => cells.get(name));
  const scale = useSyncExternalStore(near && visible && running ? cells.subscribeScale : noSub, cells.getScale);
  return <>
    <ResourceCell kind="cpu" value={running ? cell.cpu : null} history={running && near ? cell.cpuHist : null} max={scale.cpu} />
    <ResourceCell kind="mem" value={running ? cell.mem : null} history={running && near ? cell.memHist : null} max={scale.mem} />
  </>;
}

const ResourceCell = memo(function ResourceCell({ kind, value, history, max }: {
  kind: "cpu" | "mem";
  value: number | null;
  history: (number | null)[] | null;
  max: number;
}) {
  const cpu = kind === "cpu";
  return <td className={`c-num c-${kind}`} data-l={cpu ? "CPU" : "Memory"}>
    <span className={value === null ? "num muted" : "num"}>{value === null ? "—" : cpu ? pct(value) : bytes(value)}</span>
    {history && <Sparkline values={history} max={max} label="" color={cpu ? "--ch-cpu" : "--ch-mem"} className="cell-spark" />}
  </td>;
});

const unsubscribeNothing = () => {};
const noSub = () => unsubscribeNothing;
