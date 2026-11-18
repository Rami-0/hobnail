import { useCallback, useEffect, useState } from "react";
import { api, useSession } from "../../api/session.ts";
import type { SessionInfo } from "../../api/gen/SessionInfo.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { toast } from "../../ui/toast.ts";
import { passwordErrors } from "./password.ts";
import "../settings/settings.css";

const sessionsPath = "/api/v1/account/sessions";

export function Sessions() {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [revoke, setRevoke] = useState<string | null>(null);
  const load = useCallback(async (signal?: AbortSignal) => {
    const result = await api.get<SessionInfo[]>(sessionsPath, { signal });
    if (signal?.aborted) return;
    if (result.kind === "ok") {
      setSessions(result.data);
      setError("");
    } else setError(result.message);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    void Promise.resolve().then(() => load(controller.signal));
    return () => controller.abort();
  }, [load]);
  return <section className="settings-section" data-testid="account-sessions">
    <h2>Sessions</h2>
    <p>This session stays signed in when you revoke other sessions.</p>
    {error && revoke === null && <Note kind="bad">{error}</Note>}
    <div className="settings-list">{sessions.map((session) => <div className="settings-row" key={session.id}>
      <div>Signed in {new Date(session.created * 1000).toLocaleString()}<br />
        <span className="muted">Expires {new Date(session.expires * 1000).toLocaleString()}</span>
      </div>
      <Button onClick={() => { setError(""); setRevoke(session.id); }}>Sign out session</Button>
    </div>)}</div>
    {sessions.length > 0 ? <div><Button onClick={() => { setError(""); setRevoke(""); }}>Sign out all other sessions</Button></div> : <p className="muted">No other sessions.</p>}
    <Dialog open={revoke !== null} title="Sign out other sessions?" onClose={() => { if (!busy) setRevoke(null); }}
      footer={<Button busy={busy} className="danger" onClick={async () => {
        if (busy || revoke === null) return;
        setBusy(true);
        const result = await api.request("DELETE", revoke ? `${sessionsPath}/${encodeURIComponent(revoke)}` : sessionsPath);
        if (result.kind === "ok") {
          setRevoke(null);
          await load();
          toast("Sessions signed out.", { kind: "ok" });
        } else setError(result.message);
        setBusy(false);
      }}>Sign out</Button>}>
      <p>{revoke ? "Sign out this other session?" : "Sign out every session for your account except this one?"}</p>
      {error && <Note kind="bad">{error}</Note>}
    </Dialog>
  </section>;
}

export default function AccountPage() {
  const user = useSession().session?.user;
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [passwordRevision, setPasswordRevision] = useState(0);
  return <div className="page settings-page" data-testid="account-page">
    <h1>Account</h1>
    <p>{user?.name} · <span className="settings-role">{user?.role}</span></p>
    <section className="settings-section">
      <h2>Change password</h2>
      <form className="settings-form" data-testid="account-password" noValidate onSubmit={async (event) => {
        event.preventDefault();
        if (busy) return;
        const form = event.currentTarget;
        const data = new FormData(form);
        const current = String(data.get("current") ?? "");
        const password = String(data.get("new") ?? "");
        const issues = passwordErrors(current, password, String(data.get("confirm") ?? ""));
        setErrors(issues);
        if (Object.keys(issues).length) {
          form.querySelector<HTMLInputElement>(`[name="${Object.keys(issues)[0]}"]`)?.focus();
          return;
        }
        setBusy(true);
        const result = await api.post("/api/v1/account/password", { current, new: password });
        if (result.kind === "ok") {
          form.reset();
          setPasswordRevision((revision) => revision + 1);
          toast("Password changed. Other sessions were signed out.", { kind: "ok" });
        } else {
          const field = result.kind === "invalid" ? result.field : result.message === "the current password is wrong" ? "current" : undefined;
          setErrors({ [field ?? "form"]: result.message });
          if (field === "current" || field === "new") form.querySelector<HTMLInputElement>(`[name="${field}"]`)?.focus();
        }
        setBusy(false);
      }}>
        <Field id="account-current" label="Current password" error={errors.current}><TextInput name="current" type="password" autoComplete="current-password" required /></Field>
        <Field id="account-new" label="New password" error={errors.new}><TextInput name="new" type="password" autoComplete="new-password" required /></Field>
        <Field id="account-confirm" label="Confirm new password" error={errors.confirm}><TextInput name="confirm" type="password" autoComplete="new-password" required /></Field>
        {errors.form && <Note kind="bad">{errors.form}</Note>}
        <div><Button type="submit" variant="primary" busy={busy}>Change password</Button></div>
      </form>
    </section>
    <Sessions key={passwordRevision} />
  </div>;
}
