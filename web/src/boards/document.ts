import type { BoardDocument } from "../api/gen/BoardDocument.ts";
import type { BoardItem } from "../api/gen/BoardItem.ts";
import type { BoardSection } from "../api/gen/BoardSection.ts";
import type { BoardPayload } from "../api/gen/BoardPayload.ts";
import type { BoardAppearance } from "../api/gen/BoardAppearance.ts";
import type { TileLook } from "../api/gen/TileLook.ts";
import { compact, firstFree, fit, moveTo, type Rect, type Size } from "./grid.ts";

export const MAX_SECTIONS = 64;
export const MAX_ITEMS = 512;
export const DEFAULT_APPEARANCE: BoardAppearance = { background: "none", density: "comfortable", surface: "solid" };

/** New tiles' size by kind (matches src/board_grid.rs default_size). */
export function defaultSize(kind: BoardPayload["kind"]): Size {
  return kind === "link" ? { w: 3, h: 1 } : kind === "widget" ? { w: 4, h: 3 } : { w: 4, h: 2 };
}
/** The smallest a tile of a kind still reads well at. A provider widget's
 * own `min_size` (grid units, from its metadata) wins when present. */
export function minSize(kind: BoardPayload["kind"], widget?: unknown): Size {
  const declared = (widget as { min_size?: Partial<Size> } | null | undefined)?.min_size;
  const dw = declared?.w; const dh = declared?.h;
  if (typeof dw === "number" && typeof dh === "number" && Number.isFinite(dw) && Number.isFinite(dh)) { const { w, h } = fit({ x: 0, y: 0, w: dw, h: dh }); return { w, h }; }
  return kind === "link" ? { w: 2, h: 1 } : kind === "widget" ? { w: 2, h: 2 } : { w: 3, h: 1 };
}
export const rectOf = (item: BoardItem): Rect => ({ x: item.x, y: item.y, w: item.w, h: item.h });
export const rectsOf = (section: BoardSection | undefined): Rect[] => section?.items.map(rectOf) ?? [];
const place = (section: BoardSection, rects: Rect[]) => { section.items = section.items.map((item, i) => ({ ...item, ...rects[i] })); };
const settle = (section: BoardSection) => place(section, compact(rectsOf(section)));
const plainLook = (look: TileLook | undefined) => !look || (!look.accent && !look.icon && !look.link && !look.hide_status && !look.hide_metrics);

const editable = (doc: BoardDocument) => JSON.stringify([doc.name ?? null, doc.icon ?? null, doc.appearance, doc.sections]);
export function dirty(base: BoardDocument, next: BoardDocument): boolean {
  return editable(base) !== editable(next);
}
export type Action =
  | { type: "section"; title: string }
  | { type: "rename"; section: number; title: string }
  | { type: "collapse"; section: number; collapsed: boolean }
  | { type: "delete-section"; section: number }
  | { type: "move-section"; section: number; to: number }
  | { type: "add"; section: number; payload: BoardPayload; size?: Size }
  | { type: "delete"; section: number; item: number }
  | { type: "layout"; section: number; rects: Rect[] }
  | { type: "transfer"; section: number; item: number; toSection: number; x: number; y: number }
  | { type: "payload"; section: number; item: number; payload: BoardPayload }
  | { type: "look"; section: number; item: number; look: TileLook | undefined }
  | { type: "board"; name?: string; icon?: string | null; appearance?: BoardAppearance };
