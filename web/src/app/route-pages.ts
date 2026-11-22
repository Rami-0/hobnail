import { matchRoute, type Route } from "./paths.ts";

function routePage<T>(load: () => Promise<T>) {
  let pending: Promise<T> | undefined;
  return () => (pending ??= load().catch((error) => {
    pending = undefined;
    throw error;
  }));
}

const loadLogsPage = routePage(() => import("../routes/logs/index.tsx"));
const loadLogin = routePage(() => import("../routes/Login.tsx"));
export const routePages = {
  setup: routePage(() => import("../routes/setup/GuidedSetup.tsx")),
  logs: loadLogsPage,
  container_logs: loadLogsPage,
  stack_logs: loadLogsPage,
  alerts: routePage(() => import("../routes/alerts/index.tsx")),
  overview: routePage(() => import("../routes/overview/index.tsx")),
  host: routePage(() => import("../routes/host/index.tsx")),
  history: routePage(() => import("../routes/history/index.tsx")),
  account: routePage(() => import("../routes/account/index.tsx")),
  settings: routePage(() => import("../routes/settings/index.tsx")),
  board: routePage(() => import("../routes/board/index.tsx")),
  containers: routePage(() => import("../routes/containers/index.tsx")),
  container: routePage(() => import("../routes/container/index.tsx")),
  login: loadLogin,
  invite: loadLogin,
} satisfies Partial<Record<Route["name"], () => Promise<unknown>>>;

export function prefetchPage(pathname: string) {
  const name = matchRoute(pathname).name;
  if (name in routePages) void routePages[name as keyof typeof routePages]().catch(() => {});
}
