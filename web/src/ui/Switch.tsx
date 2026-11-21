import { useContext, type ReactNode, type Ref, type ButtonHTMLAttributes } from "react";
import { Switch as SdkSwitch } from "@ui-sdk/react/switch";
import { Ctx } from "./Field.tsx";

export interface SwitchProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onChange" | "type" | "value"> {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  /** Saved at once and waiting for the answer: shown busy, presses ignored (§11.1). */
  busy?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

/** A checkbox with role=switch, its label beside it. */
export function Switch({ checked, onChange, label, busy, className, ...rest }: SwitchProps) {
  const f = useContext(Ctx);
  return (
    <label className={["cb", className ?? ""].filter(Boolean).join(" ")}>
      <SdkSwitch
        {...rest}
        aria-checked={checked}
        className="sw"
        id={rest.id ?? f?.id}
        aria-describedby={f?.describedBy}
        aria-invalid={f?.invalid || rest["aria-invalid"] || undefined}
        aria-busy={busy || undefined}
        checked={checked}
        onCheckedChange={(next) => {
          if (!busy) onChange(next);
        }}
      />
      <span>{label}</span>
    </label>
  );
}
