import { Badge } from "../../ui/Badge.tsx";
import { PageHeader } from "../../ui/PageHeader.tsx";
import { LayersIcon, SearchIcon, ScrollTextIcon } from "lucide-react";
import { useRead } from "../../live/read.ts";
import { useEffect, useMemo, useRef, useState } from "react";
import type { LogsPicker } from "../../api/gen/LogsPicker.ts";
import { api } from "../../api/session.ts";
import { logsHref } from "../../app/paths.ts";
import { navigate, setSearch, useRoute } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { matcher, names, paneHref, permalink, searchState, type Row } from "./model.ts";
import { copyLogText, LogPane, type PaneStatus, type ViewOptions } from "./pane.ts";
import { ContainerContext } from "./ContainerContext.tsx";
import "./logs.css";

const savePref = (key: string, value: boolean) => { try { localStorage.setItem(`rh-logs-${key}`, value ? "1" : "0"); } catch { /* private browser */ } };

const initialStatus: PaneStatus = { state: "connecting", count: 0, matches: 0, current: 0, unseen: 0, following: true, waiting: 0, message: "", counts: {} };
const levelNames: Record<string, string> = { e: "Error", w: "Warning", i: "Info", d: "Debug", o: "Other" };
function stored(key: string): boolean { try { return localStorage.getItem(`rh-logs-${key}`) !== "0"; } catch { return true; } }
function Pane({ title, query, picker, options, range, target, close, stack }: {
  title: string; query: string; picker: LogsPicker; options: ViewOptions; range: string; target: number | null; close?: string; stack?: boolean;
}) {
  const box = useRef<HTMLDivElement>(null), controller = useRef<LogPane | null>(null), copy = useRef<HTMLButtonElement>(null);
  const [status, setStatus] = useState(initialStatus), [selected, setSelected] = useState<Row | null>(null), [copied, setCopied] = useState("");
  const project = picker.groups.find((g) => g.project && g.containers.some((c) => c.name === title));
  const opts = useRef(options);
  useEffect(() => { opts.current = options; }, [options]);
  useEffect(() => {
    if (!box.current) return;
    const pane = new LogPane(box.current, query, picker.ring, picker.tail, opts.current, range, target, setStatus, (r, focus) => {
      setSelected(r); if (focus) requestAnimationFrame(() => copy.current?.focus());
    });
    controller.current = pane;
    return () => { pane.close(); controller.current = null; };
  }, [query, picker.ring, picker.tail, range, target]);
  useEffect(() => controller.current?.update(options), [options]);
  useEffect(() => {
    const latest = () => controller.current?.latest();
    const step = (e: Event) => controller.current?.step((e as CustomEvent<number>).detail, e);
    window.addEventListener("rh:logs-latest", latest); window.addEventListener("rh:logs-step", step);
    return () => { window.removeEventListener("rh:logs-latest", latest); window.removeEventListener("rh:logs-step", step); };
  }, []);
  const copyText = async (link: boolean) => {
    if (!selected) return;
    const text = link ? `${location.origin}${location.pathname}${location.search}#t=${selected.t}` : selected.plain;
    try { await copyLogText(text); setCopied(link ? "Link copied" : "Line copied"); } catch { setCopied("Copy unavailable; select the line text to copy."); }
  };
  return <section className="log-pane" data-testid="log-pane" data-state={status.state}>
    <header className="log-pane-head"><strong>{stack ? title : <a href={logsHref(title)}>{title}</a>}</strong><Badge role="status" variant={status.state === "live" ? "success" : status.state === "paused" || status.state.includes("retrying") ? "warning" : status.state === "error" ? "destructive" : "secondary"}>{status.state}</Badge>{project && !stack && <a href={`/app/stacks/${encodeURIComponent(project.name)}/logs`}>Stack</a>}{close && <a href={close} aria-label={`Close ${title}`}>Close</a>}</header>
    {stack && <details className="log-members"><summary>Containers merged by time (up to 24)</summary>{picker.groups.find((g) => g.project && g.name === title)?.containers.toSorted((a, b) => a.name.localeCompare(b.name)).slice(0, 24).map((container) => <a key={container.name} href={logsHref(container.name)}>{container.name}</a>)}</details>}
    {status.message && <p className="log-note" role="status">{status.message}</p>}
    <div className="log-pane-tools"><Button size="sm" variant="ghost" onClick={() => controller.current?.loadOlder()} data-testid="log-older">Load older lines</Button>
      <span>{status.count.toLocaleString()} / {picker.ring.toLocaleString()} lines{options.paused && status.waiting > 0 ? ` · ${status.waiting} waiting` : ""}</span>
      {options.search && <><output data-testid="log-matches">{status.current}/{status.matches}</output><Button size="sm" variant="ghost" onClick={() => controller.current?.step(-1)} aria-label="Older match">↑</Button><Button size="sm" variant="ghost" onClick={() => controller.current?.step(1)} aria-label="Newer match">↓</Button></>}
    </div>
    {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- The log region handles line selection and End. */}
    <div ref={box} className="log-body" data-testid="log-body" tabIndex={0} role="region" aria-label={`Logs of ${title}`} />
    {!status.following && <Button className="log-jump" data-testid="log-latest" onClick={() => controller.current?.latest()}>Jump to latest · {status.unseen} new</Button>}
    {!status.count && status.state !== "connecting" && <p className="log-note">No lines in this range.</p>}
    {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Keyboard navigation is delegated from the line action buttons. */}
    {selected && <div className="log-actions" role="group" aria-label="Line actions" onKeyDown={(e) => {
      if (e.key === "Escape") { e.stopPropagation(); controller.current?.clearSelection(); box.current?.focus(); }
      if (e.key === "ArrowUp" || e.key === "ArrowDown") { e.preventDefault(); controller.current?.moveSelected(e.key === "ArrowUp" ? -1 : 1); }
    }}><Button size="sm" variant="ghost" ref={copy} onClick={() => void copyText(false)}>Copy line</Button><Button size="sm" variant="ghost" onClick={() => void copyText(true)}>Copy link</Button><output>{copied}</output></div>}
    <footer className="log-pane-foot">{Object.entries(levelNames).map(([l, label]) => <span key={l}>{label} {status.counts[l] ?? 0}</span>)}</footer>
  </section>;
}
function Picker({ data }: { data: LogsPicker }) {
  const route = useRoute(), q = route.search.get("q") ?? "", selected = names(route.search, "sel").slice(0, 4);
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  const groups = data.groups.map((g) => ({ ...g, containers: g.containers.filter((c) => terms.every((t) => `${g.title} ${c.title} ${c.name} ${c.search}`.toLowerCase().includes(t))) })).filter((g) => g.containers.length);
  const total = data.groups.reduce((n, g) => n + g.containers.length, 0), shown = groups.reduce((n, g) => n + g.containers.length, 0);
  return <div className="page logs-page lpick" data-testid="logs-picker">
    <PageHeader title="Logs" description="Choose a container, a stack, or up to four containers to view side by side." />
    <div className="logs-picker-toolbar">
      <div className="lpick-f" role="search">
        <div className="search-in">
          <SearchIcon aria-hidden="true" />
          <TextInput type="search" data-testid="filter" aria-label="Filter containers" placeholder="Filter by name, stack or image" value={q}
            onChange={(e) => setSearch({ q: e.target.value }, { replace: true })}
            onKeyDown={(e) => { if (e.key === "Enter" && groups[0]?.containers[0]) { e.preventDefault(); navigate(logsHref(groups[0].containers[0].name)); } }} />
        </div>
      </div>
      <output className="lpick-n muted">{shown === total ? total : shown + " of " + total} containers</output>
      <Button variant="default" leadingIcon={<ScrollTextIcon aria-hidden="true" />} disabled={selected.length < 2}
        onClick={() => setSearch({ c: selected.join(","), sel: null, q: null })}>
        Side by side{selected.length > 0 ? " · " + selected.length : ""}
      </Button>
    </div>
    {selected.length === 4 && <p className="muted lpick-hint">Four selected. Untick a container to choose another.</p>}
    {!shown && <p>No containers match.</p>}
    {groups.map((g) => <section className="lpick-g" key={g.name}>
      <h2 className="lpick-h">
        <span className="lpick-t" title={g.title}>{g.title}</span>
        <span className="cnt num">{g.containers.filter((c) => c.state === "running").length}/{g.containers.length}</span>
        {g.project && <a className="btn sm" href={"/app/stacks/" + encodeURIComponent(g.name) + "/logs"}><LayersIcon aria-hidden="true" /> Whole stack</a>}
      </h2>
      <ul className="lpick-list">{g.containers.map((c) => {
        const running = c.state === "running";
        return <li className={running ? "rl" : "rl stopped"} data-testid="logs-picker-row" key={c.name}>
          <Switch className="lpick-cb" checked={selected.includes(c.name)} disabled={selected.length >= 4 && !selected.includes(c.name)}
            label={"Select " + c.title} onChange={(on) => setSearch({ sel: (on ? [...selected, c.name] : selected.filter((n) => n !== c.name)).join(",") }, { replace: true })} />
          <span className={running ? "dot ok" : "dot off"} aria-hidden="true" />
          <a className="row-link" href={logsHref(c.name)} title={"Logs of " + c.name}>
            <span className="lpick-nm">{c.title}</span>{c.title !== c.name && <> <span className="sub mono">{c.name}</span></>}
          </a>
          {!running && <span className="muted lpick-st">{c.state}</span>}
        </li>;
      })}</ul>
    </section>)}
  </div>;
}
function LevelCounts({ name, range }: { name: string; range: string }) {
  const [open, setOpen] = useState(false);
  const { data, error } = useRead<{ points: Array<[number, number[]]> }>(open
    ? `/api/logs/levels?c=${encodeURIComponent(name)}&range=${encodeURIComponent(range)}` : null);
  const points = data?.points;
  const counts = points?.reduce((sum, p) => sum.map((n, i) => n + (p[1][i] ?? 0)), [0, 0, 0, 0, 0]);
  const maximum = Math.max(1, ...(points ?? []).map((p) => (p[1][0] ?? 0) + (p[1][1] ?? 0)));
  return <details className="log-level-history" onToggle={(e) => setOpen(e.currentTarget.open)}><summary>Errors and warnings, last {range}</summary>
    {error ? <p role="alert">{error}</p> : points?.length ? <figure aria-label={`${counts?.[0]} error and ${counts?.[1]} warning lines`}>
      <svg viewBox={`0 0 ${points.length} 40`} preserveAspectRatio="none" width="100%" height="40" aria-hidden="true">{points.map(([t, c], i) => {
        const e = (c[0] ?? 0) / maximum * 40, w = (c[1] ?? 0) / maximum * 40;
        return <g key={t}><rect x={i} y={40-e} width="0.8" height={e} fill="var(--bad)"><title>{new Date(t*1000).toLocaleString()}: {c[0]} errors, {c[1]} warnings</title></rect><rect x={i} y={40-e-w} width="0.8" height={w} fill="var(--warn)" /></g>;
      })}</svg><figcaption>{counts?.[0]} errors · {counts?.[1]} warnings</figcaption>
    </figure> : <p>{points ? <>No log lines counted in this range yet. Counts are recorded for containers watched by a log alert. <a href="/app/alerts">Add a log alert</a></> : "Loading counts…"}</p>}
  </details>;
}
export default function LogsPage() {
  const route = useRoute();
  const [picker, setPicker] = useState<LogsPicker | null>(null), [error, setError] = useState(""), [errorState, setErrorState] = useState("error");
  const [paneCounts, setPaneCounts] = useState<Record<string, Record<string, number>>>({});
  useEffect(() => {
    const count = (e: Event) => { const detail = (e as CustomEvent<{ query: string; counts: Record<string, number> | null }>).detail; setPaneCounts((previous) => { const next = { ...previous }; if (detail.counts) next[detail.query] = detail.counts; else delete next[detail.query]; return next; }); };
    window.addEventListener("rh:log-counts", count); return () => window.removeEventListener("rh:log-counts", count);
  }, []);
  useEffect(() => { const ac = new AbortController(); void api.get<LogsPicker>("/api/v1/logs/picker", { signal: ac.signal }).then((r) => { if (ac.signal.aborted) return; if (r.kind === "ok") setPicker(r.data); else { setError(r.message); setErrorState(r.kind === "sign_in" || r.kind === "forbidden" ? "no access" : "error"); } }); return () => ac.abort(); }, []);
  const [linked] = useState(() => searchState(route.search));
  const [regex, setRegex] = useState(false), [wrap, setWrap] = useState(() => stored("wrap")), [time, setTime] = useState(() => stored("time")), [paused, setPaused] = useState(false), [expanded, setExpanded] = useState(false);
  const search = route.search.get("search") ?? "", sensitive = route.search.get("case") === "1", show = route.search.get("show") ?? "all", levels = route.search.get("levels") ?? "ewido";
  const range = route.search.get("range") ?? (route.search.has("from") || route.search.has("to") ? "custom" : "");
  const [from, setFrom] = useState(""), [to, setTo] = useState(""), [previousRange, setPreviousRange] = useState(""), [rangeError, setRangeError] = useState(""), [rangeErrorField, setRangeErrorField] = useState("from");
  const since = route.search.get("since") ?? route.search.get("from"), until = route.search.get("until") ?? route.search.get("to");
  const closedRange = (since && Number.isFinite(Number(since)) ? `&since=${Number(since)}` : "") + (until && Number.isFinite(Number(until)) ? `&until=${Number(until)}` : "");
  const rangeQuery = range === "custom" ? closedRange || previousRange : range ? `&since=${encodeURIComponent(range)}` : "";
  const options = useMemo<ViewOptions>(() => ({ search, regex, sensitive, only: show !== "all", context: Number(show) || 0, levels, wrap, time, paused }), [search, regex, sensitive, show, levels, wrap, time, paused]);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      const typing = el.matches("input,textarea,select") || el.isContentEditable;
      if (e.ctrlKey || e.metaKey || e.altKey || document.querySelector('[role="dialog"][data-open]')) return;
      if (e.key === "/" && !typing && input.current) { e.preventDefault(); e.stopImmediatePropagation(); input.current?.focus(); }
      if (e.key === "End" && !typing && input.current) { e.preventDefault(); window.dispatchEvent(new Event("rh:logs-latest")); }
    };
    document.addEventListener("keydown", key, true); return () => document.removeEventListener("keydown", key, true);
  }, []);
  if (error) return <div className="page" data-testid="log-pane" data-state={errorState} role="alert">{error}</div>;
  if (!picker) return <div className="page" aria-busy="true">Loading logs…</div>;
  const asked = route.route.name === "container_logs" ? [route.route.ctr] : names(route.search, "c");
  const stack = route.route.name === "stack_logs" ? route.route.stack : null;
  if (!asked.length && stack === null) return <Picker data={picker} />;
  const available = new Set(picker.groups.flatMap((g) => g.containers.map((c) => c.name)));
  const missing = asked.filter((n, i) => i >= 4 || !available.has(n));
  const panes = asked.slice(0, 4).filter((n) => available.has(n));
  const stackExists = picker.groups.some((g) => g.name === stack && g.project);
  const download = new URLSearchParams(stack ? { stack } : { c: panes.join(",") });
  if (!range) download.set("tail", String(picker.ring));
  const filters = [..."ewido"].filter((l) => !levels.includes(l)).length + Number(show !== "all") + Number(!!range);
  return <div className="page logs-page" data-testid="logs-viewer"><nav className="crumb" aria-label="Breadcrumb"><a href="/app/logs">Logs</a></nav>
    {route.route.name === "container_logs" ? <ContainerContext name={route.route.ctr} query={route.query} /> :
      <PageHeader title={stack ?? (panes.length === 1 ? panes[0] : "Side by side")} />}
    {(missing.length > 0 || (stack !== null && !stackExists)) && <p className="banner warn" role="alert">Not found: {missing.join(", ") || stack}</p>}
    <div className="log-toolbar"><div className="log-search"><TextInput ref={input} type="search" data-testid="filter" aria-label="Search logs" aria-invalid={!!matcher(search, regex, sensitive).error} title={matcher(search, regex, sensitive).error ?? undefined} placeholder="Search logs" value={search} onChange={(e) => setSearch({ search: e.target.value }, { replace: true })} onKeyDown={(e) => {
      if (e.key === "Enter") { e.preventDefault(); window.dispatchEvent(new CustomEvent("rh:logs-step", { detail: e.shiftKey ? 1 : -1 })); }
      if (e.key === "Escape") { e.stopPropagation(); setSearch({ search: null }, { replace: true }); }
    }} /><Button size="sm" variant="ghost" aria-label="Regular expression" aria-pressed={regex} onClick={() => { setRegex(!regex); setSearch({ re: !regex ? "1" : null }, { replace: true }); }}>.*</Button><Button size="sm" variant="ghost" aria-label="Match case" aria-pressed={sensitive} onClick={() => setSearch({ case: sensitive ? null : "1" }, { replace: true })}>Aa</Button></div>
    <Button className="log-filters" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>Filters {filters || ""}</Button>
    <div className={`log-filter-controls ${expanded ? "is-open" : ""}`}>
      <Select label="Show" value={show} onChange={(v) => setSearch({ show: v === "all" ? null : v }, { replace: true })} options={[{ value: "all", label: "All lines" }, { value: "0", label: "Only matching" }, ...[1, 3, 10].map((n) => ({ value: String(n), label: `Matching ±${n} lines` }))]} />
      <div className="log-levels seg" role="group" aria-label="Log levels">{Object.entries(levelNames).map(([l, label]) => <Button key={l} size="sm" variant="ghost" aria-label={`Show ${label.toLowerCase()} lines`} aria-pressed={levels.includes(l)} onClick={() => setSearch({ levels: levels.includes(l) ? levels.replace(l, "") || "-" : levels.replace("-", "") + l }, { replace: true })}>{label}<span className="log-level-count">{Object.values(paneCounts).reduce((total, counts) => total + (counts[l] ?? 0), 0)}</span></Button>)}</div>
      <Select label="Range" value={range} onChange={(v) => { if (v === "custom") setPreviousRange(rangeQuery); setSearch({ range: v, since: null, until: null, from: null, to: null }, { replace: true }); }} options={[{ value: "", label: "Live" }, { value: "15m", label: "15 min" }, { value: "1h", label: "1 h" }, { value: "6h", label: "6 h" }, { value: "24h", label: "24 h" }, { value: "start", label: "Since start" }, { value: "custom", label: "Custom" }]} />
      <div className="log-view-options">
      <Button size="sm" variant="ghost" aria-pressed={wrap} onClick={() => { setWrap(!wrap); savePref("wrap", !wrap); }}>Wrap</Button><Button size="sm" variant="ghost" aria-pressed={time} onClick={() => { setTime(!time); savePref("time", !time); }}>Time</Button><Button size="sm" variant="ghost" aria-pressed={paused} onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</Button>
      <a className="btn btn-sm btn-link" href={`/api/logs/download?${download}${rangeQuery}`} download title="Download the entire selected range without search or level filters">Download</a>
      </div>
    </div></div>
    {linked.regexFromLink && !regex && <p className="log-note">Search from the link is plain text. Turn on .* to use it as a regular expression.</p>}
    {range === "custom" && <form className="log-custom" onSubmit={(e) => { e.preventDefault(); const f = from ? new Date(from).getTime() / 1000 : null, t = to ? new Date(to).getTime() / 1000 : null;
      if (!f && !t) { setRangeError("Choose a From or To time."); setRangeErrorField("from"); document.getElementById("logs-from")?.focus(); return; }
      if (f && f > Date.now() / 1000) { setRangeError("From is in the future."); setRangeErrorField("from"); document.getElementById("logs-from")?.focus(); return; }
      if (f && t && t <= f) { setRangeError("To must be later than From."); setRangeErrorField("to"); document.getElementById("logs-to")?.focus(); return; }
      setRangeError(""); setSearch({ since: f ? String(f) : null, until: t ? String(t) : null }, { replace: true });
    }}><Field id="logs-from" label="From" error={rangeErrorField === "from" ? rangeError : ""}><TextInput type="datetime-local" value={from} onChange={(e) => setFrom(e.target.value)} /></Field><Field id="logs-to" label="To" error={rangeErrorField === "to" ? rangeError : ""}><TextInput type="datetime-local" value={to} onChange={(e) => setTo(e.target.value)} /></Field><Button type="submit">Apply</Button></form>}
    <div className="log-panes" style={{ gridTemplateColumns: `repeat(${stack ? 1 : Math.max(1, panes.length)}, minmax(0, 1fr))` }}>
      {stack && stackExists ? <Pane key={`stack:${stack}`} title={stack} stack query={`stack=${encodeURIComponent(stack)}`} picker={picker} options={options} range={rangeQuery} target={permalink(route.hash)} /> : panes.map((name) => <Pane key={name} title={name} query={`c=${encodeURIComponent(name)}`} picker={picker} options={options} range={rangeQuery} target={permalink(route.hash)} close={route.route.name === "logs" ? paneHref(route.query, panes.filter((n) => n !== name), route.hash) : undefined} />)}
    </div>
    {route.route.name === "container_logs" && <LevelCounts name={route.route.ctr} range={route.search.get("range") || "6h"} />}
    {route.route.name === "logs" && <a href={paneHref(route.query, panes, route.hash, true)}>Add a pane</a>}
  </div>;
}
