import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { MinusIcon, PlusIcon, Trash2Icon, XIcon } from "lucide-react";
import type { Accent } from "../../api/gen/Accent.ts";
import type { BoardDocument } from "../../api/gen/BoardDocument.ts";
import type { BoardItem } from "../../api/gen/BoardItem.ts";
import type { BoardPayload } from "../../api/gen/BoardPayload.ts";
import type { TileLook } from "../../api/gen/TileLook.ts";
import { DEFAULT_APPEARANCE, rectsOf, type Action } from "../../boards/document.ts";
import { resizeTo, type Size } from "../../boards/grid.ts";
import { LinkTest } from "../../boards/LinkTest.tsx";
import { LinkUrl } from "../../boards/LinkUrl.tsx";
import { Button } from "../../ui/Button.tsx";
import { Field, TextInput } from "../../ui/Field.tsx";
import { Select } from "../../ui/Select.tsx";
import { ServiceIcon } from "../../ui/ServiceIcon.tsx";
import { Switch } from "../../ui/Switch.tsx";
import { ACCENT_NAMES, ACCENTS, BACKGROUNDS, DENSITIES, SURFACES } from "./look.ts";

type Change = (action: Action, message: string, merge?: string) => void;

/** A non-modal side panel: the canvas stays visible and live beside it.
 * Focus moves in on open; Escape or Close returns it to the opener. */
function Side({ title, testId, onClose, children }: { title: string; testId: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  useEffect(() => { close.current = onClose; });
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("input, button:not(.inspector-close)")?.focus({ preventScroll: true });
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !ref.current?.contains(document.activeElement) || document.querySelector('[role="listbox"]')) return;
      e.stopPropagation(); close.current();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);
  return <aside ref={ref} className="inspector" role="region" aria-label={title} data-testid={testId}>
    <header className="inspector-head"><h2>{title}</h2>
      <Button variant="ghost" size="icon" className="inspector-close" aria-label="Close" title="Close (Esc)" onClick={onClose}><XIcon aria-hidden="true" /></Button></header>
    <div className="inspector-body">{children}</div>
  </aside>;
}

function Swatches({ label, value, onChange }: { label: string; value: Accent | undefined; onChange: (accent: Accent | undefined) => void }) {
  return <div className="inspector-group" role="group" aria-label={label}><span className="inspector-label">{label}</span>
    <div className="swatches">
      <Button variant="ghost" size="icon" className="swatch swatch-none" aria-pressed={!value} aria-label="No accent" title="None" onClick={() => onChange(undefined)}><span aria-hidden="true" /></Button>
      {ACCENT_NAMES.map((name) => <Button key={name} variant="ghost" size="icon" className="swatch" aria-pressed={value === name} aria-label={ACCENTS[name].label} title={ACCENTS[name].label}
        style={{ "--swatch": ACCENTS[name].color } as CSSProperties} onClick={() => onChange(name)}><span aria-hidden="true" /></Button>)}
    </div></div>;
}

function Segmented<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: { value: T; label: string }[]; onChange: (value: T) => void }) {
  return <div className="inspector-group" role="group" aria-label={label}><span className="inspector-label">{label}</span>
    <div className="segmented">{options.map((option) => <Button key={option.value} variant="ghost" size="sm" aria-pressed={value === option.value} onClick={() => onChange(option.value)}>{option.label}</Button>)}</div>
  </div>;
}

export function BoardPanel({ doc, fallbackName, icons, change, onClose }: { doc: BoardDocument; fallbackName: string; icons: boolean; change: Change; onClose: () => void }) {
  const appearance = doc.appearance ?? DEFAULT_APPEARANCE;
  const name = doc.name ?? fallbackName;
  const set = (patch: Partial<typeof appearance>, message: string) => {
    const next = { ...appearance, ...patch };
    if (!next.accent) delete next.accent;
    change({ type: "board", appearance: next }, message);
  };
  return <Side title="Board style" testId="board-panel" onClose={onClose}>
    <Field id="board-name" label="Name"><TextInput value={name} required maxLength={80} onChange={(e) => change({ type: "board", name: e.target.value, icon: doc.icon ?? null }, "Board renamed", "board-name")} /></Field>
    <Field id="board-icon" label="Icon" hint="A dashboard-icons name, e.g. homarr">
      <div className="icon-field"><ServiceIcon className="board-avatar" tint name={doc.icon} title={name} enabled={icons} />
        <TextInput value={doc.icon ?? ""} maxLength={96} onChange={(e) => change({ type: "board", name, icon: e.target.value || null }, "Board icon updated", "board-icon")} /></div>
    </Field>
    <div className="inspector-group" role="group" aria-label="Background"><span className="inspector-label">Background</span>
      <div className="bg-swatches">{BACKGROUNDS.map((bg) => <Button key={bg.value} variant="ghost" className="bg-swatch" data-bg={bg.value} aria-pressed={appearance.background === bg.value}
        onClick={() => set({ background: bg.value }, `Background: ${bg.label}`)}><span className="bg-swatch-preview" aria-hidden="true" />{bg.label}</Button>)}</div>
    </div>
    <Segmented label="Density" value={appearance.density} options={DENSITIES} onChange={(density) => set({ density }, `Density: ${density}`)} />
    <Segmented label="Tile style" value={appearance.surface} options={SURFACES} onChange={(surface) => set({ surface }, `Tile style: ${surface}`)} />
    <Swatches label="Accent" value={appearance.accent} onChange={(accent) => set({ accent }, accent ? `Accent: ${ACCENTS[accent].label}` : "No accent")} />
  </Side>;
}

