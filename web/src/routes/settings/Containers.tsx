import { useState } from "react";
import { api } from "../../api/session.ts";
import { useRoute } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { EditDialog } from "../container/EditDialog.tsx";
import { Collection, type Entry } from "./Collection.tsx";
import type { Values } from "./forms.ts";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

type Known = { name: string; project: string | null; image: string; state: string; overridden: boolean };
type Data = { hash: string; docker: { exclude: string[]; standalone_group: string }; groups: Values[]; overrides: Record<string, Values> | null; known: Known[] };
const patterns = (data: Data): Entry[] => data.docker.exclude.map((pattern, id) => ({ id, item: { pattern } }));
const groups = (data: Data): Entry[] => data.groups.map((item) => ({ id: String(item.name), item }));
const patternFields: Control[] = [{ key: "pattern", label: "Pattern", required: true, max: 256, hint: "A container name or glob." }];
const groupFields: Control[] = [
  { key: "name", label: "Name", required: true, max: 80 },
  { key: "title", label: "Title", max: 80 },
  { key: "icon", label: "Icon" },
  { key: "collapsed", label: "Start collapsed", format: "switch" },
  { key: "hide", label: "Hide group", format: "switch" },
];

export default function Containers() {
  const { data, load, failure } = useSettings<Data>("containers");
  const { search, setSearch } = useRoute();
  const [query, setQuery] = useState("");
  const [only, setOnly] = useState(false);
  const [selection, setSelection] = useState<string[]>([]);
  const [group, setGroup] = useState("");
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Failure>();
  const edit = search.get("edit");
  const name = edit?.startsWith("override:") ? edit.slice(9) : "";
  const close = () => setSearch({ edit: null }, { replace: true });
  const refresh = (entries: (value: Data) => Entry[]) => async () => {
    const result = await load();
    if (result.kind === "ok") return { hash: result.data.hash, entries: entries(result.data) };
  };
  const rows = [...(data?.known ?? [])];
  for (const key of Object.keys(data?.overrides ?? {})) {
    if (!rows.some((row) => row.name === key)) rows.push({ name: key, overridden: true, image: "", state: "Not running", project: null });
  }
  const visible = rows.filter((row) => (!only || row.overridden) && `${row.name} ${row.project ?? ""} ${row.image}`.toLowerCase().includes(query.trim().toLowerCase()));
  async function bulk(patch: Values) {
    if (!data || busy) return;
    setBusy(true);
    setError(undefined);
    const result = await api.post<Save>(endpoint("containers/override"), { base_hash: data.hash, names: selection, ...patch });
    if (result.kind === "ok") {
      setConfirm(false);
      setSelection([]);
      saved(result.data);
      await load();
    } else setError(result);
    setBusy(false);
  }
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <>
      <Collection kind="pattern" title="Hidden containers" path="docker" fields={patternFields} entries={patterns(data)}
        hash={data.hash} refresh={refresh(patterns)} />
      <Collection kind="group" title="Groups" path="groups" fields={groupFields} entries={groups(data)}
        hash={data.hash} reorder body={(patch, input) => ({ name: input.name, ...patch })} refresh={refresh(groups)} />
      <section className="settings-section" data-testid="settings-containers">
        <h2>Containers</h2>
        <div className="settings-actions">
          <Field label="Search containers" id="settings-container-search"><TextInput type="search" value={query}
            onChange={(event) => setQuery(event.target.value)} placeholder="Name, project, image" /></Field>
          <Switch checked={only} label="Only with overrides" onChange={setOnly} />
        </div>
        <p role="status">{visible.length} containers</p>
        {visible.length > 0 && <Switch label="Select visible containers" checked={visible.every((row) => selection.includes(row.name))}
          onChange={(checked) => setSelection((selected) => checked
            ? [...new Set([...selected, ...visible.map((row) => row.name)])]
            : selected.filter((key) => !visible.some((row) => row.name === key)))} />}
        {selection.length > 0 && <div className="settings-actions" role="region" aria-label="Bulk edit">
          <output>{selection.length} selected</output>
          <Field label="Group for the selected containers" id="settings-bulk-group"><TextInput value={group} onChange={(event) => setGroup(event.target.value)} /></Field>
          <Button disabled={busy || !group.trim()} onClick={() => { void bulk({ set: { group: group.trim() } }); }}>Set group</Button>
          <Button disabled={busy} onClick={() => { void bulk({ set: { hide: true } }); }}>Hide</Button>
          <Button disabled={busy} onClick={() => { void bulk({ set: { hide: false } }); }}>Show</Button>
          <Button disabled={busy} className="danger" onClick={() => { setError(undefined); setConfirm(true); }}>Remove overrides</Button>
          <Button disabled={busy} onClick={() => setSelection([])}>Clear selection</Button>
        </div>}
        {!confirm && <FailureNote failure={error?.kind === "invalid" ? { ...error, field: undefined } : error} rebase={() => { void load(); }} />}
        <div className="settings-list">{visible.map((row) => <div className="settings-row" key={row.name}>
          <div><Switch checked={selection.includes(row.name)} label={row.name}
            onChange={(checked) => setSelection((items) => checked ? [...items, row.name] : items.filter((key) => key !== row.name))} />
            <p className="muted">{row.project || row.image} · {row.state}{row.overridden && " · Has overrides"}</p></div>
          <Button onClick={() => setSearch({ add: null, edit: `override:${row.name}` })}>Edit override</Button>
        </div>)}</div>
      </section>
    </>}
    {name && <EditDialog open name={name} title={name} onClosed={close} onSaved={() => { void load(); }} />}
    <Dialog open={confirm} title="Remove overrides?" onClose={() => { if (!busy) setConfirm(false); }}
      footer={<Button className="danger" busy={busy} onClick={() => { void bulk({ remove: true }); }}>Remove overrides</Button>}>
      <p>Remove the configuration overrides for {selection.length} selected containers?</p>
      <FailureNote failure={error?.kind === "invalid" ? { ...error, field: undefined } : error} rebase={() => { void load(); }} />
    </Dialog>
  </>;
}
