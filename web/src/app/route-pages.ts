import { matchRoute, type Route } from "./paths.ts";

function routePage<T>(load: () => Promise<T>) {
  let pending: Promise<T> | undefined;
  return () => (pending ??= load());
}

const loadSetupPage = routePage(() => import("../routes/setup/GuidedSetup.tsx"));
const loadLogsPage = routePage(() => import("../routes/logs/index.tsx"));
const loadAlertsPage = routePage(() => import("../routes/alerts/index.tsx"));
const loadOverviewPage = routePage(() => import("../routes/overview/index.tsx"));
const loadHostPage = routePage(() => import("../routes/host/index.tsx"));
const loadHistoryPage = routePage(() => import("../routes/history/index.tsx"));
const loadAccountPage = routePage(() => import("../routes/account/index.tsx"));
const loadSettingsPage = routePage(() => import("../routes/settings/index.tsx"));
const loadBoardPage = routePage(() => import("../routes/board/index.tsx"));
const loadContainersPage = routePage(() => import("../routes/containers/index.tsx"));
const loadContainerPage = routePage(() => import("../routes/container/index.tsx"));
const loadLogin = routePage(() => import("../routes/Login.tsx"));
const routePages = {
  setup: loadSetupPage,
  logs: loadLogsPage,
  container_logs: loadLogsPage,
  stack_logs: loadLogsPage,
  alerts: loadAlertsPage,
  overview: loadOverviewPage,
  host: loadHostPage,
  history: loadHistoryPage,
  account: loadAccountPage,
  settings: loadSettingsPage,
  board: loadBoardPage,
  containers: loadContainersPage,
  container: loadContainerPage,
  login: loadLogin,
  invite: loadLogin,
} satisfies Partial<Record<Route["name"], () => Promise<unknown>>>;

export { loadSetupPage, loadLogsPage, loadAlertsPage, loadOverviewPage, loadHostPage, loadHistoryPage, loadAccountPage, loadSettingsPage, loadBoardPage, loadContainersPage, loadContainerPage, loadLogin };

export function prefetchPage(pathname: string) {
  const name = matchRoute(pathname).name;
  if (name in routePages) void routePages[name as keyof typeof routePages]().catch(() => {});
}
