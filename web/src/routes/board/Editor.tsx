import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type KeyboardEvent } from "react";
import { ArrowDownIcon, ArrowUpIcon, BoxIcon, ChevronDownIcon, GripVerticalIcon, LayoutDashboardIcon, Link2Icon, PaletteIcon, PlugIcon, PlusIcon, Redo2Icon, SlidersHorizontalIcon, Trash2Icon, Undo2Icon } from "lucide-react";
import type { BoardPayload } from "../../api/gen/BoardPayload.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { BoardSuggestion } from "../../api/gen/BoardSuggestion.ts";
import { api } from "../../api/session.ts";
import { dirty, freshSuggestions, itemCount, MAX_SECTIONS, minSize, payloadIdentity, rebase, rectsOf, recoveryBase, reduceDocument, type Action } from "../../boards/document.ts";
import { COLS, compact, moveTo, nudge, readingOrder, resizeTo, rows, shift, stretch, type Rect, type Step } from "../../boards/grid.ts";
import { navigate } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Note } from "../../ui/Note.tsx";
import { LinkTest } from "../../boards/LinkTest.tsx";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { CardButton } from "../../ui/CardButton.tsx";
import { useBootstrap, useContainers } from "../../live/index.ts";
import { Tile } from "./Tile.tsx";
import { BoardPanel, TilePanel } from "./Inspector.tsx";
import { canvasVars, cellVars } from "./look.ts";

type Kind = "all" | BoardPayload["kind"];
const KIND_LABEL: Record<BoardPayload["kind"], string> = { container: "Container", widget: "Widget", integration: "Integration", link: "Link" };
const KIND_ICON = { container: BoxIcon, widget: LayoutDashboardIcon, integration: PlugIcon, link: Link2Icon };
const LONG_PRESS = 350;
/** Below this canvas width the board is one column: editing is reorder only. */
const NARROW = 560;
const ZERO: Rect = { x: 0, y: 0, w: 1, h: 1 };
const where = (r: Rect) => `column ${r.x + 1}, row ${r.y + 1}`;

type History = { past: BoardDocument[]; doc: BoardDocument; future: BoardDocument[] };
type Drag = {
  mode: "move" | "resize"; s: number; i: number; origin: Rect;
  /** The section under the pointer and its layout with the tile in it
   * (for another section, the tile is the last rectangle). */
  target: number; preview: Rect[]; placeholder: Rect;
  /** The source section without the tile, when it is moving elsewhere. */
  source: Rect[] | null;
  dx: number; dy: number;
};
type Panel = { kind: "tile"; s: number; i: number } | { kind: "board" } | null;

function metrics(canvas: HTMLElement) {
  const box = canvas.getBoundingClientRect();
  const style = getComputedStyle(canvas);
  const gap = parseFloat(style.getPropertyValue("--cv-gap")) || 12;
  const row = parseFloat(style.getPropertyValue("--cv-row")) || 80;
  return { box, col: (box.width + gap) / COLS, row: row + gap };
}

