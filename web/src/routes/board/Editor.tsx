import { startTransition, useEffect, useRef, useState, type PointerEvent, type KeyboardEvent } from "react";
import { ArrowUpIcon, ArrowDownIcon, BoxIcon, ChevronLeftIcon, ChevronRightIcon, GripVerticalIcon, LayoutDashboardIcon, Link2Icon, PencilIcon, PlugIcon, PlusIcon, Trash2Icon } from "lucide-react";
import type { BoardPayload } from "../../api/gen/BoardPayload.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { BoardSuggestion } from "../../api/gen/BoardSuggestion.ts";
import { api } from "../../api/session.ts";
import { dirty, freshSuggestions, payloadIdentity, rebase, recoveryBase, reduceDocument, stepSize, TILE_SIZES, tileSize, type Action, type TileSize } from "../../boards/document.ts";
import { navigate } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Note } from "../../ui/Note.tsx";
import { LinkTest } from "../../boards/LinkTest.tsx";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { CardButton } from "../../ui/CardButton.tsx";
import { useBootstrap, useContainers } from "../../live/index.ts";
import { Tile, cellStyle } from "./Tile.tsx";

type Kind = "all" | BoardPayload["kind"];
const KIND_LABEL: Record<BoardPayload["kind"], string> = { container: "Container", widget: "Widget", integration: "Integration", link: "Link" };
const KIND_ICON = { container: BoxIcon, widget: LayoutDashboardIcon, integration: PlugIcon, link: Link2Icon };
const targetAt = (x: number, y: number) => document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-section]");
const SIZE_SHORT: Record<TileSize, string> = { small: "S", wide: "W", tall: "T", large: "L" };

