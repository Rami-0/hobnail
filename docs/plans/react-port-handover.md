# Hobnail React port handover

## Preview

Open [port 3300 over Tailscale](http://100.125.149.51:3300/app) or
[port 3300 on the LAN](http://192.168.8.200:3300/app). Use **Set owner password**
on the first visit. The owner preview starts with its own empty database and
uses this host's Docker and metrics feeds. Create an empty board in
**Settings → Boards** and open it to try editing, cancelling, and saving.
Use **Settings → Users** to invite a Viewer or Admin and grant board or
integration access.

The build runs as `hobnail-react-3300` with image `hobnail-rp:m6` and data at
`/tmp/hobnail-3300-data.U0duOA` on the host. The preview
uses plain HTTP on the two listed interfaces. Use a password dedicated to this
test. To stop it and preserve the preview data, run
`docker stop hobnail-react-3300` on the host. Restart it with
`docker start hobnail-react-3300`.

[PR #3](https://github.com/Rami-0/hobnail/pull/3) remains open as a draft
against `main`.

## Milestone record

| Milestone | Commit range | Gate result |
| --- | --- | --- |
| M1 · Accounts and auth | `1bd6f10..00fedaa` (33) | Rust fmt, clippy, 513 tests (1 ignored), all web commands, app and slice passed. |
| M2 · Boards and settings | `00fedaa..3cc3dd3` (44) | Rust fmt, clippy, 528 tests (1 ignored), all web commands, app, slice and board passed. |
| M3 · Page parity | `3cc3dd3..bc8f027` (272) | Rust fmt, clippy, 536 tests (1 ignored), all web commands, app, slice, board, pages, logs, settings, accessibility and CSP passed. Performance failed on LCP, tasks and one INP sample. |
| M4 · React-only switch | `bc8f027..0ced494` (28) | Rust fmt, clippy, 381 tests, all web commands and all eight browser checks passed. Image was 13.3 MB against 13 MB. |
| M5 · Docs and measurements | `0ced494..5fa783d` (44) | Rust fmt, clippy, 382 tests and all web commands passed at the current script caps. Seven browser checks passed. Performance and the phone-dark log burst missed their targets. |
| M6 · 3300 preview | `5fa783d..react-port` | See the verification results below. |

To list every commit with its subject for a milestone, run
`git log --reverse --oneline <range>` with the range above. M1–M5 rows
summarize recorded milestone results. M6 was verified in the Codex wrap-up.

## M6 verification

The installed image embeds `5fa783d` with content hash
`93da0f7f50edb275af333c569d9641b4981b4aa3e63f06c70fd72a87d34b3196`.
This matches the measured M5 runtime. Later commits change browser harnesses
and documentation. The owner preview mount started empty. Direct
HTTP checks confirmed health, React entry and the fresh session state on both
published addresses. Unauthenticated containers returned 401, a foreign Host
on the app and private API returned 421, and the retired root returned 404.

The web wrapper passed check, test (140), build and size. The Cargo wrapper
passed fmt, clippy and all 382 tests. The release image build also passed.

Browser checks used a disposable 150-container fixture on port 3300. The
first pass used the measured image over host loopback. Retries used the
deployed image in a separate Docker network namespace. The fixture databases
were independent of the owner preview.

| Check | Result |
| --- | --- |
| App | 69 assertions passed, including browser owner setup, sign-in and sign-out. |
| Board | 97 assertions passed, including empty board creation, Save/Cancel, scoped reads, and credential origin binding. |
| Pages | All four profiles passed across container detail, history, host and the other signed-in pages. |
| Settings | All four profiles passed, including invitations, roles and board grants. |
| CSP | 80 route visits and two fresh onboarding profiles passed. |
| Slice | 166 assertions passed on retry. The first run had two Chromium network assertions fail with `ERR_NETWORK_CHANGED` while build containers were changing. |
| Accessibility | 60 route audits and two fresh onboarding audits passed with zero violations. The first invocation hit a fixture port guard, which was fixed before the rerun. |
| No JavaScript | Direct HTTP confirmed health, the authenticated containers API, and a plain-text log attachment. |
| Logs | All four desktop and phone profiles passed, including the burst limit and download flow. |

## Remaining acceptance items

- The M5 image was 13.3 MB against 13 MB. The client distribution was
  1,344,189 bytes raw and 449,533 bytes gzip against the original 1,300,000
  and 400,000 limits. The raised script caps of 1.35 MB raw and 450 KB gzip
  still need owner sign-off. The four named chart files total 115,235 bytes
  gzip and individually pass the per-chunk limits.
- The 20 MiB lifetime memory budget was exceeded during authentication and
  load testing. The full workload peaked at 22.24 MiB. Browser LCP, phone
  load and live-update tasks, and one chart INP sample also missed their
  targets. Containers CLS passed. M5 captured a 107 ms phone-dark log burst
  against its 100 ms limit.
- Generic HTTP integrations are available. Provider selection and guided
  setup beyond account creation remain open, along with the planned
  certificate-pinning step. The full set of deviations, test conditions and
  unverified cases is in the [measurement report](react-port-phase-2-5.md).
- The pinned SDK packages are `@ui-sdk/react` and `@ui-sdk/core`
  `0.1.0-beta.10`, both published on 2026-11-18 UTC at 09:45:28.252 and
  09:47:47.539 respectively. The SDK is the sole release-age exception.
  The package age and publication evidence is in commit `8e72022`.

## Port 3200

The Codex wrap-up made no application requests to 3200. The existing
`hobnail` container retained its ID, start time and zero restarts.
Earlier M3 fixture runs did issue GET requests to 3200 before that fixture
dependency was removed. No restart, replacement or data write was reported.
