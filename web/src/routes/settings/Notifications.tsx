import { useState } from "react";
import { api } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Note } from "../../ui/Note.tsx";
import { Collection, type Entry } from "./Collection.tsx";
import type { Values } from "./forms.ts";
import { endpoint, FailureNote, useSettings, type Control } from "./shared.tsx";

type Channel = Entry & { secrets: { field: string; name?: string; path?: string; set?: boolean; exists?: boolean }[]; warnings: string[] };
type Data = { hash: string; channels: Channel[]; rules: Entry[] };
const envName = (value: string) => /^[A-Z_][A-Z0-9_]*$/.test(value) ? undefined : "Enter an environment variable name.";
const named = (value: string) => /^[\w-]{1,64}$/.test(value) ? undefined : "Use 1–64 letters, digits, hyphens or underscores.";
const sourceKeys = ["url_env", "url_file", "url", "webhook_url_env", "webhook_url_file", "token_env", "token_file"];
const activeSource = (item: Values) => {
  if (item.type === "slack") return item.source === "file" ? "webhook_url_file" : "webhook_url_env";
  if (item.type === "telegram") return item.source === "file" ? "token_file" : "token_env";
  return item.source === "plain" ? "url" : item.source === "file" ? "url_file" : "url_env";
};
const channelFields: Control[] = [
  { key: "name", label: "Name", required: true, validate: named },
  { key: "type", label: "Type", default: "webhook", options: ["webhook", "slack", "telegram"] },
  { key: "source", label: "Secret source", default: "env", options: ["env", "file", "plain"],
    validate: (value) => ["env", "file", "plain"].includes(value) ? undefined : "Select a secret source." },
  ...sourceKeys.map((key): Control => ({
    key, label: key.endsWith("_env") ? "Environment variable name" : key.endsWith("_file") ? "Secret file path" : "Plain URL",
    show: (input) => activeSource(input) === key, required: key !== "url",
    format: key === "url" ? "url" : undefined, validate: key.endsWith("_env") ? envName : undefined,
    hint: key === "url" ? "Stored as plain text. Use a variable or file for credentials. Leave blank to keep an existing URL." : undefined,
  })),
  { key: "chat_id", label: "Chat ID", show: (input) => input.type === "telegram", locked: (input) => !!input.chat_id_env },
  { key: "timeout", format: "duration", label: "Timeout", placeholder: "10s" },
];

function channelBody(patch: Values, input: Values) {
  const { source, ...changes } = patch;
  const key = activeSource(input);
  if (source !== undefined || changes.type !== undefined) {
    for (const candidate of sourceKeys) if (candidate !== key) changes[candidate] = null;
    if (input[key]) changes[key] = input[key];
  }
  if (changes.url === null && key === "url") delete changes.url;
  return { name: input.name, type: input.type, ...changes };
}

function newChannel(input: Values) {
  const key = activeSource(input);
  return { name: input.name, type: input.type, [key]: input[key], timeout: input.timeout,
    ...(input.type === "telegram" ? { chat_id: input.chat_id } : {}) };
}

const ofType = (...types: string[]) => (input: Values) => types.includes(String(input.type));
const ruleFields: Control[] = [
  { key: "name", label: "Name", required: true, max: 80 },
  { key: "type", label: "Type", default: "container", options: ["container", "log_match", "http", "host", "widget"] },
  { key: "enabled", label: "Enabled", format: "switch", default: true },
  { key: "containers", label: "Containers", format: "list", show: ofType("container", "log_match") },
  { key: "exclude", label: "Except", format: "list", show: ofType("container", "log_match") },
  { key: "on", label: "Fire on", format: "list", show: ofType("container"), hint: "stopped, exit_nonzero, unhealthy, restarting" },
  { key: "restarts", label: "Restarts", format: "number", show: ofType("container") },
  { key: "window", format: "duration", label: "Window", show: ofType("container", "log_match") },
  { key: "pattern", label: "Pattern", required: true, show: ofType("log_match") },
  { key: "stack", label: "Compose stack", show: ofType("log_match") },
  { key: "count", label: "Matching lines", format: "number", show: ofType("log_match") },
  { key: "url", label: "URL", format: "url", required: true, show: ofType("http") },
  { key: "expect_status", label: "Expected status", format: "number", show: ofType("http") },
  { key: "contains", label: "Body contains", show: ofType("http") },
  { key: "failures", label: "Failures in a row", format: "number", show: ofType("http") },
  { key: "timeout", format: "duration", label: "Timeout", show: ofType("http") },
  { key: "interval", format: "duration", label: "Check every", show: ofType("http", "host") },
  { key: "metric", label: "Metric", options: ["disk", "memory", "cpu", "temp"], default: "disk", show: ofType("host") },
  { key: "disk", label: "Disk label or glob", show: (input) => input.type === "host" && input.metric === "disk" },
  { key: "widget", label: "Widget ID", required: true, show: ofType("widget") },
  { key: "field", label: "Widget field", required: true, show: ofType("widget") },
  { key: "above", label: "Above", format: "number", show: ofType("host", "widget") },
  { key: "below", label: "Below", format: "number", show: ofType("host", "widget") },
  { key: "equals", label: "Equals", show: ofType("widget") },
  { key: "not_equals", label: "Does not equal", show: ofType("widget") },
  { key: "on_error", label: "Fire when the widget fails", format: "switch", show: ofType("widget") },
  { key: "channels", label: "Channels", format: "list", hint: "Separate channel names with commas." },
  { key: "for", format: "duration", label: "For" },
  { key: "cooldown", format: "duration", label: "Cooldown" },
  { key: "message", label: "Message" },
  { key: "recovery", label: "Recovery message" },
];

