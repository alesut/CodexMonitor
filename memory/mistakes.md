# Mistakes

## Entry Template

## YYYY-MM-DD HH:mm
Context: <task or feature>
Type: mistake
Event: <what happened>
Action: <what changed / fix applied>
Rule: <one-line future behavior>
Root cause: <why it happened>
Fix applied: <what was changed>
Prevention rule: <how to avoid recurrence>

## 2026-02-07 10:51
Context: Settings modal migration to `ModalShell`
Type: mistake
Event: Settings window stopped centering after switching to DS modal shell.
Action: Removed `position` and `z-index` from `.settings-window` to let `.ds-modal-card` own centering.
Rule: Avoid redefining primitive-owned positioning styles in migrated feature shell classes.
Root cause: `.settings-window { position: relative; }` overrode `.ds-modal-card` absolute centering because `settings.css` loads after `ds-modal.css`.
Fix applied: Removed `position` and `z-index` from `.settings-window` so DS card positioning controls centering.
Prevention rule: When migrating existing shell classes onto DS primitives, avoid redeclaring layout-positioning properties (`position/top/left/transform`) already owned by the primitive.

## 2026-02-07 16:58
Context: Codex utility dedup refactor (`src-tauri/src/codex/mod.rs`)
Type: mistake
Event: A bulk file rewrite command truncated `codex/mod.rs` during refactor, temporarily dropping unrelated command handlers.
Action: Restored file from `HEAD` immediately and reapplied refactor using targeted replacements/patches only.
Rule: For large Rust modules, avoid full-file/head-tail rewrites unless line boundaries are verified; prefer function-scoped `apply_patch` edits.
Root cause: Used brittle line-count/head-tail rewrite workflow while file contents were changing.
Fix applied: Recovered from git snapshot and switched to explicit function-level patching.
Prevention rule: Use patch hunks anchored on function signatures for high-churn files and verify file length/function inventory after each structural edit.

## 2026-02-07 18:52
Context: Orbit sign-in Settings test stability
Type: mistake
Event: Initial Orbit sign-in test used fake timers with async polling and left the suite vulnerable to timer-state bleed/timeouts.
Action: Reworked the test to use an injected Orbit client prop with deterministic mocked responses and real timer waits.
Rule: For UI flows with delayed async polling, prefer dependency injection + deterministic mocks over fake timer orchestration unless timer control is strictly required.
Root cause: The test depended on module-level service references and fake timer scheduling that conflicted with React async update timing.
Fix applied: Added `orbitServiceClient` prop to `SettingsView`, switched test to pass explicit mock client, and removed fake timer manipulation.
Prevention rule: Keep side-effect service dependencies injectable for settings workflows so tests can validate behavior without global spies/timer hacks.

## 2026-02-07 19:08
Context: Orbit runner startup + settings token sync
Type: mistake
Event: Orbit runner startup resolved the wrong daemon binary name and Orbit auth actions updated backend token state without syncing `appSettings`, allowing stale token overwrite on later settings saves.
Action: Updated runner binary resolution to prioritize `codex_monitor_daemon` naming and added explicit token sync (`onUpdateAppSettings`) after Orbit sign-in authorization and sign-out.
Rule: For process launches and out-of-band settings mutations, keep UI state synchronized with backend writes and verify binary naming against packaged targets.
Root cause: Assumed hyphenated daemon executable naming and relied on draft-only token updates after backend-side token mutation.
Fix applied: Added candidate lookup for underscored daemon binary (with compatibility fallback) and implemented `syncRemoteBackendToken` in `SettingsView` plus regression tests.
Prevention rule: Validate executable names against real build outputs and treat auth token changes as persisted settings updates, not UI-only draft changes.

## 2026-02-07 19:21
Context: Orbit token sync follow-up regression
Type: mistake
Event: Token sync fix used stale `appSettings` snapshot during async sign-in polling, and URL token guard matched substring `token=` instead of exact query key.
Action: Switched token sync merge source to a live settings ref and changed URL query-key detection to exact parameter-name matching with fragment-safe append behavior.
Rule: Async settings writes must merge against latest state references, and query-parameter guards must match exact keys.
Root cause: Closure-captured props were reused after user edits, and string containment check was too loose for query parsing.
Fix applied: Added `latestSettingsRef`-based merge in `SettingsView`, plus exact query key parsing in `append_query` and expanded Orbit URL unit tests.
Prevention rule: For async UI flows, avoid merging with captured props; for URL query logic, parse keys explicitly instead of substring scans.

## 2026-02-07 19:39
Context: Orbit auth hardening follow-up (backend + shared URL/error helpers)
Type: mistake
Event: Orbit error-body truncation could panic on UTF-8 boundaries, websocket token query values were appended without URL encoding, and Orbit token persistence in app/daemon poll paths could overwrite newer settings snapshots.
Action: Made error excerpt truncation UTF-8-boundary safe, percent-encoded appended query components, and introduced shared `update_remote_backend_token_core` to persist token updates from latest settings state in both app and daemon.
Rule: For shared auth/network helpers, avoid raw byte string slicing and raw query interpolation, and persist token mutations through latest-state merge helpers rather than stale snapshots.
Root cause: Manual string slicing/interpolation shortcuts and copy-pasted token persistence logic between adapters.
Fix applied: Updated `shared/orbit_core.rs` and `shared/settings_core.rs`, then rewired `orbit/mod.rs` and daemon Orbit handlers to call the shared token updater.
Prevention rule: Keep app/daemon settings mutation logic centralized in shared core APIs and require edge-case tests for UTF-8 and reserved query characters.