const title = (payload: BoardPayload) => payload.kind === "link" ? payload.title : payload.title ?? "";
const plain: TileLook = { hide_status: false, hide_metrics: false };

export function TilePanel({ doc, s, i, item, title: shown, min, scoped, change, onMove, onRemove, onClose }: {
  doc: BoardDocument; s: number; i: number; item: BoardItem; title: string; min: Size; scoped: boolean;
  change: Change; onMove: (section: number) => void; onRemove: () => void; onClose: () => void;
}) {
  const p = item.payload;
  const look = item.look ?? plain;
  const at = { section: s, item: i };
  const setLook = (patch: Partial<TileLook>, message: string, merge?: string) => {
    const next: TileLook = { ...look, ...patch };
    for (const k of ["accent", "icon", "link"] as const) if (!next[k]) delete next[k];
    change({ type: "look", ...at, look: next }, message, merge);
  };
  const setPayload = (payload: BoardPayload, message: string, merge: string) => change({ type: "payload", ...at, payload }, message, merge);
  const setTitle = (value: string) => {
    if (p.kind === "link") return setPayload({ ...p, title: value }, "Title updated", "title");
    if (p.kind === "container") return setPayload({ ...p, title: value || null }, "Title updated", "title");
    const { title: _old, ...rest } = p;
    setPayload(value ? { ...rest, title: value } : rest, "Title updated", "title");
  };
  const icon = p.kind === "container" || p.kind === "link" ? p.icon ?? "" : look.icon ?? "";
  const setIcon = (value: string) => p.kind === "container" || p.kind === "link" ? setPayload({ ...p, icon: value || null }, "Icon updated", "icon") : setLook({ icon: value }, "Icon updated", "icon");
  const linkValue = p.kind === "container" ? p.link ?? "" : p.kind === "link" ? p.url : look.link ?? "";
  const setLink = (value: string) => p.kind === "container" ? setPayload({ ...p, link: value || null }, "Link updated", "link") : p.kind === "link" ? setPayload({ ...p, url: value }, "URL updated", "link") : setLook({ link: value }, "Link updated", "link");
  const rect = rectsOf(doc.sections[s])[i];
  const size = (dw: number, dh: number) => {
    if (!rect) return;
    const next = resizeTo(rectsOf(doc.sections[s]), i, rect.w + dw, rect.h + dh, min);
    const r = next[i] ?? rect;
    change({ type: "layout", section: s, rects: next }, `Resized to ${r.w} by ${r.h}`);
  };
  return <Side title={`Customize ${shown}`} testId="board-item-editor" onClose={onClose}>
    <Field id="item-title" label={p.kind === "link" ? "Title" : "Custom title"}><TextInput value={title(p)} required={p.kind === "link"} maxLength={80} placeholder={p.kind === "link" ? "" : shown} onChange={(e) => setTitle(e.target.value)} /></Field>
    <Field id="item-icon" label="Icon" hint="A dashboard-icons name, e.g. jellyfin"><TextInput value={icon} maxLength={96} onChange={(e) => setIcon(e.target.value)} /></Field>
    {p.kind === "link" ? <LinkUrl id="item-url" value={linkValue} onChange={setLink} />
      : <LinkUrl id="item-link" label="Link" optional value={linkValue} onChange={setLink} />}
    {p.kind === "link" && !scoped && <LinkTest url={p.url} />}
    <Swatches label="Accent" value={look.accent} onChange={(accent) => setLook({ accent }, accent ? `Accent: ${ACCENTS[accent].label}` : "No accent")} />
    {p.kind !== "link" && <div className="inspector-group" role="group" aria-label="Show">
      <span className="inspector-label">Show</span>
      <Switch checked={!look.hide_status} label="Status" onChange={(on) => setLook({ hide_status: !on }, on ? "Status shown" : "Status hidden")} />
      {(p.kind === "container" || p.kind === "integration") && <Switch checked={!look.hide_metrics} label={p.kind === "container" ? "Metrics and history" : "Connection evidence"} onChange={(on) => setLook({ hide_metrics: !on }, on ? "Metrics shown" : "Metrics hidden")} />}
    </div>}
    {rect && <div className="inspector-group" role="group" aria-label="Size"><span className="inspector-label">Size <span className="muted">{rect.w} × {rect.h} cells</span></span>
      <div className="size-steppers">
        <span>Width</span><Button variant="outline" size="icon" aria-label="Narrower" disabled={rect.w <= min.w} onClick={() => size(-1, 0)}><MinusIcon aria-hidden="true" /></Button><Button variant="outline" size="icon" aria-label="Wider" disabled={rect.w >= 12} onClick={() => size(1, 0)}><PlusIcon aria-hidden="true" /></Button>
        <span>Height</span><Button variant="outline" size="icon" aria-label="Shorter" disabled={rect.h <= min.h} onClick={() => size(0, -1)}><MinusIcon aria-hidden="true" /></Button><Button variant="outline" size="icon" aria-label="Taller" disabled={rect.h >= 16} onClick={() => size(0, 1)}><PlusIcon aria-hidden="true" /></Button>
      </div></div>}
    {doc.sections.length > 1 && <Select label="Section" value={String(s)} options={doc.sections.map((entry, index) => ({ value: String(index), label: entry.title || `Section ${index + 1}` }))}
      onChange={(value) => { const target = Number(value); if (target !== s) onMove(target); }} />}
    <Button variant="destructive" className="inspector-remove" icon={<Trash2Icon aria-hidden="true" />} onClick={onRemove}>Remove tile</Button>
  </Side>;
}
