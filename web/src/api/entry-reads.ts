// Run before the route imports start. Each consumer takes its own first read.
import { matchRoute } from "../app/paths.ts";
import { apiQuery, parseWindow } from "../routes/container/range.ts";
import { historyWindow } from "../routes/history/window.ts";
import { prefetchEntryRead, refreshSession } from "./session.ts";

void refreshSession();
const { pathname, search } = window.location;
const route = matchRoute(pathname);
if (route.name !== "login" && route.name !== "invite" && route.name !== "notfound") {
  prefetchEntryRead("/api/v1/bootstrap");
  prefetchEntryRead("/api/v1/containers");
}
switch (route.name) {
  case "board": prefetchEntryRead(`/api/v1/boards/${route.id}`); break;
  case "container":
    prefetchEntryRead(`/api/v1/containers/${encodeURIComponent(route.ctr)}?${apiQuery(parseWindow(search))}`);
    break;
  case "history": prefetchEntryRead(`/api/v1/history?${historyWindow(new URLSearchParams(search)).query}`); break;
  case "host": prefetchEntryRead("/api/v1/host"); break;
  case "logs": case "container_logs": case "stack_logs": prefetchEntryRead("/api/v1/logs/picker"); break;
  case "settings": if (route.tab === "general") prefetchEntryRead("/api/settings/general"); break;
}
