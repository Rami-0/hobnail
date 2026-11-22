// Run before the route imports start. Each consumer takes its own first read.
import { prefetchPage } from "../app/route-pages.ts";
import { matchRoute } from "../app/paths.ts";
import { apiQuery, parseWindow } from "../routes/container/range.ts";
import { overviewPath, readSeen } from "../routes/overview/request.ts";
import { historyWindow } from "../routes/history/window.ts";
import { createClient } from "./client.ts";
import type { Session } from "./gen/Session.ts";

const entryReads = new Map<string, Promise<Response | null>>();
const read = (path: string) => fetch(path, { credentials: "same-origin", headers: { Accept: "application/json" } });

function prefetchEntryRead(path: string): void {
  entryReads.set(path, read(path).catch(() => null));
  window.setTimeout(() => entryReads.delete(path), 10_000);
}

export function clearEntryReads() {
  entryReads.clear();
}

export function takeEntryRead(path: string) {
  const request = entryReads.get(path);
  entryReads.delete(path);
  return request;
}

const reader = createClient({ csrf: () => null, refreshCsrf: async () => null });
export const entrySession = reader.get<Session>("/api/v1/session");

const { pathname, search } = window.location;
const route = matchRoute(pathname);
// The HTML session preload resolves while route code is loading. Avoid
// protected preloads entirely for visitors without a session.
void entrySession.then((reply) => {
  if (reply.kind !== "ok") return;
  const session = reply.data;
  if (session.user || route.name === "login" || route.name === "invite") prefetchPage(pathname);
  if (!session?.user || route.name === "login" || route.name === "invite" || route.name === "setup" || (route.name === "home" && session.setup_pending) || route.name === "not_found") return;
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
      prefetchEntryRead(overviewPath(readSeen(), true));
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
