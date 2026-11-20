import { useState } from "react";
import { api } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { toast } from "../../ui/toast.ts";
import { Note } from "../../ui/Note.tsx";
import { Collection, type Entry } from "./Collection.tsx";
import type { Values } from "./forms.ts";
import { endpoint, FailureNote, useSettings, type Control } from "./shared.tsx";

type Channel = Entry & { secrets: { field: string; kind: "plain" | "env" | "file"; name?: string; path?: string; set?: boolean; exists?: boolean }[]; warnings: string[] };
type Data = { hash: string; channels: Channel[]; rules: Entry[] };
const envName = (value: string) => /^[A-Z_][A-Z0-9_]*$/.test(value) ? undefined : "Enter an environment variable name.";
const named = (value: string) => /^[\w-]{1,64}$/.test(value) ? undefined : "Use 1–64 letters, digits, hyphens or underscores.";
const sourceKeys = ["url_env", "url_file", "url", "webhook_url_env", "webhook_url_file", "token_env", "token_file"];
const bound = (item: Values) => item.bound === true;
const activeSource = (item: Values) => {
  if (item.type === "slack") return item.source === "file" ? "webhook_url_file" : "webhook_url_env";
  if (item.type === "telegram") return item.source === "file" ? "token_file" : "token_env";
  return item.source === "plain" ? "url" : item.source === "file" ? "url_file" : "url_env";
};
const channelFields: Control[] = [
  { key: "name", label: "Name", required: true, validate: named },
  { key: "type", label: "Type", default: "webhook", options: ["webhook", "slack", "telegram"], locked: bound },
  { key: "source", label: "Secret source", default: "env", options: (input) => input.type === "webhook" ? ["env", "file", "plain"] : ["env", "file"], locked: bound,
    validate: (value) => ["env", "file", "plain"].includes(value) ? undefined : "Select a secret source." },
  ...sourceKeys.map((key): Control => ({
    key, label: key.endsWith("_env") ? "Environment variable name" : key.endsWith("_file") ? "Secret file path" : "Plain URL",
    show: (input) => activeSource(input) === key, required: key !== "url", locked: bound,
    format: key === "url" ? "url" : undefined, validate: key.endsWith("_env") ? envName : undefined,
    hint: key === "url" ? "Stored as plain text. Use a variable or file for credentials. Leave blank to keep an existing URL." : undefined,
  })),
  { key: "chat_id", label: "Chat ID", show: (input) => input.type === "telegram", locked: (input) => !!input.chat_id_env },
  { key: "timeout", format: "duration", label: "Timeout", placeholder: "10s" },
];

function channelBody(patch: Values, input: Values, original?: Values) {
  const { source, ...changes } = patch;
  const key = activeSource(input);
  if (source !== undefined || changes.type !== undefined) {
    for (const candidate of sourceKeys) if (candidate !== key) changes[candidate] = null;
    if (input[key]) changes[key] = input[key];
  }
  if (original && input.type !== original.type) {
    for (const obsolete of ["chat_id_env", "api_base", "headers", "headers_env", "body"]) changes[obsolete] = null;
    if (input.type !== "telegram") changes.chat_id = null;
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
  { key: "on", label: "Fire on", format: "choices", show: ofType("container"),
    options: ["stopped", "exit_nonzero", "unhealthy", "restarting"], default: ["stopped", "unhealthy", "restarting"] },
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
  { key: "channels", label: "Channels", format: "choices" },
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
    const { item } = entry;
    const plain = (key: string) => entry.secrets.some(({ field, kind }) => field === key && kind === "plain");
    const source = sourceKeys.some((key) => key.endsWith("_file") && item[key]) ? "file" : plain("url") ? "plain" : "env";
    const boundDestination = item.type === "telegram" && plain("api_base")
      || item.type === "webhook" && entry.secrets.some(({ field }) => field.startsWith("headers_") || field === "headers");
    return { ...entry, item: { ...item, name: entry.id, source, bound: boundDestination } };
  });
}

