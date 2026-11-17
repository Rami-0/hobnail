// The one fetch wrapper of the client. Every response becomes a typed
// `ApiResult`, so a page handles "sign in", "stale", "field error" and
// "try again in N s" the same way everywhere.
//
// Error bodies the server sends today (all mapped here):
// - /api/v1: `{"error":{"code","message","field"?}}` (src/api, Phase 0).
// - /api/settings/*, /api/login: `{"error":"…"}` plus `field`/`line`/`col`
//   (422), `stale` (409), `locked` (403 guard refusal), `retry_after` (429)
//   (src/settings/store.rs ApiError, src/auth.rs).
// - The write gate (auth::WriteDenied): plain text, 401 "sign in required",
//   403 "missing or bad X-RH-CSRF header" / "cross-origin write refused" /
//   "read-only: no password set (run `hobnail set-password`)".
//
// A 403 for the CSRF header means the token we hold is old (signed in again
// in another tab, or the session was replaced): the client asks the session
// store for a fresh one and retries the request once.
import type { ApiErrorBody } from "./gen/ApiErrorBody.ts";

export type Forbidden = "csrf" | "cross_origin" | "read_only" | "locked" | "other";

export type ApiResult<T> =
  | { kind: "ok"; status: number; data: T }
  /** 401: no or expired session (on POST /api/login: wrong password). */
  | { kind: "sign_in"; status: 401; message: string; code?: string }
  | { kind: "forbidden"; status: 403; reason: Forbidden; message: string }
  /** 409: the config changed on disk since it was read. */
  | { kind: "stale"; status: 409; message: string }
  /** 422 (and 400 with a field): a value the server refused. */
  | { kind: "invalid"; status: number; message: string; field?: string; line?: number; col?: number }
  /** 429: wait this many whole seconds (at least 1). */
  | { kind: "rate_limited"; status: 429; message: string; retryAfter: number }
  /** Any other HTTP error. */
  | { kind: "error"; status: number; message: string; code?: string }
  /** No HTTP answer at all (offline, server down, aborted). */
  | { kind: "network"; message: string };

export type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

export interface RequestOptions {
  /** Sent as JSON. */
  body?: unknown;
  signal?: AbortSignal;
}

export interface ClientDeps {
  /** Defaults to the global fetch. Tests pass a fake. */
  fetch?: (input: string, init: RequestInit) => Promise<Response>;
  /** The CSRF token of the current session, if any. */
  csrf: () => string | null;
  /** Re-read the session; resolves to its CSRF token (null when signed out). */
  refreshCsrf: () => Promise<string | null>;
}

export interface Client {
  request<T>(method: Method, path: string, opts?: RequestOptions): Promise<ApiResult<T>>;
  get<T>(path: string, opts?: RequestOptions): Promise<ApiResult<T>>;
  post<T>(path: string, body?: unknown, opts?: RequestOptions): Promise<ApiResult<T>>;
}

export const CSRF_HEADER = "X-RH-CSRF";

/** What an error body says, whichever of the three shapes it has. */
interface ErrorInfo {
  message: string;
  code?: string;
  field?: string;
  line?: number;
  col?: number;
  locked?: boolean;
  retryAfter?: number;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);
const num = (v: unknown): number | undefined => (typeof v === "number" && Number.isFinite(v) ? v : undefined);

/** Parse an error body (JSON in either shape, or plain text). Exported for tests. */
export function errorInfo(text: string, status: number): ErrorInfo {
  let j: unknown;
  try {
    j = JSON.parse(text);
  } catch {
    j = undefined;
  }
  if (isRecord(j)) {
    const e = j.error;
    if (isRecord(e)) {
      // /api/v1 shape.
      const v1 = e as Partial<ApiErrorBody["error"]>;
      const info: ErrorInfo = { message: str(v1.message) ?? `HTTP ${status}` };
      const code = str(v1.code);
      const field = str(v1.field);
      if (code !== undefined) info.code = code;
      if (field !== undefined) info.field = field;
      return info;
    }
    const info: ErrorInfo = { message: str(e) ?? `HTTP ${status}` };
    const field = str(j.field);
    const line = num(j.line);
    const col = num(j.col);
    const retry = num(j.retry_after);
    if (field !== undefined) info.field = field;
    if (line !== undefined) info.line = line;
    if (col !== undefined) info.col = col;
    if (retry !== undefined) info.retryAfter = retry;
    if (j.locked === true) info.locked = true;
    return info;
  }
  const t = text.trim();
  return { message: t || `HTTP ${status}` };
}

