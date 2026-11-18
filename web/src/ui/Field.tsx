// Form controls (DESIGN.md §11.5; static/components.css `.fld`): a Field is
// a label, one control, a hint and an error; the control inside picks up
// its id, `aria-describedby` and `aria-invalid` from the Field, so a page
// writes <Field label="Name" id="name" error={e}><TextInput name="name"/></Field>.
// Text is 16 px on touch screens (no iOS zoom on focus) and controls are
// 44 px high there (tokens.css --fs-input, --ctl-h).
import { createContext, useContext, useId, useMemo, type InputHTMLAttributes, type ReactNode, type Ref, type SelectHTMLAttributes } from "react";

interface FieldCtx {
  id: string;
  describedBy: string | undefined;
  invalid: boolean;
}

const Ctx = createContext<FieldCtx | null>(null);

export interface FieldProps {
  label: ReactNode;
  hint?: ReactNode;
  /** The server's or the form's message for this field; marks the control invalid. */
  error?: ReactNode;
  children: ReactNode;
  /** The control's id (the label points at it). */
  id: string;
  /** Spans both columns of a two-column form. */
  wide?: boolean;
}

export function Field({ label, hint, error, children, id, wide }: FieldProps) {
  const hintId = hint ? `${id}-hint` : undefined;
  const errId = error ? `${id}-err` : undefined;
  const describedBy = [errId, hintId].filter(Boolean).join(" ") || undefined;
  const invalid = !!error;
  const ctx = useMemo(() => ({ id, describedBy, invalid }), [id, describedBy, invalid]);
  return (
    <div className={wide ? "fld wide" : "fld"}>
      <label htmlFor={id}>{label}</label>
      <Ctx.Provider value={ctx}>{children}</Ctx.Provider>
      {error && (
        <p className="fld-err" id={errId}>
          {error}
        </p>
      )}
      {hint && (
        <p className="hint" id={hintId}>
          {hint}
        </p>
      )}
    </div>
  );
}

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  ref?: Ref<HTMLInputElement>;
  /** Monospace text (names, URLs, patterns). */
  mono?: boolean;
}

export function TextInput({ className, mono, type = "text", ...rest }: TextInputProps) {
  const f = useContext(Ctx);
  const cls = ["in", mono ? "mono" : "", className ?? ""].filter(Boolean).join(" ");
  return (
    <input
      type={type}
      id={f?.id}
      aria-describedby={f?.describedBy}
      aria-invalid={f?.invalid || rest["aria-invalid"] || undefined}
      {...rest}
      className={cls}
    />
  );
}

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
    <select
      {...rest}
      id={sid}
      className={["in", className ?? ""].filter(Boolean).join(" ")}
      value={value}
      aria-describedby={f?.describedBy}
      aria-invalid={f?.invalid || undefined}
      onChange={(e) => onChange(e.target.value)}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
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

export interface SwitchProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "onChange" | "checked" | "type"> {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  /** Saved at once and waiting for the answer: shown busy, presses ignored (§11.1). */
  busy?: boolean;
  ref?: Ref<HTMLInputElement>;
}

/** A checkbox with role=switch, its label beside it. */
export function Switch({ checked, onChange, label, busy, className, ...rest }: SwitchProps) {
  const f = useContext(Ctx);
  return (
    <label className={["cb", className ?? ""].filter(Boolean).join(" ")}>
      <input
        {...rest}
        type="checkbox"
        role="switch"
        aria-checked={checked}
        className="sw"
        id={rest.id ?? f?.id}
        aria-describedby={f?.describedBy}
        aria-busy={busy || undefined}
        checked={checked}
        onChange={(e) => {
          if (!busy) onChange(e.target.checked);
        }}
      />
      <span>{label}</span>
    </label>
  );
}
