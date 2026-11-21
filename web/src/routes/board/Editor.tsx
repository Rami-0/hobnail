import { PageHeader } from "../../ui/PageHeader.tsx";
import { startTransition, useEffect, useRef, useState, type CSSProperties, type PointerEvent, type KeyboardEvent } from "react";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { BoardSuggestion } from "../../api/gen/BoardSuggestion.ts";
import type { GridSpans } from "../../api/gen/GridSpans.ts";
import { api } from "../../api/session.ts";
import { dirty, rebase, recoveryBase, reduceDocument, type Action } from "../../boards/document.ts";
import { navigate } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Note } from "../../ui/Note.tsx";
import { LinkTest } from "../../boards/LinkTest.tsx";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { Tile } from "./index.tsx";

const breakpoint = (): keyof GridSpans => window.innerWidth < 700 ? "mobile" : window.innerWidth < 1100 ? "tablet" : "desktop";

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
      const dimension = event.key === "ArrowUp" || event.key === "ArrowDown" ? "rows" : breakpoint();
      const delta = event.key === "ArrowUp" || event.key === "ArrowLeft" ? -1 : 1;
      change({ type: "resize", section: s, item: i, spans: { ...item.spans, [dimension]: item.spans[dimension] + delta } }, `Item resized: ${dimension}`);
    }
  };
  const drag = (event: PointerEvent<HTMLButtonElement>, s: number, i: number, resize: boolean) => {
    if (event.button !== 0 || busy) return; event.preventDefault(); const button = event.currentTarget; button.setPointerCapture(event.pointerId);
    const start = { x: event.clientX, y: event.clientY }; const item = doc.sections[s]?.items[i]; if (!item) return;
    const up = (end: globalThis.PointerEvent) => {
      button.removeEventListener("pointerup", up); button.removeEventListener("pointercancel", stop);
      if (resize) {
        const grid = button.closest(".board-grid")?.getBoundingClientRect(); const bp = breakpoint(); const columns = bp === "mobile" ? 4 : bp === "tablet" ? 8 : 12;
        change({ type: "resize", section: s, item: i, spans: { ...item.spans, [bp]: item.spans[bp] + Math.round((end.clientX - start.x) / ((grid?.width ?? 400) / columns)), rows: item.spans.rows + Math.round((end.clientY - start.y) / 130) } }, "Item resized");
      } else {
        const target = document.elementFromPoint(end.clientX, end.clientY)?.closest<HTMLElement>("[data-section]");
        if (target) { const targetSection = Number(target.dataset.section); move(s, i, targetSection, target.dataset.item === undefined ? doc.sections[targetSection]?.items.length ?? 0 : Number(target.dataset.item)); }
      }
    };
    const stop = () => { button.removeEventListener("pointerup", up); button.removeEventListener("pointercancel", stop); };
    button.addEventListener("pointerup", up); button.addEventListener("pointercancel", stop);
  };
  const openPicker = (section: number) => { setManual(false); setPicker(section); setQuery(""); setLink(""); };
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
  return <div ref={root} className="board-editor" data-testid="board-editor">
    <PageHeader title={`Edit ${view.board.name}`}
      description={<p role="status" data-testid="draft-status">{draftState} · {modified ? "Unsaved changes" : "No changes"}</p>}
      actions={<><Button variant="outline" busy={busy} data-testid="board-cancel" onClick={() => { void cancel(); }}>Cancel</Button>
        <Button variant="default" busy={busy} disabled={conflict} data-testid="board-save" onClick={() => { void save(); }}>Save</Button></>} />

    <p className="muted">Drag Move onto an item or section; drag Resize to change size. On a focused item: Alt + arrows moves; Shift + arrows resizes at this screen width.</p>
    {(changed || conflict) && <Note kind="warn" testId="board-conflict">Another session saved this board. Your changes are kept here.
      {initial.version !== view.document.version && <p>A recovered draft keeps server additions. Review it before saving; you may need to apply removals again.</p>}
      <Button data-testid="board-rebase" disabled={busy} onClick={async () => { if (operation.current) return; operation.current = true; setBusy(true); await settle(); const result = await api.get<BoardView>(path); if (result.kind !== "ok") { setError(result.message); operation.current = false; setBusy(false); return; }
        const next = rebase(base, doc, result.data.document); setBase(result.data.document); setDoc(next); setConflict(false); onRebase(); setError(""); setAnnouncement("Changes rebased. Review them and choose Save."); operation.current = false; setBusy(false);
      }}>Reload and keep my changes</Button><Button onClick={() => { void cancel(); }}>Discard mine</Button>
    </Note>}
    {error && <Note kind="bad">{error}</Note>}<output className="sr" aria-live="polite">{announcement}</output>
    <fieldset className="editor-fields" disabled={busy}><legend className="sr">Board layout</legend>
    {doc.sections.map((section, s) => <section key={section.id ?? `section-${s}`} data-section={s} className="board-section">
      <div className="editor-section-head"><Field id={`section-${s}`} label="Section title"><TextInput value={section.title} required maxLength={80} onChange={(e) => change({ type: "rename", section: s, title: e.target.value }, "Section renamed")} /></Field>
        <div className="settings-actions"><Button variant="ghost" size="icon" aria-label="Move section up" disabled={s === 0} onClick={() => change({ type: "move-section", section: s, to: s - 1 }, "Section moved up")}>↑</Button><Button variant="ghost" size="icon" aria-label="Move section down" disabled={s === doc.sections.length - 1} onClick={() => change({ type: "move-section", section: s, to: s + 1 }, "Section moved down")}>↓</Button><Button onClick={() => { change({ type: "delete-section", section: s }, "Section removed"); focusItem(Math.min(s, doc.sections.length - 2), 0); }}>Delete section</Button></div>
      </div><div className="board-grid">{section.items.map((item, i) => {
        const showTools = itemIndex++ < toolCount;
        return <div key={item.id ?? `item-${i}`} className="board-cell editor-cell" data-section={s} data-item={i}
        style={{ "--mobile": item.spans.mobile, "--tablet": item.spans.tablet, "--desktop": item.spans.desktop, "--rows": item.spans.rows } as CSSProperties}>
        <Tile item={item} resolved={view.resolved.find((r) => r.id === item.id)} />
        {showTools && <>
        <div className="editor-tools"><Button variant="ghost" size="sm" className="drag-handle" onKeyDown={(e) => key(e, s, i)} onPointerDown={(e) => drag(e, s, i, false)} aria-label="Move item">Move</Button><Button variant="ghost" size="sm" className="drag-handle" onKeyDown={(e) => key(e, s, i)} onPointerDown={(e) => drag(e, s, i, true)} aria-label="Resize item">Resize</Button><Button variant="ghost" size="sm" onClick={() => { change({ type: "delete", section: s, item: i }, "Item removed"); focusItem(s, Math.max(0, i - 1)); }}>Remove</Button></div>
        {doc.sections.length > 1 && <Select label="Move to section" value={String(s)} options={doc.sections.map((entry, index) => ({ value: String(index), label: entry.title }))} onChange={(value) => move(s, i, Number(value), doc.sections[Number(value)]?.items.length ?? 0)} />}
        <div className="editor-spans">{(["mobile", "tablet", "desktop", "rows"] as const).map((dimension) => <Field key={dimension} id={`span-${s}-${i}-${dimension}`} label={dimension}><TextInput type="number" min={1} max={dimension === "mobile" ? 4 : dimension === "tablet" ? 8 : 12} value={item.spans[dimension]} onChange={(e) => change({ type: "resize", section: s, item: i, spans: { ...item.spans, [dimension]: Number(e.target.value) } }, `${dimension} resized`)} /></Field>)}</div>
        {item.payload.kind === "container" && <div className="editor-custom">{(["title", "icon", "link"] as const).map((field) => <Field key={field} id={`custom-${s}-${i}-${field}`} label={`Custom ${field}`}><TextInput value={item.payload.kind === "container" ? item.payload[field] ?? "" : ""} onChange={(e) => { if (item.payload.kind === "container") change({ type: "payload", section: s, item: i, payload: { ...item.payload, [field]: e.target.value || null } }, `Custom ${field} updated`); }} /></Field>)}</div>}
        {item.payload.kind !== "container" && <div className="editor-custom">
          <Field id={`title-${s}-${i}`} label="Custom title"><TextInput value={item.payload.title ?? ""} maxLength={80}
            onChange={(e) => {
              if (item.payload.kind === "container") return;
              const { title: _title, ...target } = item.payload;
              const title = e.target.value;
              const payload = title || target.kind === "link" ? { ...target, title } : target;
              change({ type: "payload", section: s, item: i, payload }, "Custom title updated");
            }} /></Field>
          {item.payload.kind === "link" && <>
            <LinkUrl id={`url-${s}-${i}`} value={item.payload.url}
              onChange={(url) => { if (item.payload.kind === "link") change({ type: "payload", section: s, item: i, payload: { ...item.payload, url } }, "URL updated"); }} />
            {!scoped && <LinkTest url={item.payload.url} />}
          </>}
        </div>}

        </>}
      </div>; })}</div><Button data-testid="board-add-item" onClick={() => { void openPicker(s); }}>Add item</Button>
    </section>)}
    <Button data-testid="board-add-section" disabled={doc.sections.length >= 64} onClick={() => change({ type: "section", title: "New section" }, "Section added")}>Add section</Button></fieldset>
    <Dialog open={picker !== null} title="Add item" initialFocus={manual ? linkTitle : undefined} onClose={() => setPicker(null)} testId="board-picker"><Field id="suggestion-search" label="Search suggestions"><TextInput value={query} onChange={(e) => setQuery(e.target.value)} /></Field>
      <div className="suggestions">{suggestions.filter((suggestion) => suggestion.title.toLowerCase().includes(query.toLowerCase()) && !doc.sections.some((section) => section.items.some((item) => JSON.stringify(item.payload) === JSON.stringify(suggestion.payload)))).map((suggestion) => <Button variant="ghost" data-testid="board-suggestion" key={JSON.stringify(suggestion.payload)} onClick={() => { if (picker !== null) change({ type: "add", section: picker, payload: suggestion.payload }, "Item added"); setPicker(null); }}>{suggestion.title}</Button>)}</div>
      <h3>Add a link</h3><form data-testid="board-link-form" onSubmit={(e) => { e.preventDefault(); if (picker === null) return; const data = new FormData(e.currentTarget); change({ type: "add", section: picker, payload: { kind: "link", title: String(data.get("title")), url: String(data.get("url")), icon: String(data.get("icon")) || null } }, "Link added"); setPicker(null); }}>
        <Field id="link-title" label="Title"><TextInput ref={linkTitle} name="title" required maxLength={80} /></Field><LinkUrl id="link-url" value={link} onChange={setLink} />{!scoped && <LinkTest url={link} />}<Field id="link-icon" label="Icon name"><TextInput name="icon" /></Field><Button type="submit" variant="default">Add link</Button>
      </form>
    </Dialog>
    <Dialog testId="board-leave" open={pendingHref !== null} title="Leave this edit?" onClose={() => setPendingHref(null)} footer={<><Button onClick={() => setPendingHref(null)}>Keep editing</Button><Button variant="default" onClick={async () => { await settle(); const saved = await saveDraft(latest.current); if (!saved) { setPendingHref(null); setError("Could not preserve the draft. Keep editing and retry when the server is available."); return; } leaving.current = true; if (pendingHref) navigate(pendingHref); }}>Leave and keep draft</Button></>}><p>Your changes have not been saved to the board. Keep editing or leave with a server-side draft.</p></Dialog>
  </div>;
}
