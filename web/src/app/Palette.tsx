import { local as localStoragePrefs, settingsHref } from "./prefs.ts";
import { settingsTabs } from "./settings-tabs.ts";
import { useEffect, useId, useRef, useState } from "react";
import { signOut } from "./Account.tsx";
import { containerHref, logsHref } from "./paths.ts";
import { navigate } from "./router.ts";
import { paletteIndex, rankPalette, type PaletteItem, type PaletteKind } from "./palette.ts";
import { chooseTheme } from "./ThemeButton.tsx";
import { THEMES, THEME_LABEL } from "./theme.ts";
import { rowLink, viewerHost } from "../format/index.ts";
import { useBootstrap, useContainers } from "../live/index.ts";
import { Button } from "../ui/Button.tsx";
import { TextInput } from "../ui/Field.tsx";
import { Dialog } from "../ui/Dialog.tsx";
import "./palette.css";

const page = (name: string, path: string, kind: PaletteKind = "Page"): PaletteItem => ({ name, href: `/app/${path}`, kind });
const PAGES = [["Home", "overview"], ["Host", "host"], ["History", "history"], ["Alerts", "alerts"], ["Logs", "logs"]];
const ADD = [["Add bookmark", "bookmarks?add=bookmark"], ["Add widget", "widgets?add=widget"],
  ["Add channel", "notifications?add=channel"], ["Add alert rule", "notifications?add=rule"]];

function external(href: string): boolean {
  try {
    const u = new URL(href, window.location.origin);
    if (u.protocol === "http:" || u.protocol === "https:") {
      window.open(u.href, "_blank", "noopener,noreferrer");
      return true;
    }
  } catch {
    return false;
  }
  return false;
}

export default function Palette({ onClose, onHelp }: { onClose: () => void; onHelp: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const id = useId();
  const boot = useBootstrap();
  const { data } = useContainers();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [bookmarks, setBookmarks] = useState<PaletteItem[]>([]);
  const scoped = boot?.scoped ?? true;
  const edit = boot && !scoped && boot.role !== "viewer";
  useEffect(() => {
    if (scoped) return;
    const c = new AbortController();
    void import("../api/bookmarks.ts").then(({ loadBookmarks }) => loadBookmarks(c.signal)).then((items) => {
      if (!c.signal.aborted) setBookmarks(items.map((b) => ({ kind: "Bookmark", name: b.name, href: b.url, detail: b.description ?? undefined })));
    });
    return () => c.abort();
  }, [scoped]);
  const base: PaletteItem[] = [
    page("Containers", "containers"),
    ...(!scoped ? PAGES.map(([name, path]) => page(name ?? "", path ?? "")) : []),
    ...(boot && boot.role !== "viewer" ? [{ kind: "Page" as const, name: "Settings", href: `/app${settingsHref(localStoragePrefs(), boot.role === "owner", scoped)}` }] : []),
    ...(edit ? Object.entries(settingsTabs).filter(([path]) => path !== "users" || boot.role === "owner").map(([path, name]) => page(name, `settings/${path}`, "Settings")) : []),
    ...bookmarks,
    ...boot?.boards.map((b) => page(b.name, `boards/${b.id}`, "Board")) ?? [],
    ...(edit ? ADD.map(([name, path]) => ({ kind: "Action" as const, name: name ?? "", href: `/app/settings/${path}`, keywords: "new create" })) : []),
    ...THEMES.map((theme): PaletteItem => ({ kind: "Action", name: `Theme: ${THEME_LABEL[theme]}`, keywords: `colour color ${theme}`, run: () => chooseTheme(theme) })),
    { kind: "Action", name: "Keyboard shortcuts", run: onHelp },
    { kind: "Action", name: "Sign out", run: () => void signOut() },
    ...(data?.groups.flatMap((g) => g.containers.map((row): PaletteItem => {
      const href = containerHref(row.name);
      const url = rowLink(row, viewerHost(window.location.hostname));
      const children: PaletteItem[] = [];
      children.push({ kind: "Logs", name: `Logs: ${row.title}`, href: logsHref(row.name) });
      if (url) children.push({ kind: "Open", name: "Open service ↗", href: url, detail: row.title });
      if (edit) children.push({ kind: "Action", name: `Edit ${row.title}`, href: `${href}?edit=override` });
      return { kind: "Container", name: row.title, detail: row.stack, keywords: row.name, href, children };
    })) ?? []),
  ];
  const local = rankPalette(base, query);
  const text = query.trim();
  const web: PaletteItem[] = text ? [{
    kind: "Search", name: text, detail: "Search the web",
    href: /^https?:\/\//i.test(text) ? text : (boot?.search_url || "https://duckduckgo.com/?q={q}").replace("{q}", encodeURIComponent(text)),
  }] : [];
  const rows = [...local, ...web];
  const active = selected === null ? (local.length ? 0 : -1) : Math.min(selected, rows.length - 1);
  useEffect(() => {
    const item = document.getElementById(`${id}-${active}`);
    const box = list.current;
    if (!item || !box) return;
    const row = item.getBoundingClientRect();
    const parent = box.getBoundingClientRect();
    if (row.top < parent.top) box.scrollTop += row.top - parent.top;
    else if (row.bottom > parent.bottom) box.scrollTop += row.bottom - parent.bottom;
  }, [active, id]);
  const open = (item: PaletteItem | undefined) => {
    if (!item) return;
    onClose();
    if (item.run) item.run();
    else if (item.href) {
      if (item.kind === "Search" || item.kind === "Open" || item.kind === "Bookmark") external(item.href);
      else navigate(item.href);
    }
  };
  return <Dialog open title="Search and commands" onClose={onClose} initialFocus={input} testId="palette" wide>
    <TextInput ref={input} className="palette-input" value={query} placeholder="Search pages, containers and actions" role="combobox"
      aria-label="Search and commands" aria-expanded="true" aria-controls={id}
      aria-activedescendant={active >= 0 ? `${id}-${active}` : undefined}
      onChange={(event) => { setQuery(event.target.value); setSelected(null); }}
      onKeyDown={(event) => {
        const next = paletteIndex(event.key, active, rows.length);
        if (next !== undefined) { event.preventDefault(); setSelected(next); }
        else if (event.key === "Enter") { event.preventDefault(); open(rows[active]); }
        else if (event.key === "Tab") event.preventDefault();
      }} />
    {text && !local.length && <p role="status">No matches. Press Arrow Down to select web search.</p>}
    <div ref={list} className="palette-results" role="listbox" id={id} aria-label="Results">
      {rows.map((row, i) => <Button variant="ghost" tabIndex={-1} key={JSON.stringify([row.kind, row.name, row.href, row.detail])}
        role="option" id={`${id}-${i}`} aria-selected={i === active} onPointerMove={() => setSelected(i)} onClick={() => open(row)}>
        <small>{row.kind}</small><span>{row.name}</span>{row.detail && <small>{row.detail}</small>}
      </Button>)}
    </div>
  </Dialog>;
}
