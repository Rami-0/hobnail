import { LockIcon } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";

import type { InviteValidation } from "../api/gen/InviteValidation.ts";
import { api, refreshSession, useSession } from "../api/session.ts";
import { safeNext, useRoute } from "../app/router.ts";
import { Button } from "../ui/index.ts";

export function Login({ token }: { token?: string }) {
  const { session } = useSession();
  const loc = useRoute();
  const next = safeNext(loc.search.get("next"));
  if (session?.onboarding_locked) {
    return <main className="login"><p>Run <code>hobnail set-password</code> on the host to create the owner account, then reload this page.</p></main>;
  }
  const mode = token !== undefined ? "invite" : session?.owner_exists === false ? "onboarding" : "login";
  return <Credentials key={mode + (token ?? "")} mode={mode} token={token} next={next} />;
}

function Credentials({ mode, token, next }: {
  mode: "login" | "onboarding" | "invite";
  token: string | undefined;
  next: string;
}) {
  const creating = mode !== "login";
  const title = mode === "invite" ? "Accept invitation" : creating ? "Set owner password" : "Sign in to Hobnail";
  const path = mode === "invite" ? "/api/v1/invites/accept" : creating ? "/api/v1/onboarding/owner" : "/api/login";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [wait, setWait] = useState(0);
  const [invite, setInvite] = useState<InviteValidation>();

  useEffect(() => {
    if (!token) return;
    const controller = new AbortController();
    void api.get<InviteValidation>(`/api/v1/invites/validate?token=${encodeURIComponent(token)}`, { signal: controller.signal }).then((r) => {
      if (!controller.signal.aborted && r.kind === "ok") setInvite(r.data);
    });
    return () => controller.abort();
  }, [token]);

  useEffect(() => {
    if (wait <= 0) return;
    const timer = window.setTimeout(() => setWait(wait - 1), 1000);
    return () => window.clearTimeout(timer);
  }, [wait]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || wait > 0 || invite?.valid === false) return;
    const form = new FormData(event.currentTarget);
    const username = String(form.get("username"));
    const password = String(form.get("password"));
    if (creating && password !== form.get("confirm")) {
      setError("The passwords do not match.");
      return;
    }
    setBusy(true);
    setError("");
    const r = await api.post(path, { username, password, token });
    if (r.kind === "ok") {
      window.location.replace(mode === "invite" ? "/app" : next);
      return;
    }
    setBusy(false);
    if (r.kind === "rate_limited") {
      setWait(r.retryAfter);
      return;
    }
    if (r.kind === "stale" && mode === "onboarding") {
      await refreshSession();
      return;
    }
    setError(r.kind === "sign_in" ? "Incorrect username or password." : r.message);
  }

  const message = wait > 0 ? `Too many attempts. Try again in ${wait} s.` :
    invite?.valid === false ? "This invitation is invalid or expired." : error;

  return (
    <main id="main" className="page narrow">
      <div className="panel primary login">
        <div className="login-h">
          <LockIcon aria-hidden="true" />
          <h1>{title}</h1>
        </div>
        {mode === "onboarding" && <p className="muted" data-testid="no-password">Create the owner account to get started.</p>}
        {invite?.valid && <p className="muted">You were invited as {invite.role}.</p>}
        <form data-testid={mode} method="post" action={path} onSubmit={(event) => void submit(event)}>
          <div className="field">
            <label htmlFor="username">Username</label>
            <input id="username" className="input" name="username" autoComplete="username" required maxLength={64} />
          </div>
          <div className="field">
            <label htmlFor="pw">Password</label>
            <input
              id="pw" className="input" type="password" name="password"
              autoComplete={creating ? "new-password" : "current-password"}
              required minLength={creating ? 8 : undefined} maxLength={1024}
              aria-describedby={message ? "login-msg" : undefined}
            />
          </div>
          {creating && (
            <div className="field">
              <label htmlFor="confirm">Confirm password</label>
              <input id="confirm" className="input" type="password" name="confirm" autoComplete="new-password" required maxLength={1024} />
            </div>
          )}
          {message && <p className="form-msg" id="login-msg" role="alert" data-testid="login-error">{message}</p>}
          <div className="login-acts">
            <Button type="submit" variant="primary" busy={busy} disabled={wait > 0 || invite?.valid === false}>
              {creating ? title : "Sign in"}
            </Button>
            {mode === "invite" && <a className="btn ghost" data-testid="signin" href="/app/login">Sign in</a>}
          </div>
        </form>
      </div>
    </main>
  );
}
