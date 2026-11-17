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

## Phase 0 contents

- `Button.tsx`: variants `primary`, `secondary` (default), `ghost`; `busy`
  (spinner, `aria-busy` + `aria-disabled`, keeps focus); `--ctl-h` height,
  44 px on touch screens.
- `index.ts`: the only import path for the app.

Nothing else yet. Add a primitive here when a page needs it, not before.
