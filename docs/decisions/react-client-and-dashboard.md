# React client and dashboard product decisions

Date: 2026-11-16
Status: agreed direction, saved for future implementation
Product: Hobnail

## Activation and scope

The user approved this direction and asked to lock the decisions in a file. **Do not begin the frontend migration or change the current product merely because this document exists.** The user will say when to use it. When activated, treat the decisions below as requirements; identify an actual conflict rather than silently changing them.

Preserve existing work, user configuration, authentication and backend features. The isolated benchmark was an experiment, not the replacement product. Keep Hobnail's identity; do not import the experiment's temporary “Hearth” name, example catalog or synthetic endpoints into production.

## 1. Client and deployment architecture

- Build the frontend with **React 19 or newer, TypeScript and Vite**.
- Use **UI SDK** (`@ui-sdk/react` and `@ui-sdk/core`, https://ui-sdk.dev/) for accessible interaction building blocks. Use **Lucide React** for icons and **Recharts** for charts in the initial implementation.
- The **existing Rust backend remains the server**: authentication, authorization, discovery, provider integrations, credentials, persistent configuration, metrics/history, logs and live data belong there.
- Produce static HTML, JavaScript, CSS and assets with Vite during a multi-stage image build. Copy only the production frontend output and Rust runtime into the final image.
- Deploy **one application container with one Rust server process** serving the frontend and API on the same origin. React executes in the browser. Node/npm/Vite are build-time tools; no production Node process, Vite dev server, SSR service, separate frontend container or required proxy container.
- Development may use Vite's dev server with an API proxy. Never use development-server RAM to represent the production deployment.
- Reuse or adapt the real Rust API and live-event facilities. Preserve its login, same-origin/CSRF and concurrent-write protections. Migrate UI responsibilities deliberately instead of wrapping the old HTML fragments in React.
- Configuration is durable on the server, with stable IDs and a documented versioned schema/migration path. React state alone is not durable configuration. Preserve supported file-based configuration and reconcile writes with browser editing.
- Bound histories, log buffers, caches, provider concurrency, polling and live connections. Suspend irrelevant updates and release subscriptions/connections when views or clients close.

### UI SDK implementation instructions

1. Read applicable repository instructions. Before using UI SDK, follow its installed skill if available and read `node_modules/@ui-sdk/react/docs/llms.txt` for the installed version.
2. Import components from their documented individual entries. Use shipped components for dialogs, selects, tabs, switches and similar interactions rather than manually recreating them.
3. To create a new primitive, first search `node_modules/@ui-sdk/react/docs/core.md` by heading and read `rendering.md` alongside it. Read relevant component/style documentation, not the entire SDK source tree.
4. Import `@ui-sdk/core/web.css` once at the app entry. Declare `@layer ui-sdk;` at the top of the global stylesheet. If using Tailwind, use this stylesheet ordering instead of its standalone Tailwind import:

   ```css
   @layer ui-sdk;
   @import "tailwindcss";
   @import "@ui-sdk/core/web.css" layer(components);
   ```

5. Install the project-local skill when setting up the new client: `npx skills@latest add vercel-labs/ui --skill ui-sdk` (use the CLI's supported project-local/noninteractive options as appropriate). Add the SDK reading instructions to the frontend's `AGENTS.md`.
6. Respect any dependency release-age policy. If it blocks a required installation, stop that installation and ask the user; do not weaken or bypass the policy.
7. Pin a reviewed dependency set with the project's lockfile. UI SDK was beta during the experiment; validate its installed APIs instead of assuming examples for another version apply.
8. SDK behavior and optional default styles do not supply the complete product design. Build a coherent, branded design system around them.

## 2. Memory and performance requirements

The budget concerns **the combined RAM of all application containers on the server**. It excludes the user's browser. Do not justify extra server containers by reporting only Rust's heap or RSS.

- Preferred target: **under 10 MiB** of application-container RAM for the defined representative workload. **20 MiB** is the user's acceptable broader design budget, not permission for an unmeasured regression.
- Validate idle, startup, concurrent dashboards and post-load behavior. Report measured peaks as well as medians. Do not impose a memory cap and then present the cap as measured consumption.
- A richer React interface does not require a server-side React runtime. Keep that architectural advantage as features grow.
- Report raw cgroup `memory.current`, Docker-style working set, kernel `memory.peak`, sample interval/count, CPU, errors, latency, memory limits and workload definitions. Distinguish binary MiB from decimal MB and asset bytes from runtime RAM.
- Include charged file/kernel/socket memory and explain shared page-cache attribution. Warm-cache results are not proof of cold-cache behavior. Browser/OS/Docker-daemon/build-tool memory must not be mixed into application-runtime figures.
- Also check browser responsiveness, bundle size and update frequency as separate quality criteria. A small server footprint does not excuse a slow or inaccessible interface.

### Historical feasibility evidence (not a future guarantee)

The isolated experiment used a real React/Vite/UI SDK/Lucide/Recharts frontend and a Rust backend with persistent JSON settings, live host/container metrics, 360-point history and SSE. Its service catalog was labeled example metadata, not real third-party connectors. The final production frontend was about 982,238 raw asset bytes / 286,284 summed gzip bytes.

| Workload | Raw container RAM median | Raw sampled maximum |
| --- | ---: | ---: |
| Idle | 2.230 MiB | 2.379 MiB |
| 1 active dashboard client | 2.275 MiB | 2.668 MiB |
| 10 active dashboard clients | 2.742 MiB | 3.418 MiB |
| 25 active dashboard clients | 3.506 MiB | 4.109 MiB |
| After load | 2.180 MiB | 2.914 MiB |

Kernel-observed lifetime peak: **5.191 MiB**, without an explicit memory cap. Complete repeat runs saved 464 samples, nominally every 500 ms. Each live-client phase ran 60 seconds with full 360-point history, SSE, state reads every 2 seconds, history reads every 5 seconds, isolated 100-entry configuration reads/atomic writes every 10 seconds, and full asset downloads initially/every 20 seconds. There were 2,573 successful HTTP operations, 2,160 SSE updates and zero errors; streams returned to zero after disconnect. Load clients modeled network traffic, with actual desktop/mobile Chromium interactions checked separately.

Host: shared x86_64 i5-7200U, four logical CPUs, about 7.15 GiB RAM, under contention; not a Raspberry Pi measurement. Shared file-cache attribution and warm image/assets limit the conclusion. Authentication, real external connectors, Docker discovery and full logs were not implemented in that experimental backend. **Benchmark the actual migrated Hobnail backend before making product memory claims.**

The user requested deletion of the experimental app, images, scripts and raw artifacts after recording these decisions. This summary is the retained evidence, not a claim that those artifacts remain available. Do not rely on the earlier minimal static-only baseline for product sizing.

## 3. Guided onboarding

Required sequence:

**Appearance → Discover services → Select what to include → Configure connections → Review dashboard**

- Every optional step is skippable. Make back/continue/skip behavior clear, retain entered values, and allow setup to resume.
- Offer sensible appearance defaults without requiring visual customization to proceed.
- Explain reachable discovery sources and unavailable sources in understandable terms. Missing Docker access or another optional source must not block manual setup.
- Discovery results are **suggestions**, not automatic additions. Let users select, exclude, group and rename before adding anything.
- Present relevant connection fields for selected services, with a test action and clear success/error feedback. A single unavailable service must not block completing the rest of the board.
- Keep unfinished choices in a setup draft. Review what will be added and how it will be organized before committing the board. Avoid duplicate entries when resuming/retrying.
- Make all skipped capabilities available later through the same normal configuration interface.
- Complete onboarding with a useful first board derived from the user's actual selections. If nothing was selected, show clear add/discover actions rather than fabricated services or health data.
- Any first-use tour is brief and skippable, and should teach actual controls in the user's board.

## 4. Ready-made integrations

Required interaction:

**Choose service → Enter URL/credentials → Test connection → Choose useful widgets → Add to board**

- Provide a searchable integration catalog with recognizable names/icons and useful categories. Only offer real supported providers as functional integrations.
- Define provider metadata, configuration schema, connection test and supported widgets together, with clear Rust/backend responsibilities.
- Users should not need API knowledge, JSON paths, scripts, YAML or custom code for supported integrations.
- Show only the credentials and options required by that provider. Explain URL reachability from the Rust server when it matters, without burdening the normal flow with implementation details.
- Test connections on the backend with bounded timeouts; show actionable errors. Preserve a draft when testing fails and support configuring/retrying later.
- Credentials stay under the existing backend security model: protected storage, masked fields, no secrets in logs or returned in ordinary client payloads. Do not replace established protections with prototype shortcuts.
- Recommend compatible widgets after connection, allow selection/configuration, and support multiple instances of the same provider.
- Distinguish service links, discovered services and active data integrations. Never show a link as healthy merely because its metadata was saved.
- Pick the initial provider set from actual supported backend capabilities/user priorities when implementation starts. Do not claim a large catalog of working connectors backed only by placeholder cards.

## 5. Intentional organization

- Let users choose what belongs on each board; do not dump every discovered container or feature onto the landing page.
- Support meaningful groups/sections, custom names, ordering and icons. Preserve those choices across discovery refreshes.
- Make unselected suggestions available to add later. Deduplicate discovered and manually configured instances using stable identity and reviewable matches.
- Provide useful defaults while allowing all normal organization changes through the interface.
- Make adding, moving, editing, hiding and removing items discoverable. Removal must make its scope clear; preserve/recover configuration where practical.

## 6. Customization through the interface

- Provide an explicit dashboard edit mode with **move/reorder and resize** controls for widgets. Include keyboard/accessibility alternatives to pointer dragging.
- Support board layout/density, groups/sections, widget sizing and responsive behavior. Avoid saving a desktop-only arrangement that breaks on phones.
- Let users choose colors, icons, appearance/theme and human-readable names through forms. Custom CSS may remain an advanced option, not the requirement for ordinary customization.
- Every supported widget has a configuration form appropriate to its options and integration. Show validation at the field, retain unsaved input, and communicate save state/errors.
- Define consistent save/cancel or clearly communicated autosave behavior. Do not silently overwrite another session's changes.
- Offer a preview where it helps users choose; clearly distinguish preview/sample data from live data.
- Persist board, layout and widget configuration on the server. Changes must survive reload, container restart and a new browser session after login.

## 7. Product quality and useful defaults

- Design a coherent application shell: consistent typography, spacing, colors, icons, navigation, buttons, forms, dialogs, charts and empty/error/loading states.
- Essential text must be comfortably readable on desktop and mobile. Use real keyboard/focus behavior, clear accessible labels, adequate contrast and reduced-motion support.
- Keep the default board relevant and restrained. Start with selected services and useful compatible widgets; expose additional features progressively.
- Surface stale, disconnected, loading and failed integration states honestly. Do not fabricate values or label examples as live measurements.
- Discovery, onboarding and normal settings must share configuration primitives so setup does not become a separate UI that cannot be maintained afterward.

## 8. Instructions for the future implementation agent

1. Wait for the user's instruction activating this file. Then inspect the current branch, repository guidance, existing APIs/configuration, tests and ongoing changes before editing.
2. Map existing functionality and data contracts. Write a scoped migration plan and preserve current Rust capabilities/configuration; do not discard features because the experiment did not exercise them.
3. Establish the React/Vite build and one-container packaging, design tokens and SDK component conventions. Implement a coherent first vertical slice rather than disconnected styled screens.
4. Build the onboarding draft/selection/review flow and a useful first board. Reuse forms for later integration/widget editing.
5. Implement real supported providers and configurable widgets incrementally. Keep example fixtures confined to clearly labeled demos/tests.
6. Verify actual rendered pages in a browser at desktop and phone sizes. A passing build, HTTP 200, or jsdom test is not visual verification. Check loaded styles/MIME, console/network failures, navigation, keyboard interaction, dialogs, persistence and live updates. If browser access fails, state that limitation and use an available verified fallback; never imply a rendering check happened when it did not.
7. Run meaningful frontend, Rust and API checks for changed behavior. Test save/reload/restart persistence, discovery selection/deduplication, optional skips, failed connection recovery and concurrent-write handling.
8. Repeat realistic **whole-application-container** memory measurements with the actual backend and selected integrations. Include startup, idle, 1/10/25 live clients, full bounded history, repeated API/config operations and cleanup. Save raw samples incrementally so an interrupted process cannot erase the run.
9. Report exactly what works, which integrations are real, how it was verified, observed resource usage and remaining limitations. Do not convert prototype measurements into a guarantee for unimplemented features.

Acceptance requires all five product outcomes: guided onboarding, ready-made functional integrations, deliberate organization, UI-driven customization, and a useful first board—alongside the agreed one-container Rust/React deployment and measured small server footprint.
