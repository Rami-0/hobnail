import { useCallback, useEffect, useId, useState } from "react";
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

export const endpoint = (path: string) => "/api/settings/" + path;
export type Failure = Exclude<ApiResult<never>, { kind: "ok" }>;
export type Save = { hash: string; revision?: number | null; warnings?: string[]; unchanged?: boolean };

export function saved(result: Save) {
  toast(result.unchanged ? "Already up to date." : result.revision ? `Saved revision #${result.revision}.` : "Saved.");
}

export function useSettings<T>(path: string) {
  const [data, setData] = useState<T>();
  const [failure, setFailure] = useState<Failure>();
  const load = useCallback(async (signal?: AbortSignal) => {
    const result = await api.get<T>(endpoint(path), { signal });
    if (!signal?.aborted) {
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
  format?: "url" | "link" | "number" | "list" | "map" | "json" | "text" | "switch" | "code";
  options?: readonly string[];
  show?: (values: Values) => boolean;
  locked?: (base: Values) => boolean;
  validate?: (value: string) => string | undefined;
};

export function displayValue(field: Control, value: unknown): string {
  if (value === null || value === undefined) return "";
  if (field.format === "list" && Array.isArray(value)) return value.join(", ");
  if (field.format === "map" && typeof value === "object") return Object.entries(value).map(([key, entry]) => `${key}=${entry}`).join("\n");
  if (field.format === "json" || typeof value === "object") return JSON.stringify(value, null, 2);
  return String(value);
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
    const value = input[key];
    if (typeof value === "boolean") { values[key] = value; continue; }
    const raw = String(value ?? "");
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
    if (format === "number") {
      if (!Number.isFinite(Number(text))) errors[key] = "Enter a number.";
      else values[key] = Number(text);
    }
    if (format === "json") {
      try { values[key] = JSON.parse(text) as unknown; }
      catch { errors[key] = "Enter valid JSON."; }
    }
    if (format === "map") {
      const entries = text.split("\n").filter((line) => line.trim()).map((line) => {
        const split = line.indexOf("=");
        return split < 1 ? [] : [line.slice(0, split).trim(), line.slice(split + 1).trim()];
      });
      if (entries.some((entry) => !entry[0] || !entry[1])) errors[key] = "Use one name=value pair per line.";
      else values[key] = Object.fromEntries(entries);
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
    if (format === "switch") return <div key={key}>
      <Switch label={label} checked={input[key] === true} onChange={change} disabled={disabled} />
      {errors[key] && <Note kind="bad">{errors[key]}</Note>}
    </div>;
    return <Field key={key} id={`${id}-${key}`} label={label} error={errors[key]} hint={field.hint}>
      {field.options ? <Select label={label} value={value} onChange={change} disabled={disabled}
        options={field.options.map((option) => ({ value: option, label: option || "Default" }))} />
        : format === "code" || format === "map" || format === "json"
          ? <Textarea name={key} className="settings-code mono" rows={format === "code" ? 14 : 3}
              wrap="soft" spellCheck={false} value={value} onChange={(event) => change(event.target.value)} disabled={disabled} />
          : <TextInput name={key} value={value} placeholder={field.placeholder} required={field.required}
              inputMode={format === "number" ? "decimal" : format === "url" ? "url" : undefined}
              onChange={(event) => change(event.target.value)} disabled={disabled} />}
    </Field>;
  });
}
