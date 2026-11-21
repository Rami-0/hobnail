import { PageHeader } from "../../ui/PageHeader.tsx";
import { lazy, Suspense, useCallback, useEffect, useState, type CSSProperties } from "react";
import { reduceDocument } from "../../boards/document.ts";
import { api } from "../../api/session.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardDraft } from "../../api/gen/BoardDraft.ts";
import { Dialog } from "../../ui/Dialog.tsx";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { BoardItem } from "../../api/gen/BoardItem.ts";
import { IntegrationStatus } from "../../ui/IntegrationStatus.tsx";
import { WidgetContent } from "../../ui/WidgetContent.tsx";
import type { WidgetView } from "../../api/gen/WidgetView.ts";
import type { ResolvedItem } from "../../api/gen/ResolvedItem.ts";
import { useBootstrap, useContainers, onConfigChange, watchBoards, watchReadTopic } from "../../live/index.ts";
import { useRoute, containerHref } from "../../app/router.ts";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { Button } from "../../ui/Button.tsx";
import { EmptyState, Note } from "../../ui/Note.tsx";
import { NotFound } from "../NotFound.tsx";
import "./board.css";

const Editor = lazy(() => import("./Editor.tsx"));

export function Tile({ item, resolved }: { item: BoardItem; resolved?: ResolvedItem }) {
  const b = useBootstrap();
  const live = useContainers();
  const p = item.payload;
  const matched = resolved?.container;
  const container = matched ? live.data ? live.data.groups.flatMap((g) => g.containers).find((c) => c.name === matched.name) ?? null : matched : null;
  const targetMissing = resolved?.missing || (p.kind === "container" && !!matched && !!live.data && !container);
  const integration = resolved?.integration;
  const widget = resolved?.widget;
  const title = p.kind === "link" ? p.title : p.kind === "container" ? p.title ?? container?.title ?? (p.target.by === "name" ? p.target.name : `${p.target.project}/${p.target.service}`) : p.kind === "widget" ? p.title ?? widget?.title ?? p.widget_id : p.title ?? integration?.name ?? "Integration";
  const icon = p.kind === "link" ? p.icon : p.kind === "container" ? p.icon ?? container?.icon : p.kind === "widget" ? widget?.icon : integration?.admin?.icon;
  const href = p.kind === "link" ? p.url : p.kind === "container" ? p.link ?? (container ? containerHref(container.name) : null) : p.kind === "widget" ? widget?.link : (integration?.admin?.base_url ?? integration?.origin);
  return <div className="board-tile" data-testid="board-tile" data-missing={targetMissing || undefined}>
    <div className="board-tile-head"><ServiceIcon className="board-avatar" name={icon} title={title} enabled={b?.remote_icons ?? false} />
      <h3>{href ? <a href={href}>{title}</a> : title}</h3></div>
    {targetMissing ? <p className="bad-fg">Missing — this target is no longer available.</p> :
      p.kind === "container" ? <p><span className={`dot ${container?.dot ?? "off"}`} /> {container?.label ?? "Status unavailable"}</p> :
      p.kind === "link" ? <p className="muted">Saved link · {p.url}</p> :
      p.kind === "integration" ? <><p className="muted">{(integration?.admin?.base_url ?? integration?.origin)}</p><IntegrationStatus integration={integration} /></> :
      widget ? <WidgetContent widget={widget} /> : <p>Waiting for widget data…</p>}
  </div>;
}
export default function BoardPage() {
  const { route } = useRoute();
  const id = route.name === "board" ? route.id : "";
  const b = useBootstrap();
  const live = useContainers();
  const containerNames = live.data?.groups.flatMap((group) => group.containers.map((container) => container.name)).join("\0");
  const admin = b?.role === "admin" || b?.role === "owner";
  const [editing, setEditing] = useState<BoardDocument | null>(null);
  const [picker, setPicker] = useState<"add" | "discover">();
  const [draft, setDraft] = useState<BoardDraft | null>(null);
  const [changed, setChanged] = useState(false);
  const [view, setView] = useState<BoardView | null>(null);
  const [widgetUpdates, setWidgetUpdates] = useState<Record<string, WidgetView>>({});
  const [error, setError] = useState("");
  const [missing, setMissing] = useState(false);
  const load = useCallback(async () => {
    const result = await api.get<BoardView>(`/api/v1/boards/${id}`);
    if (result.kind === "ok") { setView(result.data); setWidgetUpdates({}); setError(""); }
    else if (result.kind === "error" && result.status === 404) setMissing(true);
    else setError(result.message);
  }, [id]);
  // load updates state only after the asynchronous server response.
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { void load(); }, [load]);
  // A changed container-name set can resolve a replacement compose target.
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { if (containerNames !== undefined && !editing) void load(); }, [load, containerNames, editing]);
  useEffect(() => onConfigChange(() => { if (!editing) void load(); }), [load, editing]);
  const hasWidgets = view?.resolved.some((item) => item.widget !== null);
  useEffect(() => {
    if (!b || !hasWidgets || editing) return;
    return watchReadTopic("widget", (data) => {
      const widget = data as WidgetView;
      setWidgetUpdates((previous) => ({ ...previous, [widget.id]: widget }));
    });
  }, [b, hasWidgets, editing]);
  useEffect(() => watchBoards((event) => { if (event.id === id) { if (editing) setChanged(event.version > editing.version); else void load(); } }), [id, load, editing]);
  useEffect(() => {
    if (!admin) return;
    const controller = new AbortController();
    void api.get<BoardDraft | null>(`/api/v1/boards/${id}/draft`, { signal: controller.signal }).then((result) => {
      if (!controller.signal.aborted && result.kind === "ok") setDraft(result.data);
    });
    return () => controller.abort();
  }, [admin, id]);
  async function openEditor(mode?: "add" | "discover") {
    if (!view) return;
    const result = await api.get<BoardDraft | null>(`/api/v1/boards/${id}/draft`);
    if (result.kind !== "ok") { setError(result.message); return; }
    if (result.data) setDraft(result.data);
    else {
      setPicker(mode);
      setEditing(mode ? reduceDocument(view.document, { type: "section", title: "Services" }) : view.document);
      setChanged(false);
    }
  }
  if (missing) return <NotFound path={`/app/boards/${id}`} />;
  if (!view) return <div className="page">{error || "Loading board…"}</div>;
  if (editing) return <div className="page board-page"><Suspense fallback={<p>Opening editor…</p>}><Editor view={view} initial={editing} initialPicker={picker} scoped={!!b?.scoped} changed={changed} onRebase={() => setChanged(false)} onSave={(next) => { setView(next); setEditing(null); setChanged(false); setDraft(null); }} onCancel={() => { setEditing(null); setChanged(false); setDraft(null); void load(); }} /></Suspense></div>;
  return <div className="page board-page" data-testid="board">
    <PageHeader title={view.board.name} icon={<ServiceIcon name={view.board.icon} title={view.board.name} enabled={b?.remote_icons ?? false} />}
      actions={admin && <Button variant="default" data-testid="board-edit" onClick={() => void openEditor()}>Edit board</Button>} />
    <Dialog open={!!draft} title="Resume your board draft?" onClose={() => setDraft(null)} testId="board-draft" footer={<><Button data-testid="draft-discard" onClick={async () => { const result = await api.request("DELETE", `/api/v1/boards/${id}/draft`); if (result.kind === "ok") setDraft(null); else setError(result.message); }}>Discard draft</Button><Button data-testid="draft-resume" variant="default" onClick={() => { if (draft) { setPicker(undefined); setEditing(draft.document); } setDraft(null); }}>Resume draft</Button></>}><p>An unfinished edit is saved for your account. Resume it or discard it.</p></Dialog>
    {error && <Note kind="bad">{error}</Note>}
    {view.document.sections.length === 0 ? <EmptyState icon={<span>◇</span>} title="This board is empty" testId="board-empty"
      actions={admin && <>
        <Button data-testid="board-empty-add" onClick={() => void openEditor("add")}>Add</Button>
        <Button data-testid="board-empty-discover" onClick={() => void openEditor("discover")}>Discover</Button>
      </>}>
      {admin ? "Add a link or browse available services." : "No sections or items have been added to this board yet."}
    </EmptyState> : view.document.sections.map((section) => <section key={section.id} className="board-section">
      <h2>{section.title}</h2><div className="board-grid">{section.items.map((item) => {
        const resolved = view.resolved.find((r) => r.id === item.id);
        const update = resolved?.widget ? widgetUpdates[resolved.widget.id] : null;
        return <div key={item.id} className="board-cell"
        style={{ "--mobile": item.spans.mobile, "--tablet": item.spans.tablet, "--desktop": item.spans.desktop, "--rows": item.spans.rows } as CSSProperties}>
        <Tile item={item} resolved={resolved && update ? { ...resolved, widget: update } : resolved} />
      </div>; })}</div>
    </section>)}
  </div>;
}