function rules(data: Data): Entry[] {
  return data.rules.map((entry) => ({ ...entry, enabled: entry.item.enabled !== false }));
}

function ChannelDetails({ channel }: { channel: Channel }) {
  return <>
    <p>{String(channel.item.type)}</p>
    {bound(channel.item) && <p>Secret references and their destination are set on the host.</p>}
    {channel.secrets.map((secret) => <p key={secret.field}><code>{secret.name ?? secret.path ?? secret.field}</code> · {secret.set || secret.exists ? "Available" : "Missing"}</p>)}
    {channel.warnings.map((warning) => <p key={warning}>{warning}</p>)}
  </>;
}

async function testDelivery(name: string) {
  const result = await api.post<{ ok: boolean; error?: string }>(endpoint("channels/test"), { name });
  if (result.kind !== "ok") return { ok: false, message: result.message };
  return { ok: result.data.ok, message: result.data.error ?? "Test sent." };
}

const saveAndTest = {
  label: "Save and send test",
  async run(item: Values) {
    const result = await testDelivery(String(item.name));
    toast(result.message, { kind: result.ok ? "ok" : "bad" });
  },
};

function SendTest({ name }: { name: string }) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ ok: boolean; message: string }>();
  return <div><Button busy={busy} onClick={async () => {
    if (busy) return;
    setBusy(true);
    setFeedback(await testDelivery(name));
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
      <Collection secondaryAction kind="channel" title="Notification channels" path="channels" fields={channelFields} hash={data.hash}
        entries={channels(data)} editorOnly={editorOnly} renameKey="name" body={channelBody} create={newChannel} afterSave={saveAndTest}
        renameRules={(id) => data.rules.filter((rule) => Array.isArray(rule.item.channels) && rule.item.channels.includes(id)).map((rule) => String(rule.id))}
        row={channelDetails}
        extra={sendTest} refresh={refresh(channels)} />
      <Collection kind="rule" title="Alert rules" path="alerts" fields={ruleFields.map((field) => field.key === "channels" ? { ...field, options: data.channels.map((channel) => String(channel.id)) } : field)}
        hash={data.hash} entries={rules(data)}
        editorOnly={editorOnly} reorder body={ruleBody} create={newRule} validate={validateRule} renameKey="name" row={ruleRow} refresh={refresh(rules)} />
    </>}
  </>;
}

function channelDetails(entry: Entry) { return <ChannelDetails channel={entry as Channel} />; }
function sendTest(entry: Entry) { return <SendTest name={String(entry.id)} />; }
function comparisonValue(value: unknown) {
  if (typeof value !== "string") return value;
  try { return JSON.parse(value) as unknown; } catch { return value; }
}
function ruleBody(changes: Values, input: Values, original?: Values) {
  const result: Values = { name: input.name, type: input.type, ...changes };
  if (original) for (const field of ruleFields) {
    if (field.show && !field.show(input) && original[field.key] !== undefined) result[field.key] = null;
  }
  for (const key of ["equals", "not_equals"]) if (key in result) result[key] = comparisonValue(result[key]);
  return result;
}
function validateRule(input: Values): Record<string, string> {
  const errors: Record<string, string> = {};
  const count = (keys: string[]) => keys.filter((key) => input[key] !== null && input[key] !== undefined).length;
  if (input.type === "host" && count(["above", "below"]) !== 1) errors.above = "Set exactly one of Above and Below.";
  if (input.type === "widget" && count(["above", "below", "equals", "not_equals"]) !== 1) {
    errors.above = "Set exactly one of Above, Below, Equals and Does not equal.";
  }
  if (input.type === "container" && !input.on) errors.on = "Pick at least one condition.";
  if (input.type === "log_match" && !input.containers && !input.stack) errors.containers = "Set Containers, Compose stack, or both.";
  return errors;
}
function newRule(input: Values) {
  return ruleBody(Object.fromEntries(ruleFields.filter((field) => !field.show || field.show(input)).map((field) => [field.key, input[field.key]])), input);
}
