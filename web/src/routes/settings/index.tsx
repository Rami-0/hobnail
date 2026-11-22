import { PageHeader } from "../../ui/PageHeader.tsx";
import { BellIcon, BookmarkIcon, BoxIcon, HistoryIcon, LayoutGridIcon, LockIcon, PlugIcon, SlidersHorizontalIcon, SunIcon, TerminalIcon, UsersIcon, ZapIcon } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { api } from "../../api/session.ts";
import type { BoardSummary } from "../../api/gen/BoardSummary.ts";
import type { DashboardSettings } from "../../api/gen/DashboardSettings.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import { IntegrationStatus } from "../../ui/IntegrationStatus.tsx";
import { local, rememberSettingsTab } from "../../app/prefs.ts";
import { settingsTabs, type SettingsTab } from "../../app/settings-tabs.ts";
import { useRoute } from "../../app/router.ts";
import { fetchBootstrap, onConfigChange, useBootstrap } from "../../live/index.ts";
import { tourHref } from "../../boards/tour.ts";
import type { Provider } from "../../api/gen/Provider.ts";
import { Switch } from "../../ui/Switch.tsx";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { Note, EmptyState } from "../../ui/Note.tsx";
import { TabNav } from "../../ui/TabNav.tsx";
import "./settings.css";

import Users from "./Users.tsx";
import Config from "./Config.tsx";
import Security from "./Security.tsx";
import History from "./History.tsx";
import Appearance from "./Appearance.tsx";
import Containers from "./Containers.tsx";
import Notifications from "./Notifications.tsx";
import Widgets from "./Widgets.tsx";
import General from "./General.tsx";
import Bookmarks from "./Bookmarks.tsx";

const ProviderDialog = lazy(() => import("./ProviderDialog.tsx"));

const panels = {
  users: Users,
  config: Config,
  security: Security,
  history: History,
  appearance: Appearance,
  containers: Containers,
  notifications: Notifications,
  widgets: Widgets,
  general: General,
  bookmarks: Bookmarks,
};

