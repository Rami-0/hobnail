import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { ApiResult } from "../../api/client.ts";
import { api } from "../../api/session.ts";
import { onConfigChange } from "../../live/index.ts";
import { Button } from "../../ui/Button.tsx";
import { Field, Textarea, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { SignInPrompt } from "../../ui/SignInPrompt.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { toast } from "../../ui/toast.ts";
import type { Values } from "./forms.ts";
import "./settings.css";

export const endpoint = (path: string) => "/api/settings/" + path;
export type Failure = Exclude<ApiResult<never>, { kind: "ok" }>;
export type Save = { hash: string; revision?: number | null; warnings?: string[]; unchanged?: boolean };

export function saved(result: Save) {
  toast(result.unchanged ? "Already up to date." : result.revision ? `Saved revision #${result.revision}.` : "Saved.");
}

export function useSettings<T>(path: string) {
  const [data, setData] = useState<T>();
  const [failure, setFailure] = useState<Failure>();
  const latest = useRef(0);
  const load = useCallback(async (signal?: AbortSignal) => {
    const request = ++latest.current;
    const result = await api.get<T>(endpoint(path), { signal });
    if (!signal?.aborted && request === latest.current) {
      if (result.kind === "ok") {
        setData(result.data);
        setFailure(undefined);
      } else setFailure(result);
    }
    return result;
  }, [path]);
  useEffect(() => {
    const controller = new AbortController();
    void Promise.resolve().then(() => load(controller.signal));
    const unlisten = onConfigChange(() => { void load(controller.signal); });
    return () => { controller.abort(); unlisten(); };
  }, [load]);
  return { data, failure, load };
}

export function FailureNote({ failure, retry, rebase, busy }: {
  failure?: Failure;
  retry?: () => void;
  rebase?: () => void;
  busy?: boolean;
}) {
  if (!failure || (failure.kind === "invalid" && failure.field)) return null;
  return <Note kind="bad" action={failure.kind === "sign_in" ? <SignInPrompt retry={retry} busy={busy} />
    : failure.kind === "stale" && rebase ? <Button busy={busy} onClick={rebase}>Reload and keep my input</Button> : undefined}>
    {failure.kind === "forbidden" && "Locked. "}{failure.message}
  </Note>;
}

export type Control = {
  key: string;
  label: string;
  hint?: string;
  placeholder?: string;
  required?: boolean;
  default?: unknown;
  max?: number;
  format?: "url" | "link" | "duration" | "number" | "list" | "paths" | "rows" | "text" | "switch" | "code";
  options?: readonly string[];
  show?: (values: Values) => boolean;
  locked?: (base: Values) => boolean;
  validate?: (value: string) => string | undefined;
};

type Pair = { key: string; label: string; value: string };

function pairs(input: unknown): Pair[] {
  try { return JSON.parse(String(input || "[]")) as Pair[]; } catch { return []; }
}

function displayValue(field: Control, value: unknown): string {
  if (value === null || value === undefined) return "";
  if (field.format === "list" && Array.isArray(value)) return value.join(", ");
  if (field.format === "paths" || field.format === "rows") {
    const entries = Array.isArray(value) ? value : Object.entries(value).map(([label, path]) => ({ label, value: path }));
    return JSON.stringify(entries.map(({ label, value: entry }, index) => ({ key: String(index), label, value: entry })));
  }
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}

function PairFields({ id, field, value, error, change }: {
  id: string; field: Control; value: string; error?: string; change: (value: string) => void;
}) {
  const entries = pairs(value);
  const update = (next: Pair[]) => change(next.length ? JSON.stringify(next) : "");
  const what = field.format === "paths" ? "field" : "row";
  const names = what === "field" ? ["Field name", "Path in the response"] : ["Label", "Value template"];
  return <fieldset className="settings-form">
    <legend>{field.label}</legend>
    {field.hint && <p className="muted">{field.hint}</p>}
    {entries.map((pair) => <div key={pair.key} className="settings-pair">
      {(["label", "value"] as const).map((key, index) => <Field key={key} id={`${id}-${pair.key}-${key}`} label={names[index]}>
        <TextInput value={pair[key]} onChange={(event) => {
          update(entries.map((entry) => entry.key === pair.key ? { ...entry, [key]: event.target.value } : entry));
        }} />
      </Field>)}
      <Button aria-label={`Remove ${what}${pair.label ? ` ${pair.label}` : ""}`}
        onClick={() => update(entries.filter((entry) => entry.key !== pair.key))}>Remove</Button>
    </div>)}
    {error && <Note kind="bad">{error}</Note>}
    <div><Button onClick={() => update([...entries, { key: String(Math.max(-1, ...entries.map((entry) => Number(entry.key))) + 1), label: "", value: "" }])}>Add {what}</Button></div>
  </fieldset>;
}

export function initialFields(fields: Control[], item: Values): Values {
  return Object.fromEntries(fields.map((field) => {
    const value = item[field.key] ?? field.default;
    return [field.key, field.format === "switch" ? Boolean(value) : displayValue(field, value)];
  }));
}

export function collectFields(fields: Control[], input: Values) {
  const errors: Record<string, string> = {};
  const values: Values = {};
  for (const field of fields) {
    const { key, format } = field;
    const entered = input[key];
    if (field.show && !field.show(input) && entered === "") { values[key] = null; continue; }
    if (typeof entered === "boolean") { values[key] = entered; continue; }
    const raw = String(entered ?? "");
    const text = format === "code" ? raw : raw.trim();
    if (!text) {
      if (field.required && (!field.show || field.show(input))) errors[key] = "This field is required.";
      values[key] = null;
      continue;
    }
    if (field.max && [...text].length > field.max) errors[key] = `Use at most ${field.max} characters.`;
    if (format === "url" || (format === "link" && !text.startsWith("/"))) {
      try {
        const url = new URL(text);
        if (!["http:", "https:"].includes(url.protocol)) errors[key] = "Enter an HTTP or HTTPS URL.";
      } catch { errors[key] = "Enter an HTTP or HTTPS URL."; }
    }
    const custom = field.validate?.(text);
    if (custom) errors[key] = custom;
    values[key] = text;
    if (format === "list") values[key] = text.split(",").map((entry) => entry.trim()).filter(Boolean);
    if (format === "duration") {
      if (/^\d+$/.test(text)) values[key] = Number(text);
      else if (!/^(?:(?:\d+(?:\.\d*)?|\.\d+)(?:ms|[smhd]))+$/.test(text)) errors[key] = "Enter a duration such as 30s or 5m.";
    }
    if (format === "number") {
      if (!Number.isFinite(Number(text))) errors[key] = "Enter a number.";
      else values[key] = Number(text);
    }
    if (format === "paths" || format === "rows") {
      const entries = pairs(text).filter((pair) => pair.label.trim() || pair.value.trim());
      if (entries.some((pair) => !pair.label.trim() || !pair.value.trim())) errors[key] = "Complete both values in each row.";
      else values[key] = !entries.length ? null : format === "rows"
        ? entries.map(({ label, value }) => ({ label: label.trim(), value: value.trim() }))
        : Object.fromEntries(entries.map(({ label, value }) => [label.trim(), value.trim()]));
      if (format === "paths" && new Set(entries.map((pair) => pair.label.trim())).size !== entries.length) {
        errors[key] = "Use each field name once.";
      }
    }
  }
  return { values, errors };
}

export function Controls({ fields, input, base, errors, onChange }: {
  fields: Control[];
  input: Values;
  base: Values;
  errors: Record<string, string>;
  onChange: (next: Values) => void;
}) {
  const id = useId();
  return fields.filter((field) => !field.show || field.show(input)).map((field) => {
    const { key, format, label } = field;
    const disabled = field.locked?.(base);
    const change = (value: string | boolean) => onChange({ ...input, [key]: value });
    const value = String(input[key] ?? "");
    if (format === "paths" || format === "rows") return <PairFields key={key} id={`${id}-${key}`} field={field} value={value} error={errors[key]} change={change} />;
    if (format === "switch") return <div key={key}>
      <Switch label={label} checked={input[key] === true} onChange={change} disabled={disabled} />
      {errors[key] && <Note kind="bad">{errors[key]}</Note>}
    </div>;
    return <Field key={key} id={`${id}-${key}`} label={label} error={errors[key]} hint={field.hint}>
      {field.options ? <Select label={label} value={value} onChange={change} disabled={disabled}
        options={field.options.map((option) => ({ value: option, label: option || "Default" }))} />
        : format === "code"
          ? <Textarea name={key} className="settings-code mono" rows={14}
              wrap="soft" spellCheck={false} value={value} onChange={(event) => change(event.target.value)} disabled={disabled} />
          : <TextInput name={key} value={value} placeholder={field.placeholder} required={field.required}
              inputMode={format === "number" ? "decimal" : format === "url" ? "url" : undefined}
              onChange={(event) => change(event.target.value)} disabled={disabled} />}
    </Field>;
  });
}
