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
| D9 | JSON compression | Measure after phase 2, then decide. *Status (2026-11-23): decided at the 2026-11-20 sign-off and built in finish-hobnail M1 (`c61b63d`): gzip for JSON responses over 1 KiB up to a 256 KiB ceiling, API only, never the event stream.* |
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
| D19 | First providers | Chosen later. *Status (2026-11-23): decided. The list is the 37 ticked lines of [integrations-candidates.md](integrations-candidates.md) (`be97ab6`); all were built in finish-hobnail M3 and M4.* |
| D20 | Service icons | (a) the existing opt-in dashboard-icons fetch and cache. |
| D21 | Self-signed certificates | Not answered. Use the recommendation: (a) public roots only, documented, then (b) a pinned fingerprint per integration. *Status (2026-11-23): (b) built in finish-hobnail M3: a per-integration pinned leaf SHA-256 fingerprint, confirmed by the owner from a failed test (trust on first use); changing a pin requires credentials again (`c318bfe`).* |

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
| Docker image size | **Accepted** at about 13.3 MB ("size still reasonable"). Not yet built: tighten the gate to 13.5 MB so growth is still caught. *Status: gate built (`b1f6c3e`, `scripts/check-image-size.py`). The final image misses it; see the status table below.* |
| D9 JSON compression | **Decided:** gzip larger JSON responses when the client accepts gzip, with `Vary: Accept-Encoding`. Never compress the SSE stream (`/api/v1/stream`). Not yet built. *Status: built in finish-hobnail M1 (`c61b63d`, `fc8f700`).* |
| D19 first providers | **Deferred** to the next step. *Status: decided; the ticked list in [integrations-candidates.md](integrations-candidates.md), all built.* |
| PR #3 | The owner merges it. |
| Cross-device testing | Required before merge: Chromium, Firefox and WebKit at phone, tablet, laptop and desktop sizes, plus a check on real devices. Not yet done. *Status: the emulated three-engine matrix is done (`scripts/e2e-matrix.sh`, finish-hobnail M6 and M7). The check on real devices is **not done** and needs the owner.* |

## Finish-hobnail status (2026-11-23)

Status notes only; no decision above is changed. "Accepted" rows are
**accepted pending owner sign-off** at merge: they are proposals made during
finish-hobnail, not owner approvals. Numbers are from the M7 gate
([measurements.md](../measurements.md#finish-hobnail-final-gate-m7-2026-11-23)).

| Item | Status | Detail |
|---|---|---|
| D9 gzip for JSON | Done (M1) | Over 1 KiB up to 256 KiB, API only, never the event stream. |
| D19 first providers | Done (M3, M4) | All 37 ticked lines of [integrations-candidates.md](integrations-candidates.md). |
| D21 (b) pinned fingerprint | Done (M3) | Per integration, confirmed on first use; a pin change needs the credentials again. |
| 13.5 MB image gate | Done (M1); **missed by the final image** | `scripts/check-image-size.py`: 13,548,510 B against 13,500,000 B (48,510 B over, already at `2257256`). `scripts/deploy.sh` stops on it. Needs a size reduction or an owner decision on the gate. |
| Cross-device testing | Emulated done (M6, M7); real devices **not done** | Chromium, Firefox and WebKit matrix at 360 to 2560 px, tablets and 200% zoom. Real iPhone Safari, Android, tablets and screen readers need the owner. |
| Touch targets | Accepted | 44 px on coarse pointers and below 700 px, 24 px on a fine pointer at 700 px and up (44 px everywhere would redesign the desktop). |
| Matrix reduction | Accepted | Flows run at four sizes (390 px, tablet portrait and landscape, 1440 px); the page sweep runs at all ten sizes. 200% zoom is emulated as 720 × 450 at device pixel ratio 2 (no engine lets Playwright set browser zoom). See [web/e2e/matrix/README.md](../../web/e2e/matrix/README.md). |
| ICMP ping | Accepted | Replaced by TCP connect: no extra container privileges. |
| Releases watcher | Accepted | Compares against a manually entered current version; no image-tag matching. |
| TrueNAS 26 | Accepted | Unsupported (WebSocket-only API); built from the 24.10 REST v2.0 API. |
| Provider shortfalls | Accepted | Duplicati has no per-job result; Nextcloud shows activity counts only. Pi-hole top clients and top domains are built (`23585d8`). |
| Real service instances | Not done | Every provider is tested against fakes only; none was checked against a real instance. |
| Credentials in plaintext | Accepted (pre-existing) | Stored in plaintext in SQLite, write-only through the API and origin-bound; protect the data volume and backups. |
| Browser budgets | **Missed** (final numbers) | Desktop LCP ≤400: table 492/496, History 584/448 ms. Phone table LCP ≤1,200: 1,456/1,196 ms. Phone History load ≤200: 253.3/343.3 ms; phone table load observer 229/207 ms (tracing passes). Live ≤50: desktop table 58.6/58.3, phone History 62.4/54.2 ms. All-route phone LCP 1,772 ms and phone Settings load 230 ms (M5 pass 2) not re-measured. INP and CLS pass. |
| Preferred 10 MiB memory | **Missed** under full history | Full workload peak 12.4 MiB (12.44 without a limit, 12.39 under 20 MiB); 25 dashboards at most 8.15 MiB. The 20 MiB budget holds. |
| Legacy first sign-in | Accepted | About 20.5 MiB, once per account with a legacy 19 MiB hash; refused under a memory limit below 36 MiB. Reset or migrate legacy accounts before applying a 20 MiB limit. |
| arm64 | Partly done | Cross-built image (`scripts/build-arm64.sh`, executable 6,831,000 B) smoke-tested under QEMU in M6. No native arm64 production build and no run on arm64 hardware. |
| Connection caps | Accepted, with a finding | 48 open connections and 12 per peer by default (`RH_MAX_CONNECTIONS`, `RH_MAX_CONNECTIONS_PER_PEER`), which keeps an attack holding every slot at 15.9 MiB. Finding (M7): 25 dashboards that each keep an event stream and a keep-alive request connection need 50 sockets, so two of them queued until their requests timed out; at 64 there were no errors, and the attack peak there was 18.1 to 18.9 MiB. The owner should choose between the default and a higher cap. |
| Request head limit | Accepted | 16 KiB per request head (request line and headers, cookies included); larger heads get 431. |
