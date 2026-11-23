import type { BookmarkView } from "./gen/BookmarkView.ts";
import { api } from "./session.ts";

export async function loadBookmarks(signal: AbortSignal): Promise<BookmarkView[]> {
  const result = await api.get<BookmarkView[]>("/api/v1/bookmarks", { signal });
  return result.kind === "ok" ? result.data : [];
}
