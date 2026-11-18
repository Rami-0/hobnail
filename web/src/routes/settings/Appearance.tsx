import { useState } from "react";
import { api } from "../../api/session.ts";
import { applyDefaultTheme, isTheme, readTheme, THEME_KEY, THEME_LABEL, THEMES } from "../../app/theme.ts";
import { local } from "../../app/prefs.ts";
import { Button } from "../../ui/Button.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import type { GeneralData } from "./General.tsx";
import { SettingsForm } from "./SettingsForm.tsx";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

const fields: Control[] = [{ key: "css", label: "custom.css", format: "code" }];

export default function Appearance() {
  const theme = useSettings<GeneralData>("general");
  const css = useSettings<{ hash: string; css: string; max_bytes: number }>("appearance");
  const [personal, setPersonal] = useState(() => readTheme(local()));
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<Failure>();
  return <>
    <section className="settings-section"><h2>Theme</h2>
      <FailureNote failure={failure ?? theme.failure} rebase={() => { void theme.load().then(() => setFailure(undefined)); }} />
      {theme.data && <Select label="Default theme" value={String(theme.data.effective.theme ?? "auto")}
        options={THEMES.map((value) => ({ value, label: THEME_LABEL[value] }))} disabled={busy}
        onChange={async (value) => {
          if (!theme.data || busy) return;
          setBusy(true);
          const result = await api.post<Save>(endpoint("general"), { base_hash: theme.data.hash, values: { theme: value } });
          if (result.kind === "ok") {
            setFailure(undefined);
            saved(result.data);
            await theme.load();
          } else setFailure(result);
          setBusy(false);
        }} />}
      {personal && <div><Button onClick={() => {
        local()?.removeItem?.(THEME_KEY);
        setPersonal(null);
        const fallback = theme.data?.effective.theme;
        applyDefaultTheme(isTheme(fallback) ? fallback : "auto");
      }}>Use the default here</Button></div>}
    </section>
    <FailureNote failure={css.failure} retry={() => { void css.load(); }} />
    {css.data && <>
      <Note kind="info">Custom CSS loads after the app styles on every page. Maximum {css.data.max_bytes / 1024} KiB.</Note>
      <SettingsForm title="Custom CSS" path="appearance" fields={fields}
        snapshot={{ hash: css.data.hash, values: { css: css.data.css } }} encode={(values) => ({ css: values.css ?? "" })}
        reload={async () => {
          const result = await css.load();
          if (result.kind === "ok") {
            const sheet = document.querySelector<HTMLLinkElement>('link[rel="stylesheet"][href^="/custom.css"]');
            if (sheet) sheet.href = `/custom.css?v=${result.data.hash}`;
            return { hash: result.data.hash, values: { css: result.data.css } };
          }
        }} wide />
    </>}
  </>;
}
