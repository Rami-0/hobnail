import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, getSession, refreshSession } from "../../api/session.ts";
import { useRoute } from "../../app/router.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Note } from "../../ui/Note.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { toast } from "../../ui/toast.ts";
import { changedFields, dialogState, fieldErrors, rebaseFields, type Values } from "./forms.ts";
import { collectFields, Controls, endpoint, FailureNote, initialFields, saved, type Control, type Failure, type Save } from "./shared.tsx";

export type Entry = { id: string | number; item: Values; locked?: boolean; locked_reason?: string; enabled?: boolean };
type Snapshot = { hash: string; entries: Entry[] };
type Refresh = () => Promise<Snapshot | undefined>;
type Operation = { action: "add" | "edit" | "delete" | "reorder"; id?: Entry["id"]; item?: Values; order?: Entry["id"][] };

type Props = Snapshot & {
  kind: string;
  path: string;
  fields: Control[];
  refresh: Refresh;
  title: string;
  reorder?: boolean;
  row?: (entry: Entry) => ReactNode;
  extra?: (entry: Entry) => ReactNode;
  preview?: (item: Values) => Promise<string>;
  body?: (changes: Values, input: Values) => Values;
  create?: (input: Values) => Values;
  renameKey?: string;
  renameRules?: (id: Entry["id"]) => string[];
  editorOnly?: boolean;
};

export function Collection(props: Props) {
  const { kind, title, entries, hash, path, refresh, reorder, row, extra, editorOnly } = props;
  const { search, setSearch } = useRoute();
  const selected = dialogState(search, kind);
  const entry = !selected || selected.id === null ? undefined : entries.find((item) => String(item.id) === selected.id);
  const [removing, setRemoving] = useState<Entry>();
  const [failure, setFailure] = useState<Failure>();
  const [busy, setBusy] = useState(false);
  const refs = useRef(new Map<Entry["id"], HTMLElement>());
  const add = useRef<HTMLButtonElement>(null);
  const close = () => setSearch({ add: null, edit: null }, { replace: true });
  function focus(id?: Entry["id"]) {
    window.setTimeout(() => (id === undefined ? add.current : refs.current.get(id) ?? add.current)?.focus(), 0);
  }
  async function run(operation: Operation) {
    if (busy) return;
    setBusy(true);
    setFailure(undefined);
    const result = await api.post<Save>(endpoint(path), { base_hash: hash, ...operation });
    if (result.kind === "ok") {
      const undo = operation.action === "delete" && removing ? removing : undefined;
      setRemoving(undefined);
      const latest = await refresh();
      if (undo) {
        const owner = getSession().session?.user?.name;
        const deletedHash = result.data.hash;
        const dismiss = toast(result.data.revision ? `Deleted in revision #${result.data.revision}.` : "Deleted.", {
          action: { label: "Undo", run: () => {
            dismiss();
            void (async () => {
              const session = await refreshSession();
              if (!owner || session.session?.user?.name !== owner) return;
              const restored = await api.post<Save>(endpoint(path), { base_hash: deletedHash, action: "add", item: undo.item });
              if (restored.kind !== "ok") { toast(restored.message, { kind: "bad" }); return; }
              saved(restored.data);
              await refresh();
            })();
          } },
        });
      } else saved(result.data);
      focus(operation.action === "delete" ? latest?.entries[0]?.id : operation.id);
    } else setFailure(result);
    setBusy(false);
  }
  function move(index: number, offset: number) {
    const order = entries.map((item) => item.id);
    const from = order[index];
    const to = order[index + offset];
    if (from === undefined || to === undefined) return;
    order[index] = to;
    order[index + offset] = from;
    void run({ action: "reorder", id: from, order });
  }
  const feedback = <FailureNote failure={failure} busy={busy} retry={() => { void refreshSession().then(() => refresh()); }} rebase={() => { void refresh().then(() => setFailure(undefined)); }} />;
  return <section className="settings-section" data-testid={`settings-${kind}-list`}>
    {!editorOnly && <>
      <header className="board-heading"><h2>{title}</h2>
        <Button ref={add} variant="primary" onClick={() => setSearch({ add: kind, edit: null })}>Add {kind}</Button>
      </header>
      {!removing && feedback}
      {entries.length === 0 && <p className="muted">No entries yet.</p>}
      <div className="settings-list">{entries.map((item, index) => {
        const name = String(item.item.name ?? item.item.title ?? item.item.pattern ?? item.id);
        return <div className="settings-row" key={item.id} tabIndex={-1} ref={(element) => {
          if (element) refs.current.set(item.id, element); else refs.current.delete(item.id);
        }}>
          <div><h3>{name}</h3>{row?.(item)}{item.locked && <p>{item.locked_reason ?? "Read-only"}</p>}</div>
          <div className="settings-actions">
            {item.enabled !== undefined && <Switch label="Enabled" checked={item.enabled} busy={busy} disabled={item.locked}
              onChange={(enabled) => { void run({ action: "edit", id: item.id, item: props.body?.({ enabled }, item.item) ?? { enabled } }); }} />}
            {reorder && <>
              <Button aria-label={`Move ${name} up`} disabled={busy || index === 0} onClick={() => move(index, -1)}>↑</Button>
              <Button aria-label={`Move ${name} down`} disabled={busy || index === entries.length - 1} onClick={() => move(index, 1)}>↓</Button>
            </>}
            {extra?.(item)}
            <Button aria-label={`Edit ${name}`} disabled={item.locked || busy} onClick={() => setSearch({ add: null, edit: `${kind}:${item.id}` })}>Edit</Button>
            <Button className="danger" aria-label={`Delete ${name}`} disabled={item.locked || busy} onClick={() => { setFailure(undefined); setRemoving(item); }}>Delete</Button>
          </div>
        </div>;
      })}</div>
    </>}
    {selected && (selected.id === null || entry) && <Edit key={selected.id ?? "new"} props={props} entry={entry} close={close} focus={focus} />}
    {selected && selected.id !== null && !entry && <Note kind="warn" action={<Button onClick={close}>Close</Button>}>This entry no longer exists.</Note>}
    <Dialog open={!!removing} title={`Delete ${kind}?`} onClose={() => { if (!busy) setRemoving(undefined); }}
      footer={<><Button onClick={() => setRemoving(undefined)} disabled={busy}>Cancel</Button>
        <Button className="danger" busy={busy} onClick={() => { if (removing) void run({ action: "delete", id: removing.id }); }}>Delete</Button></>}>
      <p>Delete {String(removing?.item.name ?? removing?.item.pattern ?? removing?.id ?? "")}?</p>
      {feedback}
    </Dialog>
  </section>;
}

