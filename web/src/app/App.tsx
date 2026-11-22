import { PageHeader } from "../ui/PageHeader.tsx";
import { Skeleton } from "../ui/Skeleton.tsx";
// Route -> page, inside the shell (which marks #root[data-ready="1"] once
// it is drawn with a known session; the browser checks wait for it).
// The Containers and container pages are lazy chunks (their default
// exports); the charts under the container page are a further lazy chunk.
import { lazy, useEffect, useState, type ComponentType } from "react";

import { useSession } from "../api/session.ts";
import { Home } from "../routes/Home.tsx";
import { NotFound } from "../routes/NotFound.tsx";
import { Banner } from "../ui/Note.tsx";
import { SignInPrompt } from "../ui/SignInPrompt.tsx";
import { matchRoute } from "./paths.ts";
import { navigate, useRoute, type Route } from "./router.ts";
import { routePages } from "./route-pages.ts";
import { LoadBoundary, LoadError } from "../ui/LoadBoundary.tsx";
import { Shell } from "./Shell.tsx";

// Start the direct-entry route while the session request is in flight.
const opened = matchRoute(window.location.pathname).name;
type PageProps = { token?: string };
type Loader = () => Promise<{ default: ComponentType<PageProps> }>;
const loaders: Partial<Record<Route["name"], Loader>> = routePages;
function pageChunk(loader: Loader) {
  if (loader !== loaders[opened]) return lazy(loader);
  // The direct entry already has a placeholder. Render its module as soon
  // as it resolves without adding a Suspense fallback delay.
  let entry: { Page: ComponentType<PageProps> } | { error: unknown } | undefined;
  const initial = loader().then(
    ({ default: Page }) => { entry = { Page }; },
    (error: unknown) => { entry = { error }; },
  );
  return function EntryPage(props: PageProps) {
    const [, refresh] = useState(false);
    useEffect(() => { void initial.then(() => refresh(true)); }, []);
    if (!entry) return <Loading />;
    if ("error" in entry) throw entry.error;
    return <entry.Page {...props} />;
  };
}

const chunks = new Map(Array.from(new Set<Loader>(Object.values(routePages)), (loader) => [loader, pageChunk(loader)] as const));
const pages = Object.fromEntries(Object.entries(routePages).map(([name, loader]) => [name, chunks.get(loader)])) as Record<keyof typeof routePages, ComponentType<PageProps>>;
const { login: LoginPage, setup: SetupPage } = pages;

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
    return <LoadBoundary key={pathname} fallback={<Loading />}><LoginPage token={route.name === "invite" ? route.token : undefined} /></LoadBoundary>;
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
        <LoadError>Could not read the session: {s.message}.</LoadError>
      </main>
    ) : <Loading />;
  }

  if (setupPage) return owner ? <LoadBoundary key={pathname} fallback={<Loading />}><SetupPage /></LoadBoundary>
    : <main className="page narrow" id="main"><Banner kind="warn">Only the owner can run guided setup.</Banner></main>;
  if (redirect) return <Loading />;

  const Page = (pages as Partial<Record<Route["name"], ComponentType<PageProps>>>)[route.name];
  const key = route.name === "settings" ? route.tab
    : route.name === "board" ? route.id : route.name === "container" ? route.ctr : undefined;
  const page = Page ? <Page key={key} /> : route.name === "home" ? <Home /> : <NotFound path={pathname} />;
  return (
    <Shell title={titleOf(route)}>
      <LoadBoundary key={route.name.endsWith("logs") ? "logs" : pathname} fallback={<Loading />}>{page}</LoadBoundary>
    </Shell>
  );
}