function ruleRow(entry: Entry) {
  return <p className="muted">{String(entry.item.type)}{Array.isArray(entry.item.channels) && ` · ${entry.item.channels.join(", ")}`}</p>;
}

function channels(data: Data): Entry[] {
  return data.channels.map((entry) => {
    const { url, headers: _headers, body: _body, api_base: _apiBase, ...safe } = entry.item;
    const source = sourceKeys.some((key) => key.endsWith("_file") && safe[key]) ? "file" : url ? "plain" : "env";
    return { ...entry, original: { ...entry.item, name: entry.id }, item: { ...safe, name: entry.id, source } };
  });
}

function rules(data: Data): Entry[] {
  return data.rules.map((entry) => ({ ...entry, enabled: entry.item.enabled !== false }));
}

function ChannelDetails({ channel }: { channel: Channel }) {
  return <>
    <p>{String(channel.item.type)}</p>
    {channel.secrets.map((secret) => <p key={secret.field}><code>{secret.name ?? secret.path}</code> · {secret.set || secret.exists ? "Available" : "Missing"}</p>)}
    {channel.warnings.map((warning) => <p key={warning}>{warning}</p>)}
  </>;
}

function SendTest({ name }: { name: string }) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ ok: boolean; message: string }>();
  return <div><Button busy={busy} onClick={async () => {
    if (busy) return;
    setBusy(true);
    const result = await api.post<{ ok: boolean; error?: string }>(endpoint("channels/test"), { name });
    setFeedback(result.kind === "ok" ? { ok: result.data.ok, message: result.data.error ?? "Test sent." } : { ok: false, message: result.message });
    setBusy(false);
  }}>Send test</Button>{feedback && <Note kind={feedback.ok ? "ok" : "bad"}>{feedback.message}</Note>}</div>;
}

export default function Notifications({ editorOnly }: { editorOnly?: boolean }) {
  const { data, failure, load } = useSettings<Data>("notifications");
  const refresh = (get: (data: Data) => Entry[]) => async () => {
    const result = await load();
    if (result.kind === "ok") return { hash: result.data.hash, entries: get(result.data) };
  };
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <>
      <Collection kind="channel" title="Notification channels" path="channels" fields={channelFields} hash={data.hash}
        entries={channels(data)} editorOnly={editorOnly} renameKey="name" body={channelBody} create={newChannel}
        renameRules={(id) => data.rules.filter((rule) => Array.isArray(rule.item.channels) && rule.item.channels.includes(id)).map((rule) => String(rule.id))}
        row={channelDetails}
        extra={sendTest} refresh={refresh(channels)} />
      <Collection kind="rule" title="Alert rules" path="alerts" fields={ruleFields} hash={data.hash} entries={rules(data)}
        editorOnly={editorOnly} reorder body={ruleBody} create={newRule} row={ruleRow} refresh={refresh(rules)} />
    </>}
  </>;
}

function channelDetails(entry: Entry) { return <ChannelDetails channel={entry as Channel} />; }
function sendTest(entry: Entry) { return <SendTest name={String(entry.id)} />; }
function ruleBody(changes: Values, input: Values) { return { name: input.name, type: input.type, ...changes }; }
function newRule(input: Values) {
  return Object.fromEntries(ruleFields.filter((field) => !field.show || field.show(input)).map((field) => [field.key, input[field.key]]));
}
