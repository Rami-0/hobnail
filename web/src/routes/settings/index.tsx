import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { api } from "../../api/session.ts";
import type { BoardSummary } from "../../api/gen/BoardSummary.ts";
import type { DashboardSettings } from "../../api/gen/DashboardSettings.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import type { IntegrationVerdict } from "../../api/gen/IntegrationVerdict.ts";
import { local, rememberSettingsTab } from "../../app/prefs.ts";
import { settingsTabs, type SettingsTab } from "../../app/settings-tabs.ts";
import { useRoute } from "../../app/router.ts";
import { fetchBootstrap, onConfigChange, useBootstrap } from "../../live/index.ts";
import { needsSecret } from "../../boards/document.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { Note, EmptyState } from "../../ui/Note.tsx";
import { TabNav } from "../../ui/TabNav.tsx";
import "./settings.css";

const Users = lazy(() => import("./Users.tsx"));
const Notifications = lazy(() => import("./Notifications.tsx"));
const Widgets = lazy(() => import("./Widgets.tsx"));
const General = lazy(() => import("./General.tsx"));
const Bookmarks = lazy(() => import("./Bookmarks.tsx"));

export default function SettingsPage() {
  const { route } = useRoute();
  const boot = useBootstrap();
  const tab = route.name === "settings" ? route.tab : "general";
  const owner = boot?.role === "owner";
  const admin = owner || boot?.role === "admin";
  const manage = admin && !boot?.scoped;
  const available = (Object.keys(settingsTabs) as SettingsTab[]).filter((id) =>
    id === "users" ? owner : manage || (!!boot?.scoped && (id === "boards" || id === "integrations")));
  const allowed = available.includes(tab);
  useEffect(() => { if (allowed) rememberSettingsTab(local(), tab); }, [allowed, tab]);
  return <div className="page settings-page"><h1>Settings</h1>
    {boot && !allowed && <Note kind="warn" testId="settings-locked">
      {tab === "users" ? "Only the owner can manage users." : "These settings require an administrator with access to everything."}
    </Note>}
    {available.length > 0 && <TabNav label="Settings" current={tab} testId="settings-tabs"
      items={available.map((id) => ({ id, label: settingsTabs[id], href: `/app/settings/${id}` }))} />}
    {allowed && tab === "boards" && <Boards admin={admin} manage={manage} />}
    {allowed && tab === "integrations" && <Integrations admin={admin} manage={manage} />}
    {allowed && tab === "users" && <Suspense fallback={<p role="status">Loading users…</p>}><Users /></Suspense>}
    {allowed && tab === "notifications" && <Suspense fallback={<p role="status">Loading notifications…</p>}><Notifications /></Suspense>}
    {allowed && tab === "widgets" && <Suspense fallback={<p role="status">Loading widgets…</p>}><Widgets /></Suspense>}
    {allowed && tab === "general" && <Suspense fallback={<p role="status">Loading general settings…</p>}><General /></Suspense>}
    {allowed && tab === "bookmarks" && <Suspense fallback={<p role="status">Loading bookmarks…</p>}><Bookmarks /></Suspense>}
  </div>;
}
function Boards({ admin, manage }: { admin: boolean; manage: boolean }) {
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [settings, setSettings] = useState<DashboardSettings>({ default_board: null });
  const [editing, setEditing] = useState<BoardSummary | "new" | null>(null);
  const [deleting, setDeleting] = useState<BoardSummary | null>(null);
  const [error, setError] = useState("");
  const [fields, setFields] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const [list, prefs] = await Promise.all([api.get<BoardSummary[]>("/api/v1/boards"), api.get<DashboardSettings>("/api/v1/settings")]);
    if (list.kind === "ok") setBoards(list.data); else setError(list.message);
    if (prefs.kind === "ok") setSettings(prefs.data);
  }, []);
  // load updates state only after the asynchronous server response.
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { void load(); return onConfigChange(() => { void load(); }); }, [load]);
  const refresh = async () => { await load(); await fetchBootstrap(); };
  const move = async (board: BoardSummary, other?: BoardSummary) => {
    if (!other || busy) return; setBusy(true); setError("");
    for (const [entry, position] of [[board, other.position], [other, board.position]] as const) {
      const result = await api.request("PATCH", `/api/v1/boards/${entry.id}`, { body: { version: entry.version, name: entry.name, icon: entry.icon, position } });
      if (result.kind !== "ok") { setError(result.message); break; }
    }
    await refresh(); setBusy(false);
  };
  return <section className="settings-section" data-testid="settings-boards"><header className="board-heading"><div><h2>Boards</h2><p className="muted">Choose what appears on each board. New boards start empty.</p></div>{manage && <Button variant="primary" data-testid="board-create" onClick={() => { setFields({}); setEditing("new"); }}>Create board</Button>}</header>
    {error && <Note kind="bad">{error}</Note>}
    {manage && <Select label="Default board" value={settings.default_board ?? ""} options={[{ value: "", label: "Home" }, ...boards.map((board) => ({ value: board.id, label: board.name }))]} onChange={async (value) => {
      const result = await api.request<DashboardSettings>("PATCH", "/api/v1/settings", { body: { default_board: value || null } });
      if (result.kind === "ok") { setSettings(result.data); await fetchBootstrap(); } else setError(result.message);
    }} />}
    {!boards.length && <EmptyState icon={<span>◇</span>} title="No boards yet">Create an empty board, then add the services and links you want.</EmptyState>}
    <div className="settings-list">{boards.map((board, index) => <div key={board.id} className="settings-row"><a href={`/app/boards/${board.id}`}>{board.name}</a>{admin && <div className="settings-actions">
      <Button aria-label={`Move ${board.name} up`} disabled={busy || index === 0} onClick={() => { void move(board, boards[index - 1]); }}>↑</Button>
      <Button aria-label={`Move ${board.name} down`} disabled={busy || index === boards.length - 1} onClick={() => { void move(board, boards[index + 1]); }}>↓</Button>
      <Button onClick={() => { setFields({}); setEditing(board); }}>Edit</Button>{manage && <Button className="danger" onClick={() => setDeleting(board)}>Delete</Button>}</div>}</div>)}</div>
    <Dialog open={editing !== null} title={editing === "new" ? "Create board" : "Edit board"} onClose={() => setEditing(null)} testId="board-form" footer={<><Button onClick={() => setEditing(null)}>Cancel</Button><Button type="submit" form="board-form" variant="primary" busy={busy}>Save</Button></>}>
      {editing && <form id="board-form" onSubmit={async (e) => { e.preventDefault(); if (busy) return; const form = new FormData(e.currentTarget); setBusy(true); setFields({});
        const body = { name: String(form.get("name")), icon: String(form.get("icon")) || null, ...(editing === "new" ? {} : { version: editing.version, position: editing.position }) };
        const result = await api.request(editing === "new" ? "POST" : "PATCH", `/api/v1/boards${editing === "new" ? "" : `/${editing.id}`}`, { body });
        if (result.kind === "ok") { setEditing(null); await refresh(); } else { setFields({ [result.kind === "invalid" ? result.field ?? "form" : "form"]: result.message }); }
        setBusy(false);
      }}><Field id="board-name" label="Name" error={fields.name}><TextInput name="name" required maxLength={80} defaultValue={editing === "new" ? "" : editing.name} /></Field>
        <Field id="board-icon" label="Icon name" hint="Dashboard icon name. Icons appear only when remote icons are enabled." error={fields.icon}><TextInput name="icon" defaultValue={editing === "new" ? "" : editing.icon ?? ""} /></Field>{fields.form && <Note kind="bad">{fields.form}</Note>}
      </form>}
    </Dialog>
    <Dialog open={!!deleting} title="Delete board?" onClose={() => setDeleting(null)} footer={<><Button onClick={() => setDeleting(null)}>Keep board</Button><Button className="danger" busy={busy} onClick={async () => {
      if (!deleting) return; setBusy(true); const result = await api.request("DELETE", `/api/v1/boards/${deleting.id}`);
      if (result.kind === "ok") { setDeleting(null); await refresh(); } else setError(result.message); setBusy(false);
    }}>Delete board</Button></>}><p>Delete “{deleting?.name}” and its layout? Services and integrations remain available.</p></Dialog>
  </section>;
}
function Integrations({ admin, manage }: { admin: boolean; manage: boolean }) {
  const [items, setItems] = useState<IntegrationView[]>([]);
  const [editing, setEditing] = useState<IntegrationView | "new" | null>(null);
  const [deleting, setDeleting] = useState<IntegrationView | null>(null);
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [verdicts, setVerdicts] = useState<Record<string, IntegrationVerdict>>({});
  const forgetVerdict = (id: string) => setVerdicts(({ [id]: _old, ...remaining }) => remaining);
  const load = useCallback(async () => { const result = await api.get<IntegrationView[]>("/api/v1/integrations"); if (result.kind === "ok") setItems(result.data); else setError(result.message); }, []);
  // load updates state only after the asynchronous server response.
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { void load(); return onConfigChange(() => { void load(); }); }, [load]);
  const close = () => { setEditing(null); setSecret(""); setFields({}); };
  const edit = (item: IntegrationView | "new") => { setEditing(item); setUrl(item === "new" ? "" : (item.admin?.base_url ?? item.origin)); setSecret(""); setEnabled(item === "new" ? true : item.admin?.enabled ?? true); setFields({}); };
  const bound = editing && editing !== "new" ? editing.admin?.secret_origin : null;
  const changed = needsSecret(bound, url);
  return <section className="settings-section" data-testid="settings-integrations"><header className="board-heading"><div><h2>Integrations</h2><p className="muted">Connect an HTTP service. Connection tests run from Hobnail.</p></div>{manage && <Button variant="primary" data-testid="integration-create" onClick={() => edit("new")}>Add integration</Button>}</header>
    {error && <Note kind="bad">{error}</Note>}
    {!items.length && <EmptyState icon={<span>↗</span>} title="No integrations yet">Add a connection to a service you use.</EmptyState>}
    <div className="settings-list">{items.map((item) => <div key={item.id} className="settings-row" data-testid="integration-row"><div><h3>{item.name}</h3><p className="muted">{(item.admin?.base_url ?? item.origin)}</p>{item.admin?.enabled === false && <p>Disabled</p>}
      {verdicts[item.id] && <Note kind={verdicts[item.id]?.ok ? "ok" : "warn"} testId="integration-verdict">{verdicts[item.id]?.class}: {verdicts[item.id]?.message}{verdicts[item.id]?.class === "tls" && " Only public certificate authorities are trusted."}</Note>}</div>
      {admin && <div className="settings-actions"><Button onClick={() => edit(item)}>Edit</Button><Button busy={busy} disabled={item.admin?.enabled === false} data-testid="integration-test" onClick={async () => {
        setBusy(true); const result = await api.post<IntegrationVerdict>(`/api/v1/integrations/${item.id}/test`);
        if (result.kind === "ok") setVerdicts((old) => ({ ...old, [item.id]: result.data })); else setError(result.message); setBusy(false);
      }}>Test connection</Button>{manage && <Button className="danger" onClick={() => setDeleting(item)}>Delete</Button>}</div>}
    </div>)}</div>
    <Dialog open={!!editing} title={editing === "new" ? "Add integration" : "Edit integration"} onClose={close} testId="integration-form" footer={<><Button onClick={close}>Cancel</Button><Button type="submit" form="integration-form" variant="primary" busy={busy}>Save</Button></>}>
      {editing && <form id="integration-form" onSubmit={async (e) => {
        e.preventDefault(); if (busy) return; setFields({});
        if (changed && !secret) { setFields({ secret: "Enter the credential again for the new origin, or clear the saved credential first." }); return; }
        const connectionChanged = editing === "new" || url !== (editing.admin?.base_url ?? editing.origin) || !!secret || enabled !== (editing.admin?.enabled ?? true);
        const data = new FormData(e.currentTarget); setBusy(true);
        const result = await api.request<IntegrationView>(editing === "new" ? "POST" : "PUT", `/api/v1/integrations${editing === "new" ? "" : `/${editing.id}`}`, { body: {
          name: String(data.get("name")), kind: "http", base_url: url, enabled, icon: String(data.get("icon")) || null, ...(secret ? { secret } : {}),
        } });
        if (result.kind === "ok") { if (connectionChanged) forgetVerdict(result.data.id); close(); await load(); } else setFields({ [result.kind === "invalid" ? result.field ?? "form" : "form"]: result.message }); setBusy(false);
      }}>
        <Field id="integration-name" label="Name" error={fields.name}><TextInput name="name" required maxLength={80} defaultValue={editing === "new" ? "" : editing.name} /></Field>
        <Field id="integration-url" label="Service URL" error={fields.base_url}><TextInput name="base_url" type="url" required value={url} onChange={(e) => setUrl(e.target.value)} /></Field>
        <Field id="integration-icon" label="Icon name" error={fields.icon}><TextInput name="icon" defaultValue={editing === "new" ? "" : editing.admin?.icon ?? ""} /></Field>
        <Field id="integration-secret" label="Bearer credential" error={fields.secret} hint={bound ? `A credential is saved for ${bound}. The credential is bound to the origin (scheme, host, and port). Path changes keep the binding.` : "Optional. The credential is bound to the origin (scheme, host, and port). Path changes keep the binding."}><TextInput name="secret" type="password" autoComplete="new-password" value={secret} onChange={(e) => setSecret(e.target.value)} /></Field>
        {secret && /^http:\/\//i.test(url.trim()) && <Note kind="warn">This HTTP connection sends the credential unencrypted.</Note>}
        {changed && <Note kind="warn">The origin changed. Enter the credential again for this destination.</Note>}
        {bound && editing !== "new" && <Button busy={busy} onClick={async () => {
          setBusy(true); const result = await api.request<IntegrationView>("DELETE", `/api/v1/integrations/${editing.id}/secret`);
          if (result.kind === "ok") { setEditing(result.data); setSecret(""); forgetVerdict(result.data.id); await load(); } else setFields({ secret: result.message }); setBusy(false);
        }}>Clear saved credential</Button>}
        <Switch checked={enabled} onChange={setEnabled} label="Enabled" />{fields.form && <Note kind="bad">{fields.form}</Note>}
      </form>}
    </Dialog>
    <Dialog open={!!deleting} title="Delete integration?" onClose={() => setDeleting(null)} footer={<><Button onClick={() => setDeleting(null)}>Keep integration</Button><Button className="danger" busy={busy} onClick={async () => {
      if (!deleting) return; setBusy(true); const result = await api.request("DELETE", `/api/v1/integrations/${deleting.id}`);
      if (result.kind === "ok") { forgetVerdict(deleting.id); setDeleting(null); await load(); } else setError(result.message); setBusy(false);
    }}>Delete integration</Button></>}><p>Delete “{deleting?.name}” and its saved credential? Board items will show a missing target.</p></Dialog>
  </section>;
}
