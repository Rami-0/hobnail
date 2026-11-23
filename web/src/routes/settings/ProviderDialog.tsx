import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { BoardSummary } from "../../api/gen/BoardSummary.ts";
import type { IntegrationInput } from "../../api/gen/IntegrationInput.ts";
import type { IntegrationVerdict } from "../../api/gen/IntegrationVerdict.ts";
import type { IntegrationView } from "../../api/gen/IntegrationView.ts";
import type { Provider } from "../../api/gen/Provider.ts";
import { api } from "../../api/session.ts";
import { origin } from "../../boards/document.ts";
import { SchemaForm, pinField } from "../../configuration/SchemaForm.tsx";
import { connectionValues, hasEndpoint, initial, schemaValues, visibleFields, type Values } from "../../configuration/schema.ts";
import { Button } from "../../ui/Button.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { CardButton } from "../../ui/CardButton.tsx";
import { Steps } from "../../ui/Steps.tsx";
import { brandColor } from "../../ui/brand.ts";
import { WidgetPreview } from "../../ui/WidgetPreview.tsx";
import { ArrowRightIcon, BookOpenIcon, LayoutGridIcon, ZapIcon } from "lucide-react";
import type { CSSProperties } from "react";
import { matchesConnectionTest, type ConnectionTest } from "./connection-test.ts";

const brand = (color: string) => ({ "--brand": brandColor(color) }) as CSSProperties;
const advanced = (field: { id: string; kind: string }) => field.kind === "bool" || field.id === "fingerprint";

