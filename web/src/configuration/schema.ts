import { origin } from "../boards/document.ts";
import type { Field } from "../api/gen/Field.ts";

export type Values = Record<string, unknown>;

export function initial(fields: Field[]): Values {
  return Object.fromEntries(fields.filter((f) => f.default !== null).map((f) => [
    f.id, ["bool", "list"].includes(f.kind) ? JSON.parse(f.default!) : f.default,
  ]));
}

export function secretHint(bound: string | null | undefined, url: string, cleared = false) {
  if (cleared) return "Saved credential will be cleared.";
  if (!bound) return "";
  return origin(url) === bound
    ? `Saved for ${bound}. Enter a replacement to change it.`
    : "Origin changed. Enter a replacement or clear the saved credential.";
}

export function schemaValues(fields: Field[], input: Values) {
  const values: Values = {};
  const errors: Record<string, string> = {};
  for (const field of fields) {
    const { id, kind, max_length: max } = field;
    const value = input[id];
    if (value === undefined || value === "" || value === null) {
      if (field.required) errors[id] = "This field is required.";
      continue;
    }
    if (kind === "list") {
      const list = (Array.isArray(value) ? value.map(String) : String(value).split("\n")).map((s) => s.trim()).filter(Boolean);
      if ((field.required && !list.length) || list.length > 16 || list.some((s) => max !== null && s.length > max)) {
        errors[id] = "Enter up to 16 valid lines.";
      }
      if (list.length) values[id] = list;
    } else if (kind === "number") {
      const n = Number(value);
      if (!Number.isSafeInteger(n) || n < (field.min ?? 0) || n > (field.max ?? Infinity)) {
        errors[id] = `Enter a whole number from ${field.min ?? 0}${field.max === null ? " or higher" : ` to ${field.max}`}.`;
      }
      values[id] = n;
    } else {
      const s = String(value);
      if (kind === "url" && !origin(s)) errors[id] = "Enter an HTTP or HTTPS URL without credentials, query, or fragment.";
      if (kind === "secret" && !/^[!-~]+$/.test(s)) errors[id] = "Use printable ASCII characters without spaces.";
      if (kind === "select" && !field.choices.includes(s)) errors[id] = "Choose a listed value.";
      if (max !== null && s.length > max) errors[id] = `Use at most ${max} characters.`;
      values[id] = value;
    }
  }
  return { values, errors };
}
