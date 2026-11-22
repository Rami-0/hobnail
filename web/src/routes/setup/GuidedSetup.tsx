import { useEffect, useRef, useState, type ReactNode } from "react";
import type { ApiResult } from "../../api/client.ts";
import { api } from "../../api/session.ts";
import type { IntegrationVerdict } from "../../api/gen/IntegrationVerdict.ts";
import type { SetupCredentialChange } from "../../api/gen/SetupCredentialChange.ts";
import type { SetupDiscovery } from "../../api/gen/SetupDiscovery.ts";
import type { SetupDocument } from "../../api/gen/SetupDocument.ts";
import type { SetupDraft } from "../../api/gen/SetupDraft.ts";
import type { SetupItem } from "../../api/gen/SetupItem.ts";
import type { SetupResult } from "../../api/gen/SetupResult.ts";
import type { SetupTarget } from "../../api/gen/SetupTarget.ts";
import type { SetupUpdate } from "../../api/gen/SetupUpdate.ts";
import { navigate } from "../../app/router.ts";
import { applyTheme, currentTheme, isTheme } from "../../app/theme.ts";
import { origin, payloadIdentity } from "../../boards/document.ts";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { Button } from "../../ui/Button.tsx";
import { Card, CardContent } from "../../ui/Card.tsx";
import { Checkbox } from "../../ui/Checkbox.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { PageHeader } from "../../ui/PageHeader.tsx";
import { SchemaForm, pinField } from "../../configuration/SchemaForm.tsx";
import { connectionValues, hasEndpoint, initial as defaults } from "../../configuration/schema.ts";
import type { Provider } from "../../api/gen/Provider.ts";
import { ThemeSelect } from "../../configuration/ThemeSelect.tsx";
import { incompleteField, move, selectedGroups, stepOrder, STEPS } from "./model.ts";
import "./setup.css";

const PATH = "/api/v1/onboarding";

function openBoard(id: string) {
  window.location.replace(`/app/boards/${id}`);
}

// The draft load runs once per page: Retry loads the page again, and with
// an expired session the reload goes to sign-in and back here.
const retry = () => window.location.reload();

export default function SetupPage() {
  const [draft, setDraft] = useState<SetupDraft>();
  const [error, setError] = useState<Failed>();
  useEffect(() => {
    const controller = new AbortController();
    void api.get<SetupDraft>(`${PATH}/draft`, { signal: controller.signal }).then((response) => {
      if (controller.signal.aborted) return;
      if (response.kind === "ok") {
        if (response.data.board_id) openBoard(response.data.board_id);
        else setDraft(response.data);
      } else setError(response);
    });
    return () => controller.abort();
  }, []);
  if (draft) return <Wizard initial={draft} />;
  return <main id="main" className="page narrow">
    <PageHeader title="Set up Hobnail" />
    {error ? <Note kind="bad" action={<><Button onClick={retry}>{error.kind === "sign_in" ? "Sign in" : "Retry"}</Button>
      <a className="btn btn-link" href="/app">Go to Home</a></>}>
      {error.kind === "error" && error.status === 404 ? "There is no guided setup draft for this account." : error.message}</Note> : <p role="status">Loading setup draft…</p>}
  </main>;
}

const dockerMessage: Record<SetupDiscovery["docker"], string> = {
  reachable: "Docker is reachable. Container names and labels are available.",
  unavailable: "Hobnail cannot reach Docker right now. You can add services manually or continue with the last known suggestions.",
  disabled: "Docker discovery is turned off. You can add services manually.",
  waiting: "Hobnail is waiting for the first response from Docker. You can refresh or add services manually.",
};

function newItem(target: SetupTarget, name: string, group = ""): SetupItem {
  const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), (n) => n.toString(16).padStart(2, "0")).join("");
  return { id, name, group, target, included: true };
}

// `draft`: a failed draft save, holding the step it was saving, which
// "Retry save" saves again.
type Failure = { message: string; stale?: boolean; draft?: SetupDocument["step"]; item?: string; field?: string };
type Failed = Exclude<ApiResult<unknown>, { kind: "ok" }>;

function setupFailure(result: Failed, draft?: SetupDocument["step"]): Failure {
  const [, item, field] = result.kind === "invalid" ? result.field?.split(".") ?? [] : [];
  return { message: result.message, stale: result.kind === "stale", draft, item, field };
}