export default function ProviderDialog({ providers, item, preferredBoard, icons, internet, onClose, onSaved }: {
  providers: Provider[]; item: IntegrationView | null; preferredBoard: string;
  icons: boolean; internet: boolean; onClose: () => void; onSaved: () => void;
}) {
  const [provider, setProvider] = useState(providers.find((p) => p.id === item?.kind));
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const verdictRef = useRef<HTMLDivElement>(null);
  const [name, setName] = useState(item?.name ?? "");
  const [icon, setIcon] = useState(item?.admin?.icon ?? "");
  const [config, setConfig] = useState<Values>({
    ...item?.admin?.options, base_url: item?.admin?.base_url,
    enabled: item?.enabled, fingerprint: item?.admin?.fingerprint,
  });
  const bound = item?.admin?.credentials;
  const [cleared, setCleared] = useState<Record<string, boolean>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const revision = useRef(0);
  const [tested, setTested] = useState<{ key: ConnectionTest; result: IntegrationVerdict }>();
  const connectionKey = () => ({ url: String(config.base_url ?? ""), pin: String(config.fingerprint ?? "").trim(), revision: revision.current });
  const verdict = tested?.result;
  useEffect(() => { if (verdict) verdictRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" }); }, [verdict]);
  const [busy, setBusy] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [board, setBoard] = useState(preferredBoard);
  const [selected, setSelected] = useState<Record<string, Values | undefined>>({});
  const [placements, setPlacements] = useState<Record<string, { id: string; options: Values }[]>>({});
  const [loadedBoard, setLoadedBoard] = useState("");
  const loadingWidgets = !!item && loadedBoard !== board;
  const [widgetErrors, setWidgetErrors] = useState<Record<string, Record<string, string>>>({});
  const form = useRef<HTMLFormElement>(null);

  useLayoutEffect(() => {
    if (Object.keys(choosing ? widgetErrors : errors).length) {
      form.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
    }
  }, [errors, widgetErrors, choosing]);

  useEffect(() => {
    let active = true;
    void api.get<BoardSummary[]>("/api/v1/boards").then((result) => {
      if (!active) return;
      if (result.kind === "ok") {
        setBoards(result.data);
        setBoard(result.data.find((b) => b.id === preferredBoard)?.id ?? result.data[0]?.id ?? "");
      } else setErrors({ form: result.message });
    });
    return () => { active = false; };
  }, [preferredBoard]);

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
    revision.current++;
    setTested(undefined);
    setErrors({});
    setSelected(Object.fromEntries(entry.widgets.filter((w) => w.recommended).map((w) => [w.id, initial(w.fields)])));
  }

  function change(key: string, value: string | boolean) {
    revision.current++;
    setTested(undefined);
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
    const { errors: connectionErrors, ...connection } = connectionValues(visibleFields(provider.fields, config), config, bound, cleared);
    const fingerprint = String(config.fingerprint ?? "").trim();
    if (fingerprint && (!/^[0-9a-f]{64}$/i.test(fingerprint) || !connection.base_url.startsWith("https://"))) {
      connectionErrors.fingerprint = "Use HTTPS and enter 64 hexadecimal digits.";
    }
    if (!name.trim()) connectionErrors.name = "Enter an instance name.";
    setErrors(connectionErrors);
    if (Object.keys(connectionErrors).length) {
      setChoosing(false);
      return;
    }

    const widgetFields: Record<string, Record<string, string>> = {};
    // Saved placements switched off: the server takes them off the board.
    const remove: string[] = [];
    const widgets = choosing && board ? rows.flatMap(({ widget, id }) => {
      if (!selected[id]) {
        if (id !== widget.id) remove.push(id);
        return [];
      }
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
      const key = connectionKey();
      const result = await api.post<IntegrationVerdict>("/api/v1/providers/test", { integration, integration_id: item?.id });
      if (matchesConnectionTest(key, connectionKey())) {
        if (result.kind === "ok") setTested({ key, result: result.data });
        else setErrors({ [result.kind === "invalid" ? result.field ?? "form" : "form"]: result.message });
      }
    } else {
      const result = item && !choosing
        ? await api.request("PUT", `/api/v1/integrations/${item.id}`, { body: integration })
        : await api.post("/api/v1/providers/install", { integration, integration_id: item?.id, board_id: board || null, widgets, remove });
      if (result.kind === "ok") onSaved();
      else {
        const field = result.kind === "invalid" ? result.field : undefined;
        const [, index, option] = /^widgets\.(\d+)\.(.+)$/.exec(field ?? "") ?? [];
        const widget = widgets[Number(index)];
        if (widget && option) {
          setWidgetErrors({ [widget.placement ?? widget.id]: { [option]: result.message } });
        } else if (field && (provider.fields.some((entry) => entry.id === field) || ["fingerprint", "name", "icon"].includes(field))) {
          setErrors({ [field]: result.message });
          setChoosing(false);
        } else {
          setErrors({ form: result.message });
        }
      }
    }
    setBusy(false);
  }

  const step = !provider ? 0 : choosing ? 2 : 1;
  const categories = [...new Set(providers.map((p) => p.category))].toSorted();
  const listed = providers.filter((p) => (!category || p.category === category) && `${p.name} ${p.category} ${p.description}`.toLowerCase().includes(query.trim().toLowerCase()));
  const shown = provider ? visibleFields(provider.fields, config) : [];
  const schema = provider && hasEndpoint(provider.fields) ? [...shown, pinField] : shown;
  const formProps = { values: config, errors, onChange: change, bound, cleared, onClear: item ? (key: string) => {
    change(key, "");
    setCleared({ ...cleared, [key]: !cleared[key] });
  } : undefined };
  return <Dialog open wide className="integration-dialog" title={choosing ? "Add to a board" : item ? "Edit integration" : provider ? `Connect ${provider.name}` : "Add integration"}
    onClose={() => { if (!busy) onClose(); }} testId="integration-form" footer={<>
      <Button onClick={() => choosing ? setChoosing(false) : provider && !item ? setProvider(undefined) : onClose()} disabled={busy}>
        {choosing || provider && !item ? "Back" : "Cancel"}
      </Button>
      {provider && !choosing && <Button busy={busy} disabled={config.enabled === false} data-testid="provider-draft-test" icon={<ZapIcon aria-hidden="true" />} onClick={() => void submit(true)}>Test connection</Button>}
      {provider && !choosing && <Button type="submit" form="integration-form" variant={(item || verdict?.ok) && boards.length > 0 && !item ? "outline" : "default"} busy={busy}>{item ? "Save" : "Save connection"}</Button>}
      {provider && !choosing && boards.length > 0 && (item || verdict?.ok) && <Button variant="default" trailingIcon={<ArrowRightIcon aria-hidden="true" />} onClick={() => { setLoadedBoard(""); setChoosing(true); }} disabled={busy}>Choose widgets</Button>}
      {provider && choosing && <Button type="submit" form="integration-form" variant="default" busy={busy} disabled={!!board && loadingWidgets}>{board ? "Save widgets to board" : "Save connection"}</Button>}
    </>}>
    {!item && <Steps steps={["Choose a service", "Connect", "Add to board"]} current={step} />}
    <form ref={form} id="integration-form" noValidate onSubmit={(e) => { e.preventDefault(); void submit(false); }}>
      {!provider ? <div className="catalog">
        <Field label="Search integrations" id="provider-search"><TextInput type="search" value={query} placeholder="Pi-hole, Jellyfin, Uptime Kuma…" onChange={(e) => setQuery(e.target.value)} /></Field>
        <div className="catalog-filters" role="group" aria-label="Category">
          {["", ...categories].map((entry) => <Button key={entry || "all"} size="sm" aria-pressed={category === entry} onClick={() => setCategory(entry)}>{entry || "All"}</Button>)}
        </div>
        {listed.length ? <div className="catalog-grid">{listed.map((p) => (
          <CardButton key={p.id} className="catalog-card" style={brand(p.color)} onClick={() => choose(p)}>
            <ServiceIcon tint name={p.icon} title={p.name} enabled={icons} />
            <small>{p.category}</small><strong><span className="sr">Choose </span>{p.name}</strong>
            <p>{p.description}</p>
            <span className="catalog-count"><LayoutGridIcon aria-hidden="true" />{p.widgets.length === 1 ? "1 widget" : `${p.widgets.length} widgets`}</span>
            {p.public_internet && !internet && <p className="catalog-warn">Needs internet access, which is off.</p>}
          </CardButton>
        ))}</div> : <p className="muted">No integration matches “{query}”. The Generic HTTP integration can read most JSON APIs.</p>}
      </div> : choosing ? <>
        <Select label="Board" value={board} options={[{ value: "", label: "Save connection only" }, ...boards.map((b) => ({ value: b.id, label: b.name }))]} onChange={(next) => { setLoadedBoard(""); setBoard(next); }} />
        {board && loadingWidgets && <p className="muted">Loading widgets…</p>}
        {board && !loadingWidgets && <div className="widget-gallery" data-testid="widget-gallery">{rows.map(({ widget, id, label, options }) =>
          <WidgetPreview key={id} widget={widget} icon={provider.icon} color={provider.color} title={provider.name}>
            <div className="widget-choice" data-on={selected[id] ? "" : undefined}>
              <Switch checked={!!selected[id]} label={`Add ${label}`} onChange={(checked) => setSelected({
                ...selected, [id]: checked ? options : undefined,
              })} />
              {selected[id] && <SchemaForm id={id} schema={widget.fields} values={selected[id] ?? {}} errors={widgetErrors[id]}
                onChange={(key, value) => setSelected({ ...selected, [id]: { ...selected[id], [key]: value } })} />}
            </div>
          </WidgetPreview>)}</div>}
      </> : <>
        <div className="provider-head" style={brand(provider.color)}><ServiceIcon tint name={provider.icon} title={provider.name} enabled={icons} />
          <div><small>{provider.category}</small><strong>{provider.name}</strong><p>{provider.description}</p></div>
          <a href={provider.docs} target="_blank" rel="noreferrer"><BookOpenIcon aria-hidden="true" />Setup guide<span className="sr"> (opens in a new tab)</span></a></div>
        <div className="provider-connect"><div className="provider-settings">
        <Field label="Name" id="integration-name" error={errors.name} hint="How it is labelled on boards."><TextInput name="name" required maxLength={80} value={name} onChange={(e) => setName(e.target.value)} /></Field>
        {item && <Field label="Icon name" id="integration-icon" error={errors.icon}><TextInput name="icon" value={icon} onChange={(e) => setIcon(e.target.value)} /></Field>}
        <SchemaForm id="integration" schema={schema.filter((field) => !advanced(field))} {...formProps} />
        {schema.some(advanced) && <details className="advanced" open={showAdvanced || schema.some((field) => advanced(field) && !!errors[field.id])} onToggle={(e) => setShowAdvanced(e.currentTarget.open)}>
          <summary>Advanced</summary>
          <div><SchemaForm id="integration" schema={schema.filter(advanced)} {...formProps} /></div>
        </details>}
        {verdict && <div ref={verdictRef}><Note testId="provider-draft-verdict" kind={verdict.ok ? "ok" : "warn"}>
          <strong>{verdict.ok ? `Connected in ${verdict.latency_ms} ms.` : "Hobnail could not connect."}</strong>{!verdict.ok && ` ${verdict.message}`}
          {verdict.ok && !item && boards.length > 0 && <p>Next, choose which widgets to show on a board.</p>}
          {verdict.fingerprint && <><p>Compare this fingerprint with the certificate on your service.</p><p className="wrap"><code>{verdict.fingerprint}</code></p>
            <Button onClick={() => {
              if (tested && matchesConnectionTest(tested.key, connectionKey())) change("fingerprint", verdict.fingerprint ?? "");
            }}>Confirm and pin fingerprint</Button></>}
        </Note></div>}
        </div>
        {!item && <aside className="provider-widgets" aria-labelledby="provider-widgets-title">
          <h3 id="provider-widgets-title">Widgets you can add</h3>
          <p className="muted">Previews use example data. You choose widgets after connecting.</p>
          {provider.widgets.map((widget) => <WidgetPreview compact key={widget.id} widget={widget} icon={provider.icon} color={provider.color} title={provider.name} />)}
        </aside>}
        </div>
      </>}
      {errors.form && <Note kind="bad">{errors.form}</Note>}
    </form>
  </Dialog>;
}