const tabIcons = {
  general: SlidersHorizontalIcon, containers: BoxIcon, bookmarks: BookmarkIcon,
  widgets: ZapIcon, notifications: BellIcon, appearance: SunIcon, history: HistoryIcon,
  config: TerminalIcon, security: LockIcon, boards: LayoutGridIcon, integrations: PlugIcon, users: UsersIcon,
} satisfies Record<SettingsTab, typeof BoxIcon>;

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
  const tour = tourHref(boot);
  const Panel = tab !== "boards" && tab !== "integrations" ? panels[tab] : null;
  useEffect(() => { if (allowed) rememberSettingsTab(local(), tab); }, [allowed, tab]);
  return <div className={available.length > 0 ? "page set" : "page set locked"}>
    <PageHeader title="Settings" actions={allowed && tour && <a className="btn btn-outline" href={tour}>Replay board tour</a>} />
    {available.length > 0 && <TabNav label="Settings sections" current={tab} testId="settings-tabs" layout="settings"
      items={available.map((id) => {
        const Icon = tabIcons[id];
        return { id, label: <><Icon aria-hidden="true" /><span>{settingsTabs[id]}</span></>, href: "/app/settings/" + id };
      })} />}
    <div className="set-main">
      {boot && !allowed && <Note kind="warn" testId="settings-locked">
        {tab === "users" ? "Only the owner can manage users." : "These settings require an administrator with access to everything."}
      </Note>}
      {allowed && tab === "boards" && <Boards admin={admin} manage={manage} />}
      {allowed && tab === "integrations" && <Integrations admin={admin} manage={manage} />}
      {allowed && Panel && <Panel />}
    </div>
  </div>;
}
function Boards({ admin, manage }: { admin: boolean; manage: boolean }) {
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [settings, setSettings] = useState<DashboardSettings>({ default_board: null, default_theme: null, provider_internet: false });
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
  return <section className="settings-section" data-testid="settings-boards"><header className="board-heading"><div><h2>Boards</h2><p className="muted">Choose what appears on each board. New boards start empty.</p></div>{manage && <Button variant="default" data-testid="board-create" onClick={() => { setFields({}); setEditing("new"); }}>Create board</Button>}</header>
    {error && <Note kind="bad">{error}</Note>}
    {manage && <Select label="Default board" value={settings.default_board ?? ""} options={[{ value: "", label: "Home" }, ...boards.map((board) => ({ value: board.id, label: board.name }))]} onChange={async (value) => {
      const result = await api.request<DashboardSettings>("PATCH", "/api/v1/settings", { body: { default_board: value || null } });
      if (result.kind === "ok") { setSettings(result.data); await fetchBootstrap(); } else setError(result.message);
    }} />}
    {!boards.length && <EmptyState level={3} icon={<span>◇</span>} title="No boards yet">Create an empty board, then add the services and links you want.</EmptyState>}
    <div className="settings-list">{boards.map((board, index) => <div key={board.id} className="settings-row"><a href={`/app/boards/${board.id}`}>{board.name}</a>{admin && <div className="settings-actions">
      <Button variant="ghost" size="icon" aria-label={`Move ${board.name} up`} disabled={busy || index === 0} onClick={() => { void move(board, boards[index - 1]); }}>↑</Button>
      <Button variant="ghost" size="icon" aria-label={`Move ${board.name} down`} disabled={busy || index === boards.length - 1} onClick={() => { void move(board, boards[index + 1]); }}>↓</Button>
      <Button variant="ghost" onClick={() => { setFields({}); setEditing(board); }}>Edit</Button>{manage && <Button variant="ghost" onClick={() => setDeleting(board)}>Delete</Button>}</div>}</div>)}</div>
    <Dialog open={editing !== null} title={editing === "new" ? "Create board" : "Edit board"} onClose={() => setEditing(null)} testId="board-form" footer={<><Button onClick={() => setEditing(null)}>Cancel</Button><Button type="submit" form="board-form" variant="default" busy={busy}>Save</Button></>}>
      {editing && <form id="board-form" onSubmit={async (e) => { e.preventDefault(); if (busy) return; const form = new FormData(e.currentTarget); setBusy(true); setFields({});
        const body = { name: String(form.get("name")), icon: String(form.get("icon")) || null, ...(editing === "new" ? {} : { version: editing.version, position: editing.position }) };
        const result = await api.request(editing === "new" ? "POST" : "PATCH", `/api/v1/boards${editing === "new" ? "" : `/${editing.id}`}`, { body });
        if (result.kind === "ok") { setEditing(null); await refresh(); } else { setFields({ [result.kind === "invalid" ? result.field ?? "form" : "form"]: result.message }); }
        setBusy(false);
      }}><Field id="board-name" label="Name" error={fields.name}><TextInput name="name" required maxLength={80} defaultValue={editing === "new" ? "" : editing.name} /></Field>
        <Field id="board-icon" label="Icon name" hint="Dashboard icon name. Icons appear only when remote icons are enabled." error={fields.icon}><TextInput name="icon" defaultValue={editing === "new" ? "" : editing.icon ?? ""} /></Field>{fields.form && <Note kind="bad">{fields.form}</Note>}
      </form>}
    </Dialog>
    <Dialog open={!!deleting} title="Delete board?" onClose={() => setDeleting(null)} footer={<><Button onClick={() => setDeleting(null)}>Keep board</Button><Button variant="destructive" busy={busy} onClick={async () => {
      if (!deleting) return; setBusy(true); const result = await api.request("DELETE", `/api/v1/boards/${deleting.id}`);
      if (result.kind === "ok") { setDeleting(null); await refresh(); } else setError(result.message); setBusy(false);
    }}>Delete board</Button></>}><p>Delete “{deleting?.name}” and its layout? Services and integrations remain available.</p></Dialog>
  </section>;
}
function Integrations({ admin, manage }: { admin: boolean; manage: boolean }) {
  const [items, setItems] = useState<IntegrationView[]>([]);
  const [editing, setEditing] = useState<IntegrationView | "new" | null>(null);
  const [deleting, setDeleting] = useState<IntegrationView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [internet, setInternet] = useState(false);
  const boot = useBootstrap();
  const board = useRoute().search.get("board") ?? "";
  const load = useCallback(async () => {
    const [result, catalog, settings] = await Promise.all([
      api.get<IntegrationView[]>("/api/v1/integrations"), api.get<Provider[]>("/api/v1/providers"),
      api.get<DashboardSettings>("/api/v1/settings"),
    ]);
    if (result.kind === "ok") setItems(result.data); else setError(result.message);
    if (catalog.kind === "ok") setProviders(catalog.data); else setError(catalog.message);
    if (settings.kind === "ok") setInternet(settings.data.provider_internet);
  }, []);
  // eslint-disable-next-line react/set-state-in-effect
  useEffect(() => { void load(); return onConfigChange(() => { void load(); }); }, [load]);
  const close = () => setEditing(null);
  return <section className="settings-section" data-testid="settings-integrations"><header className="board-heading"><div><h2>Integrations</h2><p className="muted">Connect an HTTP service. Connection tests run from Hobnail.</p></div>{manage && <Button variant="default" data-testid="integration-create" disabled={!providers.length} onClick={() => setEditing("new")}>Add integration</Button>}</header>
    {error && <Note kind="bad">{error}</Note>}
    {!items.length && <EmptyState level={3} icon={<span>↗</span>} title="No integrations yet">Add a connection to a service you use.</EmptyState>}
    <div className="settings-list">{items.map((item) => <div key={item.id} className="settings-row" data-testid="integration-row"><div><h3>{item.name}</h3><p className="muted">{item.admin?.base_url ?? item.origin}</p><IntegrationStatus integration={item} /></div>
      {admin && <div className="settings-actions"><Button variant="ghost" disabled={!providers.length} onClick={() => setEditing(item)}>Edit</Button><Button busy={busy} disabled={!item.enabled} data-testid="integration-test" onClick={async () => {
        setBusy(true); const result = await api.post(`/api/v1/integrations/${item.id}/test`);
        if (result.kind === "ok") await load(); else setError(result.message); setBusy(false);
      }}>Test connection</Button>{manage && <Button variant="ghost" onClick={() => setDeleting(item)}>Delete</Button>}</div>}
    </div>)}</div>
    {editing && <Suspense fallback={<p>Opening integration catalog…</p>}>
      <ProviderDialog providers={providers} item={editing === "new" ? null : editing} preferredBoard={board}
        icons={!!boot?.remote_icons} internet={internet} onClose={close} onSaved={() => { close(); void load(); }} />
    </Suspense>}
    {manage && <div className="settings-section">
      <h3>Provider internet access</h3>
      <p>{internet ? "Providers may contact public destinations." : "Providers are blocked from public destinations."}</p>
      <Switch label="Allow provider internet access" checked={internet} onChange={async (provider_internet) => {
        const result = await api.request<DashboardSettings>("PATCH", "/api/v1/settings", { body: { provider_internet } });
        if (result.kind === "ok") setInternet(result.data.provider_internet); else setError(result.message);
      }} />
      {providers.map((provider) => <p key={provider.id}><strong>{provider.name}</strong> · {provider.contacts}</p>)}
    </div>}
    <Dialog open={!!deleting} title="Delete integration?" onClose={() => setDeleting(null)} footer={<><Button onClick={() => setDeleting(null)}>Keep integration</Button><Button variant="destructive" busy={busy} onClick={async () => {
      if (!deleting) return; setBusy(true); const result = await api.request("DELETE", `/api/v1/integrations/${deleting.id}`);
      if (result.kind === "ok") { setDeleting(null); await load(); } else setError(result.message); setBusy(false);
    }}>Delete integration</Button></>}><p>Delete “{deleting?.name}” and its saved credential? Board items will show a missing target.</p></Dialog>
  </section>;
}
