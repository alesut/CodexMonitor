# Supervisor Global Chat Plan (CM-013)

## Goal

Add a global Supervisor chat to operate CodexMonitor through text commands (and dictation), alongside the existing live operations view.

## Business Outcome

1. One control point for cross-workspace operations.
2. Faster response to incidents through chat commands.
3. Unified operations model: `Operations + Chat`.
4. Voice-assisted command input in Supervisor workflow.

## Core Use Cases

1. Dispatch one prompt to 2+ workspaces from one chat command.
2. Acknowledge critical Supervisor signals from chat.
3. Query global status and activity feed from chat.
4. Use dictation to author and submit Supervisor commands.
5. Restore Supervisor chat context after daemon restart.

## MVP Scope

1. Add structured command chat (no free-form NL planner in this phase).
2. Support commands:
   - `/dispatch --ws ws-1,ws-2 --prompt "..." [--thread ...] [--dedupe ...]`
   - `/ack <signal_id>`
   - `/status [workspace_id]`
   - `/feed [needs_input]`
   - `/help`
3. Persist and restore Supervisor chat history/state.

## Implementation Plan

1. Add Supervisor chat domain model in shared core.
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/shared/supervisor_core.rs`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/shared/supervisor_core/service.rs`

2. Add command parser/executor in shared core.
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/shared/supervisor_core/chat.rs` (new)
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/shared/supervisor_core/service.rs`

3. Expose backend parity surfaces (app + daemon).
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/supervisor.rs`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/lib.rs`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/bin/codex_monitor_daemon/rpc/supervisor.rs`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/bin/codex_monitor_daemon/rpc.rs`
   - Methods: `supervisor_chat_send`, `supervisor_chat_history`

4. Add typed frontend IPC wrappers.
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/services/tauri.ts`

5. Implement Supervisor chat UI and state hooks.
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/components/SupervisorChat.tsx` (new)
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/hooks/useSupervisorChat.ts` (new)
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/components/SupervisorHome.tsx`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/styles/supervisor.css`

6. Integrate dictation into Supervisor composer.
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/App.tsx`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/components/SupervisorChat.tsx`
   - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/hooks/useSupervisorChat.ts`

7. Add tests.
   - Frontend:
     - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/components/SupervisorChat.test.tsx` (new)
     - `/Users/alexey/Projects/Pet/CodexMonitor/src/features/supervisor/components/SupervisorHome.test.tsx`
   - Backend:
     - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/shared/supervisor_core/service.rs`
     - `/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/bin/codex_monitor_daemon.rs`

8. Update docs.
   - `/Users/alexey/Projects/Pet/CodexMonitor/docs/codebase-map.md`
   - `/Users/alexey/Projects/Pet/CodexMonitor/docs/multi-agent-sync-runbook.md`

## Definition of Done

1. `/dispatch` from Supervisor chat creates dispatch results in chat and operations state.
2. `/ack`, `/status`, `/feed` return consistent state with Supervisor core.
3. Dictation works in Supervisor chat composer.
4. Supervisor chat history/state survives daemon restart.
5. App/daemon parity and contract coverage are preserved.

## Validation Matrix

1. `npm run typecheck`
2. `npm run test`
3. `cd /Users/alexey/Projects/Pet/CodexMonitor/src-tauri && cargo check`
