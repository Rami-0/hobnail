# React port: locked decisions

Status: locked by the owner, 2026-11-18. This file is the source of truth for
Part A and Part B. It overrides the recommendations in `react-port.md`
section 2 where they differ. Decision documents (`docs/decisions/*`) are not
edited by this file.

## Part A (the port)

| # | Decision | Locked as |
|---|---|---|
| D1 | Order of work | Port to parity, switch, delete the old UI, then product features. |
| D2 | UI component library | **UI SDK, pinned to one exact version, used in production.** Every component wrapped in `web/src/ui/`. If a release has problems, fix the wrapped components or move to a later release of the same SDK. Not shadcn. The release-age rule (D3) applies to the install. |
| D3 | Release-age policy | `min-release-age=7`, `ignore-scripts=true` in `web/.npmrc`. No exceptions except the UI SDK, see D2. New Rust crates are checked against the same 7-day rule, see below. |
| D4 | Charts | Recharts on routes that have a full chart. Sparklines, health strips, treemap and meters in plain SVG. |
| D5 | CSRF token after reload | `GET /api/v1/session`, `no-store`, returns the token only to a valid session. |
| D6 | JavaScript blocked | The React app needs JavaScript. `<noscript>` explains it. `/healthz`, the API and log download still answer. |
| D7 | Client storage | Embedded in the binary, raw plus gzip. |
| D8 | Read access in the new API | Same public/session split as the pages have today, enforced by one helper. `/api/state` becomes session-only. |
| D9 | JSON compression | Measure after phase 2, then decide. |
| D10 | Image size during side-by-side | Accept about 13.3 MB until the old UI is removed. |
| D11 | Rust to TypeScript types | `ts-rs` as a dev-dependency, generated files committed, a test fails on drift. |

### D2 install (owner override of D3 for the UI SDK only)

The UI SDK is a fast-moving new project that publishes new releases daily. The
owner's decision: install the **newest published UI SDK version now**, pin it
exactly (no `^` or `~`), and use it in production. This is the one exception
to D3, and it applies to `@ui-sdk/*` packages only. Record the pinned version
and its publish date in the install notes and in the commit message. Later,
when the SDK is stable, review the modules built on it, re-run the checks, and
fix what needs fixing.

Every other package keeps the D3 rule: 7 days, no exceptions.

### Rust crates

The 7-day rule also applies to every new Rust crate added to `Cargo.toml` or
`Cargo.lock`. Check each new crate's publish date on crates.io, as was done
for the Phase 0 crates, and record the check in the commit message.

### Old UI removal (M4)

The old UI is preserved on the `rust-only` branch. The new branch removes it
completely. No redirects, no compatibility for old URLs, and no migration
scripts.

## Part B (product features)

| # | Decision | Locked as |
|---|---|---|
| D12 | First login (onboarding) | Part of onboarding. After the container starts, the first visitor opens the UI, sets the owner password in the browser, and that is the saved credential. No console step. Password stored as a salted hash (argon2id). |
| D13 | Users | Multi-user. One owner, plus invited users with accounts and logins. |
| D14 | Destination binding for browser-entered credentials | Not answered. Use the recommendation: (b) a browser-entered secret is stored with the origin it was entered for and sent only there. Changing the origin means entering it again. |
| D15 | Where boards, layout and integrations are stored | In the SQLite database. Setup drafts also in the database. |
| D16 | Board configuration | Edited in the UI, inside Settings. A new board is empty. Things are added to it by the owner. No automatic "All containers" board. |
| D17 | Saving in edit mode | Not answered. Use the recommendation: explicit Save and Cancel, one write per editing session. |
| D18 | Access | Login required for everything. Three roles: **Owner, Admin, Viewer.** Access can be scoped to specific boards or integrations, managed by the owner. |
| D19 | First providers | Chosen later. |
| D20 | Service icons | (a) the existing opt-in dashboard-icons fetch and cache. |
| D21 | Self-signed certificates | Not answered. Use the recommendation: (a) public roots only, documented, then (b) a pinned fingerprint per integration. |

## Conflicts this creates with existing documents

These must be applied in Phase 5, as the plan says. Do not edit them earlier.

- **GOALS.md** says nothing should have to be listed just to appear. D16 means
  new boards start empty. Update GOALS.md to match.
- **GOALS.md / DESIGN.md** say the app has one password and no users. D12
  and D13 replace that. Update the security sections.
- **docs/DESIGN.md:341** says every page works with JavaScript blocked. D6
  replaces it. Already in the plan's section 3.
- **Secrets:** the earlier rule "secrets are never shown or stored by the app"
  is replaced by D13/D14, which store secrets with origin binding.

## Owner sign-off (2026-11-20)

Decisions taken after reviewing the Phase 2–5 results
(`react-port-phase-2-5.md`).

| Item | Decision |
|---|---|
| Whole-dist bundle cap | **Accepted** at 450 KB gzip / 1.35 MB raw, as in `web/scripts/size.mjs`. The combined chart payload (about 115 KB gzip) is accepted; the 60 KB per-chunk cap stays. |
| Docker image size | **Accepted** at about 13.3 MB ("size still reasonable"). Not yet built: tighten the gate to 13.5 MB so growth is still caught. |
| D9 JSON compression | **Decided:** gzip larger JSON responses when the client accepts gzip, with `Vary: Accept-Encoding`. Never compress the SSE stream (`/api/v1/stream`). Not yet built. |
| D19 first providers | **Deferred** to the next step. |
| PR #3 | The owner merges it. |
| Cross-device testing | Required before merge: Chromium, Firefox and WebKit at phone, tablet, laptop and desktop sizes, plus a check on real devices. Not yet done. |
