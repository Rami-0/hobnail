import { PageHeader } from "../../ui/PageHeader.tsx";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ChevronDownIcon, LayoutGridIcon, PencilIcon, PlugIcon, PlusIcon } from "lucide-react";
import { itemCount as countItems, rectOf, reduceDocument } from "../../boards/document.ts";
import { readingOrder, rows as rowCount } from "../../boards/grid.ts";
import type { BoardSection } from "../../api/gen/BoardSection.ts";
import { tourBoard } from "../../boards/tour.ts";
import { api, useSession } from "../../api/session.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardDraft } from "../../api/gen/BoardDraft.ts";
import { Dialog } from "../../ui/Dialog.tsx";
import type { BoardView } from "../../api/gen/BoardView.ts";
import type { WidgetView } from "../../api/gen/WidgetView.ts";
import { useBootstrap, useContainers, onConfigChange, watchBoards, watchReadTopic } from "../../live/index.ts";
import { useRoute } from "../../app/router.ts";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { Button } from "../../ui/Button.tsx";
import { EmptyState, Note } from "../../ui/Note.tsx";
import { LoadBoundary } from "../../ui/LoadBoundary.tsx";
import { NotFound } from "../NotFound.tsx";
import { Tile } from "./Tile.tsx";
import { boardLook, canvasVars, cellVars } from "./look.ts";
import type { Provider } from "../../api/gen/Provider.ts";
import type { DashboardSettings } from "../../api/gen/DashboardSettings.ts";
import "./board.css";

const Tour = lazy(() => import("./Tour.tsx"));
const Editor = lazy(() => import("./Editor.tsx"));
const ProviderDialog = lazy(() => import("../settings/ProviderDialog.tsx"));

/** "6 running · 1 stopped" for a section's containers. */
function sectionSummary(view: BoardView, ids: (string | null | undefined)[]): string {
  const rows = ids.map((id) => view.resolved.find((r) => r.id === id)?.container).filter((c) => !!c);
  if (!rows.length) return "";
  const up = rows.filter((c) => c.state === "running").length;
  return up === rows.length ? `${up} running` : `${up} running · ${rows.length - up} stopped`;
}

/** A section on the read page: a heading that folds it and its canvas,
 * tiles in reading order (the phone layout is that order, one column). */
function BoardGroup({ section, view, updates }: { section: BoardSection; view: BoardView; updates: Record<string, WidgetView> }) {
  const [open, setOpen] = useState(!section.collapsed);
  const rects = section.items.map(rectOf);
  const id = `section-${section.id}`;
  // Not a landmark: two sections may share a title.
  return <section className="board-section" data-collapsed={open ? undefined : ""}>
    <header className="board-section-head">
      <Button variant="ghost" size="sm" className="section-toggle" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <ChevronDownIcon aria-hidden="true" className="section-chevron" /><h2>{section.title}</h2>
      </Button>
      <span className="section-count" aria-label={`${section.items.length} items`}>{section.items.length}</span>
      <span className="section-summary">{sectionSummary(view, section.items.map((item) => item.id))}</span>
    </header>
    {open && <div className="canvas" id={id} style={canvasVars(rowCount(rects))}>{readingOrder(rects).map((index) => {
      const item = section.items[index];
      if (!item) return null;
      const resolved = view.resolved.find((r) => r.id === item.id);
      const update = resolved?.widget ? updates[resolved.widget.id] : null;
      return <div key={item.id} className="canvas-item" style={cellVars(rectOf(item))}>
        <Tile item={item} resolved={resolved && update ? { ...resolved, widget: update } : resolved} />
      </div>;
    })}</div>}
  </section>;
}