function Edit({ props, entry, close, focus }: { props: Props; entry?: Entry; close: () => void; focus: (id?: Entry["id"]) => void }) {
  const { fields, hash, kind, path, refresh } = props;
  const [base, setBase] = useState(() => ({ entry, hash, values: initialFields(fields, entry?.item ?? {}) }));
  const [input, setInput] = useState(base.values);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<Failure>();
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState("");
  const [rename, setRename] = useState(false);
  const dirty = Object.keys(changedFields(base.values, input)).length > 0;
  useEffect(() => {
    if (!dirty) return;
    const before = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", before);
    return () => window.removeEventListener("beforeunload", before);
  }, [dirty]);
  function check() {
    const result = collectFields(fields, input);
    setErrors(result.errors);
    setFailure(undefined);
    if (Object.keys(result.errors).length) {
      document.getElementsByName(Object.keys(result.errors)[0] ?? "")[0]?.focus();
      return undefined;
    }
    return result.values;
  }
  async function save() {
    if (busy || base.entry?.locked) return;
    const values = check();
    if (!values) return;
    const changed = changedFields(base.values, input);
    const patch = Object.fromEntries(Object.keys(changed).map((key) => [key, values[key]]));
    const item = base.entry ? props.body?.(patch, values) ?? patch : props.create?.(values) ?? values;
    if (base.entry && !dirty) { close(); return; }
    const renamed = props.renameKey && patch[props.renameKey];
    if (base.entry && renamed && props.renameRules?.(base.entry.id).length && !rename) {
      setRename(true);
      return;
    }
    setBusy(true);
    const result = await api.post<Save>(endpoint(path), {
      base_hash: base.hash, action: base.entry ? "edit" : "add", id: base.entry?.id, item,
    });
    if (result.kind === "ok") {
      close();
      saved(result.data);
      const latest = await refresh();
      focus(renamed && typeof renamed === "string" ? renamed : base.entry?.id ?? latest?.entries.at(-1)?.id);
    } else {
      setFailure(result);
      if (result.kind === "invalid") setErrors(fieldErrors(result));
    }
    setBusy(false);
  }
  async function rebase() {
    const snapshot = await refresh();
    if (!snapshot) return;
    const previous = base.entry;
    const matches = previous && snapshot.entries.filter((item) => typeof previous.id === "number"
      ? JSON.stringify(item.item) === JSON.stringify(previous.item) : item.id === previous.id);
    const latest = matches && matches.length === 1 ? matches[0] : undefined;
    if (previous && (!latest || latest.locked)) {
      setErrors({ form: "This entry changed or disappeared. Copy your input before closing." });
      return;
    }
    const values = initialFields(fields, latest?.item ?? {});
    setInput(rebaseFields(base.values, input, values));
    setBase({ entry: latest, hash: snapshot.hash, values });
    setErrors({});
    setFailure(undefined);
    setRename(false);
  }
  return <Dialog open title={`${base.entry ? "Edit" : "Add"} ${kind}`} onClose={() => { if (!busy) close(); }} guard={() => dirty}
    testId="settings-edit" footer={<>
      {props.preview && !base.entry?.locked && <Button busy={busy} onClick={async () => {
        if (busy) return;
        const item = check();
        if (item && props.preview) {
          setBusy(true);
          setPreview(await props.preview(props.create?.(item) ?? item));
          setBusy(false);
        }
      }}>Fetch now</Button>}
      <Button type="submit" variant="primary" form="settings-edit" busy={busy} disabled={base.entry?.locked}>{rename ? "Rename and update rules" : "Save"}</Button>
    </>}>
    <form id="settings-edit" className="settings-form" noValidate onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <Controls fields={fields} input={input} base={base.entry?.item ?? {}} errors={errors} onChange={(values) => { setInput(values); setRename(false); }} />
      {rename && base.entry && <Note kind="warn">The rename also updates these rules: {props.renameRules?.(base.entry.id).join(", ")}</Note>}
      {errors.form && failure?.kind !== "invalid" && <Note kind="bad">{errors.form}</Note>}
      <FailureNote failure={failure} busy={busy} retry={() => { void refreshSession().then((session) => { if (session.session?.user) void save(); }); }} rebase={() => { void rebase(); }} />
      {preview && <Note kind="info">{preview}</Note>}
    </form>
  </Dialog>;
}
