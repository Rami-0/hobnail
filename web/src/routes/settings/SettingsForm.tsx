import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, editingSessionRequired, refreshEditingSession } from "../../api/session.ts";
import { navigate } from "../../app/router.ts";
import { Dialog } from "../../ui/Dialog.tsx";
import { Button } from "../../ui/Button.tsx";
import { Note } from "../../ui/Note.tsx";
import { changedFields, conflictingFields, fieldErrors, rebaseFields, type Values } from "./forms.ts";
import { collectFields, Controls, endpoint, FailureNote, initialFields, saved, type Control, type Failure, type Save } from "./shared.tsx";

type Snapshot = { hash: string; values: Values };

export function SettingsForm({ title, path, fields, snapshot, reload, encode, renderActions, renderDetails, onSaved, wide }: {
  title: string;
  path: string;
  fields: Control[];
  snapshot: Snapshot;
  reload: () => Promise<Snapshot | undefined>;
  encode?: (values: Values) => Values;
  renderActions?: (input: Values, busy: boolean, replace: (values: Values) => void) => ReactNode;
  renderDetails?: (input: Values, dirty: boolean) => ReactNode;
  onSaved?: (previous: Values) => void;
  wide?: boolean;
}) {
  const [draft, setDraft] = useState<{ base: Snapshot; input: Values }>();
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [warnings, setWarnings] = useState<string[]>([]);
  const [failure, setFailure] = useState<Failure>();
  const [busy, setBusy] = useState(false);
  const base = draft?.base ?? { hash: snapshot.hash, values: initialFields(fields, snapshot.values) };
  const input = draft?.input ?? base.values;
  const changed = changedFields(base.values, input);
  const dirty = Object.keys(changed).length > 0;
  const [destination, setDestination] = useState<string>();
  const leaving = useRef(false);
  useEffect(() => {
    if (!dirty) return;
    const navigation = (event: Event) => {
      if (leaving.current) return;
      event.preventDefault();
      setDestination((event as CustomEvent<string>).detail);
    };
    const unload = (event: BeforeUnloadEvent) => {
      if (leaving.current) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("rh:navigate", navigation);
    window.addEventListener("beforeunload", unload);
    return () => {
      window.removeEventListener("rh:navigate", navigation);
      window.removeEventListener("beforeunload", unload);
    };
  }, [dirty]);
  function discardAndLeave() {
    if (!destination || busy) return;
    leaving.current = true;
    setDraft(undefined);
    setErrors({});
    setFailure(undefined);
    setDestination(undefined);
    const external = new URL(destination).origin !== window.location.origin;
    navigate(destination);
    if (!external) leaving.current = false;
  }
  async function save() {
    if (busy || !dirty) return;
    const result = collectFields(fields, input);
    setErrors(result.errors);
    setFailure(undefined);
    if (Object.keys(result.errors).length) return;
    setBusy(true);
    const patch = Object.fromEntries(Object.keys(changed).map((key) => [key, result.values[key]]));
    const response = await api.post<Save>(endpoint(path), { base_hash: base.hash, ...(encode?.(patch) ?? { values: patch }) });
    if (response.kind === "ok") {
      await reload();
      onSaved?.(base.values);
      setDraft(undefined);
      setWarnings(response.data.warnings ?? []);
      saved(response.data);
    } else {
      setFailure(response);
      if (response.kind === "invalid") {
        const field = fields.find((control) => control.key === response.field)?.key;
        if (!field) setFailure({ ...response, field });
        const mapped = fieldErrors({ ...response, field });
        setErrors(mapped);
      }
    }
    setBusy(false);
  }
  async function rebase() {
    const fresh = await reload();
    if (!fresh) return;
    const values = initialFields(fields, fresh.values);
    if (conflictingFields(base.values, input, values).length) {
      setFailure({ kind: "error", status: 409, message: "A field you edited also changed on disk. Copy your input or discard it to load the latest values." });
      return;
    }
    setDraft({ base: { hash: fresh.hash, values }, input: rebaseFields(base.values, input, values) });
    setFailure(undefined);
    setErrors({});
  }
  return <section className="settings-section">
    <h2>{title}</h2>
    <form className={wide ? "settings-form settings-wide" : "settings-form"} noValidate onSubmit={(event) => { event.preventDefault(); void save(); }}>
      {renderDetails?.(input, dirty)}
      <Controls fields={fields} input={input} base={snapshot.values} errors={errors} onChange={(values) => setDraft({ base, input: values })} />
      <FailureNote failure={failure} busy={busy} retry={() => { void refreshEditingSession().then((session) => { if (session?.user) void save(); else setFailure({ kind: "sign_in", status: 401, message: editingSessionRequired }); }); }} rebase={() => { void rebase(); }} />
      {warnings.map((warning) => <Note key={warning} kind="warn">{warning}</Note>)}
      <div className="settings-actions">
        <Button type="submit" variant="primary" disabled={!dirty} busy={busy}>Save</Button>
        {dirty && <Button disabled={busy} onClick={() => { setDraft(undefined); setFailure(undefined); setErrors({}); }}>Discard changes</Button>}
        {renderActions?.(input, busy, (values) => setDraft({ base, input: values }))}
      </div>
    </form>
    <Dialog open={destination !== undefined} testId="settings-leave" title="Discard unsaved changes?"
      onClose={() => setDestination(undefined)}
      footer={<><Button onClick={() => setDestination(undefined)}>Keep editing</Button><Button disabled={busy} onClick={discardAndLeave}>Discard and leave</Button></>} />
  </section>;
}