export { Tile } from "./Tile.tsx";
export default function BoardPage() {
  const { route, search, setSearch } = useRoute();
  const { session } = useSession();
  const id = route.name === "board" ? route.id : "";
  const b = useBootstrap();
  const live = useContainers();
  const containerNames = live.data?.groups.flatMap((group) => group.containers.map((container) => container.name)).join("\0");
  const admin = b?.role === "admin" || b?.role === "owner";
  const [tourClosed, setTourClosed] = useState(false);
  const tourReplay = search.get("tour") === "1";
  const [editing, setEditing] = useState<BoardDocument | null>(null);
  const [picker, setPicker] = useState<"add" | "discover">();
  const [draft, setDraft] = useState<BoardDraft | null>(null);
  const [changed, setChanged] = useState(false);
  const [view, setView] = useState<BoardView | null>(null);
  const [widgetUpdates, setWidgetUpdates] = useState<Record<string, WidgetView>>({});
  const [error, setError] = useState("");
  const [missing, setMissing] = useState(false);
  const [catalog, setCatalog] = useState<{ providers: Provider[]; internet: boolean } | null>(null);
  async function openCatalog() {
    const [providers, settings] = await Promise.all([api.get<Provider[]>("/api/v1/providers"), api.get<DashboardSettings>("/api/v1/settings")]);
    if (providers.kind !== "ok") { setError(providers.message); return; }
    setCatalog({ providers: providers.data, internet: settings.kind === "ok" && settings.data.provider_internet });
  }
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
    }, id);
  }, [b, hasWidgets, editing, id]);
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
  const itemCount = view ? countItems(view.document) : 0;
  if (missing) return <NotFound path={`/app/boards/${id}`} />;
  if (!view) return <div className="page">{error || "Loading board…"}</div>;
  if (editing) return <div className="page board-page board-editing" {...boardLook(editing.appearance)}><Suspense fallback={<p>Opening editor…</p>}><Editor view={view} initial={editing} initialPicker={picker} scoped={!!b?.scoped} changed={changed} onRebase={() => setChanged(false)} onSave={(next) => { setView(next); setEditing(null); setChanged(false); setDraft(null); }} onCancel={() => { setEditing(null); setChanged(false); setDraft(null); void load(); }} /></Suspense></div>;
  return <div className="page board-page" data-testid="board" {...boardLook(view.document.appearance)}>
    <PageHeader title={view.board.name} icon={<ServiceIcon tint name={view.board.icon} title={view.board.name} enabled={b?.remote_icons ?? false} />}
      description={itemCount > 0 && <p className="muted">{itemCount} {itemCount === 1 ? "tile" : "tiles"} in {view.document.sections.length} {view.document.sections.length === 1 ? "section" : "sections"}</p>}
      actions={admin && <div className="page-actions board-actions">
        {!b?.scoped && <Button data-testid="board-add-integration" icon={<PlugIcon aria-hidden="true" />} onClick={() => void openCatalog()}>Add integration</Button>}
        <Button variant="default" data-testid="board-edit" icon={<PencilIcon aria-hidden="true" />} onClick={() => void openEditor()}>Edit board</Button>
      </div>} />
    {catalog && <Suspense fallback={null}><ProviderDialog providers={catalog.providers} item={null} preferredBoard={id} icons={!!b?.remote_icons} internet={catalog.internet}
      onClose={() => setCatalog(null)} onSaved={() => { setCatalog(null); void load(); }} /></Suspense>}
    <Dialog open={!!draft} title="Resume your board draft?" onClose={() => setDraft(null)} testId="board-draft" footer={<><Button data-testid="draft-discard" onClick={async () => { const result = await api.request("DELETE", `/api/v1/boards/${id}/draft`); if (result.kind === "ok") setDraft(null); else setError(result.message); }}>Discard draft</Button><Button data-testid="draft-resume" variant="default" onClick={() => { if (draft) { setPicker(undefined); setEditing(draft.document); } setDraft(null); }}>Resume draft</Button></>}><p>An unfinished edit is saved for your account. Resume it or discard it.</p></Dialog>
    {!draft && (tourReplay || (!tourClosed && session?.board_tour_pending && tourBoard(b) === id)) &&
      // A missing tour chunk leaves the board as it is.
      <LoadBoundary fallback={null} failed={null}><Tour admin={admin} empty={view.document.sections.length === 0} onClose={() => {
        setTourClosed(true);
        if (tourReplay) setSearch({ tour: null }, { replace: true });
      }} /></LoadBoundary>}
    {error && <Note kind="bad">{error}</Note>}
    {view.document.sections.length === 0 ? <div className="board-empty-card"><EmptyState icon={<LayoutGridIcon aria-hidden="true" />} title={admin ? "Add your first service" : "This board is empty"} testId="board-empty"
      actions={admin && <>
        <Button variant="default" data-testid="board-empty-add" icon={<PlusIcon aria-hidden="true" />} onClick={() => void openEditor("add")}>Add a service</Button>
        <Button data-testid="board-empty-discover" onClick={() => void openEditor("discover")}>Browse containers</Button>
        {!b?.scoped && <Button variant="ghost" icon={<PlugIcon aria-hidden="true" />} onClick={() => void openCatalog()}>Connect an integration</Button>}
      </>}>
      {admin ? "Pin containers, links and live widgets here. Pick from what Hobnail already found, or connect a service like Pi-hole." : "No sections or items have been added to this board yet."}
    </EmptyState></div> : view.document.sections.map((section) => <BoardGroup key={section.id} section={section} view={view} updates={widgetUpdates} />)}
  </div>;
}
