# web/: the Hobnail React client

Read the repository's own instructions first (`../AGENTS.md` / `../CLAUDE.md`
if present), then this file. Binding documents:
`../docs/decisions/react-client-and-dashboard.md` (the decision) and
`../docs/plans/react-port.md` (the plan). Do not edit either from here.

React 19, TypeScript, Vite. The Rust server embeds `web/dist` and serves it:
`/app`, `/app/*` → `index.html`; `/assets/*` → hashed files; `/api/v1/*` →
the JSON API. Same origin, no Node in production.

## Layout

| Path | What |
|---|---|
| `index.html` | the document; no inline script or style |
| `src/main.tsx` | entry: styles, stored theme, session, React root |
| `src/app/` | shell, router (`paths.ts` pure, `router.ts` History API), theme |
| `src/routes/` | one component per page |
| `src/api/client.ts` | the only fetch wrapper: typed `ApiResult`, CSRF, retry |
| `src/api/session.ts` | `GET /api/v1/session` store, `useSession()`, the `api` client |
| `src/api/gen/` | TypeScript types generated from Rust by ts-rs; never edit by hand (`RH_UPDATE_TYPES=1 scripts/cargo.sh test` rewrites them) |
| `src/ui/` | the wrapper folder for interactive primitives (see below) |
| `src/charts/` | the only place that may import `recharts` (when charts arrive) |
| `src/styles/` | `global.css` (imported once by main.tsx), `tokens.css` (tokens and themes) |
| `scripts/guard.mjs` | source guard run by `npm run check` |

## Rules

- **Wrapper folder.** Every interactive primitive (button, dialog, select,
  tabs, switch, menu, toast, tooltip …) comes from `src/ui/` (`import {
  Button } from "../ui/index.ts"`), never from a library directly. Only
  `src/ui/` may import `@ui-sdk/…` or any other primitive library; only
  `src/main.tsx` imports `@ui-sdk/core/web.css`. `scripts/guard.mjs` enforces
  it. See `src/ui/README.md`.
- **CSP.** The document is served with `script-src 'self'; style-src 'self'`
  (plan 4.3). So: no inline `<script>` or `<style>` in `index.html`, no
  `dangerouslySetInnerHTML`, no `.innerHTML`/`.outerHTML`/`insertAdjacentHTML`,
  no `eval(`, no `new Function(`, no `document.write`. React's `style={…}`
  prop sets CSSOM properties and is allowed by CSP, but prefer classes and
  tokens. Keep `<meta property="csp-nonce" />` in `index.html` (UI SDK's
  switch to constructed style sheets).
- **Styles.** Hand-written CSS on the tokens in `src/styles/tokens.css`
  (ported from `../static/app.css`; keep both in step until the old UI goes).
  No Tailwind. `global.css` starts with `@layer ui-sdk;`. Themes are
  `<html data-theme>` (`auto` | `dark` | `light` | `ember` | `contrast`),
  stored in `localStorage["rh-theme"]`, the key the old UI uses.
- **Imports.** Relative imports carry the explicit `.ts`/`.tsx` extension
  (Node's test runner strips types but does not resolve extensionless
  paths). Type-only imports use `import type` (`verbatimModuleSyntax`).
  Only erasable TypeScript (no `enum`, no parameter properties).
- **Tests** run with `node --test` and type stripping: `src/**/*.test.ts`,
  no JSX, no DOM. Keep logic worth testing in plain `.ts` modules
  (`paths.ts`, `theme.ts`, `client.ts`) and keep components thin.
- **Dependencies: release-age policy** (owner decision D3). `web/.npmrc` sets
  `min-release-age=7` and `ignore-scripts=true`; versions are exact and
  `package-lock.json` is committed. No new dependency without the lead, and
  no exception to the age rule: if it blocks an install, stop and ask.
  Check the locked set with `scripts/web.sh node scripts/ages.mjs`.

## Commands (from the repository root; Node runs in a container)

