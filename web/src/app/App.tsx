import { PageHeader } from "../ui/PageHeader.tsx";
import { Skeleton } from "../ui/Skeleton.tsx";
// Route -> page, inside the shell (which marks #root[data-ready="1"] once
// it is drawn with a known session; the browser checks wait for it).
// The Containers and container pages are lazy chunks (their default
// exports); the charts under the container page are a further lazy chunk.
import { lazy, Suspense, useEffect, useState, type ComponentType } from "react";

import { useSession } from "../api/session.ts";
import { Home } from "../routes/Home.tsx";
import { NotFound } from "../routes/NotFound.tsx";
import { Banner } from "../ui/Note.tsx";
import { Button } from "../ui/Button.tsx";
import { SignInPrompt } from "../ui/SignInPrompt.tsx";
import { matchRoute } from "./paths.ts";
import { navigate, useRoute, type Route } from "./router.ts";
import { loadSetupPage, loadLogsPage, loadAlertsPage, loadOverviewPage, loadHostPage, loadHistoryPage, loadAccountPage, loadSettingsPage, loadBoardPage, loadContainersPage, loadContainerPage, loadLogin } from "./route-pages.ts";
import { Shell } from "./Shell.tsx";

// Start the direct-entry route while the session request is in flight.
const opened = matchRoute(window.location.pathname).name;
function pageChunk(loader: () => Promise<{ default: ComponentType }>, ...routes: Route["name"][]) {
  if (!routes.includes(opened)) return lazy(loader);
  let Resolved: ComponentType | undefined;
  const initial = loader().then((module) => (Resolved = module.default));
  // The direct entry already has a reserved placeholder. Publish its module
  // as soon as it resolves, without adding a Suspense fallback delay.
  let error: unknown;
  let failed = false;
  void initial.catch((reason) => { failed = true; error = reason; });
  return function EntryPage() {
    const [, refresh] = useState(false);
    useEffect(() => {
      let active = true;
      const ready = () => { if (active) refresh(true); };
      void initial.then(ready, ready);
      return () => { active = false; };
    }, []);
    if (Resolved) return <Resolved />;
    if (failed) throw error;
    return <Loading />;
  };
}

const SetupPage = pageChunk(loadSetupPage, "setup");
const LogsPage = pageChunk(loadLogsPage, "logs", "container_logs", "stack_logs");
const AlertsPage = pageChunk(loadAlertsPage, "alerts");
const OverviewPage = pageChunk(loadOverviewPage, "overview");
const HostPage = pageChunk(loadHostPage, "host");
const HistoryPage = pageChunk(loadHistoryPage, "history");
const AccountPage = pageChunk(loadAccountPage, "account");
const SettingsPage = pageChunk(loadSettingsPage, "settings");
const BoardPage = pageChunk(loadBoardPage, "board");
const ContainersPage = pageChunk(loadContainersPage, "containers");
const ContainerPage = pageChunk(loadContainerPage, "container");
const LoginPage = lazy(() => loadLogin().then((m) => ({ default: m.Login })));

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

function Loading() {
  return (
    <div className="page" aria-busy="true">
      <div className="sk-rows">
        <output className="sr">Loading…</output>
        <Skeleton />
        <Skeleton />
        <Skeleton />
      </div>
    </div>
  );
}

export function App() {
  const { route, pathname, query, hash } = useRoute();
  const s = useSession();
  const authPage = route.name === "login" || route.name === "invite";
  const owner = s.session?.user?.role === "owner";
  const setupPage = route.name === "setup";
  const redirect = owner && s.session?.setup_pending && route.name === "home" ? "/app/setup"
    : s.session && !s.session.user && !authPage && route.name !== "settings"
      ? `/app/login?next=${encodeURIComponent(pathname + query + hash)}` : null;
  useEffect(() => {
    if (redirect) navigate(redirect, { replace: true });
  }, [redirect]);
  const showCredentials = authPage && s.session !== null;
  useEffect(() => {
    if (showCredentials || (setupPage && s.session?.user)) document.getElementById("root")?.setAttribute("data-ready", "1");
  }, [showCredentials, setupPage, s.session?.user]);


  // The placeholder has the shell's frame (sidebar column, top bar), so
  // nothing moves when the shell arrives.
  if (s.status === "loading") {
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
    return <Suspense fallback={<Loading />}><LoginPage token={route.name === "invite" ? route.token : undefined} /></Suspense>;
  }
  if (!s.session?.user) {
    if (s.session && route.name === "settings") return <main className="page narrow" id="main">
      <PageHeader title="Settings" />
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

  if (setupPage) return owner ? <Suspense fallback={<Loading />}><SetupPage /></Suspense>
    : <main className="page narrow" id="main"><Banner kind="warn">Only the owner can run guided setup.</Banner></main>;
  if (redirect) return <Loading />;

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
