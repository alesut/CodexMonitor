# Supervisor Layer + Live Operations View (MVP)

## Epic Goal

Add a Supervisor layer to CodexMonitor: one global Supervisor chat plus a live operations panel that shows what is currently happening across all workspaces and threads, and can dispatch work to multiple workspace threads through `codex app-server`.

## Problem

Current workflow is fragmented across workspace-specific threads. There is no single operational view or orchestration layer for cross-workspace execution.

## MVP Scope

1. Add `Supervisor` section in UI with global chat and Operations panel.
2. Add shared Supervisor state model:
   - `workspaces`
   - `threads`
   - `jobs`
   - `signals`
   - `activity_feed`
   - `open_questions`
   - `pending_approvals`
3. Implement Supervisor loop:
   - push updates from app-server events
   - pull health checks on interval
4. Implement dispatch flow for workspace actions:
   - select/create target thread
   - `thread/resume`
   - `turn/start`
5. Add user-facing notifications/signals:
   - `needs_approval`
   - `failed`
   - `completed`
   - `stalled`
   - `disconnected`
6. Define Planner-to-Executor structured action contract (JSON).

## Out of Scope

1. Full dynamic tools orchestration (experimental path).
2. Advanced autonomous prioritization/optimization.
3. Deep external integration set beyond basic polling.

## Acceptance Criteria

1. Supervisor UI shows per workspace/thread:
   - current task
   - last activity
   - next expected step
   - blockers
2. One Supervisor request can dispatch actions to at least 2 workspaces in one cycle.
3. Unified cross-workspace activity feed exists, including "needs my input" filtering.
4. `needs_approval` and `failed` signals produce notifications with actionable context.
5. Operations state is restored after restart when daemon mode is enabled.
6. App/Daemon parity is preserved for backend behavior and contracts.

## Tickets

### P0

1. `CM-001` Supervisor domain model in shared core
   - Files:
     - `src-tauri/src/shared/supervisor_core.rs`
     - `src-tauri/src/shared/mod.rs`
   - Deliverable: canonical aggregate state types and pure reducer/update functions.

2. `CM-002` App-server event normalization
   - Files:
     - `src-tauri/src/shared/supervisor_core/events.rs`
   - Deliverable: normalized `SupervisorEvent` mapping for turn/item/approval/error events.

3. `CM-003` Supervisor loop (push + pull)
   - Files:
     - `src-tauri/src/shared/supervisor_core/loop.rs`
   - Deliverable: real-time state updates plus stale/disconnect signal detection.

4. `CM-004` Loop lifecycle integration in app and daemon
   - Files:
     - `src-tauri/src/lib.rs`
     - `src-tauri/src/bin/codex_monitor_daemon.rs`
   - Deliverable: shared loop lifecycle used identically in both runtimes.

5. `CM-005` Workspace dispatch executor
   - Files:
     - `src-tauri/src/shared/supervisor_core/dispatch.rs`
   - Deliverable: idempotent multi-workspace dispatch via `thread/resume` and `turn/start`.

6. `CM-006` Planner-to-Executor action contract
   - Files:
     - `src-tauri/src/shared/supervisor_core/contract.rs`
   - Deliverable: validated JSON action schema and rejection of invalid actions.

7. `CM-007` Backend surface (Tauri commands + daemon RPC)
   - Files:
     - `src-tauri/src/lib.rs`
     - `src-tauri/src/bin/codex_monitor_daemon/rpc.rs`
     - `src-tauri/src/bin/codex_monitor_daemon/rpc/*`
   - Deliverable: `supervisor_snapshot`, `supervisor_feed`, `supervisor_dispatch`, `supervisor_ack_signal`.

8. `CM-008` Frontend IPC wrapper + event fanout
   - Files:
     - `src/services/tauri.ts`
     - `src/services/events.ts`
   - Deliverable: typed Supervisor IPC access only through service layer.

9. `CM-009` Supervisor UI shell
   - Files:
     - `src/App.tsx`
     - `src/features/app/*`
     - `src/features/supervisor/*`
   - Deliverable: route/entry/sidebar integration and snapshot rendering.

10. `CM-010` Live Operations View
   - Files:
     - `src/features/supervisor/components/*`
   - Deliverable: real-time operational visibility across workspaces and threads.

### P1

11. `CM-011` Notifications and escalation UX
   - Files:
     - `src/features/app/*`
     - `src/features/supervisor/*`
   - Deliverable: actionable badges/toasts for critical signals.

12. `CM-012` Persistence, restore, docs, tests
   - Files:
     - `docs/codebase-map.md`
     - `docs/multi-agent-sync-runbook.md`
     - backend/frontend test suites for Supervisor units
   - Deliverable: state restore plus baseline coverage for reducer/dispatch/contract.

## Implementation Order (Fast MVP)

1. `CM-001`
2. `CM-002`
3. `CM-003`
4. `CM-007`
5. `CM-008`
6. `CM-009`
