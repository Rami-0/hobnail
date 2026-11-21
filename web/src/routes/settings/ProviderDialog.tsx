import { useEffect, useState } from "react";
import type { BoardSummary } from "../../api/gen/BoardSummary.ts";
import type { IntegrationInput } from "../../api/gen/IntegrationInput.ts";
import type { IntegrationVerdict } from "../../api/gen/IntegrationVerdict.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import type { Provider } from "../../api/gen/Provider.ts";
import { api } from "../../api/session.ts";
import { origin } from "../../boards/document.ts";
import { SchemaForm, pinField } from "../../configuration/SchemaForm.tsx";
import { connectionValues, initial, schemaValues, type Values } from "../../configuration/schema.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { Switch } from "../../ui/Switch.tsx";

export default function ProviderDialog({ providers, item, preferredBoard, manage, icons, internet, onClose, onSaved }: {
  providers: Provider[]; item: IntegrationView | null; preferredBoard: string;
  manage: boolean; icons: boolean; internet: boolean; onClose: () => void; onSaved: () => void;
}) {
  const [provider, setProvider] = useState(providers.find((p) => p.id === item?.kind));
  const [query, setQuery] = useState("");
  const [name, setName] = useState(item?.name ?? "");
  const [icon, setIcon] = useState(item?.admin?.icon ?? "");
  const [config, setConfig] = useState<Values>({
    ...item?.admin?.options, base_url: item?.admin?.base_url,
    enabled: item?.enabled, fingerprint: item?.admin?.fingerprint,
  });
  const bound = item?.admin?.credentials;
  const [cleared, setCleared] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [verdict, setVerdict] = useState<IntegrationVerdict>();
  const [busy, setBusy] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [board, setBoard] = useState(preferredBoard);
  const [selected, setSelected] = useState<Record<string, Values | undefined>>({});
  const [placements, setPlacements] = useState<Record<string, { id: string; options: Values }[]>>({});
  const [loadedBoard, setLoadedBoard] = useState("");
  const loadingWidgets = !!item && loadedBoard !== board;
  const [widgetErrors, setWidgetErrors] = useState<Record<string, Record<string, string>>>({});

  useEffect(() => {
    if (!manage) return;
    let active = true;
    void api.get<BoardSummary[]>("/api/v1/boards").then((result) => {
      if (!active) return;
      if (result.kind === "ok") {
        setBoards(result.data);
        setBoard(result.data.find((b) => b.id === preferredBoard)?.id ?? result.data[0]?.id ?? "");
      } else setErrors({ form: result.message });
    });
    return () => { active = false; };
  }, [preferredBoard, manage]);

  useEffect(() => {
    if (!choosing || !item || !board || !provider) return;
    let active = true;
    void api.get<Record<string, { id: string; options: Values }[]>>(`/api/v1/providers/widgets?integration=${item.id}&board=${board}`).then((result) => {
      if (!active) return;
      if (result.kind === "ok") {
        setPlacements(result.data);
        setSelected(Object.fromEntries(provider.widgets.flatMap((widget) => {
          const saved = result.data[widget.id];
          return saved ? saved.map(({ id, options }) => [id, options])
            : widget.recommended ? [[widget.id, initial(widget.fields)]] : [];
        })));
        setLoadedBoard(board);
      } else setErrors({ form: result.message });
    });
    return () => { active = false; };
  }, [item, board, provider, choosing]);

  const rows = provider?.widgets.flatMap((widget) => {
    const instances = placements[widget.id] ?? [{ id: widget.id, options: initial(widget.fields) }];
    return instances.map(({ id, options }, index) => ({ widget, id, options,
      label: instances.length > 1 ? `${widget.name} ${index + 1}` : widget.name }));
  }) ?? [];

  function choose(entry: Provider) {
    setProvider(entry);
    setConfig(initial(entry.fields));
    setName(entry.name);
    setVerdict(undefined);
    setErrors({});
    setSelected(Object.fromEntries(entry.widgets.filter((w) => w.recommended).map((w) => [w.id, initial(w.fields)])));
  }

  function change(key: string, value: string | boolean) {
    setVerdict(undefined);
    if (value && cleared[key]) setCleared({ ...cleared, [key]: false });
    setConfig((previous) => {
      if (key === "base_url" && origin(String(previous.base_url ?? "")) !== origin(String(value))) {
        return { ...previous, ...Object.fromEntries(provider?.fields.filter((f) => f.kind === "secret").map((f) => [f.id, ""]) ?? []), base_url: value, fingerprint: "" };
      }
      return { ...previous, [key]: value };
    });
  }

  async function submit(test: boolean) {
    if (!provider || busy || (choosing && board && loadingWidgets)) return;
    const { errors: connectionErrors, ...connection } = connectionValues(provider.fields, config, bound, cleared);
    const fingerprint = String(config.fingerprint ?? "").trim();
    if (fingerprint && (!/^[0-9a-f]{64}$/i.test(fingerprint) || !connection.base_url.startsWith("https://"))) {
      connectionErrors.fingerprint = "Use HTTPS and enter 64 hexadecimal digits.";
    }
    if (!name.trim()) connectionErrors.name = "Enter an instance name.";
    setErrors(connectionErrors);
    if (Object.keys(connectionErrors).length) return;

    const widgetFields: Record<string, Record<string, string>> = {};
    const widgets = choosing && board ? rows.flatMap(({ widget, id }) => {
      if (!selected[id]) return [];
      const fields = schemaValues(widget.fields, selected[id] ?? {});
      if (Object.keys(fields.errors).length) widgetFields[id] = fields.errors;
      return [{ id: widget.id, placement: id === widget.id ? undefined : id, options: fields.values }];
    }) : [];
    setWidgetErrors(widgetFields);
    if (Object.keys(widgetFields).length) return;

    const integration: IntegrationInput = {
      kind: provider.id, name: name.trim(), ...connection, icon: icon || null, fingerprint: fingerprint || null,
    };
    setBusy(true);
    if (test) {
      const result = await api.post<IntegrationVerdict>("/api/v1/providers/test", { integration, integration_id: item?.id });
      if (result.kind === "ok") setVerdict(result.data);
      else setErrors({ [result.kind === "invalid" ? result.field ?? "form" : "form"]: result.message });
    } else {
      const result = item && !choosing
        ? await api.request("PUT", `/api/v1/integrations/${item.id}`, { body: integration })
        : await api.post("/api/v1/providers/install", { integration, integration_id: item?.id, board_id: board || null, widgets });
      if (result.kind === "ok") onSaved();
      else setErrors({ [result.kind === "invalid" && !choosing ? result.field ?? "form" : "form"]: result.message });
    }
    setBusy(false);
  }

  return <Dialog open title={choosing ? "Choose widgets" : item ? "Edit integration" : provider?.name ?? "Integration catalog"}
    onClose={() => { if (!busy) onClose(); }} testId="integration-form" footer={<>
      <Button onClick={() => choosing ? setChoosing(false) : provider && !item ? setProvider(undefined) : onClose()} disabled={busy}>
        {choosing || provider && !item ? "Back" : "Cancel"}
      </Button>
      {provider && !choosing && manage && <Button busy={busy} disabled={config.enabled === false} data-testid="provider-draft-test" onClick={() => void submit(true)}>Test connection</Button>}
      {provider && !choosing && manage && verdict?.ok && <Button variant="default" onClick={() => { setLoadedBoard(""); setChoosing(true); }} disabled={busy}>Choose widgets</Button>}
      {provider && <Button type="submit" form="integration-form" variant="default" busy={busy} disabled={choosing && !!board && loadingWidgets}>{choosing && board ? "Save widgets to board" : item ? "Save" : "Save connection"}</Button>}
    </>}>
    <form id="integration-form" noValidate onSubmit={(e) => { e.preventDefault(); void submit(false); }}>
      {!provider ? <>
        <Field label="Search integrations" id="provider-search"><TextInput type="search" value={query} onChange={(e) => setQuery(e.target.value)} /></Field>
        {providers.filter((p) => `${p.name} ${p.category} ${p.description}`.toLowerCase().includes(query.trim().toLowerCase())).map((p) => (
          <div className="settings-row" key={p.id}>
            <ServiceIcon name={p.icon} title={p.name} enabled={icons} />
            <div><p className="muted">{p.category}</p><h3>{p.name}</h3><p>{p.description}</p>
              {p.public_internet && !internet && <p>Internet access is off.</p>}</div>
            <Button onClick={() => choose(p)}>Choose {p.name}</Button>
          </div>
        ))}
      </> : choosing ? <>
        <Select label="Board" value={board} options={[{ value: "", label: "Save connection only" }, ...boards.map((b) => ({ value: b.id, label: b.name }))]} onChange={(next) => { setLoadedBoard(""); setBoard(next); }} />
        {board && !loadingWidgets && rows.map(({ widget, id, label, options }) => <div key={id}>
          <Switch checked={!!selected[id]} label={label} onChange={(checked) => setSelected({
            ...selected, [id]: checked ? options : undefined,
          })} />
          <p className="muted">{widget.description}</p>
          {selected[id] && <SchemaForm id={id} schema={widget.fields} values={selected[id] ?? {}} errors={widgetErrors[id]}
            onChange={(key, value) => setSelected({ ...selected, [id]: { ...selected[id], [key]: value } })} />}
        </div>)}
      </> : <>
        {!item && <p><a href={provider.docs}>Provider documentation</a></p>}
        <Field label="Name" id="integration-name" error={errors.name}><TextInput name="name" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} /></Field>
        {item && <Field label="Icon name" id="integration-icon" error={errors.icon}><TextInput name="icon" value={icon} onChange={(e) => setIcon(e.target.value)} /></Field>}
        <SchemaForm id="integration" schema={[...provider.fields, pinField]} values={config} errors={errors}
          onChange={change} bound={bound} cleared={cleared} onClear={item ? (key) => {
            change(key, "");
            setCleared({ ...cleared, [key]: !cleared[key] });
          } : undefined} />
        {verdict && <Note testId="provider-draft-verdict" kind={verdict.ok ? "ok" : "warn"}>{verdict.message}
          {verdict.fingerprint && <><p>Compare this fingerprint with the certificate on your service.</p><p className="wrap"><code>{verdict.fingerprint}</code></p>
            <Button onClick={() => { change("fingerprint", verdict.fingerprint ?? ""); }}>Confirm and pin fingerprint</Button></>}
        </Note>}
      </>}
      {errors.form && <Note kind="bad">{errors.form}</Note>}
    </form>
  </Dialog>;
}
