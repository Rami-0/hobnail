import { useCallback, useEffect, useState } from "react";
import type { BoardSummary } from "../../api/gen/BoardSummary.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import type { Invite } from "../../api/gen/Invite.ts";
import type { IssuedInvite } from "../../api/gen/IssuedInvite.ts";
import type { ManagedUser } from "../../api/gen/ManagedUser.ts";
import type { Role } from "../../api/gen/Role.ts";
import type { Scopes } from "../../api/gen/Scopes.ts";
import type { ScopeTargets } from "../../api/gen/ScopeTargets.ts";
import type { Method } from "../../api/client.ts";
import { api } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { toast } from "../../ui/toast.ts";
import { emptyScopes, toggleScope } from "./scopes.ts";

const roles = [{ value: "viewer", label: "Viewer" }, { value: "admin", label: "Admin" }];

type Edit =
  | { kind: "invite" }
  | { kind: "revoke"; invite: Invite }
  | { kind: "role" | "scopes" | "disabled" | "sign-out" | "delete"; user: ManagedUser };

function inviteStatus(invite: Invite) {
  if (invite.used_at !== null) return "Used";
  if (invite.revoked_at !== null) return "Revoked";
  return invite.expires <= Date.now() / 1000 ? "Expired" : "Pending";
}

function ScopeFields({ scopes, targets, onChange }: { scopes: Scopes | null; targets: ScopeTargets; onChange: (value: Scopes | null) => void }) {
  return <>
    <Select label="Access" value={scopes ? "limited" : "all"}
      options={[{ value: "all", label: "Everything" }, { value: "limited", label: "Selected boards and integrations" }]}
      onChange={(value) => onChange(value === "all" ? null : emptyScopes())} />
    {scopes && (["boards", "integrations"] as const).map((kind) => <fieldset key={kind} className="settings-form">
      <legend className="settings-role">{kind}</legend>
      {targets[kind].length === 0 && <p className="muted">No {kind} available.</p>}
      {targets[kind].map((target) => <Switch key={target.id} label={target.name} aria-label={target.name} checked={scopes[kind].includes(target.id)}
        onChange={(checked) => onChange(toggleScope(scopes, kind, target.id, checked))} />)}
    </fieldset>)}
  </>;
}

function operation(edit: Edit, role: Role, scopes: Scopes | null): { path: string; method: Method; body?: unknown } {
  if (edit.kind === "invite") return { path: "/invites", method: "POST", body: { role, scopes } };
  if (edit.kind === "revoke") return { path: `/invites/${edit.invite.id}`, method: "DELETE" };
  const base = `/users/${edit.user.id}`;
  switch (edit.kind) {
    case "role": return { path: base + "/role", method: "PATCH", body: { role } };
    case "scopes": return { path: base + "/scopes", method: "PUT", body: { scopes } };
    case "disabled": return { path: base + "/disabled", method: "PATCH", body: { disabled: !edit.user.disabled } };
    case "sign-out": return { path: base + "/sign-out", method: "POST" };
    case "delete": return { path: base, method: "DELETE" };
  }
}

