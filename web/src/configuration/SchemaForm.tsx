import type { Field as Spec } from "../api/gen/Field.ts";
import { Button } from "../ui/Button.tsx";
import { Field, Textarea, TextInput } from "../ui/Field.tsx";
import { Select } from "../ui/Select.tsx";
import { Switch } from "../ui/Switch.tsx";
import { origin } from "../boards/document.ts";
import { secretHint, type Values } from "./schema.ts";

export const pinField: Spec = {
  id: "fingerprint", label: "TLS certificate fingerprint", kind: "text", required: false,
  help: "Optional SHA-256 leaf fingerprint, 64 hexadecimal digits.", max_length: 64,
  min: null, max: null, choices: [], default: null,
};

export function SchemaForm({ id, schema, values, errors, onChange, bound, cleared, onClear }: {
  id: string; schema: Spec[]; values: Values; errors?: Record<string, string>;
  onChange: (id: string, value: string | boolean) => void;
  bound?: Record<string, string>; cleared?: Record<string, boolean>; onClear?: (key: string) => void;
}) {
  return <>{schema.map((field) => {
    const { kind, label, help } = field;
    const key = field.id;
    const name = `${id}-${key === "base_url" ? "url" : key}`;
    const value = values[key] ?? field.default;
    const text = Array.isArray(value) ? value.join("\n") : String(value ?? "");
    const change = (next: string | boolean) => onChange(key, next);
    const secret = kind === "secret";
    const url = String(values.base_url ?? "");
    const required = field.required && (!secret || !bound?.[key] || cleared?.[key] || origin(url) !== bound[key]);
    const hint = (secret && secretHint(bound?.[key], url, cleared?.[key])) || help;
    return <div key={key}>
      <Field id={name} label={kind === "bool" ? null : label} error={errors?.[key]} hint={hint}>
        {kind === "bool" ? <Switch label={label} aria-required={required || undefined} checked={value === true || value === "true"} onChange={change} />
          : kind === "select" ? <Select label={label} required={required} options={field.choices.map((option) => ({ value: option, label: option }))} value={text} onChange={change} />
          : kind === "list" || kind === "multiline" ? <Textarea name={key} required={required} rows={kind === "list" ? 3 : 8}
            maxLength={kind === "multiline" ? field.max_length ?? undefined : undefined} value={text} onChange={(e) => change(e.target.value)} />
          : <TextInput name={key} required={required} type={secret ? "password" : kind === "url" || kind === "number" ? kind : "text"}
            autoComplete={secret ? "new-password" : undefined} maxLength={field.max_length ?? undefined}
            min={field.min ?? undefined} max={field.max ?? undefined} value={text} onChange={(e) => change(e.target.value)} />}
      </Field>
      {secret && text && url.toLowerCase().startsWith("http://") && <p className="warn-fg">This HTTP connection sends the credential unencrypted.</p>}
      {secret && bound?.[key] && onClear && <Button onClick={() => onClear(key)}>{cleared?.[key] ? "Keep saved credential" : "Clear saved credential"}</Button>}
    </div>;
  })}</>;
}
