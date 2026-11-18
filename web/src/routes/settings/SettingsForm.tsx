import { useState, type ReactNode } from "react";
import { api, refreshEditingSession } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Note } from "../../ui/Note.tsx";
import { changedFields, fieldErrors, rebaseFields, type Values } from "./forms.ts";
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
        const mapped = fieldErrors(response);
        setErrors(mapped);
      }
    }
    setBusy(false);
  }
  async function rebase() {
    const fresh = await reload();
    if (!fresh) return;
    const values = initialFields(fields, fresh.values);
    setDraft({ base: { hash: fresh.hash, values }, input: rebaseFields(base.values, input, values) });
    setFailure(undefined);
    setErrors({});
  }
  return <section className="settings-section">
    <h2>{title}</h2>
    <form className={wide ? "settings-form settings-wide" : "settings-form"} noValidate onSubmit={(event) => { event.preventDefault(); void save(); }}>
      {renderDetails?.(input, dirty)}
      <Controls fields={fields} input={input} base={snapshot.values} errors={errors} onChange={(values) => setDraft({ base, input: values })} />
      <FailureNote failure={failure} busy={busy} retry={() => { void refreshEditingSession().then((session) => { if (session?.user) void save(); }); }} rebase={() => { void rebase(); }} />
      {warnings.map((warning) => <Note key={warning} kind="warn">{warning}</Note>)}
      <div className="settings-actions">
        <Button type="submit" variant="primary" disabled={!dirty} busy={busy}>Save</Button>
        {dirty && <Button disabled={busy} onClick={() => { setDraft(undefined); setFailure(undefined); setErrors({}); }}>Discard changes</Button>}
        {renderActions?.(input, busy, (values) => setDraft({ base, input: values }))}
      </div>
    </form>
  </section>;
}
