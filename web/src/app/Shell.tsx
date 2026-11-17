// The application shell: skip link, sidebar, top bar (title, theme, account)
// and the main landmark. Phase 0: the sidebar lists the pages the client
// will have; none exists yet, so they are shown as plain text, not links.
import {
  BellIcon,
  BoxIcon,
  HistoryIcon,
  HouseIcon,
  LayoutGridIcon,
  LogInIcon,
  LogOutIcon,
  ScrollTextIcon,
  ServerIcon,
  SlidersHorizontalIcon,
  UserIcon,
} from "lucide-react";
import { useCallback, useState, type ReactNode } from "react";

import { api, refreshSession, useSession } from "../api/session.ts";
import { Button } from "../ui/index.ts";
import { appHref, useRoute } from "./router.ts";
import { ThemeButton } from "./ThemeButton.tsx";

/** The pages of the server-rendered UI, in its order (src/ui/shell.rs NAV). */
const FUTURE: { label: string; icon: ReactNode }[] = [
  { label: "Overview", icon: <LayoutGridIcon aria-hidden="true" /> },
  { label: "Containers", icon: <BoxIcon aria-hidden="true" /> },
  { label: "Host", icon: <ServerIcon aria-hidden="true" /> },
  { label: "Logs", icon: <ScrollTextIcon aria-hidden="true" /> },
  { label: "History", icon: <HistoryIcon aria-hidden="true" /> },
  { label: "Alerts", icon: <BellIcon aria-hidden="true" /> },
  { label: "Settings", icon: <SlidersHorizontalIcon aria-hidden="true" /> },
];

function Brand() {
  return (
    <a className="brand" href={appHref("/")}>
      <img src="/icon.svg" alt="" width="24" height="24" />
      <span>Hobnail</span>
    </a>
  );
}

function Account({ announce }: { announce: (text: string) => void }) {
  const s = useSession();
  const loc = useRoute();
  const [busy, setBusy] = useState(false);
  if (s.status === "loading") return <div className="acct" />;
  const user = s.session?.user ?? null;
  if (!user) {
    if (loc.route.name === "login") return <div className="acct" />;
    // From the home page the link is plain /app/login (next defaults to /app).
    const here = loc.pathname + loc.search + loc.hash;
    const href = loc.route.name === "home" && !loc.search && !loc.hash
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
  const signOut = async () => {
    setBusy(true);
    const r = await api.post("/api/logout");
    await refreshSession();
    setBusy(false);
    // 401: the session had already ended; the refresh shows that.
    if (r.kind !== "ok" && r.kind !== "sign_in") announce(`Sign-out failed: ${r.message}`);
    else announce("Signed out.");
  };
  return (
    <div className="acct">
      <span className="acct-user" data-testid="user" title={`Signed in as ${user}`}>
        <UserIcon aria-hidden="true" />
        {user}
      </span>
      <Button
        variant="ghost"
        className="sm"
        data-testid="signout"
        busy={busy}
        icon={<LogOutIcon aria-hidden="true" />}
        onClick={() => void signOut()}
      >
        Sign out
      </Button>
    </div>
  );
}

export function Shell({ title, children }: { title: string; children: ReactNode }) {
  const loc = useRoute();
  const [said, setSaid] = useState("");
  // A polite live region stands in for the toast the old UI shows (no toast
  // primitive yet). Same text twice still re-announces: clear, then set.
  const announce = useCallback((text: string) => {
    setSaid("");
    window.setTimeout(() => setSaid(text), 50);
  }, []);
  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <div className="app">
        <aside className="side" aria-label="Sidebar">
          <div className="side-head">
            <Brand />
          </div>
          <nav className="nav" aria-label="Pages">
            <a className="nav-item" href={appHref("/")} aria-current={loc.route.name === "home" ? "page" : undefined}>
              <HouseIcon aria-hidden="true" />
              <span>Home</span>
            </a>
          </nav>
          <div className="nav-note" id="nav-soon-note">
            Not in the new client yet; use <a href="/">the current interface</a>:
          </div>
          <ul className="nav" aria-describedby="nav-soon-note">
            {FUTURE.map((n) => (
              <li key={n.label} className="nav-item soon">
                {n.icon}
                <span>{n.label}</span>
              </li>
            ))}
          </ul>
        </aside>
        <div className="col">
          <header className="top">
            <span className="top-brand">
              <Brand />
            </span>
            <div className="top-title">{title}</div>
            <ThemeButton announce={announce} />
            <Account announce={announce} />
          </header>
          <main id="main" tabIndex={-1}>
            {children}
          </main>
        </div>
      </div>
      <output className="sr" aria-live="polite">
        {said}
      </output>
    </>
  );
}
