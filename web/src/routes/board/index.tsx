import { lazy, Suspense, useCallback, useEffect, useState, type CSSProperties } from "react";
import { api } from "../../api/session.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardDraft } from "../../api/gen/BoardDraft.ts";
import { Dialog } from "../../ui/Dialog.tsx";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { BoardItem } from "../../api/gen/BoardItem.ts";
import type { ResolvedItem } from "../../api/gen/ResolvedItem.ts";
import { useBootstrap, useContainers, onConfigChange, watchBoards } from "../../live/index.ts";
import { useRoute, containerHref } from "../../app/router.ts";
import { avatar, iconSource } from "../../boards/document.ts";
import { Button } from "../../ui/Button.tsx";
import { EmptyState, Note } from "../../ui/Note.tsx";
import { NotFound } from "../NotFound.tsx";
import "./board.css";

const Editor = lazy(() => import("./Editor.tsx"));

export function BoardIcon({ name, title, enabled }: { name?: string | null; title: string; enabled: boolean }) {
  const src = iconSource(name, enabled);
  const [failed, setFailed] = useState<string | null>(null);
  return src && failed !== src ? <img className="board-avatar" src={src} alt="" onError={() => setFailed(src)} /> : <span className="board-avatar" aria-hidden="true">{avatar(title)}</span>;
}
export function Tile({ item, resolved }: { item: BoardItem; resolved?: ResolvedItem }) {
  const b = useBootstrap();
  const live = useContainers();
  const p = item.payload;
  const matched = resolved?.container;
  const container = matched ? live.data ? live.data.groups.flatMap((g) => g.containers).find((c) => c.name === matched.name) ?? null : matched : null;
  const targetMissing = resolved?.missing || (p.kind === "container" && !!matched && !!live.data && !container);
  const integration = resolved?.integration;
  const title = p.kind === "link" ? p.title : p.kind === "container" ? p.title ?? container?.title ?? (p.target.by === "name" ? p.target.name : `${p.target.project}/${p.target.service}`) : p.kind === "widget" ? p.widget_id : integration?.name ?? "Integration";
  const icon = p.kind === "link" ? p.icon : p.kind === "container" ? p.icon ?? container?.icon : integration?.admin?.icon;
  const href = p.kind === "link" ? p.url : p.kind === "container" ? p.link ?? (container ? containerHref(container.name) : null) : integration?.base_url;
  return <div className="board-tile" data-testid="board-tile" data-missing={targetMissing || undefined}>
    <div className="board-tile-head"><BoardIcon name={icon} title={title} enabled={b?.remote_icons ?? false} />
      <h3>{href ? <a href={href}>{title}</a> : title}</h3></div>
    {targetMissing ? <p className="bad-fg">Missing — this target is no longer available.</p> :
      p.kind === "container" ? <p><span className={`dot ${container?.dot ?? "off"}`} /> {container?.label ?? "Status unavailable"}</p> :
      p.kind === "link" ? <p className="muted">Saved link · {p.url}</p> :
      p.kind === "integration" ? <><p className="muted">{integration?.base_url}</p><p>{integration?.admin?.enabled === false ? "Disabled" : "No connection test recorded"}</p></> :
      <p className="muted">Configured widget · live data is unavailable in this client.</p>}
  </div>;
}
export default function BoardPage() {
  const { route } = useRoute();
  const id = route.name === "board" ? route.id : "";
  const b = useBootstrap();
  const admin = b?.role === "admin" || b?.role === "owner";
  const [editing, setEditing] = useState<BoardDocument | null>(null);
  const [draft, setDraft] = useState<BoardDraft | null>(null);
  const [changed, setChanged] = useState(false);
  const [view, setView] = useState<BoardView | null>(null);
  const [error, setError] = useState("");
  const [missing, setMissing] = useState(false);
  const load = useCallback(async () => {
    const result = await api.get<BoardView>(`/api/v1/boards/${id}`);
    if (result.kind === "ok") { setView(result.data); setError(""); }
    else if (result.kind === "error" && result.status === 404) setMissing(true);
    else setError(result.message);
  }, [id]);
  // load updates state only after the asynchronous server response.
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { void load(); return onConfigChange(() => { if (!editing) void load(); }); }, [load, editing]);
  useEffect(() => watchBoards((event) => { if (event.id === id) { if (editing) setChanged(event.version > editing.version); else void load(); } }), [id, load, editing]);
  useEffect(() => {
    if (!admin) return;
    const controller = new AbortController();
    void api.get<BoardDraft | null>(`/api/v1/boards/${id}/draft`, { signal: controller.signal }).then((result) => {
      if (!controller.signal.aborted && result.kind === "ok") setDraft(result.data);
    });
    return () => controller.abort();
  }, [admin, id]);
  if (missing) return <NotFound path={`/app/boards/${id}`} />;
  if (!view) return <div className="page">{error || "Loading board…"}</div>;
  if (editing) return <div className="page board-page"><Suspense fallback={<p>Opening editor…</p>}><Editor view={view} initial={editing} changed={changed} onRebase={() => setChanged(false)} onSave={(next) => { setView(next); setEditing(null); setChanged(false); setDraft(null); }} onCancel={() => { setEditing(null); setChanged(false); setDraft(null); void load(); }} /></Suspense></div>;
  return <div className="page board-page" data-testid="board">
    <header className="board-heading"><div><p className="muted">Board</p><h1>{view.board.name}</h1></div>
      {admin && <Button data-testid="board-edit" onClick={() => { setEditing(view.document); setChanged(false); }}>Edit board</Button>}</header>
    <Dialog open={!!draft} title="Resume your board draft?" onClose={() => setDraft(null)} testId="board-draft" footer={<><Button data-testid="draft-discard" onClick={async () => { const result = await api.request("DELETE", `/api/v1/boards/${id}/draft`); if (result.kind === "ok") setDraft(null); else setError(result.message); }}>Discard draft</Button><Button data-testid="draft-resume" variant="primary" onClick={() => { if (draft) setEditing(draft.document); setDraft(null); }}>Resume draft</Button></>}><p>An unfinished edit is saved for your account. Resume it or discard it.</p></Dialog>
    {error && <Note kind="bad">{error}</Note>}
    {view.document.sections.length === 0 ? <EmptyState icon={<span>◇</span>} title="This board is empty" testId="board-empty">
      {admin ? "Choose Edit board to add a section and the services or links you want here." : "No sections or items have been added to this board yet."}
    </EmptyState> : view.document.sections.map((section) => <section key={section.id} className="board-section">
      <h2>{section.title}</h2><div className="board-grid">{section.items.map((item) => <div key={item.id} className="board-cell"
        style={{ "--mobile": item.spans.mobile, "--tablet": item.spans.tablet, "--desktop": item.spans.desktop, "--rows": item.spans.rows } as CSSProperties}>
        <Tile item={item} resolved={view.resolved.find((r) => r.id === item.id)} />
      </div>)}</div>
    </section>)}
  </div>;
}
