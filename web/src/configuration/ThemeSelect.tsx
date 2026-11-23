import { isTheme, THEME_LABEL, THEMES } from "../app/theme.ts";
import { Select } from "../ui/Select.tsx";

export function ThemeSelect({ value, fallback, disabled, onChange }: {
  value: string | null; fallback: string; disabled?: boolean; onChange: (value: string | null) => void;
}) {
  const label = isTheme(fallback) ? THEME_LABEL[fallback] : THEME_LABEL.auto;
  return <Select label="Default theme" value={value ?? "default"} disabled={disabled}
    options={[{ value: "default", label: `Use current default (${label})` }, ...THEMES.map((theme) => ({ value: theme, label: THEME_LABEL[theme] }))]}
    onChange={(next) => onChange(next === "default" ? null : next)} />;
}
