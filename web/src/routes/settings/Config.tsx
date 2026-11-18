import { useState } from "react";
import { api } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Note } from "../../ui/Note.tsx";
import { SettingsForm } from "./SettingsForm.tsx";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

type Report = { ok: boolean; warnings?: string[]; error?: string; line?: number; col?: number; locked?: boolean };
type Raw = { hash: string; text: string; path: string; exists: boolean; bytes: number; valid: Report };
type Revision = { id: number; ts: number; author: string; note: string; bytes: number };
type Diff = { added: number; removed: number; approximate: boolean; hunks: { old_start: number; new_start: number; lines: { op: string; text: string }[] }[] };
type View = { action: "view" | "diff" | "restore"; revision: Revision; hash: string; text?: string; diff?: Diff };

const fields: Control[] = [{ key: "text", label: "Config file (TOML)", format: "code" }];

function ReportNote({ report }: { report?: Report }) {
  if (!report) return null;
  return <>
    <Note kind={report.ok ? "ok" : "bad"}>{report.ok ? "Configuration is valid." : <>
      {report.locked && "Locked. "}{report.error}{report.line && ` (line ${report.line}, column ${report.col ?? 1})`}
    </>}</Note>
    {report.warnings?.map((warning) => <Note key={warning} kind="warn">{warning}</Note>)}
  </>;
}

export default function Config() {
  const raw = useSettings<Raw>("config");
  const revisions = useSettings<{ enabled: boolean; revisions: Revision[] }>("revisions");
  const [report, setReport] = useState<Report>();
  const [validationFailure, setValidationFailure] = useState<Failure>();
  const [viewFailure, setViewFailure] = useState<Failure>();
  const [validating, setValidating] = useState(false);
  const [working, setWorking] = useState(false);
  const [view, setView] = useState<View>();
  const [count, setCount] = useState(20);
  async function open(action: View["action"], revision: Revision) {
    if (!raw.data || working) return;
    const opened = { action, revision, hash: raw.data.hash };
    setView(opened);
    setViewFailure(undefined);
    if (action === "restore") return;
    setWorking(true);
    const result = await api.get<{ text: string } | Diff>(endpoint(`revision/${revision.id}${action === "diff" ? "/diff" : ""}`));
    if (result.kind === "ok") setView({ ...opened, ...("text" in result.data ? { text: result.data.text } : { diff: result.data }) });
    else setViewFailure(result);
    setWorking(false);
  }
  return <>
    <FailureNote failure={raw.failure} retry={() => { void raw.load(); }} />
    {raw.data && <>
      <p className="muted settings-path"><code>{raw.data.path}</code> · {raw.data.exists ? `${raw.data.bytes.toLocaleString()} bytes` : "No file yet"}</p>
      <SettingsForm path="config" title="Raw editor" fields={fields} wide snapshot={{ hash: raw.data.hash, values: { text: raw.data.text } }}
        encode={(values) => ({ text: values.text ?? "" })}
        reload={async () => {
          const result = await raw.load();
          if (result.kind === "ok") {
            setReport(undefined);
            await revisions.load();
            return { hash: result.data.hash, values: { text: result.data.text } };
          }
        }}
        renderActions={(input, busy) => <Button disabled={busy} busy={validating} onClick={async () => {
          if (validating) return;
          setValidating(true);
          const result = await api.post<Report>(endpoint("config/validate"), { text: input.text });
          if (result.kind === "ok") { setValidationFailure(undefined); setReport(result.data); }
          else { setReport(undefined); setValidationFailure(result); }
          setValidating(false);
        }}>Validate</Button>} />
      <FailureNote failure={validationFailure} />
      <ReportNote report={report ?? (!raw.data.valid.ok ? raw.data.valid : undefined)} />
    </>}
    <section className="settings-section"><h2>Revisions</h2>
      <FailureNote failure={revisions.failure} />
      {revisions.data && !revisions.data.enabled && <p>The revision database is unavailable.</p>}
      {revisions.data?.enabled && !revisions.data.revisions.length && <p>No revisions yet.</p>}
      <div className="settings-list">{revisions.data?.revisions.slice(0, count).map((revision) => <div key={revision.id} className="settings-row">
        <div><strong>#{revision.id} · {revision.note}</strong><p className="muted">{revision.author} · {new Date(revision.ts * 1000).toLocaleString()}</p></div>
        <div className="settings-actions">
          <Button disabled={!raw.data || working} onClick={() => { void open("view", revision); }}>View</Button>
          <Button disabled={!raw.data || working} onClick={() => { void open("diff", revision); }}>Diff</Button>
          <Button disabled={!raw.data || working} onClick={() => { void open("restore", revision); }}>Restore…</Button>
        </div>
      </div>)}</div>
      {(revisions.data?.revisions.length ?? 0) > count && <div><Button onClick={() => setCount((value) => value + 20)}>Show more revisions</Button></div>}
    </section>
    <Dialog open={!!view} wide title={view ? `${view.action === "restore" ? "Restore" : view.action === "diff" ? "Diff for" : "View"} revision #${view.revision.id}` : ""}
      onClose={() => { if (!working) setView(undefined); }}
      footer={view?.action === "restore" && <Button className="danger" busy={working} onClick={async () => {
        if (working) return;
        setWorking(true);
        const result = await api.post<Save>(endpoint(`revision/${view.revision.id}/restore`), { base_hash: view.hash });
        if (result.kind === "ok") {
          setView(undefined);
          saved(result.data);
          await Promise.all([raw.load(), revisions.load()]);
        } else setViewFailure(result);
        setWorking(false);
      }}>Restore revision</Button>}>
      {working && <p role="status">Working…</p>}
      {view?.action === "restore" && <p>Replace the current config with this revision? The current version remains in revision history.</p>}
      <FailureNote failure={viewFailure} rebase={() => {
        void raw.load().then((result) => {
          if (view && result.kind === "ok") {
            setView({ ...view, hash: result.data.hash });
            setViewFailure(undefined);
          }
        });
      }} />
      {view?.text !== undefined && <pre className="settings-code">{view.text}</pre>}
      {view?.diff && <>
        <p>{view.diff.added} added · {view.diff.removed} removed{view.diff.approximate && " · Approximate diff"}</p>
        {!view.diff.hunks.length && <p>No differences.</p>}
        {view.diff.hunks.map((hunk) => <pre className="settings-code" key={`${hunk.old_start}:${hunk.new_start}`}>
          {`@@ -${hunk.old_start} +${hunk.new_start} @@\n`}{hunk.lines.map((line) => `${line.op} ${line.text}\n`).join("")}
        </pre>)}
      </>}
    </Dialog>
  </>;
}
