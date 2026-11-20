// Form controls (DESIGN.md §11.5): a Field is
// a label, one control, a hint and an error; the control inside picks up
// its id, `aria-describedby` and `aria-invalid` from the Field, so a page
// writes <Field label="Name" id="name" error={e}><TextInput name="name"/></Field>.
// Text is 16 px on touch screens (no iOS zoom on focus) and controls are
// 44 px high there (tokens.css --fs-input, --ctl-h).
import { Field as SdkField, FieldLabel, FieldDescription, FieldError } from "@ui-sdk/react/field";
import { Input } from "@ui-sdk/react/input";
import { createContext, useContext, useMemo, type InputHTMLAttributes, type ReactNode, type Ref } from "react";

interface FieldCtx {
  id: string;
  describedBy: string | undefined;
  invalid: boolean;
}

export const Ctx = createContext<FieldCtx | null>(null);

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
    <SdkField invalid={invalid} className={wide ? "fld wide" : "fld"}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Ctx.Provider value={ctx}>{children}</Ctx.Provider>
      {error && (
        <FieldError className="fld-err" id={errId}>
          {error}
        </FieldError>
      )}
      {hint && (
        <FieldDescription className="hint" id={hintId}>
          {hint}
        </FieldDescription>
      )}
    </SdkField>
  );
}

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  ref?: Ref<HTMLInputElement>;
  /** Monospace text (names, URLs, patterns). */
  mono?: boolean;
}

export function TextInput({ className, mono, type = "text", value, defaultValue, ...rest }: TextInputProps) {
  const f = useContext(Ctx);
  const cls = ["in", mono ? "mono" : "", className ?? ""].filter(Boolean).join(" ");
  return (
    <Input
      type={type}
      value={value === undefined ? undefined : String(value)}
      defaultValue={defaultValue === undefined ? undefined : String(defaultValue)}
      id={f?.id}
      aria-describedby={f?.describedBy}
      aria-invalid={f?.invalid || rest["aria-invalid"] || undefined}
      {...rest}
      className={cls}
    />
  );
}


export function Textarea({ className, ...props }: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const field = useContext(Ctx);
  return <textarea id={field?.id} aria-describedby={field?.describedBy} aria-invalid={field?.invalid || undefined}
    className={`in ${className ?? ""}`} {...props} />;
}