export const itemCount = (doc: BoardDocument) => doc.sections.reduce((n, s) => n + s.items.length, 0);
export function reduceDocument(document: BoardDocument, action: Action): BoardDocument {
  const doc = structuredClone(document);
  const section = "section" in action ? doc.sections[action.section] : undefined;
  const item = section && "item" in action ? section.items[action.item] : undefined;
  switch (action.type) {
    case "section": if (doc.sections.length < MAX_SECTIONS) doc.sections.push({ title: action.title, position: 0, collapsed: false, items: [] }); break;
    case "rename": if (section) section.title = action.title; break;
    case "collapse": if (section) section.collapsed = action.collapsed; break;
    case "delete-section": doc.sections.splice(action.section, 1); break;
    case "move-section": if (section) { doc.sections.splice(action.section, 1); doc.sections.splice(Math.max(0, action.to), 0, section); } break;
    case "add": if (section && itemCount(doc) < MAX_ITEMS) {
      const size = action.size ?? defaultSize(action.payload.kind);
      section.items.push({ ...firstFree(rectsOf(section), size.w, size.h), payload: action.payload });
    } break;
    case "delete": if (item && section) { section.items.splice(action.item, 1); settle(section); } break;
    case "layout": if (section && action.rects.length === section.items.length) place(section, compact(action.rects.map((r) => fit(r)))); break;
    case "transfer": {
      const target = doc.sections[action.toSection];
      if (!item || !section || !target) break;
      if (target === section) { place(section, moveTo(rectsOf(section), action.item, action.x, action.y)); break; }
      section.items.splice(action.item, 1); settle(section);
      target.items.push(item);
      place(target, moveTo(rectsOf(target), target.items.length - 1, action.x, action.y));
      break;
    }
    case "payload": if (item) item.payload = action.payload; break;
    case "look": if (item) { if (plainLook(action.look)) delete item.look; else item.look = action.look; } break;
    case "board":
      if (action.name !== undefined) doc.name = action.name;
      if (action.icon !== undefined) { if (action.icon) doc.icon = action.icon; else delete doc.icon; }
      if (action.appearance) doc.appearance = action.appearance;
      break;
  }
  return normalize(doc);
}
export function normalize(doc: BoardDocument): BoardDocument {
  return { ...doc, sections: doc.sections.map((s, position) => ({ ...s, position })) };
}
const changed = (a: unknown, b: unknown) => JSON.stringify(a) !== JSON.stringify(b);
/** Three-way rebase: explicit local edits win; unrelated remote additions survive.
 * Entries deleted remotely are recreated without their now-invalid server IDs.
 * Merged sections are compacted, so concurrent moves never overlap. */
export function rebase(base: BoardDocument, mine: BoardDocument, current: BoardDocument): BoardDocument {
  const sections: BoardSection[] = [];
  const baseIds = new Set(base.sections.map((s) => s.id).filter(Boolean));
  for (const local of mine.sections) {
    const before = local.id ? base.sections.find((s) => s.id === local.id) : undefined;
    const remote = local.id ? current.sections.find((s) => s.id === local.id) : undefined;
    if (!before) {
      const { id: _id, ...entry } = local;
      const validItems = new Set(current.sections.flatMap((s) => s.items).map((i) => i.id));
      sections.push({ ...entry, ...(remote?.id ? { id: remote.id } : {}), items: local.items.map((i) => {
        if (!i.id || validItems.has(i.id)) return i;
        const { id: _itemId, ...newItem } = i; return newItem;
      }) });
      continue;
    }
    if (!remote) {
      if (changed(before, local)) sections.push({ title: local.title, position: local.position, collapsed: local.collapsed, items: local.items.map(({ id: _id, ...i }) => i) });
      continue;
    }
    const items: BoardItem[] = [];
    const originalItems = base.sections.flatMap((s) => s.items);
    const localItems = mine.sections.flatMap((s) => s.items);
    for (const item of local.items) {
      const old = item.id ? originalItems.find((i) => i.id === item.id) : undefined;
      const currentSection = item.id ? current.sections.find((s) => s.items.some((i) => i.id === item.id)) : undefined;
      const latest = currentSection?.items.find((i) => i.id === item.id);
      const originalSection = old ? base.sections.find((s) => s.items.some((i) => i.id === old.id)) : undefined;
      const moved = !!old && (originalSection?.id !== local.id || old.x !== item.x || old.y !== item.y);
      if (!old) {
        if (item.id && !latest) { const { id: _id, ...newItem } = item; items.push(newItem); }
        else items.push(item);
        continue;
      }
      const payloadChanged = changed(old.payload, item.payload);
      const sized = old.w !== item.w || old.h !== item.h;
      const lookChanged = changed(old.look, item.look);
      if (!latest) {
        if (moved || payloadChanged || sized || lookChanged) { const { id: _id, ...newItem } = item; items.push(newItem); }
        continue;
      }
      if (!moved && currentSection?.id !== local.id) continue;
      const merged: BoardItem = { ...latest, ...(payloadChanged ? { payload: item.payload } : {}), ...(sized ? { w: item.w, h: item.h } : {}), ...(moved ? { x: item.x, y: item.y } : {}) };
      if (lookChanged) { if (item.look) merged.look = item.look; else delete merged.look; }
      items.push(merged);
    }
    for (const item of remote.items) {
      if (items.some((i) => i.id === item.id)) continue;
      const old = originalItems.find((i) => i.id === item.id);
      const localItem = localItems.find((i) => i.id === item.id);
      const originalSection = old ? base.sections.find((s) => s.items.some((i) => i.id === old.id)) : undefined;
      const localSection = localItem ? mine.sections.find((s) => s.items.some((i) => i.id === item.id)) : undefined;
      if (!old || (localItem && localItem.x === old.x && localItem.y === old.y && localSection?.id === originalSection?.id)) items.push(item);
    }
    sections.push({ ...remote, title: local.title === before.title ? remote.title : local.title, collapsed: local.collapsed === before.collapsed ? remote.collapsed : local.collapsed, items });
  }
  for (const remote of current.sections) if (!baseIds.has(remote.id)) sections.push(remote);
  for (const section of sections) settle(section);
  const pick = <K extends "name" | "icon" | "appearance">(key: K) => changed(base[key], mine[key]) ? mine[key] : current[key];
  const doc: BoardDocument = { version: current.version, appearance: pick("appearance") ?? DEFAULT_APPEARANCE, sections };
  const name = pick("name"); const icon = pick("icon");
  if (name !== undefined) doc.name = name;
  if (icon !== undefined) doc.icon = icon;
  return normalize(doc);
}
export function iconSource(name: string | null | undefined, enabled: boolean): string | null {
  return enabled && name && /^[a-zA-Z0-9_-]{1,96}$/.test(name) ? `/api/v1/icons/${encodeURIComponent(name)}` : null;
}
export function avatar(title: string): string { return Array.from(title.trim())[0]?.toUpperCase() ?? "?"; }
export function origin(url: string): string | null {
  try { const u = new URL(url); return /^https?:$/.test(u.protocol) && !u.username && !u.password && !u.search && !u.hash ? u.origin : null; } catch { return null; }
}
export function needsSecret(savedOrigin: string | null | undefined, url: string): boolean { return !!savedOrigin && origin(url) !== savedOrigin; }

