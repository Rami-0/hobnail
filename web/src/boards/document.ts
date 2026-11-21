import type { BoardDocument } from "../api/gen/BoardDocument.ts";
import type { BoardItem } from "../api/gen/BoardItem.ts";
import type { BoardSection } from "../api/gen/BoardSection.ts";
import type { GridSpans } from "../api/gen/GridSpans.ts";
import type { BoardPayload } from "../api/gen/BoardPayload.ts";

export const DEFAULT_SPANS: GridSpans = { mobile: 4, tablet: 4, desktop: 4, rows: 1 };
const MAX: GridSpans = { mobile: 4, tablet: 8, desktop: 12, rows: 12 };
export function clampSpans(spans: GridSpans): GridSpans {
  return Object.fromEntries(Object.entries(MAX).map(([k, max]) => [k, Math.min(max, Math.max(1, Math.round(spans[k as keyof GridSpans]) || 1))])) as GridSpans;
}
export function dirty(base: BoardDocument, next: BoardDocument): boolean {
  return JSON.stringify(base.sections) !== JSON.stringify(next.sections);
}
export type Action =
  | { type: "section"; title: string }
  | { type: "rename"; section: number; title: string }
  | { type: "delete-section"; section: number }
  | { type: "move-section"; section: number; to: number }
  | { type: "add"; section: number; payload: BoardPayload }
  | { type: "delete"; section: number; item: number }
  | { type: "move"; section: number; item: number; toSection: number; to: number }
  | { type: "resize"; section: number; item: number; spans: GridSpans }
  | { type: "payload"; section: number; item: number; payload: BoardPayload };
export function reduceDocument(document: BoardDocument, action: Action): BoardDocument {
  const doc = structuredClone(document);
  const section = "section" in action ? doc.sections[action.section] : undefined;
  const item = section && "item" in action ? section.items[action.item] : undefined;
  switch (action.type) {
    case "section": if (doc.sections.length < 64) doc.sections.push({ title: action.title, position: 0, items: [] }); break;
    case "rename": if (section) section.title = action.title; break;
    case "delete-section": doc.sections.splice(action.section, 1); break;
    case "move-section": if (section) { doc.sections.splice(action.section, 1); doc.sections.splice(Math.max(0, action.to), 0, section); } break;
    case "add": if (section && doc.sections.reduce((n, s) => n + s.items.length, 0) < 512) section.items.push({ position: 0, spans: { ...DEFAULT_SPANS }, payload: action.payload }); break;
    case "delete": section?.items.splice(action.item, 1); break;
    case "move": if (item && doc.sections[action.toSection]) { section?.items.splice(action.item, 1); doc.sections[action.toSection]?.items.splice(Math.max(0, action.to), 0, item); } break;
    case "resize": if (item) item.spans = clampSpans(action.spans); break;
    case "payload": if (item) item.payload = action.payload; break;
  }
  return normalize(doc);
}
export function normalize(doc: BoardDocument): BoardDocument {
  return { version: doc.version, sections: doc.sections.map((s, position) => ({ ...s, position, items: s.items.map((i, itemPosition) => ({ ...i, position: itemPosition, spans: clampSpans(i.spans) })) })) };
}
/** Three-way rebase: explicit local edits win; unrelated remote additions survive.
 * Entries deleted remotely are recreated without their now-invalid server IDs. */
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
      if (JSON.stringify(before) !== JSON.stringify(local)) sections.push({ title: local.title, position: local.position, items: local.items.map(({ id: _id, ...i }) => i) });
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
      const moved = !!old && (originalSection?.id !== local.id || old.position !== item.position);
      if (!old) {
        if (item.id && !latest) { const { id: _id, ...newItem } = item; items.push(newItem); }
        else items.push(item);
        continue;
      }
      const payloadChanged = JSON.stringify(old.payload) !== JSON.stringify(item.payload);
      const spansChanged = JSON.stringify(old.spans) !== JSON.stringify(item.spans);
      if (!latest) {
        if (moved || payloadChanged || spansChanged) { const { id: _id, ...newItem } = item; items.push(newItem); }
        continue;
      }
      if (!moved && currentSection?.id !== local.id) continue;
      items.push({ ...latest, ...(payloadChanged ? { payload: item.payload } : {}), ...(spansChanged ? { spans: item.spans } : {}), ...(moved ? { position: item.position } : {}) });
    }
    for (const item of remote.items) {
      if (items.some((i) => i.id === item.id)) continue;
      const old = originalItems.find((i) => i.id === item.id);
      const localItem = localItems.find((i) => i.id === item.id);
      const originalSection = old ? base.sections.find((s) => s.items.some((i) => i.id === old.id)) : undefined;
      const localSection = localItem ? mine.sections.find((s) => s.items.some((i) => i.id === item.id)) : undefined;
      if (!old || (localItem && localItem.position === old.position && localSection?.id === originalSection?.id)) items.push(item);
    }
    if (JSON.stringify(before.items.map((i) => i.id)) === JSON.stringify(local.items.map((i) => i.id))) items.sort((a, b) => a.position - b.position);
    sections.push({ ...remote, title: local.title === before.title ? remote.title : local.title, items });
  }
  for (const remote of current.sections) if (!baseIds.has(remote.id)) sections.push(remote);
  return normalize({ version: current.version, sections });
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
  return { version: draft.version, sections: current.sections.filter((s) => sectionIds.has(s.id)).map((s) => ({ id: s.id, title: s.title, position: s.position, items: s.items.filter((i) => itemIds.has(i.id)) })) };
}

export function payloadIdentity(payload: BoardPayload): string {
  switch (payload.kind) {
    case "container": return JSON.stringify(payload.target);
    case "link": {
      const match = /^(https?):\/\/(\[[^\]]+\]|[^:/?#@]+)(?::(\d+))?([/?#].*|)$/i.exec(payload.url);
      if (!match) return `link:${payload.url}`;
      const [, rawScheme, host, port, suffix] = match;
      const scheme = rawScheme.toLowerCase();
      const defaultPort = scheme === "https" ? 443 : 80;
      const authority = port && Number(port) !== defaultPort ? `${host.toLowerCase()}:${Number(port)}` : host.toLowerCase();
      return `link:${scheme}://${authority}${suffix.startsWith("/") ? "" : "/"}${suffix}`;
    }
    case "widget": return `widget:${payload.widget_id}`;
    case "integration": return `integration:${payload.integration_id}`;
  }
}

