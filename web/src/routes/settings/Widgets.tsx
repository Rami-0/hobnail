import { useState } from "react";
import { api } from "../../api/session.ts";
import { Note } from "../../ui/Note.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { Collection, type Entry } from "./Collection.tsx";
import type { Values } from "./forms.ts";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

type WidgetsData = { hash: string; widgets: Entry[]; system: { enabled: boolean } };
const bound = (item: Values) => [item.headers_env, item.headers_file].some((map) =>
  map && typeof map === "object" && Object.keys(map).length > 0);
const fields: Control[] = [
  { key: "type", label: "Type", default: "custom-api", show: () => false },
  { key: "id", label: "ID", required: true, validate: (value) => /^[\w-]{1,64}$/.test(value) ? undefined : "Use 1–64 letters, digits, hyphens or underscores." },
  { key: "title", label: "Title", max: 80 },
  { key: "url", label: "URL", format: "url", required: true, locked: bound },
  { key: "method", label: "Method", options: ["GET", "POST"], default: "GET", locked: (item) =>
    typeof item.method === "string" && !["GET", "POST"].includes(item.method) },
  { key: "link", label: "Link", format: "link" },
  { key: "icon", label: "Icon" },
  { key: "refresh", label: "Refresh", placeholder: "30s" },
  { key: "timeout", label: "Timeout", placeholder: "5s" },
  { key: "enabled", label: "Enabled", format: "switch", default: true },
  { key: "fields", label: "Fields", format: "map", hint: "One name=path pair per line." },
  { key: "mode", label: "Show as", options: ["template", "rows"], default: "template" },
  { key: "template", label: "Template", show: (item) => item.mode === "template", hint: "Use double braces around field names." },
  { key: "rows", label: "Display rows", format: "json", show: (item) => item.mode === "rows", hint: 'Enter a list of {"label":"…","value":"…"} objects.' },
  { key: "status_field", label: "Status field" },
  { key: "status_ok", label: "OK values", format: "list", hint: "Separate multiple values with commas." },
];

function body(changes: Values, input: Values) {
  const { mode, status_field, status_ok, ...patch } = changes;
  if (mode !== undefined) {
    patch.rows = mode === "rows" ? input.rows : null;
    patch.template = mode === "template" ? input.template : null;
  }
  if (status_field !== undefined || status_ok !== undefined) {
    patch.status = input.status_field ? { field: input.status_field, ok: Array.isArray(input.status_ok)
      ? input.status_ok.map((value: string) => { try { return JSON.parse(value) as unknown; } catch { return value; } }) : [] } : null;
  }
  return { id: input.id, type: input.type ?? "custom-api", url: input.url, ...patch };
}

function widgetRow(entry: Entry) {
  return <>
    <p>{String(entry.item.type ?? "custom-api")} · {String(entry.item.refresh ?? "30s")}</p>
    {bound(entry.item) && <p>Secret headers are bound to the saved URL.</p>}
    {entry.locked && Array.isArray(entry.item.command) && <p><code>{entry.item.command.join(" ")}</code></p>}
  </>;
}

export default function Widgets({ editorOnly }: { editorOnly?: boolean }) {
  const { data, failure, load } = useSettings<WidgetsData>("widgets");
  const { data: notifications } = useSettings<{ rules: Entry[] }>("notifications");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<Failure>();
  const entries = (value: WidgetsData) => value.widgets.map((entry) => {
    const { status } = entry.item;
    const state = status && typeof status === "object" ? status as Values : {};
    return { ...entry, original: entry.item, item: { ...entry.item, enabled: entry.enabled, mode: entry.item.rows && !entry.item.template ? "rows" : "template",
      status_field: state.field, status_ok: state.ok } };
  });
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <>
      {!editorOnly && <section className="settings-section"><h2>Built-in widgets</h2>
        <Switch label="Host vitals" checked={data.system.enabled} busy={busy} onChange={async (enabled) => {
          setBusy(true);
          const result = await api.post<Save>(endpoint("system"), { base_hash: data.hash, enabled });
          if (result.kind === "ok") { setProblem(undefined); saved(result.data); await load(); }
          else setProblem(result);
          setBusy(false);
        }} /><FailureNote failure={problem} />
      </section>}
      <Collection kind="widget" title="Custom widgets" path="widgets" fields={fields} hash={data.hash}
        entries={entries(data)} reorder editorOnly={editorOnly} row={widgetRow} renameKey="id"
        renameRules={(id) => notifications?.rules.filter((rule) => rule.item.widget === id).map((rule) => String(rule.id)) ?? []}
        body={body} create={(input) => body(input, input)}
        preview={async (item, entry) => {
          const kept = entry ? Object.fromEntries(["headers", "headers_env", "headers_file", "body", "output", "type"]
            .filter((key) => entry.item[key] !== undefined).map((key) => [key, entry.item[key]])) : {};
          const result = await api.post<{ ok: boolean; error?: string; values?: unknown }>(endpoint("widgets/preview"), { item: { ...item, ...kept } });
          return result.kind !== "ok" ? result.message : result.data.ok
            ? JSON.stringify(result.data.values ?? {}, null, 2) : result.data.error ?? "Fetch failed.";
        }}
        refresh={async () => {
          const result = await load();
          if (result.kind === "ok") return { hash: result.data.hash, entries: entries(result.data) };
        }} />
      {!editorOnly && <Note kind="info">Command widgets are managed in the host configuration file.</Note>}
    </>}
  </>;
}
