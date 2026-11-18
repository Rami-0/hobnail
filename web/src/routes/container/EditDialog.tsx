// The Edit override dialog (static/settings.js OVERRIDE + listDialog,
// static/ui.js HB.result), opened by `?edit=override` on the container page.
// It reads the file's values and hash from GET /api/settings/containers,
// sends only the changed fields with that hash as `base_hash`, and handles
// every answer the way the old dialog does: field errors on their fields
// (422), "Reload and keep my input" under THE RETARGET RULE (409), "Sign in
// (new tab)" and "Retry" (401), the refusal's own words (403). The
// decisions are in override.ts; this file wires them to the form. The
// note's buttons are data (`NoteAct`), drawn with this render's handlers,
// so a Retry always sends what the form holds now.
import { useEffect, useEffectEvent, useId, useRef, useState, type FormEvent } from "react";

import type { ApiResult } from "../../api/client.ts";
import { api, refreshSession } from "../../api/session.ts";
import { Button, Dialog, Field, Note, Select, Switch, TextInput, toast, type MessageKind, type TextInputProps } from "../../ui/index.ts";
import {
  canRetarget,
  changes,
  EMPTY_VALUES,
  hideFromSwitch,
  isDirty,
  outcome,
  readOverride,
  requestBody,
  savedText,
  signInHref,
  validate,
  type OvKey,
  type OvValues,
  type SavedBody,
} from "./override.ts";

export interface EditDialogProps {
  open: boolean;
  /** The container's name (the override's key). */
  name: string;
  /** Its display name, for the messages. */
  title: string;
  /** Closed, saved or not: the page removes `?edit` (replacing the entry). */
  onClosed: () => void;
  /** Saved: the page reads the container again. */
  onSaved: () => void;
}

/** Which buttons the note offers. */
type NoteAct = "none" | "stale" | "signin-save" | "signin-load" | "retry-load" | "stuck";

interface NoteState {
  kind: MessageKind;
  text: string;
  act: NoteAct;
}

type Phase = "loading" | "ready" | "failed";

const SIZES = [
  { value: "", label: "Default" },
  { value: "small", label: "Small" },
  { value: "normal", label: "Normal" },
  { value: "wide", label: "Wide" },
];

async function readFile(name: string): Promise<{ r: ApiResult<unknown>; parsed: ReturnType<typeof readOverride> }> {
  const r = await api.get<unknown>("/api/settings/containers");
  return { r, parsed: r.kind === "ok" ? readOverride(r.data, name) : null };
}

function focusSoon(el: () => HTMLElement | null | undefined): void {
  window.setTimeout(() => el()?.focus(), 0);
}