export default function Editor({ view, initial, initialPicker, scoped, changed, onSave, onCancel, onRebase }: {
  view: BoardView; initial: BoardDocument; initialPicker?: "add" | "discover"; scoped: boolean; changed: boolean; onSave: (view: BoardView) => void; onCancel: () => void; onRebase: () => void;
}) {
  const [base, setBase] = useState(() => initial.version === view.document.version ? view.document : recoveryBase(initial, view.document));
  const [history, setHistory] = useState<History>({ past: [], doc: initial, future: [] });
  const doc = history.doc;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [conflict, setConflict] = useState(initial.version !== view.document.version);
  const [draftState, setDraftState] = useState("Draft ready");
  const [announcement, setAnnouncement] = useState("");
  const [picker, setPicker] = useState<number | null>(initialPicker ? 0 : null);
  const [suggestions, setSuggestions] = useState<BoardSuggestion[]>([]);
  const [query, setQuery] = useState("");
  const [link, setLink] = useState("");
  const [pendingHref, setPendingHref] = useState<string | null>(null);
  const [panel, setPanel] = useState<Panel>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [added, setAdded] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingDraft = useRef<Promise<boolean>>(Promise.resolve(true));
  const latest = useRef(doc);
  const leaving = useRef(false);
  const operation = useRef(false);
  const coalesce = useRef<{ key: string; at: number } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const linkTitle = useRef<HTMLInputElement>(null);
  const [manual, setManual] = useState(initialPicker === "add");
  const search = useRef<HTMLInputElement>(null);
  const [kind, setKind] = useState<Kind>("all");
  const boot = useBootstrap();
  const live = useContainers();
  useEffect(() => { if (!initialPicker) root.current?.querySelector<HTMLButtonElement>(".editor-fields button")?.focus({ preventScroll: true }); }, [initialPicker]);
  const path = `/api/v1/boards/${view.board.id}`;
  const modified = dirty(base, doc);
  const name = doc.name ?? view.board.name;

  /** Apply an edit as one undo step. Typing in one field within a moment
   * of the last keystroke extends that step instead of adding one. */
  const change = (action: Action, message: string, merge?: string) => {
    if (operation.current) return;
    const now = Date.now();
    const extend = !!merge && coalesce.current?.key === merge && now - coalesce.current.at < 1500;
    coalesce.current = merge ? { key: merge, at: now } : null;
    setHistory((h) => ({ past: extend ? h.past : [...h.past.slice(-99), h.doc], doc: reduceDocument(h.doc, action), future: [] }));
    setAnnouncement(message); setDraftState("Draft pending");
  };
  const undo = () => { coalesce.current = null; setHistory((h) => { const prev = h.past.at(-1); return prev ? { past: h.past.slice(0, -1), doc: prev, future: [h.doc, ...h.future] } : h; }); setAnnouncement("Undone"); setDraftState("Draft pending"); };
  const redo = () => { coalesce.current = null; setHistory((h) => { const next = h.future[0]; return next ? { past: [...h.past, h.doc], doc: next, future: h.future.slice(1) } : h; }); setAnnouncement("Redone"); setDraftState("Draft pending"); };
  const focusItem = (section: number, item: number) => requestAnimationFrame(() => {
    const button = root.current?.querySelector<HTMLButtonElement>(`[data-section="${section}"][data-item="${item}"] .drag-handle`)
      ?? root.current?.querySelector<HTMLButtonElement>(`section[data-section="${section}"] [data-testid="board-add-item"]`)
      ?? root.current?.querySelector<HTMLButtonElement>('[data-testid="board-add-section"]');
    button?.focus({ preventScroll: true });
    button?.closest(".canvas-item")?.scrollIntoView?.({ block: "nearest", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  });
  const saveDraft = (document: BoardDocument) => {
    pendingDraft.current = pendingDraft.current.then(async () => {
      const result = await api.request("PUT", `${path}/draft`, { body: document });
      if (leaving.current) return false;
      if (result.kind === "ok") { if (latest.current === document) setDraftState("Draft saved"); }
      else if (result.kind === "stale") { setConflict(true); setDraftState("Draft needs a newer board version"); }
      else setDraftState(`Draft could not be saved: ${result.message}`);
      return result.kind === "ok";
    });
    return pendingDraft.current;
  };
  useEffect(() => {
    latest.current = doc;
    if (conflict || busy) return;
    timer.current = setTimeout(() => { timer.current = null; void saveDraft(doc); }, 600);
    return () => { if (timer.current) clearTimeout(timer.current); timer.current = null; };
    // saveDraft only closes over this mounted board's immutable path.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc, conflict, busy]);
  useEffect(() => {
    const href = window.location.href;
    const click = (e: MouseEvent) => {
      if (leaving.current || !modified || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey) return;
      const anchor = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>("a[href]") : null;
      if (!anchor || anchor.target === "_blank" || anchor.href === href) return;
      e.preventDefault(); e.stopPropagation(); if (operation.current) { setAnnouncement("Wait for the current operation to finish."); return; } setPendingHref(anchor.href);
    };
    const navigation = (e: Event) => {
      if (leaving.current || !modified) return;
      e.preventDefault(); if (operation.current) { setAnnouncement("Wait for the current operation to finish."); return; } setPendingHref((e as CustomEvent<string>).detail);
    };
    const unload = (e: BeforeUnloadEvent) => { if (modified && !leaving.current) { e.preventDefault(); e.returnValue = ""; } };
    document.addEventListener("click", click, true); window.addEventListener("beforeunload", unload); window.addEventListener("rh:navigate", navigation);
    return () => { document.removeEventListener("click", click, true); window.removeEventListener("beforeunload", unload); window.removeEventListener("rh:navigate", navigation); };
  }, [modified]);
  useEffect(() => {
    // Undo and redo for the edit session; text fields keep their own.
    const keys = (e: globalThis.KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const target = e.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable]") || target?.closest('[role="dialog"]')) return;
      const k = e.key.toLowerCase();
      if (k === "z" && !e.shiftKey) { e.preventDefault(); undo(); }
      else if ((k === "z" && e.shiftKey) || k === "y") { e.preventDefault(); redo(); }
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, []);
  const settle = async () => { if (timer.current) clearTimeout(timer.current); timer.current = null; await pendingDraft.current; };
  const cancel = async () => {
    if (operation.current) return; operation.current = true; setBusy(true); await settle();
    const result = await api.request("DELETE", `${path}/draft`);
    if (result.kind === "ok") { leaving.current = true; onCancel(); } else { setError(result.message); operation.current = false; setBusy(false); }
  };
  const save = async () => {
    if (operation.current || conflict) return; operation.current = true; setBusy(true); setError(""); await settle();
    const result = await api.request<BoardView>("PUT", path, { body: latest.current });
    if (result.kind === "ok") { leaving.current = true; onSave(result.data); }
    else { if (result.kind === "stale") setConflict(true); setError(result.message); operation.current = false; setBusy(false); }
  };

  const titleOf = (s: number, i: number) => {
    const item = doc.sections[s]?.items[i];
    if (!item) return "Tile";
    return item.payload.kind === "link" ? item.payload.title : item.payload.title ?? view.resolved.find((r) => r.id === item.id)?.container?.title ?? view.resolved.find((r) => r.id === item.id)?.widget?.title ?? KIND_LABEL[item.payload.kind];
  };
  const minOf = (s: number, i: number) => {
    const item = doc.sections[s]?.items[i];
    return minSize(item?.payload.kind ?? "link", view.resolved.find((r) => r.id === item?.id)?.widget);
  };
  const commitLayout = (s: number, rects: Rect[], message: string) => change({ type: "layout", section: s, rects }, message);

  // ---- keyboard: arrows move, Shift+arrows resize (Alt+arrows still move) ----
  const key = (event: KeyboardEvent, s: number, i: number) => {
    if (!event.key.startsWith("Arrow") || event.ctrlKey || event.metaKey) return;
    event.preventDefault(); event.stopPropagation();
    const rects = rectsOf(doc.sections[s]); const label = titleOf(s, i);
    if (event.shiftKey) {
      const next = stretch(rects, i, event.key as Step, minOf(s, i)); const r = next[i] ?? ZERO;
      commitLayout(s, next, `${label} resized to ${r.w} by ${r.h}`);
    } else {
      const next = nudge(rects, i, event.key as Step); const r = next[i] ?? ZERO;
      commitLayout(s, next, `${label} moved to ${where(r)}`);
    }
  };
  const reorder = (s: number, i: number, by: -1 | 1) => {
    const next = shift(rectsOf(doc.sections[s]), i, by);
    const place = readingOrder(next).indexOf(i) + 1;
    commitLayout(s, next, `${titleOf(s, i)} moved to position ${place}`); focusItem(s, i);
  };

  // ---- pointer drag and resize ----
  const begin = (event: ReactPointerEvent<HTMLElement>, s: number, i: number, mode: Drag["mode"]) => {
    if (busy || event.button !== 0 || drag) return;
    const section = doc.sections[s]; const origin = rectsOf(section)[i];
    const cell = event.currentTarget.closest<HTMLElement>(".canvas-item");
    const canvas = cell?.closest<HTMLElement>("[data-canvas]");
    if (!section || !origin || !cell || !canvas || canvas.clientWidth < NARROW) return;
    const touch = event.pointerType === "touch";
    if (mode === "resize" || !touch) event.preventDefault();
    const box = cell.getBoundingClientRect();
    const x0 = event.clientX; const y0 = event.clientY;
    const grab = { x: x0 - box.left, y: y0 - box.top };
    const pointer = event.pointerId;
    let started = false; let frame = 0; let current: Drag | null = null; let last = { x: x0, y: y0 };
    let press: ReturnType<typeof setTimeout> | null = null;
    const block = (e: TouchEvent) => { if (started) e.preventDefault(); };
    const compute = (cx: number, cy: number): Drag | null => {
      if (mode === "resize") {
        const { col, row } = metrics(canvas);
        const w = Math.min(COLS - origin.x, Math.round(origin.w + (cx - x0) / col));
        const h = Math.round(origin.h + (cy - y0) / row);
        const preview = resizeTo(rectsOf(section), i, w, h, minOf(s, i));
        return { mode, s, i, origin, target: s, preview, placeholder: preview[i] ?? origin, source: null, dx: 0, dy: 0 };
      }
      const under = document.elementsFromPoint(cx, cy).map((el) => el.closest<HTMLElement>("[data-canvas]")).find((el) => el && el !== null);
      const target = under ? Number(under.dataset.canvas) : current?.target ?? s;
      const targetCanvas = under ?? root.current?.querySelector<HTMLElement>(`[data-canvas="${target}"]`) ?? canvas;
      const { box: area, col, row } = metrics(targetCanvas);
      const x = Math.round((cx - grab.x - area.left) / col);
      const y = Math.max(0, Math.round((cy - grab.y - area.top) / row));
      const dx = cx - x0; const dy = cy - y0;
      if (target === s) {
        const preview = moveTo(rectsOf(section), i, x, y);
        return { mode, s, i, origin, target, preview, placeholder: preview[i] ?? origin, source: null, dx, dy };
      }
      const into = [...rectsOf(doc.sections[target]), { ...origin, x, y }];
      const preview = moveTo(into, into.length - 1, x, y);
      const rest = rectsOf(section).filter((_, j) => j !== i);
      const packed = compact(rest);
      const source = rectsOf(section).map((r, j) => j === i ? r : packed[j < i ? j : j - 1] ?? r);
      return { mode, s, i, origin, target, preview, placeholder: preview.at(-1) ?? origin, source, dx, dy };
    };
    const update = (cx: number, cy: number) => {
      last = { x: cx, y: cy };
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => { current = compute(cx, cy); setDrag(current); });
    };
    const start = (cx: number, cy: number) => {
      started = true;
      cell.setPointerCapture?.(pointer);
      if (touch) navigator.vibrate?.(8);
      setAnnouncement(mode === "move" ? `Moving ${titleOf(s, i)}. Release to drop, Escape to cancel.` : `Resizing ${titleOf(s, i)}.`);
      current = compute(cx, cy); setDrag(current);
    };
    const finish = (drop: boolean) => {
      if (press) clearTimeout(press);
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); window.removeEventListener("pointercancel", abort);
      window.removeEventListener("keydown", escape, true); document.removeEventListener("touchmove", block);
      // The last move may still be waiting for its frame: place the drop
      // where the pointer was released.
      const result = drop && started ? compute(last.x, last.y) : current; setDrag(null);
      if (!drop || !started || !result) { if (started) setAnnouncement("Move cancelled"); return; }
      const label = titleOf(s, i);
      if (result.mode === "resize") commitLayout(s, result.preview, `${label} resized to ${result.placeholder.w} by ${result.placeholder.h}`);
      else if (result.target === s) commitLayout(s, result.preview, `${label} moved to ${where(result.placeholder)}`);
      else {
        change({ type: "transfer", section: s, item: i, toSection: result.target, x: result.placeholder.x, y: result.placeholder.y }, `${label} moved to ${doc.sections[result.target]?.title ?? "another section"}, ${where(result.placeholder)}`);
        focusItem(result.target, doc.sections[result.target]?.items.length ?? 0);
      }
    };
    const move = (e: PointerEvent) => {
      if (e.pointerId !== pointer) return;
      const far = Math.hypot(e.clientX - x0, e.clientY - y0);
      if (!started) {
        if (touch) { if (far > 8) finish(false); return; }
        if (mode === "move" && far < 4) return;
        start(e.clientX, e.clientY); return;
      }
      update(e.clientX, e.clientY);
    };
    const up = (e: PointerEvent) => { if (e.pointerId !== pointer) return; if (started) last = { x: e.clientX, y: e.clientY }; finish(true); };
    const abort = (e: PointerEvent) => { if (e.pointerId === pointer) finish(false); };
    const escape = (e: globalThis.KeyboardEvent) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); } };
    window.addEventListener("pointermove", move); window.addEventListener("pointerup", up); window.addEventListener("pointercancel", abort);
    window.addEventListener("keydown", escape, true); document.addEventListener("touchmove", block, { passive: false });
    if (mode === "resize") start(x0, y0);
    else if (touch) press = setTimeout(() => start(x0, y0), LONG_PRESS);
  };
  const grabTile = (event: ReactPointerEvent<HTMLElement>, s: number, i: number) => {
    if ((event.target as Element).closest("button:not(.drag-handle), a, input, .resize-handle")) return;
    begin(event, s, i, "move");
  };

  // ---- picker ----
  const openPicker = (section: number) => { setManual(false); setPicker(section); setQuery(""); setLink(""); setKind("all"); };
  const addWhere = () => panel?.kind === "tile" ? panel.s : 0;
  const addFromBar = () => { if (!doc.sections.length) change({ type: "section", title: "Services" }, "Section added"); openPicker(doc.sections.length ? addWhere() : 0); };
  useEffect(() => {
    if (picker === null) return;
    const controller = new AbortController();
    void api.get<BoardSuggestion[]>(`${path}/suggestions`, { signal: controller.signal }).then((result) => {
      if (controller.signal.aborted) return;
      if (result.kind === "ok") setSuggestions(result.data);
      else setError(result.message);
    });
    return () => controller.abort();
  }, [path, picker]);
  const selectedTargets = new Set(doc.sections.flatMap(({ items }) => items.map(({ payload }) => payloadIdentity(payload))));
  const fresh = freshSuggestions(suggestions, selectedTargets);
  const matching = fresh.filter((suggestion) => suggestion.title.toLowerCase().includes(query.trim().toLowerCase()));
  const shown = matching.filter((suggestion) => kind === "all" || suggestion.payload.kind === kind);
  const kinds = (["container", "widget", "integration"] as const).filter((k) => fresh.some((suggestion) => suggestion.payload.kind === k));
  const liveRows = new Map(live.data?.groups.flatMap((group) => group.containers).map((row) => [row.name, row]) ?? []);
  const suggestionRow = (payload: BoardPayload) => payload.kind === "container" ? liveRows.get(payload.target.by === "name" ? payload.target.name : "") ?? [...liveRows.values()].find((row) => payload.target.by === "compose" && row.stack === payload.target.project && row.title === payload.target.service) : undefined;
  const add = (payload: BoardPayload, message: string) => {
    if (picker === null) return;
    const at = doc.sections[picker]?.items.length ?? 0;
    change({ type: "add", section: picker, payload }, message);
    setAdded(`${picker}:${at}`); focusItem(picker, at); setPicker(null);
  };
  useEffect(() => { if (!added) return; const t = setTimeout(() => setAdded(null), 900); return () => clearTimeout(t); }, [added]);

  const tilePanel = panel?.kind === "tile" ? panel : null;
  const panelItem = tilePanel ? doc.sections[tilePanel.s]?.items[tilePanel.i] : undefined;
  useEffect(() => { if (panel?.kind === "tile" && !panelItem) setPanel(null); }, [panel, panelItem]);
  const closePanel = () => {
    const was = panel; setPanel(null);
    requestAnimationFrame(() => {
      if (was?.kind === "tile") focusItem(was.s, was.i);
      else root.current?.querySelector<HTMLButtonElement>('[data-testid="board-style"]')?.focus();
    });
  };

  return <div ref={root} className="board-editor" data-testid="board-editor" data-dragging={drag ? drag.mode : undefined}>
    <div className="editor-bar">
      <div className="editor-bar-copy"><h1>Editing {name}</h1>
        <p role="status" data-testid="draft-status">{draftState} · {modified ? "Unsaved changes" : "No changes"}</p></div>
      <div className="editor-bar-tools" role="toolbar" aria-label="Board editing">
        <Button variant="ghost" size="icon" aria-label="Undo" title="Undo (Ctrl+Z)" data-testid="board-undo" disabled={!history.past.length || busy} onClick={undo}><Undo2Icon aria-hidden="true" /></Button>
        <Button variant="ghost" size="icon" aria-label="Redo" title="Redo (Ctrl+Shift+Z)" data-testid="board-redo" disabled={!history.future.length || busy} onClick={redo}><Redo2Icon aria-hidden="true" /></Button>
        <span className="editor-bar-sep" aria-hidden="true" />
        <Button variant="ghost" data-testid="board-style" aria-pressed={panel?.kind === "board"} icon={<PaletteIcon aria-hidden="true" />} aria-label="Board style" title="Board style" onClick={() => setPanel(panel?.kind === "board" ? null : { kind: "board" })}><span className="bar-label">Style</span></Button>
        <Button variant="ghost" data-testid="board-add-tile" icon={<PlusIcon aria-hidden="true" />} disabled={busy || itemCount(doc) >= 512} aria-label="Add a tile" title="Add a tile" onClick={addFromBar}><span className="bar-label">Add</span></Button>
      </div>
      <div className="page-actions"><Button variant="outline" busy={busy} data-testid="board-cancel" onClick={() => { void cancel(); }}>Cancel</Button>
        <Button variant="default" busy={busy} disabled={conflict} data-testid="board-save" onClick={() => { void save(); }}>Save</Button></div>
    </div>
    <p className="editor-hint"><span className="hint-wide">Drag a tile anywhere; drag its corner to resize (on touch, press and hold first). On a focused <GripVerticalIcon aria-label="grip" width={14} height={14} />, arrows move and <kbd>Shift</kbd> + arrows resize.</span>
      <span className="hint-phone">Reorder tiles with the arrows on each one. Resize and place them freely on a wider screen.</span></p>
    {(changed || conflict) && <Note kind="warn" testId="board-conflict">Another session saved this board. Your changes are kept here.
      {initial.version !== view.document.version && <p>A recovered draft keeps server additions. Review it before saving; you may need to apply removals again.</p>}
      <Button data-testid="board-rebase" disabled={busy} onClick={async () => { if (operation.current) return; operation.current = true; setBusy(true); await settle(); const result = await api.get<BoardView>(path); if (result.kind !== "ok") { setError(result.message); operation.current = false; setBusy(false); return; }
        const next = rebase(base, doc, result.data.document); setBase(result.data.document); setHistory({ past: [], doc: next, future: [] }); setConflict(false); onRebase(); setError(""); setAnnouncement("Changes rebased. Review them and choose Save."); operation.current = false; setBusy(false);
      }}>Reload and keep my changes</Button><Button onClick={() => { void cancel(); }}>Discard mine</Button>
    </Note>}
    {error && <Note kind="bad">{error}</Note>}<output className="sr" aria-live="polite">{announcement}</output>
    <div className="editor-body" data-panel={panel ? panel.kind : undefined}>
    <fieldset className="editor-fields" disabled={busy}><legend className="sr">Board layout</legend>
    {doc.sections.length === 0 && <div className="canvas-empty board-start"><LayoutDashboardIcon aria-hidden="true" /><strong>Start with a section</strong><span>Sections group tiles under a title. Add one, then drag widgets into it.</span>
      <Button variant="default" icon={<PlusIcon aria-hidden="true" />} onClick={addFromBar}>Add a tile</Button></div>}
    {doc.sections.map((section, s) => {
      const saved = rectsOf(section);
      let rects = saved; let placeholder: Rect | null = null;
      if (drag?.mode === "resize" && drag.s === s) rects = drag.preview;
      if (drag?.mode === "move") {
        if (drag.target === s) { rects = drag.s === s ? drag.preview : drag.preview.slice(0, -1); placeholder = drag.placeholder; }
        else if (drag.s === s && drag.source) rects = drag.source;
      }
      const height = Math.max(rows(rects), placeholder ? placeholder.y + placeholder.h : 0) + (drag ? 2 : 1);
      const order = readingOrder(saved);
      return <section key={section.id ?? `section-${s}`} data-section={s} className="board-section editor-section" data-collapsed={section.collapsed ? "" : undefined} data-drop={drag?.mode === "move" && drag.target === s ? "" : undefined}>
      <div className="editor-section-head"><h2 className="sr">{section.title}</h2>
        <Button variant="ghost" size="icon" className="section-toggle" aria-expanded={!section.collapsed} aria-label={section.collapsed ? `Expand ${section.title}` : `Collapse ${section.title}`} title={section.collapsed ? "Expand" : "Collapse"}
          onClick={() => change({ type: "collapse", section: s, collapsed: !section.collapsed }, section.collapsed ? "Section expanded" : "Section collapsed")}><ChevronDownIcon aria-hidden="true" className="section-chevron" /></Button>
        <div className="section-title-input"><TextInput id={`section-${s}`} aria-label="Section title" value={section.title} required maxLength={80} onChange={(e) => change({ type: "rename", section: s, title: e.target.value }, "Section renamed", `rename:${s}`)} /></div>
        <span className="section-count" aria-label={`${section.items.length} items`}>{section.items.length}</span>
        <div className="editor-section-tools">
          <Button variant="ghost" size="sm" data-testid="board-add-item" icon={<PlusIcon aria-hidden="true" />} onClick={() => openPicker(s)}>Add item</Button>
          <Button variant="ghost" size="icon" aria-label="Move section up" title="Move section up" disabled={s === 0} onClick={() => change({ type: "move-section", section: s, to: s - 1 }, "Section moved up")}><ArrowUpIcon aria-hidden="true" /></Button>
          <Button variant="ghost" size="icon" aria-label="Move section down" title="Move section down" disabled={s === doc.sections.length - 1} onClick={() => change({ type: "move-section", section: s, to: s + 1 }, "Section moved down")}><ArrowDownIcon aria-hidden="true" /></Button>
          <Button variant="ghost" size="icon" aria-label={`Delete section ${section.title}`} title="Delete section" onClick={() => { change({ type: "delete-section", section: s }, "Section removed"); focusItem(Math.min(s, doc.sections.length - 2), 0); }}><Trash2Icon aria-hidden="true" /></Button>
        </div>
      </div>
      {!section.collapsed && <div className="canvas editing" data-canvas={s} style={canvasVars(height)}>
        {section.items.length === 0 && !placeholder && <div className="canvas-empty"><LayoutDashboardIcon aria-hidden="true" /><strong>Drag widgets here</strong><span>or add one from what Hobnail found.</span>
          <Button size="sm" variant="outline" icon={<PlusIcon aria-hidden="true" />} onClick={() => openPicker(s)}>Add</Button></div>}
        {placeholder && <div className="canvas-placeholder" style={cellVars(placeholder)} aria-hidden="true"><span>{placeholder.w} × {placeholder.h}</span></div>}
        {order.map((i) => {
          const item = section.items[i]; const at = saved[i];
          if (!item || !at) return null;
          const label = titleOf(s, i);
          const moving = drag?.mode === "move" && drag.s === s && drag.i === i;
          const resizing = drag?.mode === "resize" && drag.s === s && drag.i === i;
          const rect = moving ? at : rects[i] ?? at;
          const style = moving ? { ...cellVars(rect), "--dx": `${drag.dx}px`, "--dy": `${drag.dy}px` } : cellVars(rect);
          const selected = tilePanel?.s === s && tilePanel.i === i;
          return <div key={item.id ?? `item-${i}`} className="canvas-item editable" data-section={s} data-item={i} style={style}
            data-moving={moving ? "" : undefined} data-resizing={resizing ? "" : undefined} data-selected={selected ? "" : undefined} data-added={added === `${s}:${i}` ? "" : undefined}
            onPointerDown={(e) => grabTile(e, s, i)} onContextMenu={(e) => { if (drag) e.preventDefault(); }}>
            <Tile item={item} resolved={view.resolved.find((r) => r.id === item.id)} />
            <Button variant="ghost" size="icon" className="drag-handle" onKeyDown={(e) => key(e, s, i)} onPointerDown={(e) => { e.stopPropagation(); begin(e, s, i, "move"); }}
              aria-label={`Move ${label}`} aria-describedby="board-move-help" title="Drag to move; arrows move, Shift+arrows resize"><GripVerticalIcon aria-hidden="true" /></Button>
            <div className="tile-tools">
              <Button variant="ghost" size="icon" className="phone-only" aria-label={`Move ${label} earlier`} title="Move earlier" disabled={order[0] === i} onClick={() => reorder(s, i, -1)}><ArrowUpIcon aria-hidden="true" /></Button>
              <Button variant="ghost" size="icon" className="phone-only" aria-label={`Move ${label} later`} title="Move later" disabled={order[order.length - 1] === i} onClick={() => reorder(s, i, 1)}><ArrowDownIcon aria-hidden="true" /></Button>
              <Button variant="ghost" size="icon" data-testid="tile-customize" aria-label={`Customize ${label}`} aria-pressed={selected} title="Customize" onClick={() => setPanel(selected ? null : { kind: "tile", s, i })}><SlidersHorizontalIcon aria-hidden="true" /></Button>
              <Button variant="ghost" size="icon" aria-label={`Remove ${label}`} title="Remove" onClick={() => { change({ type: "delete", section: s, item: i }, `${label} removed`); if (selected) setPanel(null); focusItem(s, Math.max(0, i - 1)); }}><Trash2Icon aria-hidden="true" /></Button>
            </div>
            <span className="resize-handle" aria-hidden="true" title="Drag to resize" onPointerDown={(e) => { e.stopPropagation(); begin(e, s, i, "resize"); }} />
            {resizing && <span className="size-badge" aria-hidden="true">{rect.w} × {rect.h}</span>}
          </div>;
        })}
      </div>}
    </section>; })}
    <p id="board-move-help" className="sr">Arrow keys move the tile one cell; Shift with arrow keys resizes it.</p>
    <Button className="add-section" data-testid="board-add-section" icon={<PlusIcon aria-hidden="true" />} disabled={doc.sections.length >= MAX_SECTIONS} onClick={() => change({ type: "section", title: "New section" }, "Section added")}>Add section</Button></fieldset>
    {panel?.kind === "board" && <BoardPanel doc={doc} fallbackName={view.board.name} icons={boot?.remote_icons ?? false} change={change} onClose={closePanel} />}
    {tilePanel && panelItem && <TilePanel key={`${tilePanel.s}:${tilePanel.i}`} doc={doc} s={tilePanel.s} i={tilePanel.i} item={panelItem} title={titleOf(tilePanel.s, tilePanel.i)} min={minOf(tilePanel.s, tilePanel.i)} scoped={scoped}
      change={change} onMove={(target) => {
        // Dropped below everything there; compaction lifts it into place.
        const y = rows(rectsOf(doc.sections[target]));
        change({ type: "transfer", section: tilePanel.s, item: tilePanel.i, toSection: target, x: 0, y }, `Moved to ${doc.sections[target]?.title ?? "section"}`);
        setPanel({ kind: "tile", s: target, i: doc.sections[target]?.items.length ?? 0 });
      }}
      onRemove={() => { change({ type: "delete", section: tilePanel.s, item: tilePanel.i }, `${titleOf(tilePanel.s, tilePanel.i)} removed`); setPanel(null); }}
      onClose={closePanel} />}
    </div>

    <Dialog open={picker !== null} wide title={`Add to ${doc.sections[picker ?? 0]?.title || "section"}`} description="Pick something Hobnail already knows about, or add a link. It goes in the first free spot; drag it from there."
      initialFocus={manual ? linkTitle : search} onClose={() => setPicker(null)} testId="board-picker">
      <div className="picker">
        <Field id="suggestion-search" label="Search"><TextInput ref={search} type="search" value={query} placeholder="Containers, widgets, integrations…" onChange={(e) => setQuery(e.target.value)} /></Field>
        {kinds.length > 1 && <div className="picker-filters" role="group" aria-label="Show">
          {(["all", ...kinds] as Kind[]).map((option) => <Button key={option} size="sm" aria-pressed={kind === option} onClick={() => setKind(option)}>
            {option === "all" ? "All" : `${KIND_LABEL[option]}s`} <span className="picker-count">{option === "all" ? matching.length : matching.filter((x) => x.payload.kind === option).length}</span></Button>)}
        </div>}
        {shown.length ? <div className="suggestions">{shown.map((suggestion) => {
          const row = suggestionRow(suggestion.payload);
          const Icon = KIND_ICON[suggestion.payload.kind];
          return <CardButton className="suggestion" data-testid="board-suggestion" key={payloadIdentity(suggestion.payload)} onClick={() => add(suggestion.payload, `${suggestion.title} added`)}>
            <ServiceIcon className="board-avatar" tint name={row?.icon ?? (suggestion.payload.kind === "container" ? suggestion.payload.icon : null)} title={suggestion.title} enabled={boot?.remote_icons ?? false} />
            <span className="suggestion-copy"><strong>{suggestion.title}</strong><span><Icon aria-hidden="true" />{KIND_LABEL[suggestion.payload.kind]}{row && <> · <span className={`dot ${row.dot}`} aria-hidden="true" />{row.label}</>}</span></span>
            <PlusIcon aria-hidden="true" />
          </CardButton>; })}</div>
          : <p className="picker-empty">{suggestions.length ? query ? `Nothing matches “${query}”. Add it as a link below.` : "Everything Hobnail found is already on this board." : "Looking for containers and widgets…"}</p>}
        <form className="picker-link" data-testid="board-link-form" onSubmit={(e) => { e.preventDefault(); const data = new FormData(e.currentTarget); add({ kind: "link", title: String(data.get("title")), url: String(data.get("url")), icon: String(data.get("icon")) || null }, "Link added"); }}>
          <h3><Link2Icon aria-hidden="true" />Add a link</h3>
          <div className="picker-link-fields"><Field id="link-title" label="Title"><TextInput ref={linkTitle} name="title" required maxLength={80} placeholder="Router" /></Field><LinkUrl id="link-url" value={link} onChange={setLink} /><Field id="link-icon" label="Icon name" hint="Optional, e.g. jellyfin"><TextInput name="icon" /></Field></div>
          <div className="page-actions"><Button type="submit" variant="default" icon={<PlusIcon aria-hidden="true" />}>Add link</Button>{!scoped && <LinkTest url={link} />}</div>
        </form>
      </div>
    </Dialog>
    <Dialog testId="board-leave" open={pendingHref !== null} title="Leave this edit?" onClose={() => setPendingHref(null)} footer={<><Button onClick={() => setPendingHref(null)}>Keep editing</Button><Button variant="default" onClick={async () => { await settle(); const saved = await saveDraft(latest.current); if (!saved) { setPendingHref(null); setError("Could not preserve the draft. Keep editing and retry when the server is available."); return; } leaving.current = true; if (pendingHref) navigate(pendingHref); }}>Leave and keep draft</Button></>}><p>Your changes have not been saved to the board. Keep editing or leave with a server-side draft.</p></Dialog>
  </div>;
}
