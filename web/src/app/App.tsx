// Route -> page, inside the shell (which marks #root[data-ready="1"] once
// it is drawn with a known session; the browser checks wait for it).
// The Containers and container pages are lazy chunks (their default
// exports); the charts under the container page are a further lazy chunk.
import { lazy, Suspense, useDeferredValue, useEffect, type ComponentType } from "react";

import { useSession } from "../api/session.ts";
import { Home } from "../routes/Home.tsx";
import { Login } from "../routes/Login.tsx";
import { NotFound } from "../routes/NotFound.tsx";
import { Banner } from "../ui/Note.tsx";
import { Button } from "../ui/Button.tsx";
import { SignInPrompt } from "../ui/SignInPrompt.tsx";
import { matchRoute } from "./paths.ts";
import { navigate, useRoute, type Route } from "./router.ts";
import { Shell } from "./Shell.tsx";

// Start the direct-entry route while the session request is in flight.
const opened = matchRoute(window.location.pathname).name;
function pageChunk(loader: () => Promise<{ default: ComponentType }>, ...routes: Route["name"][]) {
  const initial = routes.includes(opened) ? loader() : null;
  // A direct entry can finish before the session. Render the resolved page
  // immediately rather than suspending on its already-settled promise.
  let Loaded: ComponentType | undefined;
  void initial?.then((module) => { Loaded = module.default; }, () => {});
  const LazyPage = lazy(() => initial ?? loader());
  return function RouteChunk() {
    return Loaded ? <Loaded /> : <LazyPage />;
  };
}

const LogsPage = pageChunk(() => import("../routes/logs/index.tsx"), "logs", "container_logs", "stack_logs");
const AlertsPage = pageChunk(() => import("../routes/alerts/index.tsx"), "alerts");
const OverviewPage = pageChunk(() => import("../routes/overview/index.tsx"), "overview");
const HostPage = pageChunk(() => import("../routes/host/index.tsx"), "host");
const HistoryPage = pageChunk(() => import("../routes/history/index.tsx"), "history");
const AccountPage = pageChunk(() => import("../routes/account/index.tsx"), "account");
const SettingsPage = pageChunk(() => import("../routes/settings/index.tsx"), "settings");
const BoardPage = pageChunk(() => import("../routes/board/index.tsx"), "board");
const ContainersPage = pageChunk(() => import("../routes/containers/index.tsx"), "containers");
const ContainerPage = pageChunk(() => import("../routes/container/index.tsx"), "container");

function titleOf(r: Route): string {
  switch (r.name) {
    case "home":
      return "Home";
    case "logs": case "container_logs": case "stack_logs": return "Logs";
    case "alerts": return "Alerts";
    case "overview": return "Overview";
    case "host": return "Host";
    case "history": return "History";
    case "board": return "Board";
    case "account": return "Account";
    case "settings": return "Settings";
    case "login":
      return "Sign in";
    case "containers":
      return "Containers";
    case "container":
      return r.ctr;
    default:
      return "Not found";
  }
}

/** Font weights the pages use (each is a font lookup the first time it is laid out). */
const WARM = [400, 500, 600, 700];

function Loading() {
  return (
    <div className="page" aria-busy="true">
      <div className="sk-rows">
        <output className="sr">Loading…</output>
        <span className="sk" />
        <span className="sk" />
        <span className="sk" />
        {/* The fonts the shell and the table use, laid out in this cheap
            first frame, so their first use does not land in a later, larger one. */}
        <span className="sr" aria-hidden="true">
          {WARM.map((w) => (
            <span key={w} style={{ fontWeight: w }}>
              Aa0 <code>Aa0</code>
            </span>
          ))}
        </span>
      </div>
    </div>
  );
}

export function App() {
  const { route, pathname, query, hash } = useRoute();
  const s = useSession();
  // The first render is a placeholder; the shell and the page follow in a
  // transition, which React renders in slices (a plain first render is one
  // long task on a slow phone).
  const ready = useDeferredValue(true, false);
  const authPage = route.name === "login" || route.name === "invite";
  const redirect = s.session && !s.session.user && !authPage && route.name !== "settings"
    ? `/app/login?next=${encodeURIComponent(pathname + query + hash)}`
    : null;
  useEffect(() => {
    if (redirect) navigate(redirect, { replace: true });
  }, [redirect]);
  const showCredentials = authPage && s.session !== null;
  useEffect(() => {
    if (ready && showCredentials) document.getElementById("root")?.setAttribute("data-ready", "1");
  }, [ready, showCredentials]);



  // The placeholder has the shell's frame (sidebar column, top bar), so
  // nothing moves when the shell arrives.
  if (!ready) {
    return (
      <div className="app">
        <aside className="side" aria-hidden="true" />
        <div className="col">
          <header className="top" />
          <main id="main">
            <Loading />
          </main>
        </div>
      </div>
    );
  }

  if (showCredentials) {
    return <Login token={route.name === "invite" ? route.token : undefined} />;
  }
  if (!s.session?.user) {
    if (s.session && route.name === "settings") return <main className="page narrow" id="main">
      <h1>Settings</h1>
      <Banner kind="warn" testId="settings-locked" action={<SignInPrompt retry={() => window.location.reload()} />}>
        Sign in to access Settings.
      </Banner>
    </main>;
    return s.status === "error" ? (
      <main id="main" className="page narrow">
        <Banner kind="bad" action={<Button className="sm" onClick={() => window.location.reload()}>Reload</Button>}>
          Could not read the session: {s.message}.
        </Banner>
      </main>
    ) : <Loading />;
  }

  let page;
  switch (route.name) {
    case "home":
      page = <Home />;
      break;
    case "logs": case "container_logs": case "stack_logs":
      page = <LogsPage />;
      break;
    case "alerts":
      page = <AlertsPage />;
      break;
    case "overview":
      page = <OverviewPage />;
      break;
    case "host":
      page = <HostPage />;
      break;
    case "history":
      page = <HistoryPage />;
      break;
    case "account":
      page = <AccountPage />;
      break;
    case "settings":
      page = <SettingsPage key={route.tab} />;
      break;
    case "board":
      page = <BoardPage key={route.id} />;
      break;
    case "containers":
      page = <ContainersPage />;
      break;
    case "container":
      // A new container is a new page: its state starts afresh.
      page = <ContainerPage key={route.ctr} />;
      break;
    default:
      page = <NotFound path={pathname} />;
  }
  return (
    <Shell title={titleOf(route)}>
      <Suspense fallback={<Loading />}>{page}</Suspense>
    </Shell>
  );
}
