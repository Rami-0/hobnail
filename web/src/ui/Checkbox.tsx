import { Checkbox as SdkCheckbox, CheckboxIndicator } from "@ui-sdk/react/checkbox";
import { CheckIcon } from "lucide-react";
import { useContext, type ReactNode } from "react";
import { Ctx } from "./Field.tsx";

export function Checkbox({ checked, onChange, label, disabled }: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  disabled?: boolean;
}) {
  const field = useContext(Ctx);
  return <label className="cb">
    <SdkCheckbox className="checkbox" checked={checked} onCheckedChange={(next) => onChange(next === true)} disabled={disabled}
      id={field?.id} aria-describedby={field?.describedBy} aria-invalid={field?.invalid || undefined}>
      <CheckboxIndicator><CheckIcon aria-hidden="true" /></CheckboxIndicator>
    </SdkCheckbox>
    <span>{label}</span>
  </label>;
}
