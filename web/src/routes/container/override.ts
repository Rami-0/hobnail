// The Edit override dialog's decisions, pure (override.test.ts): the values
// a container's `[containers."<name>"]` table gives the form, the field
// checks, which fields changed (an edit sends only those: THE DIFF RULE,
// static/settings.js OVERRIDE.request), the RETARGET RULE after a 409, and
// what a settings API answer means for the dialog (static/ui.js HB.result).
// The dialog component (EditDialog.tsx) only wires these to the DOM.
import type { ApiResult } from "../../api/client.ts";

export const OV_KEYS = ["name", "group", "icon", "url", "description", "hide", "order", "size"] as const;
export type OvKey = (typeof OV_KEYS)[number];
/** Every field as the text the form shows; `hide` is "" (default), "true" or "false". */
export type OvValues = Record<OvKey, string>;

export const EMPTY_VALUES: OvValues = { name: "", group: "", icon: "", url: "", description: "", hide: "", order: "", size: "" };

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** dlg::sval: a value of the file as the text of its field. */
function sval(v: unknown): string {
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  if (Array.isArray(v)) return v.map((x) => (typeof x === "string" ? x : JSON.stringify(x))).join(", ");
  return "";
}

/** containers.rs `ov_item`: one override table as the form's values. */
export function valuesOf(table: unknown): OvValues {
  const t = isRecord(table) ? table : {};
  const out = { ...EMPTY_VALUES };
  for (const k of OV_KEYS) out[k] = k === "hide" ? (t.hide === true ? "true" : t.hide === false ? "false" : "") : sval(t[k]);
  return out;
}

/** GET /api/settings/containers → the file's hash and this container's values (empty when it has no override). */
export function readOverride(body: unknown, name: string): { hash: string; values: OvValues; groups: string[] } | null {
  if (!isRecord(body) || typeof body.hash !== "string") return null;
  const ovs = isRecord(body.overrides) ? body.overrides : {};
  const groups = Array.isArray(body.groups)
    ? body.groups.flatMap((g) => (isRecord(g) && typeof g.name === "string" ? [g.name] : []))
    : [];
  return { hash: body.hash, values: valuesOf(ovs[name]), groups };
}

/** The values as the form sends them: text trimmed (static/ui.js `values`). */
export function trimmed(v: OvValues): OvValues {
  const out = { ...v };
  for (const k of OV_KEYS) out[k] = v[k].trim();
  return out;
}

