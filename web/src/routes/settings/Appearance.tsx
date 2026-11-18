import { useEffect, useState } from "react";
import { cssSyntax } from "./cssSyntax.ts";
import { api } from "../../api/session.ts";
import { applyDefaultTheme, isTheme, readTheme, THEME_KEY, THEME_LABEL, THEMES } from "../../app/theme.ts";
import { local } from "../../app/prefs.ts";
import { Button } from "../../ui/Button.tsx";
import { Note } from "../../ui/Note.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { Select } from "../../ui/Select.tsx";
import type { GeneralData } from "./General.tsx";
import { SettingsForm } from "./SettingsForm.tsx";
import { endpoint, FailureNote, saved, useSettings, type Control, type Failure, type Save } from "./shared.tsx";

const fields: Control[] = [{ key: "css", label: "custom.css", format: "code" }];

const retired = {
  "--ctl": "--ctl-border",
  "--border-2": "--ctl-border",
  "--accent-bg": "--accent",
  "--pop": "--shadow-2",
  "--fs-label": "--fs-micro",
};

function CssDetails({ source, cap, dirty }: { source: string; cap: number; dirty: boolean }) {
  const [preview, setPreview] = useState(true);
  useEffect(() => {
    if (!preview || !dirty || typeof CSSStyleSheet.prototype.replaceSync !== "function") return;
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(source);
    const savedSheet = document.querySelector<HTMLLinkElement>('link[href^="/custom.css"]');
    if (savedSheet) savedSheet.disabled = true;
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    return () => {
      if (savedSheet) savedSheet.disabled = false;
      document.adoptedStyleSheets = document.adoptedStyleSheets.filter((item) => item !== sheet);
    };
  }, [source, preview, dirty]);
  const clean = source.replace(/\/\*[\s\S]*?(?:\*\/|$)/g, "");
  const sets = (name: string) => new RegExp(`(^|[^\\w-])${name}\\s*:`).test(clean);
  return <>
    <p role="status" aria-live="polite" data-testid="css-syntax">{cssSyntax(source) ? `Check the CSS: ${cssSyntax(source)}` : ""}</p>
    <Switch checked={preview} onChange={setPreview} label="Live preview on this page" />
    <p className="muted">{new TextEncoder().encode(source).length.toLocaleString()} / {cap.toLocaleString()} bytes</p>
    {Object.entries(retired).filter(([old]) => sets(old)).map(([old, next]) =>
      <Note kind="warn" key={old}>Replace <code>{old}</code> with <code>{next}</code>.</Note>)}
    {sets("--glow") && <Note kind="warn">Remove the retired <code>--glow</code> declaration.</Note>}
    {sets("--border") && !sets("--line") && <Note kind="warn">Set <code>--line</code> as well to recolour table rows and dividers.</Note>}
  </>;
}

function cssDetails(input: Record<string, unknown>, dirty: boolean) {
  return <CssDetails source={String(input.css ?? "")} cap={64 * 1024} dirty={dirty} />;
}

export default function Appearance() {
  const theme = useSettings<GeneralData>("general");
  const css = useSettings<{ hash: string; css: string; max_bytes: number }>("appearance");
  const [personal, setPersonal] = useState(() => readTheme(local()));
  const [previous, setPrevious] = useState<string>();
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
      <SettingsForm title="Custom CSS" path="appearance" fields={fields} renderDetails={cssDetails}
        onSaved={(before) => setPrevious(String(before.css ?? ""))}
        renderActions={(_input, saving, replace) => previous === undefined ? null : <Button disabled={saving} onClick={() => replace({ css: previous })}>Previous version</Button>}
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
