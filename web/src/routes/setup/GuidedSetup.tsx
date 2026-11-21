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
import { payloadIdentity } from "../../boards/document.ts";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { Button } from "../../ui/Button.tsx";
import { Card, CardContent } from "../../ui/Card.tsx";
import { Checkbox } from "../../ui/Checkbox.tsx";
import { Dialog } from "../../ui/Dialog.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Note } from "../../ui/Note.tsx";
import { Select } from "../../ui/Select.tsx";
import { PageHeader } from "../../ui/PageHeader.tsx";
import { HttpConnectionFields } from "../../configuration/HttpConnectionFields.tsx";
import { ThemeSelect } from "../../configuration/ThemeSelect.tsx";
import { incompleteField, move, selectedGroups, stepOrder, STEPS } from "./model.ts";
import "./setup.css";

const PATH = "/api/v1/onboarding";

function openBoard(id: string) {
  window.location.replace(`/app/boards/${id}`);
}

export default function SetupPage() {
  const [draft, setDraft] = useState<SetupDraft>();
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    void api.get<SetupDraft>(`${PATH}/draft`, { signal: controller.signal }).then((response) => {
      if (controller.signal.aborted) return;
      if (response.kind === "ok") {
        if (response.data.board_id) openBoard(response.data.board_id);
        else setDraft(response.data);
      } else setError(response.kind === "error" && response.status === 404 ? "There is no guided setup draft for this account." : response.message);
    });
    return () => controller.abort();
  }, []);
  if (draft) return <Wizard initial={draft} />;
  return <main id="main" className="page narrow">
    <PageHeader title="Set up Hobnail" />
    {error ? <Note kind="bad" action={<a className="btn btn-link" href="/app">Go to Home</a>}>{error}</Note> : <p role="status">Loading setup draft…</p>}
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

type Failure = { message: string; stale?: boolean; draft?: boolean; item?: string; field?: string };

function setupFailure(result: Exclude<ApiResult<unknown>, { kind: "ok" }>, draft = false): Failure {
  const [, item, field] = result.kind === "invalid" ? result.field?.split(".") ?? [] : [];
  return { message: result.message, stale: result.kind === "stale", draft, item, field };
}

function Wizard({ initial }: { initial: SetupDraft }) {
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
    const body: SetupUpdate = { revision: draft.revision, document: next, credentials: changes };
    const response = await api.request<SetupDraft>("PUT", `${PATH}/draft`, { body });
    savingRef.current = false;
    setSaving(false);
    if (response.kind !== "ok") {
      setFailure(setupFailure(response, true));
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

  function fieldError(id: string, field: string) {
    return failure?.item === id && failure.field === field ? failure.message : undefined;
  }

  function update(id: string, replacement: Partial<SetupItem>) {
    clearItemFailure(id);
    setDoc((previous) => ({ ...previous, items: previous.items.map((item) => item.id === id ? { ...item, ...replacement } : item) }));
  }

  function add(item: SetupItem) {
    setDoc((previous) => ({ ...previous, items: [...previous.items, item] }));
  }

  function changeTarget(item: SetupItem, target: SetupTarget) {
    const changedEndpoint = item.target.kind === "http" && target.kind === "http" && item.target.base_url !== target.base_url;
    update(item.id, changedEndpoint ? { target, integration_choice: undefined } : { target });
    setVerdicts(({ [item.id]: _previous, ...others }) => others);
  }

  function changeSecret(item: SetupItem, base_url: string, secret: string | null) {
    clearItemFailure(item.id);
    setCredentials(({ [item.id]: _previous, ...others }) =>
      secret === "" ? others : { ...others, [item.id]: { item_id: item.id, base_url, secret } });
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
    const change = credentials[item.id];
    const current = await persist(doc, change ? [change] : []);
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

  function connection(item: SetupItem) {
    const { target, id } = item;
    const change = credentials[id];
    const result = verdicts[id];
    let content: ReactNode;
    let canTest = true;
    if (target.kind === "http") {
      const bound = change?.secret === null ? null : draft.credential_origins[id];
      canTest = !!target.base_url && target.enabled;
      content = <>
        <HttpConnectionFields id={id} url={target.base_url} enabled={target.enabled} bound={bound}
          secret={change?.secret ?? ""} busy={busy} fields={{ base_url: fieldError(id, "base_url") ?? "", secret: fieldError(id, "secret") ?? "" }}
          setSecret={(secret) => changeSecret(item, target.base_url, secret)}
          setUrl={(base_url) => changeTarget(item, { ...target, base_url })}
          setEnabled={(enabled) => changeTarget(item, { ...target, enabled })}
          clearSecret={() => changeSecret(item, target.base_url, null)} />
        {change && <Button disabled={controlsDisabled} onClick={() => void persist(doc, [change])}>
          {change.secret === null ? "Confirm credential removal" : "Save credential"}
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
      <Button data-testid="setup-test" disabled={controlsDisabled || !canTest} onClick={() => void test(item)}>Test connection</Button>
      {result && <Note kind={typeof result !== "string" && result.ok ? "ok" : "warn"} testId="setup-verdict">
        {typeof result === "string" ? result : result.message}
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
          {step === "select" && <>
            <p>Include the services you want, then give them names and place them in sections.</p>
            <div className="acts">
              <Button disabled={itemLimit} onClick={() => add(newItem({ kind: "board", payload: { kind: "link", title: "", url: "" } }, "New link"))}>Add a link</Button>
              <Button disabled={itemLimit} onClick={() => add(newItem({ kind: "http", base_url: "", enabled: true }, "New HTTP integration"))}>Add an HTTP integration</Button>
            </div>
            {doc.items.map((item) => <section className="setup-item" key={item.id} data-testid="setup-selection">
              <Field id={`${item.id}-include`} label={null}
                error={failure?.item === item.id && !["name", "group", "url", "base_url", "secret"].includes(failure.field ?? "") ? failure.message : undefined}>
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
            {connections.length
              ? connections
              : <p className="muted">There are no selected services with connection settings.</p>}
          </>}
          {step === "review" && <>
            <p>Hobnail will create a board named Home and make it your landing page.</p>
            {credentialPending && <Note kind="warn">Only saved credentials are included. Go back to save or clear any remaining credential entries.</Note>}
            {groups.size === 0 ? <p>Your board will be empty. Use Add or Discover when you are ready.</p> :
              [...groups].map(([group, items]) => <section key={group}>
                <h3>{group}</h3>
                <ul>{items.map((item) => {
                  const incomplete = incompleteField(item);
                  const error = failure?.item === item.id ? failure : undefined;
                  const matchId = draft.integration_matches[item.id];
                  const matches = matchId ? draft.matching_integrations[matchId] : undefined;
                  const matchError = error?.field === "integration_choice";
                  const fix = matchError ? (!matches ? "base_url" : undefined) : error?.field ?? incomplete;
                  return <li key={item.id}>{item.name || "Unnamed service"}
                    {item.target.kind === "http" ? " · HTTP integration" : item.target.payload.kind === "link" ? " · Link" : ""}
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
                      ...doc, step: ["url", "base_url", "secret"].includes(fix) ? "connections" : "select",
                    })}>Fix {item.name || "unnamed service"}</Button>}
                  </li>;
                })}</ul>
              </section>)}
          </>}
        </fieldset>
        {failure && !failure.item && <Note kind="bad" action={failure.stale ? <Button onClick={() => window.location.reload()}>Reload saved draft</Button>
          : failure.draft ? <Button disabled={controlsDisabled} onClick={() => void persist(doc)}>Retry save</Button> : undefined}>{failure.message}</Note>}
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
