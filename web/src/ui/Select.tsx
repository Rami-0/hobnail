import { CheckIcon, ChevronDownIcon } from "lucide-react";
import { useContext, useId, type ReactNode, type Ref, type SelectHTMLAttributes } from "react";
import { Select as SdkSelect, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from "@ui-sdk/react/select";
import { Ctx } from "./Field.tsx";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value"> {
  label: ReactNode;
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  ref?: Ref<HTMLSelectElement>;
  /** The label is for screen readers only (a toolbar select). */
  hideLabel?: boolean;
  placeholder?: string;
}

/** Outside a Field, the select supplies its own label. */
export function Select({ label, value, onChange, options, className, hideLabel, id, placeholder, ...rest }: SelectProps) {
  const f = useContext(Ctx);
  const auto = useId();
  const sid = f?.id ?? id ?? auto;
  const control = (
    <SdkSelect value={value} items={options} onValueChange={(next) => { if (next !== null) onChange(next); }}
      name={rest.name} disabled={rest.disabled} placeholder={placeholder}>
      <SelectTrigger id={sid} className={["in select-trigger", className ?? ""].filter(Boolean).join(" ")}
        aria-describedby={f?.describedBy} aria-invalid={f?.invalid || undefined} aria-required={rest.required || undefined}
        aria-label={typeof label === "string" ? label : undefined}>
        <SelectValue />
        <ChevronDownIcon aria-hidden="true" />
      </SelectTrigger>
      <SelectContent className="sdk-select-list">
        {options.map((o) => <SelectItem key={o.value} value={o.value} data-value={o.value} className="menu-item">{o.label}<SelectItemIndicator><CheckIcon aria-hidden="true" /></SelectItemIndicator></SelectItem>)}
      </SelectContent>
    </SdkSelect>
  );
  if (f) return control;
  return (
    <span className="sel" data-testid={"data-testid" in rest ? String(rest["data-testid"]) : undefined}>
      <label htmlFor={sid} className={hideLabel ? "sr" : "sel-l"}>
        {label}
      </label>
      {control}
    </span>
  );
}