export default function Users() {
  const [users, setUsers] = useState<ManagedUser[]>([]);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [targets, setTargets] = useState<ScopeTargets>({ boards: [], integrations: [] });
  const [failure, setFailure] = useState("");
  const [error, setError] = useState("");
  const [edit, setEdit] = useState<Edit | null>(null);
  const [role, setRole] = useState<Role>("viewer");
  const [scopes, setScopes] = useState<Scopes | null>(null);
  const [busy, setBusy] = useState(false);
  const [link, setLink] = useState("");
  const [copied, setCopied] = useState(false);
  const load = useCallback(async (signal?: AbortSignal) => {
    const results = await Promise.all([
      api.get<ManagedUser[]>("/api/v1/users", { signal }),
      api.get<Invite[]>("/api/v1/invites", { signal }),
      api.get<BoardSummary[]>("/api/v1/boards", { signal }),
      api.get<IntegrationView[]>("/api/v1/integrations", { signal }),
    ]);
    if (signal?.aborted) return;
    const [accounts, invitations, boards, integrations] = results;
    if (accounts.kind === "ok") setUsers(accounts.data);
    else setUsers([]);
    if (invitations.kind === "ok") setInvites(invitations.data);
    else setInvites([]);
    if (boards.kind === "ok" && integrations.kind === "ok") setTargets({ boards: boards.data, integrations: integrations.data });
    const refused = results.find((result) => result.kind !== "ok");
    setFailure(refused?.message ?? "");
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    void Promise.resolve().then(() => load(controller.signal));
    return () => controller.abort();
  }, [load]);
  function open(next: Edit) {
    setRole("user" in next ? next.user.role : "viewer");
    setScopes("user" in next ? next.user.scopes : null);
    setError("");
    setEdit(next);
  }
  const title = edit && ({
    invite: "Create invitation", revoke: "Revoke invitation", role: "Change role",
    scopes: "Edit access", disabled: "user" in edit && edit.user.disabled ? "Enable account" : "Disable account",
    "sign-out": "Force sign-out", delete: "Delete account",
  }[edit.kind]);
  const closeLink = () => { setLink(""); setCopied(false); setError(""); };
  return <section className="settings-section" data-testid="settings-users">
    <h2>Users</h2>
    {failure && <Note kind="bad">{failure}</Note>}
    <div className="settings-list">{users.map((user) => {
      const granted = [...(user.scope_targets?.boards ?? []), ...(user.scope_targets?.integrations ?? [])];
      return <div className="settings-row" key={user.id} data-testid="user-row" data-user-id={user.id}>
        <div><h3>{user.name}</h3>
          <p><span className="settings-role">{user.role}</span> · {user.disabled ? "Disabled" : "Active"}</p>
          <p>{user.scopes ? granted.map((target) => target.name).join(", ") || "No boards or integrations" : "Everything"}</p>
          {user.role === "owner" && <p className="muted">The owner account is protected. Its role and access are permanent, and it cannot be disabled or deleted.</p>}
        </div>
        {user.role !== "owner" && <div className="settings-actions">
          <Button onClick={() => open({ kind: "role", user })}>Change role</Button>
          <Button onClick={() => open({ kind: "scopes", user })}>Edit access</Button>
          <Button onClick={() => open({ kind: "disabled", user })}>{user.disabled ? "Enable" : "Disable"}</Button>
          <Button onClick={() => open({ kind: "sign-out", user })}>Force sign-out</Button>
          <Button className="danger" onClick={() => open({ kind: "delete", user })}>Delete</Button>
        </div>}
      </div>;
    })}</div>
    <header className="board-heading"><h2>Invitations</h2><Button variant="primary" data-testid="invite-create" onClick={() => open({ kind: "invite" })}>Create invitation</Button></header>
    <div className="settings-list">{invites.map((invite) => {
      const status = inviteStatus(invite);
      return <div className="settings-row" key={invite.id} data-testid="invite-row">
        <div><span className="settings-role">{invite.role}</span> · {status}
          <p className="muted">Expires {new Date(invite.expires * 1000).toLocaleString()}</p>
        </div>
        {status === "Pending" && <Button onClick={() => open({ kind: "revoke", invite })}>Revoke</Button>}
      </div>;
    })}</div>
    <Dialog open={edit !== null} title={title ?? ""} onClose={() => { if (!busy) setEdit(null); }}
      guard={() => edit?.kind === "invite" ? role !== "viewer" || scopes !== null
        : !!edit && "user" in edit && (role !== edit.user.role || scopes !== edit.user.scopes)}
      footer={<Button type="submit" form="manage-user" variant="primary" busy={busy}>{title}</Button>}>
      {edit && <form id="manage-user" className="settings-form" onSubmit={async (event) => {
        event.preventDefault();
        if (busy) return;
        const { path, method, body } = operation(edit, role, scopes);
        setBusy(true);
        setError("");
        const result = await api.request<IssuedInvite>(method, `/api/v1${path}`, { body });
        if (result.kind === "ok") {
          setEdit(null);
          if (edit.kind === "invite") setLink(new URL(`/app/invite/${result.data.token}`, window.location.origin).href);
          else toast("Changes saved.", { kind: "ok" });
          await load();
        } else setError(result.message);
        setBusy(false);
      }}>
        {"user" in edit && <p>{title} for <strong>{edit.user.name}</strong>?</p>}
        {(edit.kind === "role" || edit.kind === "invite") && <Select label="Role" value={role} options={roles} onChange={(value) => setRole(value === "admin" ? "admin" : "viewer")} />}
        {(edit.kind === "scopes" || edit.kind === "invite") && <ScopeFields scopes={scopes} targets={targets} onChange={setScopes} />}
        {edit.kind === "revoke" && <p>Revoke this invitation? Its link will stop working.</p>}
        {error && <Note kind="bad">{error}</Note>}
      </form>}
    </Dialog>
    <Dialog open={!!link} title="Invitation link" onClose={closeLink}
      footer={<><Button onClick={closeLink}>Done</Button><Button variant="primary" onClick={async () => {
        try { await navigator.clipboard.writeText(link); setCopied(true); }
        catch { setError("Select the link and copy it manually."); }
      }}>Copy link</Button></>}>
      <Field id="new-invite-link" label="Copy this link before closing" hint="This link is shown only once."><TextInput readOnly value={link} data-testid="invite-link" onFocus={(event) => event.currentTarget.select()} /></Field>
      {copied && <Note kind="ok">Link copied.</Note>}
      {error && <Note kind="bad">{error}</Note>}
    </Dialog>
  </section>;
}
