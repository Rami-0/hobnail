import { Field, TextInput } from "../ui/Field.tsx";

export function LinkUrl({ id, label = "URL", value, optional, error, onChange }: {
  id: string; label?: string; value?: string; optional?: boolean; error?: string; onChange?: (value: string) => void;
}) {
  return <Field id={id} label={label} error={error}><TextInput name="url" type="url" required={!optional}
    maxLength={2048} pattern="[Hh][Tt][Tt][Pp][Ss]?://.*" value={value} onChange={onChange && ((e) => onChange(e.target.value))} /></Field>;
}
