import { Field, TextInput } from "../ui/Field.tsx";

export function LinkUrl({ id, label = "URL", value, optional, onChange }: {
  id: string; label?: string; value?: string; optional?: boolean; onChange?: (value: string) => void;
}) {
  return <Field id={id} label={label}><TextInput name="url" type="url" required={!optional}
    maxLength={2048} pattern="https?://.*" value={value} onChange={onChange && ((e) => onChange(e.target.value))} /></Field>;
}
