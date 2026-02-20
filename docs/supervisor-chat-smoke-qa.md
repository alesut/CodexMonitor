# Supervisor Chat Smoke QA

## Goal

Confirm critical Supervisor chat scenarios after UX copy updates.

## Scenario Matrix

1. Dispatch result summary
   - Coverage: `shared::supervisor_core::chat::tests::formats_dispatch_message_with_user_facing_copy`
   - Status: PASS
   - Checks:
     - User-facing dispatch summary is shown.
     - Technical route fields are not shown.
2. User input request
   - Coverage: `shared::supervisor_core::supervisor_loop::tests::child_question_is_bridged_and_marks_job_waiting_for_user`
   - Status: PASS
   - Checks:
     - Message explains what happened and why it matters.
     - Message includes explicit next action.
3. Approval request
   - Coverage: `shared::supervisor_core::supervisor_loop::tests::child_approval_is_bridged_with_next_action_message`
   - Status: PASS
   - Checks:
     - Approval message is actionable.
     - Next action is explicit.
4. Success/completion
   - Coverage:
     - `shared::supervisor_core::supervisor_loop::tests::child_final_result_is_bridged_into_supervisor_chat`
     - `shared::supervisor_core::service::tests::apply_dispatch_outcome_events_bridges_final_result_into_supervisor_chat`
   - Status: PASS
   - Checks:
     - Completion message uses unified copy.
     - Completion message includes next action guidance.
5. Failure
   - Coverage: `shared::supervisor_core::supervisor_loop::tests::child_error_is_bridged_and_marks_job_failed`
   - Status: PASS
   - Checks:
     - Failure message includes explicit follow-up action.
6. Lifecycle noise suppression
   - Coverage: `shared::supervisor_core::supervisor_loop::tests::suppresses_lifecycle_noise_messages_in_supervisor_chat`
   - Status: PASS
   - Checks:
     - Lifecycle-only events are not pushed into user-facing chat.
7. Technical prefix suppression
   - Coverage:
     - `shared::supervisor_core::supervisor_loop::tests::child_final_result_is_bridged_into_supervisor_chat`
     - `shared::supervisor_core::supervisor_loop::tests::child_question_is_bridged_and_marks_job_waiting_for_user`
     - `shared::supervisor_core::supervisor_loop::tests::child_error_is_bridged_and_marks_job_failed`
     - `shared::supervisor_core::service::tests::apply_dispatch_outcome_events_bridges_final_result_into_supervisor_chat`
   - Status: PASS
   - Checks:
     - Chat messages do not contain technical prefixes like `[subtask:... ws:... thread:...]`.

## Verification Commands

1. `cd /Users/alexey/Projects/Pet/CodexMonitor/src-tauri && cargo test --lib supervisor_core::`
2. `cd /Users/alexey/Projects/Pet/CodexMonitor && npm run typecheck`
3. `cd /Users/alexey/Projects/Pet/CodexMonitor/src-tauri && cargo check`

## Current Known Blocker

`cargo check` currently fails on unrelated daemon compile issue:
`/Users/alexey/Projects/Pet/CodexMonitor/src-tauri/src/bin/codex_monitor_daemon/telegram.rs:242` (`supervisor_chat_send` missing `client_version` argument).