export function EditDialog({ open, name, title, onClosed, onSaved }: EditDialogProps) {
  const uid = useId();
  const formId = `${uid}-form`;
  const [phase, setPhase] = useState<Phase>("loading");
  const [base, setBase] = useState("");
  const [was, setWas] = useState<OvValues>(EMPTY_VALUES);
  const [values, setValues] = useState<OvValues>(EMPTY_VALUES);
  const [groups, setGroups] = useState<string[]>([]);
  const [errors, setErrors] = useState<Partial<Record<OvKey, string>>>({});
  const [note, setNote] = useState<NoteState | null>(null);
  const [busy, setBusy] = useState(false);
  /** Save refused for good: the entry cannot be re-found after a 409. */
  const [stuck, setStuck] = useState(false);
  const saveRef = useRef<HTMLButtonElement>(null);
  const noteRef = useRef<HTMLDivElement>(null);
  const formRef = useRef<HTMLFormElement>(null);

  const showNote = (n: NoteState | null) => {
    setNote(n);
    if (n) focusSoon(() => noteRef.current?.querySelector<HTMLElement>(".note"));
  };
  const focusField = (k: string | undefined) => {
    if (k) focusSoon(() => formRef.current?.querySelector<HTMLElement>(`[name="${k}"]`));
  };

  // Opening: a fresh form (set while rendering the change), then the file as it is now.
  const [shownOpen, setShownOpen] = useState(false);
  if (open !== shownOpen) {
    setShownOpen(open);
    if (open) {
      setPhase("loading");
      setNote(null);
      setErrors({});
      setStuck(false);
    }
  }
  const applyRead = ({ r, parsed }: Awaited<ReturnType<typeof readFile>>) => {
    if (parsed) {
      setBase(parsed.hash);
      setWas(parsed.values);
      setValues(parsed.values);
      setGroups(parsed.groups);
      setPhase("ready");
      return;
    }
    setPhase("failed");
    if (r.kind === "sign_in") showNote({ kind: "bad", text: "Your session expired. Sign in again in a new tab, then retry.", act: "signin-load" });
    else showNote({ kind: "bad", text: `Could not load the current values: ${r.kind === "ok" ? "unexpected answer" : r.message}.`, act: "retry-load" });
  };
  const load = async () => {
    setPhase("loading");
    setNote(null);
    setErrors({});
    setStuck(false);
    applyRead(await readFile(name));
  };
  const onOpen = useEffectEvent(() => void readFile(name).then(applyRead));
  useEffect(() => {
    if (open) onOpen();
  }, [open]);

  const guard = () => busy || (phase === "ready" && isDirty(values, was));

  const set = (k: OvKey, v: string) => {
    setValues((x) => ({ ...x, [k]: v }));
    if (errors[k]) setErrors((e) => ({ ...e, [k]: undefined }));
  };

  // 409: re-read the file; adopt its hash only under THE RETARGET RULE.
  const reloadKeep = async () => {
    setBusy(true);
    const { parsed } = await readFile(name);
    setBusy(false);
    if (!parsed) {
      showNote({ kind: "bad", text: "Could not load the current file. Your input is still here; try again.", act: "stale" });
      return;
    }
    setGroups(parsed.groups);
    if (canRetarget(was, parsed.values)) {
      setBase(parsed.hash);
      setNote({ kind: "ok", text: "Reloaded the current file. Save again to apply your input on top of it.", act: "none" });
      focusSoon(() => saveRef.current);
      return;
    }
    setStuck(true);
    showNote({
      kind: "bad",
      text: "This container's settings were changed or removed in the file meanwhile, so your input cannot be saved onto them. Copy what you need, discard, and open Edit again.",
      act: "stuck",
    });
  };

  const copyInput = async () => {
    const c = changes(values, was);
    const text = JSON.stringify(c?.set ?? {}, null, 2) + (c?.clear.length ? `\ncleared: ${c.clear.join(", ")}` : "");
    try {
      await navigator.clipboard.writeText(text);
      toast("Copied your input", { kind: "ok" });
    } catch {
      toast("Could not copy: select the fields and copy them by hand.", { kind: "warn" });
    }
  };

  const save = async () => {
    if (busy || stuck || phase !== "ready") return;
    const errs = validate(values, was);
    const n = Object.values(errs).filter(Boolean).length;
    setErrors(errs);
    if (n) {
      setNote({ kind: "bad", text: n === 1 ? "1 field needs attention." : `${n} fields need attention.`, act: "none" });
      focusField(Object.keys(errs)[0]);
      return;
    }
    const c = changes(values, was);
    if (!c) {
      toast("No changes", { kind: "info" });
      onClosed();
      return;
    }
    setNote(null);
    setBusy(true);
    const r = await api.post<SavedBody>("/api/settings/containers/override", requestBody(name, base, c));
    setBusy(false);
    const o = outcome(r);
    switch (o.kind) {
      case "saved":
        onClosed();
        toast(savedText(title, o), { kind: "ok" });
        if (o.warnings.length) toast(o.warnings.join(" "), { kind: "info" });
        onSaved();
        return;
      case "fields":
        setErrors(o.fields);
        setNote({ kind: "bad", text: "Not saved: see the highlighted field.", act: "none" });
        focusField(Object.keys(o.fields)[0]);
        return;
      case "stale":
        showNote({ kind: "warn", text: o.message, act: "stale" });
        return;
      case "signin":
        showNote({ kind: "bad", text: o.message, act: "signin-save" });
        return;
      default:
        showNote({ kind: o.tone, text: o.message, act: "none" });
    }
  };

  // After signing in elsewhere the session has a new CSRF token: read it, then go again.
  const retryAfterSignIn = async (what: "save" | "load") => {
    await refreshSession();
    if (what === "save") await save();
    else await load();
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    void save();
  };

  const signIn = (
    <a className="btn primary sm" data-testid="signin-newtab" href={signInHref(window.location.pathname, window.location.search)} target="_blank" rel="noopener noreferrer">
      Sign in (new tab)
    </a>
  );
  let actions = null;
  switch (note?.act) {
    case "stale":
      actions = (
        <>
          <Button variant="primary" className="sm" data-testid="reload-keep" busy={busy} onClick={() => void reloadKeep()}>
            Reload and keep my input
          </Button>
          <Button className="sm" data-testid="discard" onClick={onClosed}>
            Discard my input
          </Button>
        </>
      );
      break;
    case "signin-save":
    case "signin-load":
      actions = (
        <>
          {signIn}
          <Button className="sm" data-testid="retry" busy={busy} onClick={() => void retryAfterSignIn(note.act === "signin-save" ? "save" : "load")}>
            Retry
          </Button>
        </>
      );
      break;
    case "retry-load":
      actions = (
        <Button className="sm" data-testid="retry" onClick={() => void retryAfterSignIn("load")}>
          Retry
        </Button>
      );
      break;
    case "stuck":
      actions = (
        <>
          <Button className="sm" data-testid="copy-input" onClick={() => void copyInput()}>
            Copy my input
          </Button>
          <Button className="sm" data-testid="discard" onClick={onClosed}>
            Discard my input
          </Button>
        </>
      );
      break;
    default:
      actions = null;
  }

  const field = (k: "name" | "group" | "icon" | "url" | "description", label: string, hint: string, extra: Partial<TextInputProps> = {}) => (
    <Field id={`${uid}-${k}`} label={label} hint={hint} error={errors[k]} wide={k === "icon" || k === "url" || k === "description"}>
      <TextInput
        name={k}
        value={values[k]}
        onChange={(e) => set(k, e.target.value)}
        disabled={phase !== "ready"}
        autoComplete="off"
        spellCheck={false}
        {...extra}
      />
    </Field>
  );

  return (
    <Dialog
      open={open}
      title={`Edit “${name}”`}
      description="How this container shows in Hobnail. Saved to the config file; empty fields use the defaults."
      onClose={() => onClosed()}
      guard={guard}
      testId="edit-dialog"
      footer={
        <>
          <Button
            onClick={(e) => {
              // Cancel is a close request like Esc: the dialog asks first when there is input.
              e.currentTarget.closest('[role="dialog"]')?.dispatchEvent(new Event("cancel", { cancelable: true }));
            }}
          >
            Cancel
          </Button>
          <Button ref={saveRef} type="submit" form={formId} variant="primary" busy={busy} disabled={stuck || phase !== "ready"} data-testid="save">
            Save
          </Button>
        </>
      }
    >
      <form id={formId} ref={formRef} className="ov-form" onSubmit={onSubmit} noValidate aria-busy={phase === "loading" || undefined}>
        <div ref={noteRef} className="dlg-note-slot">
          {note && (
            <Note kind={note.kind} testId="dialog-note" action={actions}>
              {note.text}
            </Note>
          )}
        </div>
        {phase === "loading" && <output className="muted">Loading the current values…</output>}
        <div className="grid-2">
          {field("name", "Display name", "Empty: the compose service or container name.", { maxLength: 80 })}
          {field("group", "Group", "Move it to another group. Empty: its compose project.", { mono: true, list: `${uid}-groups` })}
          {field("icon", "Icon", "A dashboard-icons name (e.g. grafana), an http(s) URL or a file in data/icons/.", { mono: true })}
          {field("url", "URL", "Card link. Empty: the first published port.", { mono: true, inputMode: "url", placeholder: "https://" })}
          {field("description", "Description", "Shown under the name on the overview.", { maxLength: 200 })}
          <Field
            id={`${uid}-hide`}
            label="Hide"
            hint={was.hide === "false" ? "Shown even when a hidden pattern matches." : "Default: shown, unless a hidden pattern matches."}
          >
            <Switch
              name="hide"
              checked={values.hide === "true"}
              onChange={(on) => set("hide", hideFromSwitch(on, was.hide))}
              label="Hide this container"
              disabled={phase !== "ready"}
            />
          </Field>
          <Field id={`${uid}-order`} label="Order" hint="Lower first within the group." error={errors.order}>
            <TextInput name="order" inputMode="numeric" value={values.order} onChange={(e) => set("order", e.target.value)} disabled={phase !== "ready"} />
          </Field>
          <Field id={`${uid}-size`} label="Card size" hint="Accepted for older configs; no effect on these pages." error={errors.size}>
            <Select label="Card size" name="size" value={values.size} onChange={(v) => set("size", v)} options={SIZES} disabled={phase !== "ready"} />
          </Field>
        </div>
        <datalist id={`${uid}-groups`}>
          {groups.map((g) => (
            <option key={g} value={g}>
              {g}
            </option>
          ))}
        </datalist>
      </form>
    </Dialog>
  );
}