## 2026-02-07 19:50
Context: Remote backend provider switch behavior
Type: mistake
Event: Switching remote provider in settings updated persisted config but left the in-memory remote transport cache active, so traffic continued over the old transport until restart/disconnect.
Action: Added transport-change detection in app settings update flow and clear `state.remote_backend` when transport-affecting fields change.
Rule: Any settings update that changes remote transport config must invalidate the cached remote backend client immediately.
Root cause: Remote client cache lifecycle was only tied to disconnect/errors, not to transport settings mutations.
Fix applied: Updated `src-tauri/src/settings/mod.rs` to compare previous vs updated transport settings and reset cached remote backend when they differ; added predicate unit tests.
Prevention rule: Treat transport-config settings as cache keys and invalidate on change at the backend boundary, not only from UI handlers.

## 2026-02-07 20:36
Context: Orbit token sync persistence retry behavior in Settings
Type: mistake
Event: `syncRemoteBackendToken` updated `latestSettingsRef` before settings persistence succeeded, so a failed save could make later retries no-op.
Action: Moved `latestSettingsRef` mutation to after successful `onUpdateAppSettings` completion and added a regression test for retry-after-failure.
Rule: In async settings flows, only advance local "latest" refs after persistence succeeds.
Root cause: Optimistically mutating in-memory settings state before awaiting durable save.
Fix applied: Updated `src/features/settings/components/SettingsView.tsx` token sync ordering and added `retries Orbit token persistence after a failed save` in `src/features/settings/components/SettingsView.test.tsx`.
Prevention rule: Keep optimistic UI drafts separate from persisted-settings refs and add explicit retry-path tests for async save failures.

## 2026-02-07 21:01
Context: Tailscale settings helper token preview freshness
Type: mistake
Event: Tailscale daemon command preview did not auto-refresh after remote token changes, leaving `tokenConfigured` warning state stale until manual refresh.
Action: Added `appSettings.remoteBackendToken` as a dependency of the auto-refresh effect in `SettingsView` so preview data is recomputed after token edits.
Rule: Any derived helper output that depends on settings values must include those values in effect dependencies (or equivalent invalidation paths).
Root cause: The effect dependency list tracked provider/mode only and omitted token changes used by preview generation.
Fix applied: Updated `src/features/settings/components/SettingsView.tsx` effect dependencies and revalidated `SettingsView` tests.
Prevention rule: When adding helper panels, explicitly audit dependency arrays against all backend inputs shown in that panel (especially token/auth state).

## 2026-02-07 21:03
Context: Tailscale helper auto-preview on mobile
Type: mistake
Event: Settings auto-fetched desktop-only Tailscale daemon command preview on mobile, creating immediate unsupported-error noise.
Action: Added `isMobilePlatform` helper and gated auto preview fetch in `SettingsView` to desktop platforms only.
Rule: Do not auto-run desktop-only diagnostics on mobile surfaces; gate by platform first.
Root cause: Auto-refresh effect was scoped by provider/mode only and assumed desktop capabilities.
Fix applied: Updated `src/features/settings/components/SettingsView.tsx` effect logic and added `src/utils/platformPaths.test.ts` coverage for mobile detection.
Prevention rule: For any auto-run settings helper, explicitly classify desktop-only vs cross-platform behavior before wiring useEffect refreshes.

## 2026-02-07 21:09
Context: Messages auto-scroll regression follow-up (thread switch)
Type: mistake
Event: Converting the auto-scroll effect to `useLayoutEffect` introduced an ordering bug where thread switches could skip initial re-pin if the previous thread was scrolled up.
Action: Switched thread-change `autoScrollRef` reset to `useLayoutEffect`, added `threadId` to auto-scroll layout effect dependencies, and added a regression test for thread-switch re-pin behavior.
Rule: When converting effects between `useEffect` and `useLayoutEffect`, preserve ordering guarantees for dependent refs across thread/navigation boundaries.
Root cause: `autoScrollRef.current = true` still ran in `useEffect` after the new layout scroll pass, so first render on thread switch could evaluate stale `false`.
Fix applied: Updated `src/features/messages/components/Messages.tsx` hook ordering/dependencies and added `re-pins to bottom on thread switch even when previous thread was scrolled up` in `src/features/messages/components/Messages.test.tsx`.
Prevention rule: For scroll/anchor refs, pair layout-timing ref resets with layout-timing consumers and add regression coverage for cross-thread transitions.

## 2026-02-07 21:14
Context: CI `test-js` failure (`platformPaths.test.ts`)
Type: mistake
Event: New mobile platform tests mutated `navigator` directly without ensuring a `navigator` object exists in Node test environments.
Action: Updated `withNavigatorValues` in `src/utils/platformPaths.test.ts` to create a temporary `globalThis.navigator` shim when missing, restore descriptors after each test, and clean up with `Reflect.deleteProperty`.
Rule: Node-targeted unit tests must not assume browser globals exist; create and tear down explicit shims in helper setup.
Root cause: The tests were authored assuming `navigator` is always available, but Vitest runs with `environment: node` in CI.
Fix applied: Added a global-scope navigator shim path and descriptor-safe restore logic in `src/utils/platformPaths.test.ts`.
Prevention rule: For tests that patch `navigator`, `window`, or `document`, guard setup with `typeof ... === \"undefined\"` and perform full teardown in `finally`.
