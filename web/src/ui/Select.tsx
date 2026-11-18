import { useContext, useId, type ReactNode, type Ref, type SelectHTMLAttributes } from "react";
import { Select as SdkSelect, SelectContent, SelectItem, SelectTrigger } from "@ui-sdk/react/select";
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
}

/** A native <select> with its own label (inside a Field, the Field's label is used instead). */
export function Select({ label, value, onChange, options, className, hideLabel, id, ...rest }: SelectProps) {
  const f = useContext(Ctx);
  const auto = useId();
  const sid = f?.id ?? id ?? auto;
  const control = (
    <SdkSelect value={value} items={options} onValueChange={(next) => { if (next !== null) onChange(next); }}
      name={rest.name} disabled={rest.disabled}>
      <SelectTrigger id={sid} className={["in", className ?? ""].filter(Boolean).join(" ")}
        aria-describedby={f?.describedBy} aria-invalid={f?.invalid || undefined}
        aria-label={typeof label === "string" ? label : undefined}>
        {options.find((o) => o.value === value)?.label ?? value}
      </SelectTrigger>
      <SelectContent className="sdk-select-list">
        {options.map((o) => <SelectItem key={o.value} value={o.value} className="menu-item">{o.label}</SelectItem>)}
      </SelectContent>
    </SdkSelect>
  );
  if (f) return control;
  return (
    <span className="sel">
      <label htmlFor={sid} className={hideLabel ? "sr" : "sel-l"}>
        {label}
      </label>
      {control}
    </span>
  );
}