/** Whole seconds from a `Retry-After` header (seconds or an HTTP date), else the body's value; at least 1. */
export function retryAfterSeconds(header: string | null, body: number | undefined, now = Date.now()): number {
  let s: number | undefined;
  if (header !== null && header.trim() !== "") {
    const h = header.trim();
    if (/^\d+$/.test(h)) s = Number(h);
    else {
      const at = Date.parse(h);
      if (!Number.isNaN(at)) s = (at - now) / 1000;
    }
  }
  if (s === undefined) s = body;
  return Math.max(1, Math.ceil(s ?? 1));
}

function forbiddenReason(info: ErrorInfo): Forbidden {
  const m = info.message;
  if (m.includes("X-RH-CSRF")) return "csrf";
  if (m.includes("cross-origin")) return "cross_origin";
  if (m.includes("no password set")) return "read_only";
  if (info.locked) return "locked";
  return "other";
}

/** Map a finished HTTP response to an ApiResult. Exported for tests. */
export async function toResult<T>(r: Response): Promise<ApiResult<T>> {
  let text = "";
  try {
    text = await r.text();
  } catch {
    return { kind: "network", message: "The connection broke while reading the answer." };
  }
  if (r.ok) {
    if (text === "") return { kind: "ok", status: r.status, data: undefined as T };
    try {
      return { kind: "ok", status: r.status, data: JSON.parse(text) as T };
    } catch {
      return { kind: "error", status: r.status, message: "The server sent something that is not JSON." };
    }
  }
  const info = errorInfo(text, r.status);
  const withCode = <R extends object>(v: R): R => (info.code !== undefined ? { ...v, code: info.code } : v);
  switch (r.status) {
    case 401:
      return withCode({ kind: "sign_in", status: 401, message: info.message });
    case 403:
      return { kind: "forbidden", status: 403, reason: forbiddenReason(info), message: info.message };
    case 409:
      return { kind: "stale", status: 409, message: info.message };
    case 429:
      return {
        kind: "rate_limited",
        status: 429,
        message: info.message,
        retryAfter: retryAfterSeconds(r.headers.get("retry-after"), info.retryAfter),
      };
    case 422: {
      const v: ApiResult<T> & { kind: "invalid" } = { kind: "invalid", status: 422, message: info.message };
      if (info.field !== undefined) v.field = info.field;
      if (info.line !== undefined) v.line = info.line;
      if (info.col !== undefined) v.col = info.col;
      return v;
    }
    default:
      if (r.status === 400 && info.field !== undefined) {
        return { kind: "invalid", status: 400, message: info.message, field: info.field };
      }
      return withCode({ kind: "error", status: r.status, message: info.message });
  }
}

export function createClient(deps: ClientDeps): Client {
  const doFetch = deps.fetch ?? ((input: string, init: RequestInit) => globalThis.fetch(input, init));

  async function once<T>(method: Method, path: string, opts: RequestOptions, csrf: string | null): Promise<ApiResult<T>> {
    const headers: Record<string, string> = { Accept: "application/json" };
    const init: RequestInit = { method, credentials: "same-origin", headers };
    if (opts.body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(opts.body);
    }
    if (method !== "GET" && csrf) headers[CSRF_HEADER] = csrf;
    if (opts.signal) init.signal = opts.signal;
    let r: Response;
    try {
      r = await doFetch(path, init);
    } catch (e) {
      if (e instanceof DOMException && e.name === "AbortError") {
        return { kind: "network", message: "Cancelled." };
      }
      return { kind: "network", message: "Could not reach the server." };
    }
    return toResult<T>(r);
  }

  async function request<T>(method: Method, path: string, opts: RequestOptions = {}): Promise<ApiResult<T>> {
    const first = await once<T>(method, path, opts, deps.csrf());
    if (method === "GET" || first.kind !== "forbidden" || first.reason !== "csrf") return first;
    // Stale token: fetch the current session and try exactly once more.
    const fresh = await deps.refreshCsrf();
    if (!fresh) return { kind: "sign_in", status: 401, message: "sign in required" };
    return once<T>(method, path, opts, fresh);
  }

  return {
    request,
    get: (path, opts) => request("GET", path, opts),
    post: (path, body, opts) => request("POST", path, { ...opts, body }),
  };
}