```sh
scripts/web.sh ci                 # install exactly the lock (if node_modules is missing)
scripts/web.sh run check          # tsc --noEmit + oxlint (no warnings) + scripts/guard.mjs
scripts/web.sh run test           # unit tests (node --test)
scripts/web.sh run build          # vite build + gzip copies -> web/dist
scripts/web.sh run size           # bundle sizes
```

Dev server against a running Hobnail (the proxy target has no default; the
config refuses to start without `RH_DEV_BACKEND`):

```sh
RH_WEB_NET=host scripts/web.sh exec -c \
  'RH_DEV_BACKEND=http://127.0.0.1:3210 vite --port 5173 --strictPort'
# then open http://localhost:5173/app
```

`/api`, `/icons`, `/custom.css`, `/manifest.webmanifest`, the PWA icons and
`/healthz` are proxied with `changeOrigin: false`, so the server sees
`Host: localhost:5173` and a matching `Origin` and its same-origin write
check passes unchanged (plan 4.4). The dev server's RAM is never product
memory. Dev mode injects Vite's and React Refresh's scripts and sends no
CSP; check CSP against the built client served by Rust.

## Test selectors (agreed with the browser checks in `e2e/`; keep them exactly)

| Selector | When |
|---|---|
| `#root[data-ready="1"]` | booted: first render with a known session committed |
| `a[data-testid="signin"]` | signed out (not on the login page); links to `/app/login` |
| `form[data-testid="login"]` with `input[name="password"]` and `button[type="submit"]` | the login page |
| `[data-testid="login-error"]` | a failed sign-in (wrong password, rate limit countdown, …) |
| `[data-testid="user"]` (contains the user name, `admin`) | signed in |
| `button[data-testid="signout"]` | signed in; posts `/api/logout`, refreshes the session, stays on the page |

Also present: `[data-testid="theme"]` (theme button), `[data-testid="no-password"]`
(login page, no password set).

## UI SDK reading instructions (decision §1, rules 1–8)

UI SDK (`@ui-sdk/react`, `@ui-sdk/core`) is **not installed yet**: on
2026-11-17 every published version was younger than 7 days, and the
release-age rule allows no exception. The project-local skill is pending
with it: `npx skills@latest add vercel-labs/ui --skill ui-sdk` is run when
the SDK is installed, not before (it would install a package outside the
age gate). When it is installed:

1. Read the repository instructions and this file. Before using UI SDK,
   follow its installed skill if available and read
   `web/node_modules/@ui-sdk/react/docs/llms.txt` for the installed version.
2. Import components from their documented individual entries
   (`@ui-sdk/react/<component>`, e.g. `@ui-sdk/react/dialog`; no root import),
   and only inside `src/ui/`. Use the shipped components for dialogs,
   selects, tabs, switches and similar interactions rather than recreating
   them by hand.
3. To create a new primitive, first search
   `web/node_modules/@ui-sdk/react/docs/core.md` by heading and read
   `rendering.md` alongside it. Read the relevant component and style
   documentation, not the entire SDK source tree.
4. Import `@ui-sdk/core/web.css` once, in `src/main.tsx`.
   `src/styles/global.css` declares `@layer ui-sdk;` at its top (already
   done). No Tailwind here, so the Tailwind ordering in the decision does
   not apply.
5. Install the project-local skill when the SDK goes in:
   `npx skills@latest add vercel-labs/ui --skill ui-sdk` (project-local,
   non-interactive options), through `scripts/web.sh`, with the lead.
6. Respect the release-age policy (`web/.npmrc`). If it blocks a required
   installation, stop and ask; never weaken or bypass it.
7. Pin the reviewed set in `package-lock.json`. UI SDK was beta when chosen;
   validate the installed version's APIs instead of assuming examples for
   another version apply.
8. SDK behaviour and its optional default styles are not the product design.
   Wrap them in `src/ui/` with Hobnail's tokens (`tokens.css`) so the whole
   app reads as one design system (DESIGN.md §11).