export default function Editor({ view, initial, initialPicker, scoped, changed, onSave, onCancel, onRebase }: {
  view: BoardView; initial: BoardDocument; initialPicker?: "add" | "discover"; scoped: boolean; changed: boolean; onSave: (view: BoardView) => void; onCancel: () => void; onRebase: () => void;
}) {
  const [base, setBase] = useState(() => initial.version === view.document.version ? view.document : recoveryBase(initial, view.document));
  const [doc, setDoc] = useState(initial);
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
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingDraft = useRef<Promise<boolean>>(Promise.resolve(true));
  const latest = useRef(doc);
  const leaving = useRef(false);
  const operation = useRef(false);
  const root = useRef<HTMLDivElement>(null);
  const linkTitle = useRef<HTMLInputElement>(null);
  const [manual, setManual] = useState(initialPicker === "add");
  const search = useRef<HTMLInputElement>(null);
  const [kind, setKind] = useState<Kind>("all");
  const [itemEditor, setItemEditor] = useState<{ s: number; i: number } | null>(null);
  const boot = useBootstrap();
  const live = useContainers();
  useEffect(() => { if (!initialPicker) root.current?.querySelector<HTMLButtonElement>("button")?.focus({ preventScroll: true }); }, [initialPicker]);
  const [toolCount, setToolCount] = useState(6);
  const itemCount = doc.sections.reduce((count, section) => count + section.items.length, 0);
  useEffect(() => {
    if (toolCount >= itemCount) return;
    const frame = requestAnimationFrame(() => startTransition(() => setToolCount(toolCount + 6)));
    return () => cancelAnimationFrame(frame);
  }, [itemCount, toolCount]);
  let itemIndex = 0;
  const path = `/api/v1/boards/${view.board.id}`;
  const modified = dirty(base, doc);
  const change = (action: Action, message: string) => { if (operation.current) return; setDoc((previous) => reduceDocument(previous, action)); setAnnouncement(message); setDraftState("Draft pending"); };
  const focusItem = (section: number, item: number) => requestAnimationFrame(() => {
    const button = root.current?.querySelector<HTMLButtonElement>(`[data-section="${section}"][data-item="${item}"] .drag-handle`)
      ?? root.current?.querySelector<HTMLButtonElement>(`section[data-section="${section}"] [data-testid="board-add-item"]`)
      ?? root.current?.querySelector<HTMLButtonElement>('[data-testid="board-add-section"]');
    button?.focus();
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
  const move = (s: number, i: number, target: number, position: number) => {
    const end = Math.max(0, (doc.sections[target]?.items.length ?? 0) - (s === target ? 1 : 0));
    const destination = Math.min(end, Math.max(0, position));
    change({ type: "move", section: s, item: i, toSection: target, to: destination }, `Item moved to section ${target + 1}, position ${destination + 1}`); focusItem(target, destination);
  };
  const key = (event: KeyboardEvent, s: number, i: number) => {
    if (!(event.altKey || event.shiftKey) || !event.key.startsWith("Arrow") || event.target instanceof HTMLInputElement) return;
    event.preventDefault(); event.stopPropagation(); const item = doc.sections[s]?.items[i]; if (!item) return;
    if (event.altKey) {
      if (event.key === "ArrowUp" && i > 0) move(s, i, s, i - 1);
      if (event.key === "ArrowDown" && i < (doc.sections[s]?.items.length ?? 0) - 1) move(s, i, s, i + 1);
      if (event.key === "ArrowLeft" && s > 0) move(s, i, s - 1, doc.sections[s - 1]?.items.length ?? 0);
      if (event.key === "ArrowRight" && s + 1 < doc.sections.length) move(s, i, s + 1, doc.sections[s + 1]?.items.length ?? 0);
    } else {
      const spans = stepSize(item.spans, event.key as "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown");
      change({ type: "resize", section: s, item: i, spans }, `Item resized: ${TILE_SIZES[tileSize(spans)].label}`);
    }
  };
  const resize = (s: number, i: number, size: TileSize) => change({ type: "resize", section: s, item: i, spans: { ...TILE_SIZES[size].spans } }, `Item resized: ${TILE_SIZES[size].label}`);
  const [dragging, setDragging] = useState<{ s: number; i: number } | null>(null);
  const [drop, setDrop] = useState<string | null>(null);
  const over = (e: globalThis.PointerEvent) => { const t = targetAt(e.clientX, e.clientY); setDrop(t?.dataset.item === undefined ? null : `${t.dataset.section}:${t.dataset.item}`); };
  const drag = (event: PointerEvent<HTMLButtonElement>, s: number, i: number) => {
    if (event.button !== 0 || busy) return; event.preventDefault(); const button = event.currentTarget; button.setPointerCapture(event.pointerId);
    if (!doc.sections[s]?.items[i]) return;
    setDragging({ s, i });
    const finish = () => { button.removeEventListener("pointerup", up); button.removeEventListener("pointercancel", finish); button.removeEventListener("pointermove", over); setDragging(null); setDrop(null); };
    const up = (end: globalThis.PointerEvent) => {
      finish();
      const target = targetAt(end.clientX, end.clientY);
      if (target) { const targetSection = Number(target.dataset.section); move(s, i, targetSection, target.dataset.item === undefined ? doc.sections[targetSection]?.items.length ?? 0 : Number(target.dataset.item)); }
    };
    button.addEventListener("pointerup", up); button.addEventListener("pointercancel", finish); button.addEventListener("pointermove", over);
  };
  const openPicker = (section: number) => { setManual(false); setPicker(section); setQuery(""); setLink(""); setKind("all"); };
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
  const rows = new Map(live.data?.groups.flatMap((group) => group.containers).map((row) => [row.name, row]) ?? []);
  const suggestionRow = (payload: BoardPayload) => payload.kind === "container" ? rows.get(payload.target.by === "name" ? payload.target.name : "") ?? [...rows.values()].find((row) => payload.target.by === "compose" && row.stack === payload.target.project && row.title === payload.target.service) : undefined;
  const editingItem = itemEditor ? doc.sections[itemEditor.s]?.items[itemEditor.i] : undefined;
  const addSuggestion = (payload: BoardPayload, title: string) => { if (picker !== null) change({ type: "add", section: picker, payload }, `${title} added`); setPicker(null); };
  return <div ref={root} className="board-editor" data-testid="board-editor">
    <div className="editor-bar">
      <div className="editor-bar-copy"><h1>Editing {view.board.name}</h1>
        <p role="status" data-testid="draft-status">{draftState} · {modified ? "Unsaved changes" : "No changes"}</p></div>
      <div className="page-actions"><Button variant="outline" busy={busy} data-testid="board-cancel" onClick={() => { void cancel(); }}>Cancel</Button>
        <Button variant="default" busy={busy} disabled={conflict} data-testid="board-save" onClick={() => { void save(); }}>Save</Button></div>
    </div>
    <p className="editor-hint">Drag <GripVerticalIcon aria-label="the grip" width={14} height={14} /> to reorder or to move a tile to another section. On a focused grip, <kbd>Alt</kbd> + arrows moves and <kbd>Shift</kbd> + arrows resizes.</p>
    {(changed || conflict) && <Note kind="warn" testId="board-conflict">Another session saved this board. Your changes are kept here.
      {initial.version !== view.document.version && <p>A recovered draft keeps server additions. Review it before saving; you may need to apply removals again.</p>}
      <Button data-testid="board-rebase" disabled={busy} onClick={async () => { if (operation.current) return; operation.current = true; setBusy(true); await settle(); const result = await api.get<BoardView>(path); if (result.kind !== "ok") { setError(result.message); operation.current = false; setBusy(false); return; }
        const next = rebase(base, doc, result.data.document); setBase(result.data.document); setDoc(next); setConflict(false); onRebase(); setError(""); setAnnouncement("Changes rebased. Review them and choose Save."); operation.current = false; setBusy(false);
      }}>Reload and keep my changes</Button><Button onClick={() => { void cancel(); }}>Discard mine</Button>
    </Note>}
    {error && <Note kind="bad">{error}</Note>}<output className="sr" aria-live="polite">{announcement}</output>
    <fieldset className="editor-fields" disabled={busy}><legend className="sr">Board layout</legend>
    {doc.sections.map((section, s) => <section key={section.id ?? `section-${s}`} data-section={s} className="board-section editor-section">
      <div className="editor-section-head"><h2 className="sr">{section.title}</h2>
        <div className="section-title-input"><TextInput id={`section-${s}`} aria-label="Section title" value={section.title} required maxLength={80} onChange={(e) => change({ type: "rename", section: s, title: e.target.value }, "Section renamed")} /></div>
        <span className="section-count" aria-label={`${section.items.length} items`}>{section.items.length}</span>
        <div className="editor-section-tools"><Button variant="ghost" size="icon" aria-label="Move section up" title="Move section up" disabled={s === 0} onClick={() => change({ type: "move-section", section: s, to: s - 1 }, "Section moved up")}><ArrowUpIcon aria-hidden="true" /></Button><Button variant="ghost" size="icon" aria-label="Move section down" title="Move section down" disabled={s === doc.sections.length - 1} onClick={() => change({ type: "move-section", section: s, to: s + 1 }, "Section moved down")}><ArrowDownIcon aria-hidden="true" /></Button><Button variant="ghost" size="icon" aria-label={`Delete section ${section.title}`} title="Delete section" onClick={() => { change({ type: "delete-section", section: s }, "Section removed"); focusItem(Math.min(s, doc.sections.length - 2), 0); }}><Trash2Icon aria-hidden="true" /></Button></div>
      </div><div className="board-grid">{section.items.map((item, i) => {
        const showTools = itemIndex++ < toolCount;
        const size = tileSize(item.spans);
        const name = item.payload.kind === "link" ? item.payload.title : item.payload.title ?? view.resolved.find((r) => r.id === item.id)?.container?.title ?? KIND_LABEL[item.payload.kind];
        return <div key={item.id ?? `item-${i}`} className="board-cell editor-cell" data-section={s} data-item={i} style={cellStyle(item)}
          data-dragging={dragging?.s === s && dragging.i === i ? "" : undefined} data-drop={drop === `${s}:${i}` && !(dragging?.s === s && dragging.i === i) ? "" : undefined}>
        <Tile item={item} resolved={view.resolved.find((r) => r.id === item.id)} />
        {showTools && <div className="editor-tools">
          <Button variant="ghost" size="icon" className="drag-handle" onKeyDown={(e) => key(e, s, i)} onPointerDown={(e) => drag(e, s, i)} aria-label={`Move ${name}`} title="Drag to move"><GripVerticalIcon aria-hidden="true" /></Button>
          <Button variant="ghost" size="icon" aria-label="Move earlier" title="Move earlier" disabled={i === 0 && s === 0} onClick={() => i > 0 ? move(s, i, s, i - 1) : move(s, i, s - 1, doc.sections[s - 1]?.items.length ?? 0)}><ChevronLeftIcon aria-hidden="true" /></Button>
          <Button variant="ghost" size="icon" aria-label="Move later" title="Move later" disabled={i === section.items.length - 1 && s === doc.sections.length - 1} onClick={() => i < section.items.length - 1 ? move(s, i, s, i + 1) : move(s, i, s + 1, 0)}><ChevronRightIcon aria-hidden="true" /></Button>
          <span className="size-group" role="group" aria-label="Size">{(Object.keys(TILE_SIZES) as TileSize[]).map((option) =>
            <Button key={option} variant="ghost" size="sm" aria-pressed={size === option} aria-label={TILE_SIZES[option].label} title={TILE_SIZES[option].label} onClick={() => resize(s, i, option)}>{SIZE_SHORT[option]}</Button>)}</span>
          <span className="spacer" />
          <Button variant="ghost" size="icon" aria-label={`Edit ${name}`} title="Edit" onClick={() => setItemEditor({ s, i })}><PencilIcon aria-hidden="true" /></Button>
          <Button variant="ghost" size="icon" aria-label={`Remove ${name}`} title="Remove" onClick={() => { change({ type: "delete", section: s, item: i }, "Item removed"); focusItem(s, Math.max(0, i - 1)); }}><Trash2Icon aria-hidden="true" /></Button>
        </div>}
      </div>; })}
        <div className="board-cell"><CardButton className="add-tile" data-testid="board-add-item" onClick={() => { void openPicker(s); }}><PlusIcon aria-hidden="true" />Add item</CardButton></div>
      </div>
    </section>)}
    <Button className="add-section" data-testid="board-add-section" icon={<PlusIcon aria-hidden="true" />} disabled={doc.sections.length >= 64} onClick={() => change({ type: "section", title: "New section" }, "Section added")}>Add section</Button></fieldset>

    <Dialog open={picker !== null} wide title={`Add to ${doc.sections[picker ?? 0]?.title || "section"}`} description="Pick something Hobnail already knows about, or add a link."
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
          return <CardButton className="suggestion" data-testid="board-suggestion" key={payloadIdentity(suggestion.payload)} onClick={() => addSuggestion(suggestion.payload, suggestion.title)}>
            <ServiceIcon className="board-avatar" tint name={row?.icon ?? (suggestion.payload.kind === "container" ? suggestion.payload.icon : null)} title={suggestion.title} enabled={boot?.remote_icons ?? false} />
            <span className="suggestion-copy"><strong>{suggestion.title}</strong><span><Icon aria-hidden="true" />{KIND_LABEL[suggestion.payload.kind]}{row && <> · <span className={`dot ${row.dot}`} aria-hidden="true" />{row.label}</>}</span></span>
            <PlusIcon aria-hidden="true" />
          </CardButton>; })}</div>
          : <p className="picker-empty">{suggestions.length ? query ? `Nothing matches “${query}”. Add it as a link below.` : "Everything Hobnail found is already on this board." : "Looking for containers and widgets…"}</p>}
        <form className="picker-link" data-testid="board-link-form" onSubmit={(e) => { e.preventDefault(); if (picker === null) return; const data = new FormData(e.currentTarget); change({ type: "add", section: picker, payload: { kind: "link", title: String(data.get("title")), url: String(data.get("url")), icon: String(data.get("icon")) || null } }, "Link added"); setPicker(null); }}>
          <h3><Link2Icon aria-hidden="true" />Add a link</h3>
          <div className="picker-link-fields"><Field id="link-title" label="Title"><TextInput ref={linkTitle} name="title" required maxLength={80} placeholder="Router" /></Field><LinkUrl id="link-url" value={link} onChange={setLink} /><Field id="link-icon" label="Icon name" hint="Optional, e.g. jellyfin"><TextInput name="icon" /></Field></div>
          <div className="page-actions"><Button type="submit" variant="default" icon={<PlusIcon aria-hidden="true" />}>Add link</Button>{!scoped && <LinkTest url={link} />}</div>
        </form>
      </div>
    </Dialog>
    <Dialog open={!!editingItem} title="Edit tile" onClose={() => setItemEditor(null)} testId="board-item-editor" footer={<Button variant="default" onClick={() => setItemEditor(null)}>Done</Button>}>
      {editingItem && itemEditor && <div className="item-form">
        <div className="size-cards size-group" role="group" aria-label="Size">{(Object.keys(TILE_SIZES) as TileSize[]).map((option) =>
          <Button key={option} variant="ghost" aria-pressed={tileSize(editingItem.spans) === option} onClick={() => resize(itemEditor.s, itemEditor.i, option)}>{TILE_SIZES[option].label}</Button>)}</div>
        {doc.sections.length > 1 && <Select label="Section" value={String(itemEditor.s)} options={doc.sections.map((entry, index) => ({ value: String(index), label: entry.title || `Section ${index + 1}` }))}
          onChange={(value) => { const target = Number(value); move(itemEditor.s, itemEditor.i, target, doc.sections[target]?.items.length ?? 0); setItemEditor({ s: target, i: Math.max(0, (doc.sections[target]?.items.length ?? 0) - (target === itemEditor.s ? 1 : 0)) }); }} />}
        {editingItem.payload.kind === "container" ? (["title", "icon", "link"] as const).map((field) => <Field key={field} id={`custom-${field}`} label={`Custom ${field}`} hint={field === "icon" ? "A dashboard-icons name, e.g. jellyfin" : field === "link" ? "Where a click on the tile goes" : undefined}><TextInput value={editingItem.payload.kind === "container" ? editingItem.payload[field] ?? "" : ""} onChange={(e) => { if (editingItem.payload.kind === "container") change({ type: "payload", section: itemEditor.s, item: itemEditor.i, payload: { ...editingItem.payload, [field]: e.target.value || null } }, `Custom ${field} updated`); }} /></Field>)
        : <>
          <Field id="item-title" label={editingItem.payload.kind === "link" ? "Title" : "Custom title"}><TextInput value={editingItem.payload.title ?? ""} maxLength={80}
            onChange={(e) => {
              if (editingItem.payload.kind === "container") return;
              const { title: _title, ...target } = editingItem.payload;
              const title = e.target.value;
              const payload = title || target.kind === "link" ? { ...target, title } : target;
              change({ type: "payload", section: itemEditor.s, item: itemEditor.i, payload }, "Title updated");
            }} /></Field>
          {editingItem.payload.kind === "link" && <>
            <LinkUrl id="item-url" value={editingItem.payload.url}
              onChange={(url) => { if (editingItem.payload.kind === "link") change({ type: "payload", section: itemEditor.s, item: itemEditor.i, payload: { ...editingItem.payload, url } }, "URL updated"); }} />
            <Field id="item-icon" label="Icon name"><TextInput value={editingItem.payload.icon ?? ""} onChange={(e) => { if (editingItem.payload.kind === "link") change({ type: "payload", section: itemEditor.s, item: itemEditor.i, payload: { ...editingItem.payload, icon: e.target.value || null } }, "Icon updated"); }} /></Field>
            {!scoped && <LinkTest url={editingItem.payload.url} />}
          </>}
        </>}
      </div>}
    </Dialog>
    <Dialog testId="board-leave" open={pendingHref !== null} title="Leave this edit?" onClose={() => setPendingHref(null)} footer={<><Button onClick={() => setPendingHref(null)}>Keep editing</Button><Button variant="default" onClick={async () => { await settle(); const saved = await saveDraft(latest.current); if (!saved) { setPendingHref(null); setError("Could not preserve the draft. Keep editing and retry when the server is available."); return; } leaving.current = true; if (pendingHref) navigate(pendingHref); }}>Leave and keep draft</Button></>}><p>Your changes have not been saved to the board. Keep editing or leave with a server-side draft.</p></Dialog>
  </div>;
}