/** A recovered stale draft has no historical base document in the API.
 * Exclude unseen server entries from its synthetic base so recovery preserves
 * additions rather than mistaking them for local deletions. */
export function recoveryBase(draft: BoardDocument, current: BoardDocument): BoardDocument {
  const sectionIds = new Set(draft.sections.map((s) => s.id).filter(Boolean));
  const itemIds = new Set(draft.sections.flatMap((s) => s.items).map((i) => i.id).filter(Boolean));
  const sections: BoardSection[] = [];
  for (const s of current.sections) if (sectionIds.has(s.id)) sections.push({ ...s, items: s.items.filter((i) => itemIds.has(i.id)) });
  return { ...current, version: draft.version, sections };
}

/** The suggestions whose target is not taken yet, the first of each
 * identity only, so a list keyed by identity has unique keys. */
export function freshSuggestions<T extends { payload: BoardPayload }>(suggestions: T[], taken: Iterable<string>): T[] {
  const seen = new Set(taken);
  return suggestions.filter(({ payload }) => {
    const id = payloadIdentity(payload);
    return !seen.has(id) && !!seen.add(id);
  });
}

export function payloadIdentity(payload: BoardPayload): string {
  switch (payload.kind) {
    case "container": return JSON.stringify(payload.target);
    case "link": {
      const match = /^(https?):\/\/(\[[^\]]+\]|[^:/?#@]+)(?::(\d+))?([/?#].*|)$/i.exec(payload.url);
      const [, rawScheme, host, port, suffix = ""] = match ?? [];
      if (!rawScheme || !host) return `link:${payload.url}`;
      const scheme = rawScheme.toLowerCase();
      const defaultPort = scheme === "https" ? 443 : 80;
      const authority = port && Number(port) !== defaultPort ? `${host.toLowerCase()}:${Number(port)}` : host.toLowerCase();
      return `link:${scheme}://${authority}${suffix.startsWith("/") ? "" : "/"}${suffix}`;
    }
    case "widget": return `widget:${payload.widget_id}`;
    case "integration": return `integration:${payload.integration_id}`;
  }
}
