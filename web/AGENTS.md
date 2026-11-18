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
| `src/main.tsx` | entry: styles, stored theme and rail, session, React root |
| `src/app/` | shell (`Shell.tsx`, `Sidebar.tsx`, vitals, account, shortcuts sheet), router (`paths.ts` pure, `router.ts` History API: `useRoute()` with `path`, `params`, `search`, `setSearch`), theme, keyboard map (`keys.ts`), remembered shell state (`prefs.ts`, the old UI's storage keys) |
| `src/live/` | the live layer: one `EventSource` per tab (`stream.ts`, pure), stores and frame scheduler (`store.ts`), reducers (`reducer.ts`), the app's wiring (`runtime.ts`) and hooks (`hooks.ts`; import from `live/index.ts`) |
| `src/routes/` | one component per page; `containers/` and `container/` are lazy chunks (default exports) |
| `src/api/client.ts` | the only fetch wrapper: typed `ApiResult`, CSRF, retry |
| `src/api/session.ts` | `GET /api/v1/session` store, `useSession()`, the `api` client |
| `src/api/gen/` | TypeScript types generated from Rust by ts-rs; never edit by hand (`RH_UPDATE_TYPES=1 scripts/cargo.sh test` rewrites them) |
| `src/ui/` | the wrapper folder for interactive primitives (see below) |
| `src/charts/` | the only place that may import `recharts`; reached only by a lazy `import()` from `src/routes/container/` |
| `src/styles/` | `global.css` (imported once by main.tsx) imports `tokens.css` (tokens and themes), `ui.css` (the primitives of `src/ui/`) and `shell.css` (sidebar, top bar, bottom bar) |
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
  the sole age exception is the owner-authorized @ui-sdk/* install (D2).
  Check the locked set with `scripts/web.sh node scripts/ages.mjs`.

## Live data (src/live)

- One stream per tab: `GET /api/v1/stream?topics=…`, the topics being what
  the mounted hooks asked for (`useContainers` → `containers,stats`,
  `useSystem`/vitals → `system`). Closed on `pagehide` and after 10 s
  hidden, opened again on return (everything refetched after a minute
  away); a stream the browser closed is retried after 5 s, doubling to a
  minute. `useLiveStatus()`: `connecting` | `live` | `reconnecting` |
  `stopped` (paused, or no topic, or no EventSource).
- Stream messages are applied in one animation frame (at most one render
  per frame) and held while the user touches or scrolls.
- `config` and `resync` refetch bootstrap, containers and strips and call
  `onConfigChange` listeners: a page refetches its own data; nothing
  reloads. `hello` with another sha/content than the page's build shows
  the "new version" banner.
- Hooks: `useBootstrap`, `useContainers` (`{data, error}`), `useSystem`,
  `useVitalsRings`, `useStrips`, `useLiveStatus`, `useLiveInfo`,
  `useNewBuild`, `useNow` (server-corrected seconds, every 30 s),
  `onConfigChange(cb)`.

## Shell state kept with the old UI

Same keys and formats as `static/app.js`, so both UIs agree: `rh-theme`
(raw), `rh-side` (`rail`/`open`, raw), `rh-tree` (JSON `{stack: true}` for
folded stacks), `rh-tree-q` (sessionStorage, the sidebar filter), and
`rh-set-tab` is read for the Settings link.

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
| `a[data-testid="signin"]` | on the invitation page; links to `/app/login` |
| `form[data-testid="login"]` with `input[name="username"]`, `input[name="password"]`, and `button[type="submit"]` | the login page |
| `form[data-testid="onboarding"]` | first owner setup, with username, password and confirmation |
| `form[data-testid="invite"]` | invitation acceptance, with username, password and confirmation |
| `[data-testid="login-error"]` | a failed sign-in (wrong password, rate limit countdown, …) |
| `[data-testid="user"]` (contains the user name) | signed in |
| `button[data-testid="user"]` | signed in: the account menu button (contains the user name); opens the menu |
| `button[data-testid="signout"]` | signed in, inside the account menu (open it first); posts `/api/logout`, returns to sign-in with the current page as the return target |
| `nav[data-testid="sidebar"]` | the sidebar's page list |
| `[data-testid="tree-stack"][data-stack="<name>"]`, inside it `[data-testid="tree-count"]` | a stack of the sidebar tree and its running/total count |
| `[data-testid="live-status"][data-state="live\|reconnecting\|stopped\|connecting"]` | the live indicator (sidebar foot) |
| `[data-testid="live-stopped"]` | the "Live updates stopped at HH:MM" pill (after 4 s reconnecting) |
| `[data-testid="vitals"]` | the top bar's vitals chips (hidden under 700 px, absent with `[system]` off) |
| `button[data-testid="drawer-open"]` | phone bottom bar "More": opens the drawer |
| `[data-testid="shortcuts"]` | the shortcuts sheet (`?`) |
| `[data-testid="new-build"]` | the "new version" banner |
| `[data-testid="docker-banner"]`, `[data-testid="config-banner"]` | Docker unreachable; config error (signed in only) |

Also present: `[data-testid="theme"]` (theme button), `[data-testid="no-password"]`
(owner onboarding). The `[data-testid="user"]` element became the
account menu's button in Phase 1, so the sign-out check opens it before
clicking `signout`.

## UI SDK reading instructions (decision §1, rules 1–8)

UI SDK (`@ui-sdk/react`, `@ui-sdk/core`) is pinned to **0.1.0-beta.10**
under D2's owner override. Published 2026-11-18 at 09:45:28.252 UTC
(React) and 09:47:47.539 UTC (core), tagged `latest` and `beta`, Apache-2.0.
Neither has install lifecycle scripts; no transitive packages were added.
The project-local skill was skipped: `skills@latest` is 1.9.0, published
2026-11-16 10:30:48.447 UTC, younger than seven days.
Before using the installed SDK:

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
6. Respect the release-age policy (`web/.npmrc`). D2 authorizes only the
   documented exact SDK exceptions in `scripts/ages.mjs`; all other packages
   must be at least seven days old.
7. Pin the reviewed set in `package-lock.json`. UI SDK was beta when chosen;
   validate the installed version's APIs instead of assuming examples for
   another version apply.
8. SDK behaviour and its optional default styles are not the product design.
   Wrap them in `src/ui/` with Hobnail's tokens (`tokens.css`) so the whole
   app reads as one design system (DESIGN.md §11).

## M2 boards and integrations

Board and Settings routes are lazy. Import specific wrapper modules from
`src/ui/` when possible; importing the barrel can eagerly evaluate SDK entries
and preload popup code onto the first route. No SDK imports outside wrappers.

`src/boards/document.ts` owns document actions, rebase, dirty tracking, icon
fallback and credential-origin comparison. New entries omit server IDs.
Board saves serialize the document only; drafts use the separate debounced
server draft endpoint. Never put credentials in drafts, URLs or storage.
`watchBoards` shares the existing stream and releases its topic on unmount.

| Selector | Meaning |
|---|---|
| `[data-testid="board"]`, `[data-testid="board-empty"]` | board read page and empty state |
| `[data-testid="board-tile"]` | a rendered board item, including missing targets |
| `button[data-testid="board-edit"]` | Admin/Owner edit entry |
| `[data-testid="board-editor"]` | active editor |
| `[data-testid="board-save"]`, `[data-testid="board-cancel"]` | explicit save / discard |
| `[data-testid="board-add-section"]`, `[data-testid="board-add-item"]` | add a section / open picker |
| `[data-testid="board-picker"]`, `form[data-testid="board-link-form"]` | searchable suggestions and manual link form |
| `[data-testid="board-suggestion"]` | one filtered suggestion action |
| `[data-testid="board-leave"]` | unsaved-navigation prompt; Keep editing / Leave and keep draft |
| `[data-section="<index>"][data-item="<index>"] .drag-handle` | item Move / Resize controls; Alt+arrows moves, Shift+arrows resizes |
| `[data-testid="draft-status"]` | pending / saved / failed server draft status |
| `[data-testid="board-draft"]`, `[data-testid="draft-resume"]`, `[data-testid="draft-discard"]` | draft recovery choices |
| `[data-testid="board-conflict"]`, `[data-testid="board-rebase"]` | explicit conflict recovery |
| `[data-testid="settings-boards"]`, `[data-testid="board-create"]`, `[data-testid="board-form"]` | board management and dialog |
| `[data-testid="settings-integrations"]`, `[data-testid="integration-create"]`, `[data-testid="integration-form"]` | integration management and dialog |
| `[data-testid="integration-row"]`, `[data-testid="integration-test"]`, `[data-testid="integration-verdict"]` | connection and test evidence |
| `[role="dialog"][data-open][data-testid="edit-dialog"]` | existing container editor, now SDK dialog |
| `[data-testid="sort-select"] button`, `[role="option"][data-value="cpu"]` | SDK sort trigger and option |

`web/e2e/board-check.mjs` runs with `scripts/e2e.sh` against throwaway ports
3395–3399, like app-check and slice-check. It writes screenshots and recorder
data to OUT. Saved link metadata never counts as health evidence. Widgets
currently show their configured identity and an explicit unavailable-data
message: the existing backend widget renderer returns HTML, not a structured
React data contract, and injecting that HTML is prohibited.

When resuming a stale draft, the API supplies its document but no historical
base document. Recovery therefore preserves server additions and asks the
user to review the merge; old removals may need to be applied again. Ordinary
same-session conflicts use the original document for a three-way rebase.
