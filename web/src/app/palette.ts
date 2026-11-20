export type PaletteKind = "Page" | "Settings" | "Board" | "Bookmark" | "Container" | "Action" | "Logs" | "Open" | "Search";

export interface PaletteItem {
  kind: PaletteKind;
  name: string;
  detail?: string;
  keywords?: string;
  href?: string;
  run?: () => void;
  children?: PaletteItem[];
}

const ORDER: Partial<Record<PaletteKind, number>> = { Page: 0, Board: 1, Bookmark: 2, Action: 3, Container: 4 };

export function score(item: PaletteItem, query: string): number {
  const name = item.name.toLowerCase();
  if (name === query) return 100;
  if (name.startsWith(query)) return 50;
  const i = name.indexOf(query);
  if (i > 0 && /[\s\-_./:]/.test(name.charAt(i - 1))) return 30;
  if (i >= 0) return 20;
  return `${item.keywords ?? ""} ${item.detail ?? ""}`.toLowerCase().includes(query) ? 8 : 0;
}

export function rankPalette(base: PaletteItem[], text: string): PaletteItem[] {
  const q = text.trim().toLowerCase();
  if (!q) return base.filter((item) => ORDER[item.kind] !== undefined)
    .toSorted((a, b) => (ORDER[a.kind] ?? 0) - (ORDER[b.kind] ?? 0)).slice(0, 60);
  const rank = (items: PaletteItem[], min: number) => items.map((item) => ({ item, points: score(item, q) }))
    .filter(({ points }) => points >= min).toSorted((a, b) => b.points - a.points).map(({ item }) => item);
  const seen = new Set<string>();
  return [
    ...rank(base, 1).flatMap((item) => [item].concat(item.children ?? [])),
    ...rank(base.flatMap((item) => item.children ?? []), 20),
  ].filter((item) => {
    const id = JSON.stringify([item.kind, item.name, item.href, item.detail]);
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  }).slice(0, 59);
}

export function paletteIndex(key: string, current: number, count: number): number | undefined {
  if (!count) return undefined;
  switch (key) {
    case "ArrowDown": return (current + 1) % count;
    case "ArrowUp": return current <= 0 ? count - 1 : current - 1;
    case "Home": return 0;
    case "End": return count - 1;
    case "PageDown": return Math.min(count - 1, current + 8);
    case "PageUp": return Math.max(0, current - 8);
  }
}
