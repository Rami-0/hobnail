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

## Installed SDK

`@ui-sdk/react` and `@ui-sdk/core` are pinned to `0.1.0-beta.10` under
D2's explicit age exception (see `web/AGENTS.md` for publication metadata).
The installed docs were read before adapting the wrappers. Imports use
individual entries, only here. `main.tsx` imports `@ui-sdk/core/web.css`
once; the global layer declaration and CSP meta tag remain in place.

| Wrapper | Implementation |
|---|---|
| Button | SDK Button; preserves busy and token styling |
| Dialog | SDK Dialog, Content, Title, Description; Hobnail guard and Back handling |
| Field / TextInput | SDK Field, Label, Description, Error and Input |
| Select | SDK Select, Trigger, Content and Item |
| Switch | SDK Switch; button semantics and controlled checked value |
| Menu | SDK Menu, Trigger, Content and Item |
| Toaster | SDK ToastRegion, Toast, Title, Action and Close; keeps the app's dialog queue |
| TabNav | Platform links: URL navigation uses aria-current, not role=tab |
| Note / Banner / EmptyState | Platform status and content markup |
| ServiceIcon | Platform image / letter avatar; opt-in same-origin icon endpoint |
| Sparkline / HealthStrip / Meter | Platform SVG and meter markup |

No SDK component has been kept on a platform fallback due to a release defect.
No separate tooltip or popover wrapper is currently used by the pages.
The wrappers use the tokens in `src/styles/tokens.css` and the shared
component styles in `src/styles/ui.css`.

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
