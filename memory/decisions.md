# Decisions

## Canonical Memory Model

- `memory/decisions.md` stores active, high-signal canonical rules only.
- Full historical detail for the pre-compaction log is archived at `memory/archive/decisions-2026-02-07-full.md`.
- New low-signal implementation-step history should go to `memory/archive/` instead of this file.

## Entry Template

```md
## YYYY-MM-DD HH:mm
Context: <task or feature>
Type: decision | preference
Rule: <one-line future behavior>
Why: <short reason this rule exists>
```

## Active Canonical Rules

### Documentation and Memory

- Keep `AGENTS.md` canonical and current-state only; avoid phase/progress framing.
- Keep design-system guidance in `AGENTS.md` aligned with implemented primitives, guardrails, and scripts.
- Keep `memory/decisions.md` high-signal; move detailed timelines to `memory/archive/`.

### Backend Architecture

- Keep domain logic in `src-tauri/src/shared/*`; app/daemon code should remain thin adapters.
- For app/daemon parity work, extract shared implementations instead of duplicating logic.
- Add new backend behavior in shared core first, then wire app commands and daemon RPC handlers.

### Remote and Mobile Direction

- Keep mobile remote implementation Orbit-first and Orbit-named across runtime/config/docs.
- Keep Orbit setup/docs/UI self-host-only unless the user explicitly requests hosted mode.
- Do not introduce CloudKit/PR31-based mobile backend patterns unless the user explicitly requests them.
- For iOS rollout, prioritize backend/transport parity for existing responsive UI flows over net-new mobile-specific UI.
- For current mobile remote scope, terminal and dictation parity remain out of scope unless the user re-enables them.
- Keep Tailscale as first-run bootstrap for TCP self-host setup while Orbit remains the target production relay path.
- Keep remote provider labels marked `(wip)` until the user requests removal.

### Remote Implementation Rules

- Keep remote transport behind provider-selected `RemoteTransport` abstractions.
- Keep Orbit auth/session contract-driven around typed `deviceCode` polling.
- Persist remote backend token changes through shared settings-core helpers to avoid stale overwrite races.
- Invalidate cached remote backend clients when transport-affecting settings change.
- Desktop CLI integrations must resolve from `PATH` plus deterministic fallback install locations.

### Frontend Design System

- Use DS primitives/tokens first for modal, toast, panel, and popover shell behavior.
- Keep feature CSS focused on feature-specific layout/content; do not duplicate DS shell chrome.
- Use DS toast sub-primitives for shared toast structure.
- Modal consumers must provide accessible labels.
- Tablist UIs must use proper tab semantics and move selection and focus together.
- For DS migration follow-ups, run `npm run codemod:ds:dry` and keep DS lint guardrails green.

### Frontend UX and Performance

- Automatic message pinning should be immediate and non-animated; smooth scrolling is user-initiated only.
- High-frequency reducers must return previous state for no-op transitions.
- Memoize high-churn UI shells when unrelated app updates can impact input responsiveness.
- Keep a single trigger path for accelerator-backed actions to avoid duplicate execution and telemetry.

### Release and Telemetry

- Keep Sentry telemetry enabled unless the user explicitly asks to remove or replace it.
- Release metadata must be generated from normalized artifact names and validated before publishing.

## 2026-02-07 21:35
Context: Memory compaction model update
Type: decision
Rule: Keep `memory/decisions.md` as a compact canonical rule register and archive detailed historical decision timelines under `memory/archive/`.
Why: This improves retrieval quality for future agents and reduces low-signal duplicate context during implementation.

## 2026-02-07 21:34
Context: Message file-link display controls
Type: decision
Rule: Keep file-link parent path display in messages behind `showMessageFilePath`, defaulting to enabled.
Why: This preserves existing context-rich defaults while allowing users to reduce visual noise in message file references.

## 2026-02-07 21:22
Context: Settings IA for mobile/backend setup
Type: preference
Rule: Keep backend/mobile connectivity controls in a dedicated `Server` settings section (not `Codex`), with explicit mobile-access daemon controls.
Why: Desktop Codex configuration and remote/mobile server operations are different mental models and should be separated in UX.

## 2026-02-08 06:27
Context: Server settings desktop/mobile behavior
Type: preference
Rule: Keep mobile access controls visible in `Server` regardless of desktop backend mode; `backendMode` only affects whether desktop traffic is local or routed through remote transport.
Why: The desktop app should stay local by default while still allowing users to configure and operate the mobile daemon path.

## 2026-02-08 06:46
Context: iOS runtime backend defaults
Type: preference
Rule: Default backend mode to `remote` on iOS/mobile runtime while keeping desktop defaults `local`.
Why: Mobile clients must connect through the remote daemon flow, while desktop continues to run local-first.

## 2026-02-08 07:02
Context: Tauri iOS target build with vendored libgit2
Type: decision
Rule: Keep iOS Xcode target linker flags including `-lz` and `-liconv` in `src-tauri/gen/apple/codex-monitor.xcodeproj/project.pbxproj` (and mirrored in `src-tauri/gen/apple/project.yml`).
Why: Vendored `libgit2` objects require zlib/iconv symbols at app link time, and iOS build fails without explicit flags.

