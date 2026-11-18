// The account control at the end of the top bar (src/ui/shell.rs account):
// signed in, a menu with who you are and Sign out (Settings is in the
// sidebar); signed out, a plain "Sign in" link that comes back to this page
// (NAV-43); nothing on the login page itself.
import { LogInIcon, LogOutIcon, UserIcon } from "lucide-react";

import { api, refreshSession, useSession } from "../api/session.ts";
import { Menu, toast } from "../ui/index.ts";
import { appHref, useRoute } from "./router.ts";

async function signOut(): Promise<void> {
  const r = await api.post("/api/logout");
  await refreshSession();
  // 401: the session had already ended; the refresh shows that.
  if (r.kind !== "ok" && r.kind !== "sign_in") toast(`Sign-out failed: ${r.message}`, { kind: "bad" });
  else toast("Signed out", { kind: "info" });
}

export function Account() {
  const s = useSession();
  const loc = useRoute();
  if (s.status === "loading") return <div className="acct" />;
  const user = s.session?.user ?? null;
  if (!user) {
    if (loc.route.name === "login") return <div className="acct" />;
    // From the home page the link is plain /app/login (next defaults to /app).
    const here = loc.pathname + loc.query + loc.hash;
    const href =
      loc.route.name === "home" && !loc.query && !loc.hash
        ? appHref("/login")
        : `${appHref("/login")}?next=${encodeURIComponent(here)}`;
    return (
      <div className="acct">
        <a className="btn sm ghost acct-in" data-testid="signin" href={href}>
          <LogInIcon aria-hidden="true" />
          <span>Sign in</span>
        </a>
      </div>
    );
  }
  return (
    <div className="acct">
      <Menu
        label={`Account: ${user}`}
        testId="user"
        triggerClassName="acct-btn"
        trigger={
          <>
            <UserIcon aria-hidden="true" />
            <span className="acct-name">{user}</span>
            <span className="signed" aria-hidden="true" />
          </>
        }
        header={
          <>
            Signed in as <strong>{user}</strong>
          </>
        }
        items={[{ label: "Sign out", icon: <LogOutIcon aria-hidden="true" />, testId: "signout", onSelect: () => void signOut() }]}
      />
    </div>
  );
}