function Wizard({ initial }: { initial: SetupDraft }) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [catalogError, setCatalogError] = useState("");
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    void api.get<Provider[]>("/api/v1/providers", { signal: controller.signal }).then((result) => {
      if (controller.signal.aborted) return;
      if (result.kind === "ok") {
        setProviders(result.data);
        setCatalogError("");
      } else setCatalogError(result.message);
    });
    return () => controller.abort();
  }, [catalogAttempt]);
  const [draft, setDraft] = useState(initial);
  const [doc, setDoc] = useState(initial.document);
  const [credentials, setCredentials] = useState<Record<string, SetupCredentialChange>>({});
  const [discovery, setDiscovery] = useState<SetupDiscovery>();
  const [discoveryError, setDiscoveryError] = useState("");
  const [scan, setScan] = useState(0);
  const [query, setQuery] = useState("");
  const [saving, setSaving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<Failure>();
  const [verdicts, setVerdicts] = useState<Record<string, IntegrationVerdict | string>>({});
  const [departure, setDeparture] = useState<{ to?: string }>();
  const leaving = useRef(false);
  const savingRef = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const { step } = doc;
  const dirty = doc !== draft.document;
  const credentialPending = Object.keys(credentials).length > 0;
  const stepIndex = stepOrder.indexOf(step);
  const chosenTheme = doc.theme ?? draft.theme_default;

  useEffect(() => {
    if (!credentialPending) return;
    const navigation = (event: Event) => {
      if (leaving.current) return;
      event.preventDefault();
      setDeparture({ to: (event as CustomEvent<string>).detail });
    };
    const unload = (event: BeforeUnloadEvent) => {
      if (leaving.current) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("rh:navigate", navigation);
    window.addEventListener("beforeunload", unload);
    return () => {
      window.removeEventListener("rh:navigate", navigation);
      window.removeEventListener("beforeunload", unload);
    };
  }, [credentialPending]);

  useEffect(() => {
    window.scrollTo({ top: 0, behavior: "instant" });
    heading.current?.focus({ preventScroll: true });
  }, [step]);

  useEffect(() => {
    if (!isTheme(chosenTheme)) return;
    const previous = currentTheme();
    applyTheme(chosenTheme);
    return () => applyTheme(previous);
  }, [chosenTheme]);

  const showDiscovery = step === "discover" || step === "select";
  useEffect(() => {
    if (!showDiscovery) return;
    const controller = new AbortController();
    void api.get<SetupDiscovery>(`${PATH}/discovery`, { signal: controller.signal }).then((response) => {
      if (controller.signal.aborted) return;
      if (response.kind === "ok") {
        setDiscovery(response.data);
        setDiscoveryError("");
      } else setDiscoveryError(response.message);
    });
    return () => controller.abort();
  }, [showDiscovery, scan]);

  async function persist(next: SetupDocument, changes: SetupCredentialChange[] = []) {
    if (savingRef.current) return null;
    if (next === draft.document && !changes.length) return draft;
    savingRef.current = true;
    setSaving(true);
    const body: SetupUpdate = { revision: draft.revision, document: {
      ...next, items: next.items.map((item) => {
        const target = item.target;
        if (target.kind === "board") return item;
        const schema = providers.find((entry) => entry.id === target.provider)?.fields ?? [];
        const { options } = connectionValues(schema, { ...target.options, ...target });
        return { ...item, target: { ...target, options: { ...target.options, ...options } } };
      }),
    }, credentials: changes };
    const response = await api.request<SetupDraft>("PUT", `${PATH}/draft`, { body });
    savingRef.current = false;
    setSaving(false);
    if (response.kind !== "ok") {
      setFailure(setupFailure(response, next.step));
      return null;
    }
    setDraft(response.data);
    // Edits made while an autosave was in flight still need saving.
    setDoc((latest) => latest === doc ? response.data.document : latest);
    if (changes.length) setCredentials((latest) => Object.fromEntries(
      Object.entries(latest).filter(([, entry]) => !changes.includes(entry)),
    ));
    setFailure((current) => current?.item && !current.draft ? current : undefined);
    return response.data;
  }

  useEffect(() => {
    if (!dirty || saving || busy || failure?.draft) return;
    const timer = window.setTimeout(() => { void persist(doc); }, 500);
    return () => window.clearTimeout(timer);
    // The current document and revision are the inputs to the autosave.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc, draft, dirty, saving, busy, failure]);

  function clearItemFailure(id: string) {
    setFailure((current) => current?.item === id ? undefined : current);
  }

  // Whether this render shows the item failure next to its item; when it
  // does not (another step, a field not on screen), the general note does.
  let shown = false;
  function shownError(message?: string) {
    shown ||= !!message;
    return message;
  }

  function fieldError(id: string, field: string) {
    return shownError(failure?.item === id && failure.field === field ? failure.message : undefined);
  }

  function update(id: string, replacement: Partial<SetupItem>) {
    clearItemFailure(id);
    setDoc((previous) => ({ ...previous, items: previous.items.map((item) => item.id === id ? { ...item, ...replacement } : item) }));
  }

  function add(item: SetupItem) {
    setDoc((previous) => ({ ...previous, items: [...previous.items, item] }));
  }

  function changeTarget(item: SetupItem, target: SetupTarget) {
    const changedEndpoint = item.target.kind === "provider" && target.kind === "provider" && item.target.base_url !== target.base_url;
    update(item.id, changedEndpoint ? { target, integration_choice: undefined } : { target });
    setVerdicts(({ [item.id]: _previous, ...others }) => others);
  }

  function itemChanges(id: string) {
    return Object.values(credentials).filter((change) => change.item_id === id);
  }

  function changeSecret(item: SetupItem, field: string, base_url: string, secret: string | null) {
    clearItemFailure(item.id);
    setCredentials(({ [`${item.id}:${field}`]: _previous, ...others }) =>
      secret === "" ? others : { ...others, [`${item.id}:${field}`]: { item_id: item.id, field, base_url, secret } });
    setVerdicts(({ [item.id]: _previous, ...others }) => others);
  }

  async function go(action: "back" | "continue" | "skip") {
    await goTo(move(doc, action));
  }

  async function goTo(next: SetupDocument) {
    setBusy(true);
    await persist(next);
    setBusy(false);
  }

  async function test(item: SetupItem) {
    setBusy(true);
    const current = await persist(doc, itemChanges(item.id));
    if (current) {
      const response = await api.post<IntegrationVerdict>(`${PATH}/test`, { revision: current.revision, item_id: item.id });
      if (response.kind === "invalid" && response.field?.startsWith(`items.${item.id}.`)) {
        setFailure(setupFailure(response));
      } else {
        clearItemFailure(item.id);
        setVerdicts((values) => ({ ...values, [item.id]: response.kind === "ok" ? response.data : response.message }));
      }
    }
    setBusy(false);
  }

  async function create() {
    setBusy(true);
    setFailure(undefined);
    const result = await api.post<SetupResult>(`${PATH}/commit`, { revision: draft.revision });
    if (result.kind === "ok") {
      leaving.current = true;
      openBoard(result.data.board_id);
    } else {
      const problem = setupFailure(result);
      if (result.kind === "error" && result.status >= 500) {
        problem.message = "Hobnail could not create the board. Your setup is saved; try again.";
      }
      if (problem.field === "integration_choice") {
        const latest = await api.get<SetupDraft>(`${PATH}/draft`);
        if (latest.kind === "ok" && latest.data.revision === draft.revision) {
          setDraft({ ...latest.data, document: draft.document });
        }
      }
      setFailure(problem);
      setBusy(false);
    }
  }

  const controlsDisabled = busy || saving || !!failure?.stale;
  const itemLimit = doc.items.length >= 512;
  const known = new Set(doc.items.flatMap(({ target }) => target.kind === "board" ? [payloadIdentity(target.payload)] : []));
  const groups = selectedGroups(doc);
  const connectionFields = ["url", "base_url", "fingerprint", ...providers.flatMap((p) => p.fields.map((f) => f.id))];
  const retryStep = failure?.draft;

  function connection(item: SetupItem) {
    const { target, id } = item;
    const changes = itemChanges(id);
    const result = verdicts[id];
    let content: ReactNode;
    let canTest = true;
    if (target.kind === "provider") {
      const schema = providers.find((entry) => entry.id === target.provider)?.fields;
      if (!schema) return null;
      const bound = draft.credential_bindings[id];
      const values = { ...target.options, ...target, ...Object.fromEntries(changes.map((entry) => [entry.field ?? "", entry.secret])) };
      const endpoint = hasEndpoint(schema);
      const fields = endpoint ? [...schema, pinField] : schema;
      canTest = (!endpoint || !!target.base_url) && target.enabled;
      content = <>
        <SchemaForm id={id} schema={fields} values={values} bound={bound}
          cleared={Object.fromEntries(changes.map((entry) => [entry.field ?? "", entry.secret === null]))}
          errors={Object.fromEntries(fields.map(({ id: key }) => [key, fieldError(id, key) ?? ""]))}
          onChange={(key, value) => {
            if (schema.some((field) => field.id === key && field.kind === "secret")) {
              changeSecret(item, key, target.base_url, String(value));
              return;
            }
            if (key === "fingerprint") {
              changeTarget(item, { ...target, fingerprint: String(value) || null });
              return;
            }
            const base_url = key === "base_url" ? String(value) : target.base_url;
            const enabled = key === "enabled" ? value === true : target.enabled;
            const options = key === "base_url" || key === "enabled" ? target.options : { ...target.options, [key]: value };
            const changedOrigin = origin(base_url) !== origin(target.base_url);
            if (changedOrigin) setCredentials((latest) => Object.fromEntries(Object.entries(latest).filter(([, entry]) => entry.item_id !== id)));
            changeTarget(item, { ...target, base_url, enabled, options,
              fingerprint: changedOrigin ? null : target.fingerprint });
          }} onClear={(key) => changeSecret(item, key, target.base_url, changes.some((entry) => entry.field === key && entry.secret === null) ? "" : null)} />
        {changes.length > 0 && <Button disabled={controlsDisabled} onClick={() => void persist(doc, changes)}>
          {changes.every((entry) => entry.secret === null) ? "Confirm credential removal" : "Save credential"}
        </Button>}
      </>;
    } else {
      const { payload } = target;
      if (payload.kind === "link" || payload.kind === "container") {
        const url = (payload.kind === "link" ? payload.url : payload.link) ?? "";
        canTest = !!url;
        content = <LinkUrl id={id} label="Service URL" error={fieldError(id, "url")} value={url} optional={payload.kind === "container"}
          onChange={(value) => changeTarget(item, { kind: "board", payload: payload.kind === "link"
            ? { ...payload, url: value } : { ...payload, link: value || null } })} />;
      } else if (payload.kind === "integration") {
        content = <p className="muted">This service uses a saved integration. You can edit it in Settings.</p>;
      } else return null;
    }
    return <section className="setup-item" key={id}>
      <h3>{item.name || "Unnamed service"}</h3>
      {content}
      <Button data-testid="setup-test" disabled={controlsDisabled || !canTest} onClick={() => void test(item)}>{changes.length ? "Save and test connection" : "Test connection"}</Button>
      {result && <Note kind={typeof result !== "string" && result.ok ? "ok" : "warn"} testId="setup-verdict">
        {typeof result === "string" ? result : result.message}
        {typeof result !== "string" && result.fingerprint && target.kind !== "board" && <>
          <p className="wrap">Compare the service certificate with <code>{result.fingerprint}</code>.</p>
          <Button onClick={() => changeTarget(item, { ...target, fingerprint: result.fingerprint })}>Confirm and pin fingerprint</Button>
        </>}
      </Note>}
    </section>;
  }

  const connections = step === "connections" ? [...groups.values()].flat().map(connection).filter(Boolean) : [];

  return <main id="main" className="page narrow" data-testid="guided-setup">
    <PageHeader title="Set up Hobnail" />
    <Card>
      <CardContent>
        <p className="muted">Step {stepIndex + 1} of {stepOrder.length}</p>
        <h2 ref={heading} tabIndex={-1} data-testid="setup-step">{STEPS[step]}</h2>
        <fieldset className="setup-fields" disabled={busy || failure?.stale}>
          {step === "appearance" && <>
            <p>Choose a default theme for Hobnail. You can change it later in Settings.</p>
            <ThemeSelect value={doc.theme} fallback={draft.theme_default} onChange={(theme) => setDoc((previous) => ({ ...previous, theme }))} />
          </>}
          {step === "discover" && <>
            <p>Hobnail looks for Docker containers, bookmarks, widgets and saved integrations. You choose what goes on your board.</p>
            {discovery && <>
              <Note kind={discovery.docker === "reachable" ? "info" : "warn"}>{dockerMessage[discovery.docker]}</Note>
              <p>{discovery.config_stale ? "Bookmarks and widgets use the last valid configuration." : "Bookmarks and widgets come from your configuration."}</p>
              <p>{discovery.suggestions.length} suggestions available (up to 256 shown).</p>
            </>}
            {!discovery && !discoveryError && <p role="status">Looking for suggestions…</p>}
            {discoveryError && <Note kind="warn">Discovery failed. You can continue with manual setup. {discoveryError}</Note>}
            <Button onClick={() => setScan((value) => value + 1)}>Refresh suggestions</Button>
          </>}
          {(step === "select" || step === "connections") && catalogError && <Note kind="bad"
            testId="setup-catalog-error" action={<Button onClick={() => setCatalogAttempt((attempt) => attempt + 1)}>Retry</Button>}>
            The integration catalog could not load. {catalogError}
          </Note>}
          {step === "select" && <>
            <p>Include the services you want, then give them names and place them in sections.</p>
            <div className="acts">
              <Button disabled={itemLimit} onClick={() => add(newItem({ kind: "board", payload: { kind: "link", title: "", url: "" } }, "New link"))}>Add a link</Button>
              <Select label="Add an integration" placeholder="Add an integration" value="" disabled={itemLimit}
                options={providers.filter((p) => p.fields.some((f) => f.id === "base_url")).map((provider) => ({ value: provider.id, label: provider.name }))}
                onChange={(id) => {
                  const provider = providers.find((entry) => entry.id === id);
                  if (provider) {
                    const { base_url, enabled, options } = connectionValues(provider.fields, defaults(provider.fields));
                    add(newItem({ kind: "provider", provider: id, base_url, enabled, options }, provider.name));
                  }
                }} />
            </div>
            {doc.items.map((item) => <section className="setup-item" key={item.id} data-testid="setup-selection">
              <Field id={`${item.id}-include`} label={null}
                error={shownError(failure?.item === item.id && !["name", "group", ...connectionFields].includes(failure.field ?? "") ? failure.message : undefined)}>
                <Checkbox checked={item.included} onChange={(included) => update(item.id, { included })}
                  label={`Include ${item.name || "unnamed service"}`} />
              </Field>
              {item.included && <div className="setup-item-fields">
                <Field id={`${item.id}-name`} label="Name" error={fieldError(item.id, "name")}><TextInput value={item.name} maxLength={80} onChange={(e) => update(item.id, { name: e.target.value })} /></Field>
                <Field id={`${item.id}-group`} label="Section" error={fieldError(item.id, "group")} hint="Defaults to Services."><TextInput value={item.group} maxLength={80} onChange={(e) => update(item.id, { group: e.target.value })} /></Field>
              </div>}
            </section>)}
            {!!discovery?.suggestions.length && <>
              <Field id="setup-search" label="Search suggestions"><TextInput value={query} onChange={(e) => setQuery(e.target.value)} /></Field>
              <div className="suggestions">{discovery.suggestions
                .filter(({ title, payload }) => !known.has(payloadIdentity(payload)) && title.toLowerCase().includes(query.trim().toLowerCase()))
                .map(({ title, payload }) => <Checkbox key={payloadIdentity(payload)} checked={false} disabled={itemLimit} label={title}
                  onChange={(checked) => {
                    if (checked) add(newItem({ kind: "board", payload }, title,
                      payload.kind === "container" && payload.target.by === "compose" ? payload.target.project : ""));
                  }} />)}
              </div>
            </>}
          </>}
          {step === "connections" && <>
            <p>Set service URLs or test connections from Hobnail. You can finish setup even when a test fails.</p>
            {connections}
            {!connections.length && !catalogError && <p className="muted">{!providers.length && [...groups.values()].flat().some((item) => item.target.kind === "provider")
              ? "Loading connection fields…" : "There are no selected services with connection settings."}</p>}
          </>}
          {step === "review" && <>
            <p>Hobnail will create a board named Home and make it your landing page.</p>
            {credentialPending && <Note kind="warn">Only saved credentials are included. Go back to save or clear any remaining credential entries.</Note>}
            {groups.size === 0 ? <p>Your board will be empty. Use Add or Discover when you are ready.</p> :
              [...groups].map(([group, items]) => <section key={group}>
                <h3>{group}</h3>
                <ul>{items.map((item) => {
                  const provider = item.target.kind === "provider" ? item.target.provider : "";
                  const fields = providers.find((entry) => entry.id === provider)?.fields;
                  const incomplete = incompleteField(item, !fields || hasEndpoint(fields));
                  const error = failure?.item === item.id ? failure : undefined;
                  shown ||= !!error;
                  const matchId = draft.integration_matches[item.id];
                  const matches = matchId ? draft.matching_integrations[matchId] : undefined;
                  const matchError = error?.field === "integration_choice";
                  const fix = matchError ? (!matches ? "base_url" : undefined) : error?.field ?? incomplete;
                  return <li key={item.id}>{item.name || "Unnamed service"}
                    {item.target.kind === "provider" ? " · Integration" : item.target.payload.kind === "link" ? " · Link" : ""}
                    {incomplete && " · incomplete — will not be added"}
                    {matches && <Field id={`${item.id}-match`} label={`Connection for ${item.name}`}
                      hint={`Matches existing ${matches.map(({ name }) => name).join(", ")}. Reuse keeps the saved connection and its credential.`}
                      error={matchError ? error.message : undefined}>
                      <Select label={`Connection for ${item.name}`} value={item.integration_choice ?? ""} placeholder="Choose a connection"
                        disabled={controlsDisabled} onChange={(integration_choice) => update(item.id, { integration_choice })}
                        options={[...matches.map(({ id, name }) => ({ value: id, label: `Reuse ${name}` })), { value: "new", label: "Add anyway" }]} />
                    </Field>}
                    {error && (!matchError || !matches) && <p className="bad-fg" role="alert">{error.message}</p>}
                    {fix && <Button variant="link" disabled={controlsDisabled} onClick={() => void goTo({
                      ...doc, step: connectionFields.includes(fix) ? "connections" : "select",
                    })}>Fix {item.name || "unnamed service"}</Button>}
                  </li>;
                })}</ul>
              </section>)}
          </>}
        </fieldset>
        {failure && (!failure.item || !shown) && <Note kind="bad" action={failure.stale ? <Button onClick={() => window.location.reload()}>Reload saved draft</Button>
          : retryStep ? <Button disabled={controlsDisabled} onClick={() => void goTo({ ...doc, step: retryStep })}>Retry save</Button>
          // Otherwise board creation failed (the commit is safe to repeat).
          : <Button disabled={controlsDisabled || dirty} onClick={() => void create()}>Retry</Button>}>{failure.message}</Note>}
        <div className="acts setup-actions">
          <Button data-testid="setup-back" disabled={controlsDisabled || stepIndex === 0} onClick={() => void go("back")}>Back</Button>
          {step !== "review" && <Button data-testid="setup-skip" disabled={controlsDisabled} onClick={() => void go("skip")}>Skip</Button>}
          {step === "review"
            ? <Button data-testid="setup-create" variant="default" busy={busy} disabled={controlsDisabled || dirty} onClick={() => credentialPending ? setDeparture({}) : void create()}>Create board</Button>
            : <Button data-testid="setup-continue" variant="default" busy={busy} disabled={controlsDisabled} onClick={() => void go("continue")}>Continue</Button>}
          <span className="muted" role="status">
            {saving ? "Saving draft…" : dirty ? "Draft has unsaved changes" : credentialPending ? "" : "Draft saved"}
            {credentialPending && `${saving || dirty ? " · " : ""}Credential not saved`}
          </span>
        </div>
      </CardContent>
    </Card>
    <Dialog open={departure !== undefined} testId="setup-leave" title="Discard unsaved credential changes?"
      onClose={() => setDeparture(undefined)}
      footer={<><Button onClick={() => setDeparture(undefined)}>Keep editing</Button>
        <Button variant="destructive" disabled={controlsDisabled && !failure?.stale} onClick={() => {
          const to = departure?.to;
          setDeparture(undefined);
          if (to) {
            leaving.current = true;
            setCredentials({});
            navigate(to);
            if (new URL(to).origin === window.location.origin) leaving.current = false;
          } else void create();
        }}>{departure?.to ? "Discard and leave" : "Discard and create board"}</Button></>}>
      <p>Credential changes in this tab will be lost.</p>
    </Dialog>
  </main>;
}
