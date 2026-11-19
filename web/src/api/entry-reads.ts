// Run before the route imports start. Each consumer takes its own first read.
import { matchRoute } from "../app/paths.ts";
import { apiQuery, parseWindow } from "../routes/container/range.ts";
import { overviewPath, readSeen } from "../routes/overview/request.ts";
import { historyWindow } from "../routes/history/window.ts";
import { prefetchEntryRead, refreshSession } from "./session.ts";

const { pathname, search } = window.location;
const route = matchRoute(pathname);
// The HTML session preload resolves while route code is loading. Avoid
// protected preloads entirely for visitors without a session.
void refreshSession().then(({ session }) => {
  if (!session?.user || route.name === "login" || route.name === "invite" || route.name === "not_found") return;
  prefetchEntryRead("/api/v1/bootstrap");
  prefetchEntryRead("/api/v1/containers");
  switch (route.name) {
    case "board": prefetchEntryRead(`/api/v1/boards/${route.id}`); break;
    case "container":
      prefetchEntryRead(`/api/v1/containers/${encodeURIComponent(route.ctr)}?${apiQuery(parseWindow(search))}`);
      break;
    case "history": if (session.user.role === "owner") prefetchEntryRead(`/api/v1/history?${historyWindow(new URLSearchParams(search)).query}`); break;
    case "host": if (session.user.role === "owner") prefetchEntryRead("/api/v1/host"); break;
    case "overview":
      if (session.user.role !== "owner") break;
      prefetchEntryRead(overviewPath(readSeen(), session.user.role !== "viewer"));
      prefetchEntryRead("/api/v1/widgets");
      prefetchEntryRead("/api/v1/bookmarks");
      break;
    case "alerts": {
      if (session.user.role !== "owner") break;
      const filters = new URLSearchParams(search);
      filters.delete("add");
      filters.delete("edit");
      prefetchEntryRead(`/api/v1/alerts?${filters}`);
      break;
    }
    case "logs": case "container_logs": case "stack_logs": prefetchEntryRead("/api/v1/logs/picker"); break;
    case "settings":
      if (route.tab === "general" && session.user.role === "owner") prefetchEntryRead("/api/settings/general");
      break;
  }
});
