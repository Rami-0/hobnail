import { SettingsForm } from "./SettingsForm.tsx";
import type { Values } from "./forms.ts";
import { FailureNote, useSettings, type Control } from "./shared.tsx";

export type GeneralData = { hash: string; page: Values | null; effective: Values; defaults: Values };

export default function General() {
  const { data, failure, load } = useSettings<GeneralData>("general");
  const fields: Control[] = [
    { key: "title", label: "Title", max: 80, placeholder: String(data?.defaults.title ?? "") },
    { key: "greeting", label: "Greeting", max: 200, hint: "Leave blank for a greeting based on the time of day." },
    { key: "search_url", label: "Search URL", format: "url", hint: "Put {q} where the search terms go.",
      validate: (value) => value.includes("{q}") ? undefined : "Include {q} for the search query." },
    { key: "show_stopped", label: "Show stopped containers", format: "switch", default: data?.effective.show_stopped },
    { key: "show_stats", label: "Show live CPU and memory", format: "switch", default: data?.effective.show_stats },
  ];
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <SettingsForm title="Page" path="general" fields={fields} snapshot={{ hash: data.hash, values: data.page ?? {} }}
      reload={async () => {
        const result = await load();
        if (result.kind === "ok") return { hash: result.data.hash, values: result.data.page ?? {} };
      }} />}
  </>;
}
