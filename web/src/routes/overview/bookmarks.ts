import type { BookmarkView } from "../../api/gen/BookmarkView.ts";
import { api } from "../../api/session.ts";

export async function loadBookmarks(signal: AbortSignal): Promise<BookmarkView[]> {
  const result = await api.get<BookmarkView[]>("/api/v1/bookmarks", { signal });
  return result.kind === "ok" ? result.data : [];
}
