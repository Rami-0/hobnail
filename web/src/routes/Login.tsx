// /app/login: the password form (wording from src/ui/pages/login.rs).
// POST /api/login with JSON {password, next}; on success the session store is
// refreshed and the client goes to `next` (only /app paths, see paths.ts
// safeNext). 401 is a wrong password; 429 counts down the server's
// Retry-After with the button off; no password set shows the host command.
import { CheckIcon, InfoIcon, LockIcon, LogInIcon } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { api, refreshSession, useSession } from "../api/session.ts";
import { navigate, safeNext, useRoute } from "../app/router.ts";
import { Button } from "../ui/index.ts";

/** The host command that sets the first password (src/ui/pages/login.rs SET_PASSWORD_CMD). */
export const SET_PASSWORD_CMD = "docker exec -it hobnail /hobnail set-password";

export function Login() {
  const s = useSession();
  const loc = useRoute();
  const next = safeNext(new URLSearchParams(loc.search).get("next"));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [invalid, setInvalid] = useState(false);
  /** Seconds left before another attempt is allowed (429). */
  const [wait, setWait] = useState(0);
  const pw = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (wait <= 0) return;
    const t = window.setTimeout(() => {
      const left = wait - 1;
      setWait(left);
      setError(left > 0 ? `Too many attempts. Try again in ${left} s.` : "You can try again now.");
    }, 1000);
    return () => window.clearTimeout(t);
  }, [wait]);

  const session = s.session;
  const passwordSet = session ? session.password_set : true;
  const user = session?.user ?? null;
  const locked = !passwordSet || wait > 0;

  // The page's one field takes focus, as on the old login page.
  useEffect(() => {
    if (passwordSet) pw.current?.focus();
  }, [passwordSet]);

  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (busy || locked) return;
    const password = pw.current?.value ?? "";
    setBusy(true);
    setError("");
    const r = await api.post<{ ok: boolean; csrf: string }>("/api/login", { password, next });
    if (r.kind === "ok") {
      await refreshSession();
      navigate(next, { replace: true });
      return;
    }
    setBusy(false);
    setInvalid(true);
    switch (r.kind) {
      case "sign_in":
        setError("Wrong password.");
        break;
      case "rate_limited":
        setWait(r.retryAfter);
        setError(`Too many attempts. Try again in ${r.retryAfter} s.`);
        break;
      case "forbidden":
        if (r.reason === "read_only") {
          setInvalid(false);
          await refreshSession();
          break;
        }
        setError(r.reason === "cross_origin" ? "Sign-in refused: the request did not come from this site." : `Sign-in failed: ${r.message}`);
        break;
      case "network":
        setError("Could not reach the server. Your password was not sent; try again.");
        break;
      default:
        setError(`Sign-in failed (HTTP ${r.status}).`);
    }
    pw.current?.select();
  };

  return (
    <div className="page narrow">
      <div className="panel primary login">
        <div className="login-h">
          <LockIcon aria-hidden="true" />
          <h1>Sign in to Hobnail</h1>
        </div>
        <p className="muted">
          Viewing is open to everyone who can reach this page. Signing in is only needed to change settings.
        </p>
        {user && (
          <div className="banner ok">
            <CheckIcon aria-hidden="true" />
            <div>
              Signed in as <strong>{user}</strong>. <a href={next}>Continue</a>
            </div>
          </div>
        )}
        {!passwordSet && (
          <div className="banner warn" data-testid="no-password">
            <InfoIcon aria-hidden="true" />
            <div>
              <strong>No password is set yet.</strong> Nothing can be changed from the browser until the owner sets one
              on the host:
              <pre className="cmdline">
                <code>{SET_PASSWORD_CMD}</code>
              </pre>
            </div>
          </div>
        )}
        <form data-testid="login" method="post" action="/api/login" onSubmit={(e) => void submit(e)}>
          <div className="field">
            <label htmlFor="pw">Password</label>
            <input
              ref={pw}
              id="pw"
              className="input"
              type="password"
              name="password"
              autoComplete="current-password"
              required
              disabled={!passwordSet}
              aria-invalid={invalid || undefined}
              aria-describedby={error ? "login-msg" : undefined}
              onInput={() => setInvalid(false)}
            />
          </div>
          {error && (
            <p className="form-msg" id="login-msg" role="alert" data-testid="login-error">
              {error}
            </p>
          )}
          <div className="login-acts">
            <Button
              type="submit"
              variant="primary"
              busy={busy}
              disabled={locked}
              icon={<LogInIcon aria-hidden="true" />}
            >
              Sign in
            </Button>
            <a className="btn ghost" href={next}>
              Back
            </a>
          </div>
        </form>
      </div>
    </div>
  );
}
