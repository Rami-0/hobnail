import { useState } from "react";
import { api } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { toast } from "../../ui/toast.ts";
import type { Values } from "./forms.ts";
import { SettingsForm } from "./SettingsForm.tsx";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

const fields: Control[] = [{ key: "sample_interval", format: "duration", label: "Sample interval", placeholder: "15s", hint: "5 seconds to 5 minutes. For example, 30s." }];

export default function HistorySettings() {
  const { data, load, failure } = useSettings<{ hash: string; history: Values | null; db: {
    enabled: boolean; size_bytes: number; sys_rows: number; ctr_rows: number; event_rows: number; revisions: number; oldest_ts: number | null;
  } }>("history");
  const [confirm, setConfirm] = useState(false);
  const [phrase, setPhrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<Failure>();
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <>
      <Switch label="Record host and container samples" checked={data.history?.enabled !== false} busy={busy}
        onChange={async (enabled) => {
          setBusy(true);
          const result = await api.post<Save>(endpoint("history"), { base_hash: data.hash, values: { enabled } });
          if (result.kind === "ok") { setProblem(undefined); saved(result.data); await load(); }
          else setProblem(result);
          setBusy(false);
        }} />
      {!confirm && <FailureNote failure={problem} rebase={() => { void load().then(() => setProblem(undefined)); }} />}
      <SettingsForm title="Recording" path="history" fields={fields}
        snapshot={{ hash: data.hash, values: data.history ?? {} }} reload={async () => {
          const result = await load();
          if (result.kind === "ok") return { hash: result.data.hash, values: result.data.history ?? {} };
        }} />
      <section className="settings-section"><h2>Database</h2>
        {data.db.enabled ? <dl>
          <dt>Size</dt><dd>{data.db.size_bytes.toLocaleString()} bytes</dd>
          <dt>Host samples</dt><dd>{data.db.sys_rows.toLocaleString()}</dd>
          <dt>Container samples</dt><dd>{data.db.ctr_rows.toLocaleString()}</dd>
          <dt>Events</dt><dd>{data.db.event_rows.toLocaleString()}</dd>
          <dt>Config revisions</dt><dd>{data.db.revisions.toLocaleString()}</dd>
          <dt>Oldest record</dt><dd>{data.db.oldest_ts ? new Date(data.db.oldest_ts * 1000).toLocaleString() : "None"}</dd>
        </dl> : <p>The history database is unavailable.</p>}
        <div className="settings-actions">
          <a className="btn" href={endpoint("history/export")} download>Export snapshot (JSON)</a>
          <Button variant="ghost" onClick={() => { setPhrase(""); setProblem(undefined); setConfirm(true); }}>Clear history…</Button>
        </div>
      </section>
    </>}
    <Dialog open={confirm} title="Clear history?" onClose={() => { if (!busy) setConfirm(false); }}
      footer={<Button variant="destructive" disabled={phrase !== "clear history"} busy={busy} onClick={async () => {
        if (busy || phrase !== "clear history") return;
        setBusy(true);
        const result = await api.post(endpoint("history/clear"));
        if (result.kind === "ok") { setConfirm(false); toast("History cleared."); await load(); }
        else setProblem(result);
        setBusy(false);
      }}>Clear history</Button>}>
      <p>This permanently deletes recorded samples and events. Config revisions and accounts are kept.</p>
      <Field id="clear-history-phrase" label='Type “clear history” to continue'><TextInput autoComplete="off" value={phrase} onChange={(event) => setPhrase(event.target.value)} /></Field>
      <FailureNote failure={problem} />
    </Dialog>
  </>;
}