const HTTP = /^https?:\/\/[^\s/?#]+[^\s]*$/i;

/**
 * Field checks (static/ui.js `problem` with the dialog's rules): name at
 * most 80 characters, description 200, URL a web link, order a whole number
 * from -100000 to 100000. A value left as the dialog opened it is not
 * checked again (a hand-written `url = "/files"` stays editable).
 */
export function validate(v: OvValues, was: OvValues): Partial<Record<OvKey, string>> {
  const t = trimmed(v);
  const w = trimmed(was);
  const err: Partial<Record<OvKey, string>> = {};
  if (t.name.length > 80) err.name = "At most 80 characters.";
  if (t.description.length > 200) err.description = "At most 200 characters.";
  if (t.order) {
    const n = Number(t.order);
    if (!Number.isFinite(n) || !Number.isInteger(n)) err.order = "Use a whole number from -100000 to 100000.";
    else if (n < -100000 || n > 100000) err.order = "Use a number from -100000 to 100000.";
  }
  if (t.url && t.url !== w.url && !HTTP.test(t.url)) err.url = "Use a link that starts with http:// or https://.";
  if (t.size && !["small", "normal", "wide"].includes(t.size)) err.size = "Pick one.";
  return err;
}

/** A field's text as the value the file gets (null: the key goes). */
function fileValue(k: OvKey, x: string): string | number | boolean | null {
  if (x === "") return null;
  if (k === "hide") return x === "true";
  if (k === "order") return Number(x);
  return x;
}

export interface Changes {
  set: Partial<Record<OvKey, string | number | boolean>>;
  clear: OvKey[];
}

/** Only the fields whose text changed: set what has a value, clear what was emptied. Null: nothing changed. */
export function changes(v: OvValues, was: OvValues): Changes | null {
  const t = trimmed(v);
  const w = trimmed(was);
  const set: Changes["set"] = {};
  const clear: OvKey[] = [];
  for (const k of OV_KEYS) {
    if (t[k] === w[k]) continue;
    const x = fileValue(k, t[k]);
    if (x === null) clear.push(k);
    else set[k] = x;
  }
  return Object.keys(set).length || clear.length ? { set, clear } : null;
}

/** The POST /api/settings/containers/override body. */
export function requestBody(name: string, baseHash: string, c: Changes): Record<string, unknown> {
  const b: Record<string, unknown> = { base_hash: baseHash, names: [name], clear: c.clear };
  if (Object.keys(c.set).length) b.set = c.set;
  return b;
}

/** True when the form holds input that is not saved. */
export function isDirty(v: OvValues, was: OvValues): boolean {
  return changes(v, was) !== null;
}

/**
 * THE RETARGET RULE (static/settings.js listDialog, for one override): after
 * a 409, the dialog may move to the file as it is now only when this
 * container's override still has every value the dialog was opened with.
 * Then the new hash is adopted and the input is kept; otherwise the input
 * cannot be saved onto it.
 */
export function canRetarget(openedWith: OvValues, now: OvValues): boolean {
  return OV_KEYS.every((k) => openedWith[k] === now[k]);
}

/** The hide switch: on is "true"; off is "false" when the file said "show", else the default. */
export function hideFromSwitch(on: boolean, openedWith: string): string {
  if (on) return "true";
  return openedWith === "false" ? "false" : "";
}

export interface SavedBody {
  ok?: boolean;
  unchanged?: boolean;
  hash?: string;
  revision?: number | null;
  warnings?: string[];
}

export type Outcome =
  | { kind: "saved"; unchanged: boolean; hash: string | null; revision: number | null; warnings: string[] }
  /** 422 on a field the form shows: the message goes under it. */
  | { kind: "fields"; fields: Partial<Record<OvKey, string>> }
  /** 409: "Reload and keep my input" / "Discard my input". */
  | { kind: "stale"; message: string }
  /** 401: "Sign in (new tab)" and "Retry". */
  | { kind: "signin"; message: string }
  /** Anything else: one message in the dialog's note. */
  | { kind: "note"; tone: "bad" | "warn"; message: string };

const isOvKey = (k: string): k is OvKey => (OV_KEYS as readonly string[]).includes(k);

/** What an answer of the settings API means for the dialog (HB.result). */
export function outcome(r: ApiResult<SavedBody>): Outcome {
  switch (r.kind) {
    case "ok": {
      const d = r.data ?? {};
      if (d.ok === false) return { kind: "note", tone: "bad", message: "Not saved: the server refused the change." };
      return {
        kind: "saved",
        unchanged: d.unchanged === true,
        hash: typeof d.hash === "string" ? d.hash : null,
        revision: typeof d.revision === "number" ? d.revision : null,
        warnings: Array.isArray(d.warnings) ? d.warnings.filter((w): w is string => typeof w === "string") : [],
      };
    }
    case "invalid":
      if (r.field !== undefined && isOvKey(r.field)) return { kind: "fields", fields: { [r.field]: r.message } };
      return { kind: "note", tone: "bad", message: `Not saved: ${r.message}` };
    case "stale":
      return {
        kind: "stale",
        message: "Not saved: the config file changed on disk since this was loaded (maybe a hand edit). Your input is still here.",
      };
    case "sign_in":
      return {
        kind: "signin",
        message: "Not saved: your session expired. Sign in again in a new tab, then save here again; your input stays.",
      };
    case "forbidden":
      if (r.reason === "locked") return { kind: "note", tone: "warn", message: r.message };
      if (r.reason === "cross_origin") return { kind: "note", tone: "bad", message: `Not saved: ${r.message}.` };
      if (r.reason === "read_only") return { kind: "note", tone: "warn", message: `Not saved: ${r.message}.` };
      return { kind: "note", tone: "bad", message: "Not saved: the security check failed. Copy your input, then reload the page." };
    case "rate_limited":
      return { kind: "note", tone: "bad", message: `Not saved: too many requests; try again in ${r.retryAfter} s.` };
    case "network":
      return { kind: "note", tone: "bad", message: "Not saved: the server did not answer. Your input is still here." };
    default:
      return { kind: "note", tone: "bad", message: `Not saved: ${r.message || `the server answered HTTP ${r.status}.`}` };
  }
}

/** The success toast: "Saved “web” · revision #12", or "No changes". */
export function savedText(label: string, o: { unchanged: boolean; revision: number | null }): string {
  if (o.unchanged) return "No changes";
  return `Saved “${label}”${o.revision !== null ? ` · revision #${o.revision}` : ""}`;
}

/** Where "Sign in (new tab)" goes: the login page, back to this dialog afterwards. */
export function signInHref(pathname: string, search: string): string {
  return `/app/login?next=${encodeURIComponent(pathname + search)}`;
}