## 2026-02-08 07:19
Context: iOS app icon composition
Type: preference
Rule: Keep iOS app icons regenerated from trimmed source artwork at high fill (~95% canvas) instead of macOS-style inset composition.
Why: iOS icons should occupy most of the masked tile area; desktop-style inset icons look undersized on Home Screen.

## 2026-02-08 07:24
Context: iOS app icon full-bleed requirement
Type: preference
Rule: Keep iOS AppIcon assets fully opaque and full-bleed (no transparent padding), with artwork scaled for edge-to-edge appearance under the iOS mask.
Why: Inset/transparent icon composition reads like desktop icon treatment and looks undersized on iOS Home Screen.

## 2026-02-08 07:39
Context: iOS documentation canonicalization
Type: preference
Rule: Keep `README.md` and `AGENTS.md` explicit that iOS support is WIP and document simulator/device launch via `scripts/build_run_ios.sh` and `scripts/build_run_ios_device.sh`, including signing/team requirements.
Why: This prevents setup ambiguity and keeps iOS onboarding repeatable while mobile support is still maturing.

## 2026-02-08 07:40
Context: AGENTS iOS section brevity
Type: preference
Rule: Keep AGENTS iOS guidance minimal: supported status + simulator/device script commands only.
Why: The user wants AGENTS concise and operational, not detailed setup prose.

## 2026-02-08 07:43
Context: iOS frontend layout selection
Type: preference
Rule: Force iOS/mobile runtime into phone layout mode so the tab-bar mobile shell is always used at app startup.
Why: Width-only layout detection can select tablet shell on iOS and hide the expected mobile tab bar.

## 2026-02-08 08:09
Context: iOS workspace source of truth
Type: preference
Rule: In iOS remote mode, workspaces/projects must come from the daemon backend; do not rely on standalone iOS-local project creation.
Why: Mobile clients are intended to operate against the host daemon state and should reflect daemon-managed workspaces.

## 2026-02-08 08:15
Context: Settings modal mobile navigation usability
Type: decision
Rule: For phone-sized settings viewports (`max-width: 720px`), use master/detail navigation (section list first, detail view second with a back action) instead of horizontal tab-strip navigation.
Why: Horizontal nav compression breaks section labels and readability on narrow screens; master/detail keeps section selection and content legible.

## 2026-02-08 08:22
Context: Reusable mobile master-list navigation styling
Type: decision
Rule: Use design-system panel navigation primitives (`PanelNavList` + `PanelNavItem`) for section-list style navigation, enabling optional disclosure chevrons for mobile master/detail patterns.
Why: This keeps section-list interactions visually consistent and reusable across features instead of re-implementing one-off list row styles.

## 2026-02-08 08:33
Context: Compact sidebar footer/action stacking
Type: preference
Rule: In compact/mobile sidebar layouts, place account/settings corner actions in normal flow under the usage widget instead of absolute bottom positioning.
Why: Absolute positioning overlaps usage bars on narrow viewports and causes control collisions.

## 2026-02-08 08:49
Context: iOS layout verification workflow
Type: preference
Rule: Validate iOS UI layout changes by running `scripts/build_run_ios.sh` and then taking a fresh simulator screenshot from that launched build.
Why: CSS/runtime changes can behave differently in the packaged iOS runtime than in incremental simulator state.

## 2026-02-08 08:39
Context: iOS mobile viewport height sync
Type: decision
Rule: When syncing `--app-height` on mobile, account for `visualViewport.offsetTop` (use `visualViewport.height + visualViewport.offsetTop`) instead of `visualViewport.height` alone.
Why: WKWebView can report a reduced visual viewport origin, and ignoring the offset under-sizes the app container and creates a bottom gap.

## 2026-02-08 08:58
Context: Mobile server settings UX scope
Type: preference
Rule: On iOS/mobile runtime, keep `Settings > Server` focused on connection setup/testing and hide desktop-only daemon/Tailscale/runner orchestration controls.
Why: Mobile clients only need backend connection setup and verification, not desktop server process management.

## 2026-02-08 09:06
Context: Mobile server settings provider support
Type: preference
Rule: On iOS/mobile runtime, keep `Settings > Server` simplified but allow provider selection (`TCP` or `Orbit`) with provider-specific endpoint/token inputs and a single connect test action; keep desktop-only daemon/Tailscale/runner controls hidden.
Why: Mobile needs a focused setup UX while still supporting both remote backends used by desktop setups.

## 2026-02-08 08:54
Context: Remote daemon RPC throughput for multi-workspace startup
Type: decision
Rule: Process authenticated daemon RPC requests concurrently (bounded by a per-connection semaphore) instead of strictly serial request/response handling.
Why: Serial remote RPC processing causes workspace `connect_workspace`/`list_threads` startup calls to queue behind each other, making non-primary projects remain in loading state noticeably longer on mobile remote mode.

## 2026-02-08 09:05
Context: Remote thread list hydration performance
Type: decision
Rule: Keep initial per-workspace thread hydration bounded with larger `thread/list` page sizes and strict max-page caps instead of unbounded deep scans driven by cached activity.
Why: In remote mode, repeated deep pagination per workspace causes long shimmering/serial-feeling sidebar loading due accumulated network round trips.
