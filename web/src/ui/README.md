# src/ui: the wrapper folder

Every interactive primitive (button, dialog, select, tabs, switch, menu,
toast, tooltip, popover, checkbox, radio, text field …) is imported by the
app **only from `src/ui/`** (`import { Button } from "../ui/index.ts"`),
never from a component library directly. `scripts/guard.mjs` fails the check
when a file outside this folder imports `@ui-sdk/…` or another UI primitive
library.

Why: one place decides how a primitive looks, which tokens it uses, how it
behaves under CSP, and which library (if any) implements it. Swapping or
upgrading the library touches this folder only.

## UI SDK is not installed yet

The decision (docs/decisions/react-client-and-dashboard.md §1) names UI SDK
(`@ui-sdk/react`, `@ui-sdk/core`) for these primitives. It is **not
installed**: on 2026-11-17 every published version was younger than 7 days
(oldest `0.1.0-beta.0`, published 2026-11-12; `0.1.0-beta.6`, the `latest`
tag, published 2026-11-16), and the release-age rule (owner decision D3,
`min-release-age=7` in `web/.npmrc`) allows no exception.

When it is installed (with the lead, once a version is old enough):

- these wrappers switch to its documented per-component entries
  (`@ui-sdk/react/<component>`, e.g. `@ui-sdk/react/dialog`; there is no
  root import) and keep their own props, so callers do not change;
- `@ui-sdk/core/web.css` is imported once, in `src/main.tsx`;
  `src/styles/global.css` already declares `@layer ui-sdk;` first;
- `index.html` already carries `<meta property="csp-nonce" />`, which makes
  UI SDK use constructed style sheets instead of `<style>` elements
  (needed under `style-src 'self'`, plan 4.3);
- read `web/AGENTS.md` (UI SDK reading instructions) before writing a new
  wrapper.

## Contents (Phase 1)

Hand-written on platform elements (UI SDK is not installed), with the props
below; pages use exactly these. Styles: `src/styles/ui.css`. Each row names
the UI SDK entry that replaces the hand-written part once a version passes
the release-age rule; the props stay, so callers do not change. Entries are
the SDK's documented per-component imports (`@ui-sdk/react/<component>`);
check the installed `docs/llms.txt` for the exact names before switching.

| Wrapper | Props | Built on | Replaced by (when installable) |
|---|---|---|---|
| `Button` | `variant` (`primary`/`secondary`/`ghost`), `busy`, `icon`, `ref`, button props | `<button>` | `@ui-sdk/react/button` |
| `Dialog` | `open`, `title`, `description?`, `onClose(reason: "escape"\|"cancel"\|"backdrop"\|"done")`, `guard?()`, `footer?`, `children`, `initialFocus?`, `testId?`, `wide?`, `className?` | `<dialog>` + `showModal()` | `@ui-sdk/react/dialog` (the guard bar and Back handling stay ours) |
| `Field` | `label`, `hint?`, `error?`, `children` (the control), `id`, `wide?` | `<label>` + context | `@ui-sdk/react/field` |
| `TextInput` | input props, `mono?`, `ref` (id / describedby / invalid from the Field) | `<input>` | `@ui-sdk/react/input` |
| `Select` | `label`, `value`, `onChange(value)`, `options: {value,label}[]`, `hideLabel?`, select props | native `<select>` | `@ui-sdk/react/select` (keep native on touch) |
| `Switch` | `checked`, `onChange(checked)`, `label`, `busy?`, input props | `<input type=checkbox role=switch>` | `@ui-sdk/react/switch` |
| `Note` | `kind` (`ok`/`warn`/`bad`/`info`), `children`, `action?`, `testId?`, `id?` | `<div role=status\|alert>` | stays (no SDK equivalent needed) |
| `Banner` | `kind`, `children`, `action?`, `testId?` | `<div role=status\|alert>` | stays |
| `EmptyState` | `icon`, `title`, `children?`, `tone?` (`good`/`bad`), `actions?`, `compact?`, `testId?` | markup | stays |
| `toast(text, {kind?, action?, ms?, sticky?})`, `<Toaster/>` | three at most; "bad" stays until closed; queued while a dialog is open | `toast.ts` store + `<output>` region | `@ui-sdk/react/toast` (keep the dialog queue) |
| `Menu` | `label`, `trigger`, `items: {label, onSelect, icon?, testId?, danger?}[]`, `header?`, `testId?`, `triggerClassName?`, `align?` | `<button aria-haspopup=menu>` + list | `@ui-sdk/react/menu` |
| `TabNav` | `label`, `items: {id, label, href, extra?}[]`, `current`, `testId?` | links, `aria-current="page"` | stays (URL tabs are links, not role=tab) |
| `Sparkline` | `values: (number\|null)[]`, `max?`, `label` ("" hides it), `width?`, `height?`, `color?` (a token name), `className?` | plain SVG (`svg.ts`) | stays (no Recharts here) |
| `HealthStrip` | `codes: number[]`, `label`, `step?` (s per cell, 900), `cells?`, `className?` | plain SVG (`svg.ts`) | stays |
| `Meter` | `value`, `max`, `label`, `className?` | `<div role=meter>` | stays |

Behaviour notes:

- **Dialog.** Focus moves to `initialFocus`, else the body's first control,
  and returns to the opener on close. Esc, Close, a backdrop click and Back
  ask `guard()` first; with unsaved input the footer becomes "Discard your
  changes? Keep editing / Discard". The dialog adds no history entry: a
  page whose URL is the dialog (`?edit=…`) has one already. On Back with
  input it restores its URL and asks; without, `onClose("cancel")` runs
  after the URL changed (remove `edit` with `replace`, never go back
  again). Ctrl/⌘ Enter submits the first `<form>` inside; footer buttons
  submit it with `form="<id>"`. Toasts wait while any dialog is open
  (`layers.ts`).
- **CSP.** No inline style attributes in markup, no injected `<style>`:
  React's `style` prop (CSSOM) is used only for computed values (a meter's
  width, a sparkline's `--s` colour).
- **Touch.** 44 px targets and 16 px input text come from the tokens
  (`--ctl-h`, `--ia`, `--hit`, `--fs-input` in tokens.css).
- Pure logic is in `svg.ts` and `toast.ts`, unit-tested by `*.test.ts`.
