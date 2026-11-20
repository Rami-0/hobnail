// The session store: GET /api/v1/session held in one place, with
// subscribe/get for `useSyncExternalStore`. Also creates the app's API
// client (`api`), which reads its CSRF token from here and asks for a
// refresh when the server says the token is stale.
import { useSyncExternalStore } from "react";

import { createClient, type ApiResult } from "./client.ts";
import type { Session } from "./gen/Session.ts";

export type SessionState =
  | { status: "loading"; session: null }
  | { status: "ready"; session: Session }
  /** The session could not be read; `session` keeps the last good one, if any. */
  | { status: "error"; session: Session | null; message: string };

let state: SessionState = { status: "loading", session: null };
const entryReads = new Map<string, Promise<Response | null>>();

/** Start a protected direct-entry read before the route chunk mounts. */
export function prefetchEntryRead(path: string): void {
  const request = fetch(path, { credentials: "same-origin", headers: { Accept: "application/json" } }).catch(() => null);
  entryReads.set(path, request);
  window.setTimeout(() => entryReads.delete(path), 10_000);
}

const listeners = new Set<() => void>();

function set(next: SessionState): void {
  state = next;
  for (const l of listeners) l();
}

export function getSession(): SessionState {
  return state;
}

export function subscribeSession(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

let inflight: Promise<SessionState> | null = null;

/** Re-read the session (after login, logout, a stale CSRF token). Concurrent calls share one request. */
export function refreshSession(): Promise<SessionState> {
  inflight ??= (async () => {
    try {
      const r: ApiResult<Session> = await api.get<Session>("/api/v1/session");
      if (r.kind === "ok") {
        if (!r.data.user || (state.session?.user && state.session.user.name !== r.data.user.name)) entryReads.clear();
        set({ status: "ready", session: r.data });
      }
      else {
        const message = r.kind === "network" ? r.message : `${r.message} (HTTP ${r.status})`;
        set({ status: "error", session: state.session, message });
      }
      return state;
    } finally {
      inflight = null;
    }
  })();
  return inflight;
}

/** Retry an in-progress write only for the same principal. Keep the editor
 * mounted when the new tab has yet to finish signing in. */
export const editingSessionRequired = "Sign in as the same account in the other tab, then retry.";

export async function refreshEditingSession(): Promise<Session | null> {
  const principal = state.session?.user?.name;
  if (!principal) return null;
  const reply = await api.get<Session>("/api/v1/session");
  if (reply.kind !== "ok" || reply.data.user?.name !== principal || state.session?.user?.name !== principal) return null;
  set({ status: "ready", session: reply.data });
  return reply.data;
}

export const api = createClient({
  async fetch(path, init) {
    const request = init.method === "GET" ? entryReads.get(path) : undefined;
    if (request) {
      entryReads.delete(path);
      const response = await request;
      init.signal?.throwIfAborted();
      if (response) return response;
    }
    return fetch(path, init);
  },
  csrf: () => state.session?.csrf ?? null,
  refreshCsrf: async () => (await refreshEditingSession())?.csrf ?? null,
});

/** The current session state; re-renders on every change. */
export function useSession(): SessionState {
  return useSyncExternalStore(subscribeSession, getSession, getSession);
}
