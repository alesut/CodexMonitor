use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::events::{normalize_app_server_event, SupervisorEvent};
use super::{
    apply_update, SupervisorActivityEntry, SupervisorChatMessage, SupervisorChatMessageRole,
    SupervisorHealth, SupervisorJobState, SupervisorJobStatus, SupervisorOpenQuestion,
    SupervisorPendingApproval, SupervisorSignal, SupervisorSignalKind, SupervisorState,
    SupervisorStateUpdate, SupervisorSubtaskEvent, SupervisorThreadState, SupervisorThreadStatus,
    SupervisorWorkspaceState, DEFAULT_ACTIVITY_FEED_LIMIT, DEFAULT_CHAT_HISTORY_LIMIT,
};
use crate::backend::app_server::WorkspaceSession;
use crate::types::WorkspaceEntry;

pub(crate) const SUPERVISOR_HEALTH_TICK_MS: u64 = 10_000;
pub(crate) const SUPERVISOR_SUBTASK_EVENT_LIMIT: usize = 24;

pub(crate) fn now_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

pub(crate) async fn run_health_pull_tick(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    now_ms: i64,
) {
    let snapshots = collect_health_inputs(workspaces, sessions).await;
    let mut supervisor_loop = supervisor_loop.lock().await;
    supervisor_loop.run_health_check(&snapshots, now_ms);
}

async fn collect_health_inputs(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
) -> Vec<SupervisorWorkspaceHealthInput> {
    let connected_workspace_ids = {
        let sessions = sessions.lock().await;
        sessions.keys().cloned().collect::<HashSet<_>>()
    };
    let workspaces = workspaces.lock().await;

    workspaces
        .values()
        .map(|workspace| SupervisorWorkspaceHealthInput {
            workspace_id: workspace.id.clone(),
            workspace_name: Some(workspace.name.clone()),
            connected: connected_workspace_ids.contains(&workspace.id),
        })
        .collect::<Vec<_>>()
}

#[derive(Debug, Clone)]
pub(crate) struct SupervisorLoopConfig {
    pub(crate) stale_after_ms: i64,
    pub(crate) disconnected_after_ms: i64,
    pub(crate) activity_feed_limit: usize,
}

impl Default for SupervisorLoopConfig {
    fn default() -> Self {
        Self {
            stale_after_ms: 90_000,
            disconnected_after_ms: 300_000,
            activity_feed_limit: DEFAULT_ACTIVITY_FEED_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SupervisorWorkspaceHealthInput {
    pub(crate) workspace_id: String,
    pub(crate) workspace_name: Option<String>,
    pub(crate) connected: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SupervisorLoop {
    state: SupervisorState,
    config: SupervisorLoopConfig,
    workspace_last_event_at_ms: BTreeMap<String, i64>,
}

impl SupervisorLoop {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(config: SupervisorLoopConfig) -> Self {
        Self {
            state: SupervisorState::default(),
            config,
            workspace_last_event_at_ms: BTreeMap::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_state(config: SupervisorLoopConfig, state: SupervisorState) -> Self {
        let workspace_last_event_at_ms = state
            .workspaces
            .iter()
            .filter_map(|(workspace_id, workspace)| {
                workspace
                    .last_activity_at_ms
                    .map(|timestamp| (workspace_id.clone(), timestamp))
            })
            .collect::<BTreeMap<_, _>>();
        Self {
            state,
            config,
            workspace_last_event_at_ms,
        }
    }

    pub(crate) fn snapshot(&self) -> SupervisorState {
        self.state.clone()
    }

    pub(crate) fn apply_app_server_event(
        &mut self,
        workspace_id: &str,
        message: &Value,
        received_at_ms: i64,
    ) {
        self.record_workspace_heartbeat(workspace_id, received_at_ms);

        if let Some(event) = normalize_app_server_event(workspace_id, message, received_at_ms) {
            self.apply_supervisor_event(event);
            return;
        }

        let method = message
            .as_object()
            .and_then(|payload| payload.get("method"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if method == "codex/connected" {
            let mut workspace = self.workspace_state(workspace_id);
            workspace.connected = true;
            workspace.health = SupervisorHealth::Healthy;
            workspace.last_activity_at_ms = Some(received_at_ms);
            apply_update(
                &mut self.state,
                SupervisorStateUpdate::UpsertWorkspace(workspace),
            );
            self.push_activity(
                format!("connected:{workspace_id}:{received_at_ms}"),
                "workspace_connected",
                "Workspace connected".to_string(),
                Some(workspace_id.to_string()),
                None,
                false,
                received_at_ms,
                Value::Null,
            );
        }
    }

    pub(crate) fn run_health_check(
        &mut self,
        snapshots: &[SupervisorWorkspaceHealthInput],
        now_ms: i64,
    ) {
        for snapshot in snapshots {
            let previous_health = self
                .state
                .workspaces
                .get(&snapshot.workspace_id)
                .map(|workspace| workspace.health.clone())
                .unwrap_or_default();

            let next_health = self.compute_health(snapshot, now_ms);
            let last_activity = self
                .workspace_last_event_at_ms
                .get(&snapshot.workspace_id)
                .copied();

            let mut workspace = self.workspace_state(&snapshot.workspace_id);
            if let Some(name) = snapshot
                .workspace_name
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                workspace.name = name.to_string();
            }
            workspace.connected = snapshot.connected;
            workspace.health = next_health.clone();
            workspace.last_activity_at_ms = last_activity.or(workspace.last_activity_at_ms);
            apply_update(
                &mut self.state,
                SupervisorStateUpdate::UpsertWorkspace(workspace),
            );

            if previous_health == next_health {
                continue;
            }

            match next_health {
                SupervisorHealth::Stale => {
                    self.push_signal(
                        format!("health:{}:stalled", snapshot.workspace_id),
                        SupervisorSignalKind::Stalled,
                        Some(snapshot.workspace_id.clone()),
                        None,
                        None,
                        "Workspace is stale (no recent events).".to_string(),
                        now_ms,
                        json!({ "health": "stale" }),
                    );
                }
                SupervisorHealth::Disconnected => {
                    self.push_signal(
                        format!("health:{}:disconnected", snapshot.workspace_id),
                        SupervisorSignalKind::Disconnected,
                        Some(snapshot.workspace_id.clone()),
                        None,
                        None,
                        "Workspace appears disconnected.".to_string(),
                        now_ms,
                        json!({ "health": "disconnected" }),
                    );
                }
                SupervisorHealth::Healthy => {}
            }
        }
    }

    pub(crate) fn ack_signal(&mut self, signal_id: &str, acknowledged_at_ms: i64) {
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::AckSignal {
                signal_id: signal_id.to_string(),
                acknowledged_at_ms,
            },
        );
    }

    pub(crate) fn append_chat_message(&mut self, message: SupervisorChatMessage) {
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::PushChatMessage {
                message,
                max_items: DEFAULT_CHAT_HISTORY_LIMIT,
            },
        );
    }

    pub(crate) fn chat_history(&self) -> Vec<SupervisorChatMessage> {
        self.state.chat_history.clone()
    }

    pub(crate) fn upsert_job(&mut self, job: SupervisorJobState) {
        apply_update(&mut self.state, SupervisorStateUpdate::UpsertJob(job));
    }

    pub(crate) fn waiting_jobs(&self) -> Vec<SupervisorJobState> {
        let mut waiting = self
            .state
            .jobs
            .values()
            .filter(|job| {
                matches!(job.status, SupervisorJobStatus::WaitingForUser)
                    && job.waiting_request_id.is_some()
                    && !job.workspace_id.trim().is_empty()
            })
            .cloned()
            .collect::<Vec<_>>();
        waiting.sort_by(|left, right| right.requested_at_ms.cmp(&left.requested_at_ms));
        waiting
    }

    pub(crate) fn record_route_decision(
        &mut self,
        route_id: &str,
        message: String,
        created_at_ms: i64,
        metadata: Value,
    ) {
        self.push_activity(
            format!("route_decision:{route_id}:{created_at_ms}"),
            "route_decision",
            message,
            None,
            None,
            false,
            created_at_ms,
            metadata,
        );
    }

    pub(crate) fn mark_reply_delivered(
        &mut self,
        job_id: &str,
        request_id: &Value,
        reply_preview: &str,
        delivered_at_ms: i64,
    ) -> Result<(), String> {
        let Some(existing) = self.state.jobs.get(job_id).cloned() else {
            return Err(format!("subtask `{job_id}` is not tracked"));
        };
        if !matches!(existing.status, SupervisorJobStatus::WaitingForUser) {
            return Err(format!(
                "subtask `{job_id}` is not waiting for user input anymore"
            ));
        }
        let Some(waiting_request_id) = existing.waiting_request_id.as_ref() else {
            return Err(format!(
                "subtask `{job_id}` no longer has a pending request to answer"
            ));
        };
        if waiting_request_id != request_id {
            return Err(format!(
                "subtask `{job_id}` is waiting on a different request id"
            ));
        }

        let mut updated = existing.clone();
        updated.status = SupervisorJobStatus::Running;
        updated.waiting_request_id = None;
        updated.waiting_question_ids.clear();

        let request_key = request_value_key(&updated.workspace_id, request_id);
        let reply_summary = summarize_text(reply_preview, 180);
        Self::append_subtask_event(
            &mut updated,
            SupervisorSubtaskEvent {
                id: format!("reply_delivered:{job_id}:{request_key}"),
                kind: "reply_delivered".to_string(),
                message: format!("Reply delivered to child request `{request_key}`."),
                created_at_ms: delivered_at_ms,
                metadata: json!({
                    "requestKey": request_key,
                    "replySummary": reply_summary,
                }),
            },
        );

        apply_update(
            &mut self.state,
            SupervisorStateUpdate::UpsertJob(updated.clone()),
        );
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::ResolveOpenQuestion {
                question_id: request_key.clone(),
                resolved_at_ms: delivered_at_ms,
            },
        );

        self.push_activity(
            format!(
                "reply_delivered:{}:{request_key}:{delivered_at_ms}",
                updated.id
            ),
            "reply_delivered",
            format!("Reply delivered for subtask `{}`.", updated.id),
            Some(updated.workspace_id.clone()),
            updated.thread_id.clone(),
            false,
            delivered_at_ms,
            json!({
                "subtaskId": updated.id,
                "requestKey": request_key,
                "replySummary": reply_summary,
            }),
        );
        self.push_subtask_chat_message(
            &updated,
            format!("Reply delivered to child task request `{request_key}`. Continuing execution."),
            "reply_delivered",
            delivered_at_ms,
        );
        Ok(())
    }

    pub(crate) fn mark_reply_delivery_failed(
        &mut self,
        job_id: &str,
        request_id: &Value,
        error: &str,
        failed_at_ms: i64,
    ) {
        let Some(existing) = self.state.jobs.get(job_id).cloned() else {
            return;
        };
        let request_key = request_value_key(&existing.workspace_id, request_id);
        let mut updated = existing.clone();
        let added = Self::append_subtask_event(
            &mut updated,
            SupervisorSubtaskEvent {
                id: format!("reply_delivery_failed:{job_id}:{request_key}"),
                kind: "failed".to_string(),
                message: format!("Reply delivery failed: {error}"),
                created_at_ms: failed_at_ms,
                metadata: json!({
                    "requestKey": request_key,
                    "error": error,
                }),
            },
        );
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::UpsertJob(updated.clone()),
        );
        self.push_activity(
            format!("reply_delivery_failed:{job_id}:{request_key}:{failed_at_ms}"),
            "reply_delivery_failed",
            format!("Failed to deliver reply for subtask `{job_id}`."),
            Some(updated.workspace_id.clone()),
            updated.thread_id.clone(),
            true,
            failed_at_ms,
            json!({
                "subtaskId": job_id,
                "requestKey": request_key,
                "error": error,
            }),
        );
        if added {
            self.push_subtask_chat_message(
                &updated,
                format!("Reply delivery failed: {error}"),
                "reply_delivery_failed",
                failed_at_ms,
            );
        }
    }

    fn apply_supervisor_event(&mut self, event: SupervisorEvent) {
        match event {
            SupervisorEvent::TurnStarted {
                workspace_id,
                thread_id,
                turn_id,
                task,
                received_at_ms,
            } => {
                self.apply_thread_activity(
                    &workspace_id,
                    &thread_id,
                    Some(turn_id.clone()),
                    SupervisorThreadStatus::Running,
                    task.clone(),
                    received_at_ms,
                );
                self.push_activity(
                    format!("turn_started:{workspace_id}:{thread_id}:{turn_id}:{received_at_ms}"),
                    "turn_started",
                    "Turn started".to_string(),
                    Some(workspace_id.clone()),
                    Some(thread_id.clone()),
                    false,
                    received_at_ms,
                    json!({ "turnId": turn_id, "task": task }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, Some(&thread_id)) {
                    job.status = SupervisorJobStatus::Running;
                    job.thread_id = Some(thread_id.clone());
                    job.error = None;
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("turn_started:{}:{}:{turn_id}", workspace_id, thread_id),
                            kind: "running".to_string(),
                            message: format!("Turn `{turn_id}` started."),
                            created_at_ms: received_at_ms,
                            metadata: json!({ "turnId": turn_id, "task": task }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        self.push_subtask_chat_message(
                            &job,
                            format!("Progress update: turn `{turn_id}` started."),
                            "turn_started",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::TurnCompleted {
                workspace_id,
                thread_id,
                turn_id,
                task,
                received_at_ms,
            } => {
                self.apply_thread_activity(
                    &workspace_id,
                    &thread_id,
                    None,
                    SupervisorThreadStatus::Completed,
                    task.clone(),
                    received_at_ms,
                );
                self.push_signal(
                    format!("turn:{}:{}:{}:completed", workspace_id, thread_id, turn_id),
                    SupervisorSignalKind::Completed,
                    Some(workspace_id.clone()),
                    Some(thread_id.clone()),
                    None,
                    "Turn completed".to_string(),
                    received_at_ms,
                    json!({ "turnId": turn_id, "task": task }),
                );
                self.push_activity(
                    format!("turn_completed:{workspace_id}:{thread_id}:{turn_id}:{received_at_ms}"),
                    "turn_completed",
                    "Turn completed".to_string(),
                    Some(workspace_id.clone()),
                    Some(thread_id.clone()),
                    false,
                    received_at_ms,
                    json!({ "turnId": turn_id, "task": task }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, Some(&thread_id)) {
                    job.status = SupervisorJobStatus::Completed;
                    job.completed_at_ms = Some(received_at_ms);
                    job.waiting_request_id = None;
                    job.waiting_question_ids.clear();
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("turn_completed:{}:{}:{turn_id}", workspace_id, thread_id),
                            kind: "completed".to_string(),
                            message: "Turn completed".to_string(),
                            created_at_ms: received_at_ms,
                            metadata: json!({ "turnId": turn_id, "task": task }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        self.push_subtask_chat_message(
                            &job,
                            "Subtask completed. Next action: review result and send follow-up instructions if needed."
                                .to_string(),
                            "turn_completed",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::ItemStarted {
                workspace_id,
                thread_id,
                item_id,
                item_type,
                task,
                item_content,
                received_at_ms,
            } => {
                self.apply_thread_activity(
                    &workspace_id,
                    &thread_id,
                    None,
                    SupervisorThreadStatus::Running,
                    task.clone(),
                    received_at_ms,
                );
                self.push_activity(
                    format!("item_started:{workspace_id}:{thread_id}:{item_id}:{received_at_ms}"),
                    "item_started",
                    "Item started".to_string(),
                    Some(workspace_id.clone()),
                    Some(thread_id.clone()),
                    false,
                    received_at_ms,
                    json!({
                        "itemId": item_id,
                        "itemType": item_type,
                        "task": task,
                        "itemContent": item_content,
                    }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, Some(&thread_id)) {
                    job.status = SupervisorJobStatus::Running;
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("item_started:{}:{}:{item_id}", workspace_id, thread_id),
                            kind: "running".to_string(),
                            message: format!(
                                "Item `{}` started.",
                                item_type.as_deref().unwrap_or("unknown").trim()
                            ),
                            created_at_ms: received_at_ms,
                            metadata: json!({
                                "itemId": item_id,
                                "itemType": item_type,
                                "task": task,
                                "itemContent": item_content,
                            }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        self.push_subtask_chat_message(
                            &job,
                            format!(
                                "Progress update: item `{}` started.",
                                item_type.as_deref().unwrap_or("unknown").trim()
                            ),
                            "item_started",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::ItemCompleted {
                workspace_id,
                thread_id,
                item_id,
                item_type,
                task,
                item_content,
                received_at_ms,
            } => {
                self.apply_thread_activity(
                    &workspace_id,
                    &thread_id,
                    None,
                    SupervisorThreadStatus::Running,
                    task.clone(),
                    received_at_ms,
                );
                self.push_activity(
                    format!("item_completed:{workspace_id}:{thread_id}:{item_id}:{received_at_ms}"),
                    "item_completed",
                    "Item completed".to_string(),
                    Some(workspace_id.clone()),
                    Some(thread_id.clone()),
                    false,
                    received_at_ms,
                    json!({
                        "itemId": item_id,
                        "itemType": item_type,
                        "task": task,
                        "itemContent": item_content,
                    }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, Some(&thread_id)) {
                    job.status = SupervisorJobStatus::Running;
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("item_completed:{}:{}:{item_id}", workspace_id, thread_id),
                            kind: "running".to_string(),
                            message: format!(
                                "Item `{}` completed.",
                                item_type.as_deref().unwrap_or("unknown").trim()
                            ),
                            created_at_ms: received_at_ms,
                            metadata: json!({
                                "itemId": item_id,
                                "itemType": item_type,
                                "task": task,
                                "itemContent": item_content,
                            }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        let mut chat_message = format!(
                            "Progress update: item `{}` completed.",
                            item_type.as_deref().unwrap_or("unknown").trim()
                        );
                        if item_type
                            .as_deref()
                            .is_some_and(|value| value.eq_ignore_ascii_case("agentMessage"))
                        {
                            if let Some(content) = item_content.as_deref() {
                                chat_message =
                                    format!("Agent response: {}", summarize_text(content, 240));
                            }
                        }
                        self.push_subtask_chat_message(
                            &job,
                            chat_message,
                            "item_completed",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::UserInputRequested {
                workspace_id,
                request_key,
                request_id,
                request_id_value,
                thread_id,
                turn_id,
                item_id,
                question,
                question_ids,
                params,
                received_at_ms,
            } => {
                apply_update(
                    &mut self.state,
                    SupervisorStateUpdate::UpsertOpenQuestion(SupervisorOpenQuestion {
                        id: request_key.clone(),
                        workspace_id: workspace_id.clone(),
                        thread_id: thread_id.clone().unwrap_or_else(|| "-".to_string()),
                        question: question.clone(),
                        created_at_ms: received_at_ms,
                        resolved_at_ms: None,
                        context: json!({
                            "requestId": request_id_value,
                            "turnId": turn_id,
                            "itemId": item_id,
                            "questionIds": question_ids,
                            "params": params,
                        }),
                    }),
                );
                self.push_activity(
                    format!("waiting_for_user:{request_key}:{received_at_ms}"),
                    "waiting_for_user",
                    "Child task is waiting for user input".to_string(),
                    Some(workspace_id.clone()),
                    thread_id.clone(),
                    true,
                    received_at_ms,
                    json!({
                        "requestKey": request_key,
                        "requestId": request_id,
                        "turnId": turn_id,
                        "itemId": item_id,
                        "question": question,
                        "questionIds": question_ids,
                        "params": params,
                    }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, thread_id.as_deref()) {
                    job.status = SupervisorJobStatus::WaitingForUser;
                    job.waiting_request_id = Some(request_id_value.clone());
                    job.waiting_question_ids = question_ids.clone();
                    let job_id = job.id.clone();
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("waiting_for_user:{job_id}:{request_key}"),
                            kind: "waiting_for_user".to_string(),
                            message: format!("Child question: {question}"),
                            created_at_ms: received_at_ms,
                            metadata: json!({
                                "requestKey": request_key,
                                "requestId": request_id,
                                "questionIds": question_ids,
                            }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        self.push_subtask_chat_message(
                            &job,
                            format!(
                                "Child task asks: {question}\nReply in this chat to continue (subtask `{}`).",
                                job.id
                            ),
                            "waiting_for_user",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::ApprovalRequested {
                workspace_id,
                request_key,
                request_id,
                method,
                thread_id,
                turn_id,
                item_id,
                params,
                received_at_ms,
            } => {
                apply_update(
                    &mut self.state,
                    SupervisorStateUpdate::UpsertPendingApproval(SupervisorPendingApproval {
                        request_key: request_key.clone(),
                        workspace_id: workspace_id.clone(),
                        thread_id: thread_id.clone(),
                        turn_id,
                        item_id: item_id.clone(),
                        request_id,
                        method,
                        params: params.clone(),
                        created_at_ms: received_at_ms,
                        resolved_at_ms: None,
                    }),
                );

                self.push_signal(
                    format!("approval:{request_key}"),
                    SupervisorSignalKind::NeedsApproval,
                    Some(workspace_id.clone()),
                    thread_id.clone(),
                    None,
                    "Action requires approval".to_string(),
                    received_at_ms,
                    json!({ "requestKey": request_key }),
                );

                self.push_activity(
                    format!("approval:{request_key}:{received_at_ms}"),
                    "needs_approval",
                    "Approval requested".to_string(),
                    Some(workspace_id.clone()),
                    thread_id.clone(),
                    true,
                    received_at_ms,
                    params,
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, thread_id.as_deref()) {
                    job.status = SupervisorJobStatus::WaitingForUser;
                    let job_id = job.id.clone();
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!("approval_requested:{job_id}:{request_key}"),
                            kind: "waiting_for_user".to_string(),
                            message: "Approval requested".to_string(),
                            created_at_ms: received_at_ms,
                            metadata: json!({ "requestKey": request_key }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        self.push_subtask_chat_message(
                            &job,
                            "Child task requires approval before it can continue.".to_string(),
                            "needs_approval",
                            received_at_ms,
                        );
                    }
                }
            }
            SupervisorEvent::Error {
                workspace_id,
                thread_id,
                turn_id,
                message,
                will_retry,
                received_at_ms,
            } => {
                if let Some(thread_id_value) = thread_id.as_ref() {
                    self.apply_thread_activity(
                        &workspace_id,
                        thread_id_value,
                        turn_id.clone(),
                        SupervisorThreadStatus::Failed,
                        None,
                        received_at_ms,
                    );
                }

                self.push_signal(
                    format!(
                        "error:{}:{}:{}",
                        workspace_id,
                        thread_id.clone().unwrap_or_default(),
                        turn_id.clone().unwrap_or_default()
                    ),
                    SupervisorSignalKind::Failed,
                    Some(workspace_id.clone()),
                    thread_id.clone(),
                    None,
                    message.clone(),
                    received_at_ms,
                    json!({ "willRetry": will_retry, "turnId": turn_id }),
                );

                self.push_activity(
                    format!(
                        "error:{}:{}:{}:{}",
                        workspace_id,
                        thread_id.clone().unwrap_or_default(),
                        turn_id.clone().unwrap_or_default(),
                        received_at_ms
                    ),
                    "error",
                    message.clone(),
                    Some(workspace_id.clone()),
                    thread_id.clone(),
                    false,
                    received_at_ms,
                    json!({ "willRetry": will_retry, "turnId": turn_id }),
                );

                if let Some(mut job) = self.job_for_event(&workspace_id, thread_id.as_deref()) {
                    job.status = if will_retry {
                        SupervisorJobStatus::Running
                    } else {
                        SupervisorJobStatus::Failed
                    };
                    if !will_retry {
                        job.error = Some(message.clone());
                    }
                    let added = Self::append_subtask_event(
                        &mut job,
                        SupervisorSubtaskEvent {
                            id: format!(
                                "error:{}:{}:{}",
                                workspace_id,
                                thread_id.as_deref().unwrap_or_default(),
                                turn_id.as_deref().unwrap_or_default()
                            ),
                            kind: "failed".to_string(),
                            message: message.clone(),
                            created_at_ms: received_at_ms,
                            metadata: json!({ "willRetry": will_retry, "turnId": turn_id }),
                        },
                    );
                    apply_update(
                        &mut self.state,
                        SupervisorStateUpdate::UpsertJob(job.clone()),
                    );
                    if added {
                        let chat_message = if will_retry {
                            format!("Child task reported an error but will retry: {message}")
                        } else {
                            format!("Child task failed: {message}")
                        };
                        self.push_subtask_chat_message(&job, chat_message, "error", received_at_ms);
                    }
                }
            }
        }
    }

    fn compute_health(
        &self,
        snapshot: &SupervisorWorkspaceHealthInput,
        now_ms: i64,
    ) -> SupervisorHealth {
        if !snapshot.connected {
            return SupervisorHealth::Disconnected;
        }

        let Some(last_activity) = self
            .workspace_last_event_at_ms
            .get(&snapshot.workspace_id)
            .copied()
        else {
            return SupervisorHealth::Stale;
        };

        let age = now_ms.saturating_sub(last_activity);
        if age >= self.config.disconnected_after_ms {
            SupervisorHealth::Disconnected
        } else if age >= self.config.stale_after_ms {
            SupervisorHealth::Stale
        } else {
            SupervisorHealth::Healthy
        }
    }

    fn record_workspace_heartbeat(&mut self, workspace_id: &str, timestamp_ms: i64) {
        self.workspace_last_event_at_ms
            .insert(workspace_id.to_string(), timestamp_ms);
    }

    fn workspace_state(&self, workspace_id: &str) -> SupervisorWorkspaceState {
        self.state
            .workspaces
            .get(workspace_id)
            .cloned()
            .unwrap_or_else(|| SupervisorWorkspaceState {
                id: workspace_id.to_string(),
                ..SupervisorWorkspaceState::default()
            })
    }

    fn thread_state(&self, workspace_id: &str, thread_id: &str) -> SupervisorThreadState {
        let key = super::thread_map_key(workspace_id, thread_id);
        self.state
            .threads
            .get(&key)
            .cloned()
            .unwrap_or_else(|| SupervisorThreadState {
                id: thread_id.to_string(),
                workspace_id: workspace_id.to_string(),
                ..SupervisorThreadState::default()
            })
    }

    fn job_for_event(
        &self,
        workspace_id: &str,
        thread_id: Option<&str>,
    ) -> Option<SupervisorJobState> {
        let mut candidates = self
            .state
            .jobs
            .values()
            .filter(|job| job.workspace_id == workspace_id)
            .filter(|job| {
                if let Some(thread_id) = thread_id {
                    return job.thread_id.as_deref() == Some(thread_id);
                }
                true
            })
            .cloned()
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let left_priority = if left.status.is_terminal() { 0i8 } else { 1i8 };
            let right_priority = if right.status.is_terminal() { 0i8 } else { 1i8 };
            right_priority
                .cmp(&left_priority)
                .then_with(|| right.requested_at_ms.cmp(&left.requested_at_ms))
                .then_with(|| left.id.cmp(&right.id))
        });
        candidates.into_iter().next()
    }

    fn append_subtask_event(job: &mut SupervisorJobState, event: SupervisorSubtaskEvent) -> bool {
        if job.recent_events.iter().any(|entry| entry.id == event.id) {
            return false;
        }
        job.recent_events.push(event);
        if job.recent_events.len() > SUPERVISOR_SUBTASK_EVENT_LIMIT {
            let to_drop = job.recent_events.len() - SUPERVISOR_SUBTASK_EVENT_LIMIT;
            job.recent_events.drain(0..to_drop);
        }
        true
    }

    fn push_subtask_chat_message(
        &mut self,
        job: &SupervisorJobState,
        message: String,
        kind: &str,
        created_at_ms: i64,
    ) {
        let thread_label = job.thread_id.as_deref().unwrap_or("-");
        let prefix = format!(
            "[subtask:{} ws:{} thread:{}]",
            job.id, job.workspace_id, thread_label
        );
        self.append_chat_message(SupervisorChatMessage {
            id: format!("chat-bridge:{kind}:{}:{created_at_ms}", job.id),
            role: SupervisorChatMessageRole::System,
            text: format!("{prefix} {message}"),
            created_at_ms,
        });
    }

    fn apply_thread_activity(
        &mut self,
        workspace_id: &str,
        thread_id: &str,
        active_turn_id: Option<String>,
        status: SupervisorThreadStatus,
        task: Option<String>,
        timestamp_ms: i64,
    ) {
        self.record_workspace_heartbeat(workspace_id, timestamp_ms);

        let mut workspace = self.workspace_state(workspace_id);
        workspace.connected = true;
        workspace.health = SupervisorHealth::Healthy;
        workspace.last_activity_at_ms = Some(timestamp_ms);
        workspace.active_thread_id = Some(thread_id.to_string());
        if task.is_some() {
            workspace.current_task = task.clone();
        }
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::UpsertWorkspace(workspace),
        );

        let mut thread = self.thread_state(workspace_id, thread_id);
        thread.status = status;
        thread.last_activity_at_ms = Some(timestamp_ms);
        if let Some(task) = task {
            thread.current_task = Some(task);
        }
        thread.active_turn_id = active_turn_id;
        apply_update(&mut self.state, SupervisorStateUpdate::UpsertThread(thread));
    }

    fn push_activity(
        &mut self,
        id: String,
        kind: &str,
        message: String,
        workspace_id: Option<String>,
        thread_id: Option<String>,
        needs_input: bool,
        created_at_ms: i64,
        metadata: Value,
    ) {
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::PushActivity {
                entry: SupervisorActivityEntry {
                    id,
                    kind: kind.to_string(),
                    message,
                    created_at_ms,
                    workspace_id,
                    thread_id,
                    needs_input,
                    metadata,
                },
                max_items: self.config.activity_feed_limit,
            },
        );
    }

    fn push_signal(
        &mut self,
        id: String,
        kind: SupervisorSignalKind,
        workspace_id: Option<String>,
        thread_id: Option<String>,
        job_id: Option<String>,
        message: String,
        created_at_ms: i64,
        context: Value,
    ) {
        apply_update(
            &mut self.state,
            SupervisorStateUpdate::PushSignal(SupervisorSignal {
                id,
                kind,
                workspace_id,
                thread_id,
                job_id,
                message,
                created_at_ms,
                acknowledged_at_ms: None,
                context,
            }),
        );
    }
}

fn summarize_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let summary = trimmed.chars().take(max_chars).collect::<String>();
    format!("{summary}...")
}

fn request_value_key(workspace_id: &str, request_id: &Value) -> String {
    let request_id_value = request_id
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| request_id.as_i64().map(|value| value.to_string()))
        .or_else(|| request_id.as_u64().map(|value| value.to_string()))
        .unwrap_or_else(|| request_id.to_string());
    format!("{workspace_id}:{request_id_value}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tracked_running_job(
        job_id: &str,
        workspace_id: &str,
        thread_id: &str,
    ) -> SupervisorJobState {
        SupervisorJobState {
            id: job_id.to_string(),
            workspace_id: workspace_id.to_string(),
            thread_id: Some(thread_id.to_string()),
            description: "Tracked subtask".to_string(),
            status: SupervisorJobStatus::Running,
            requested_at_ms: 1,
            started_at_ms: Some(2),
            ..Default::default()
        }
    }

    #[test]
    fn push_approval_event_updates_pending_approvals_and_signals() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig::default());

        loop_state.apply_app_server_event(
            "ws-1",
            &json!({
                "id": 42,
                "method": "workspace/requestApproval",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "item-1",
                    "mode": "full"
                }
            }),
            100,
        );

        let snapshot = loop_state.snapshot();
        assert!(snapshot.pending_approvals.contains_key("ws-1:42"));
        assert_eq!(snapshot.signals.len(), 1);
        assert_eq!(
            snapshot.signals[0].kind,
            SupervisorSignalKind::NeedsApproval
        );
        assert_eq!(snapshot.activity_feed[0].kind, "needs_approval");
        assert!(snapshot.activity_feed[0].needs_input);
    }

    #[test]
    fn pull_health_check_emits_stale_and_disconnected_signals_once() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig {
            stale_after_ms: 10,
            disconnected_after_ms: 20,
            activity_feed_limit: 100,
        });

        loop_state.apply_app_server_event(
            "ws-health",
            &json!({
                "method": "turn/started",
                "params": {
                    "threadId": "thread-health",
                    "turnId": "turn-health"
                }
            }),
            100,
        );

        let input = vec![SupervisorWorkspaceHealthInput {
            workspace_id: "ws-health".to_string(),
            workspace_name: Some("Health Workspace".to_string()),
            connected: true,
        }];

        loop_state.run_health_check(&input, 105);
        assert_eq!(loop_state.snapshot().signals.len(), 0);

        loop_state.run_health_check(&input, 112);
        let stale_snapshot = loop_state.snapshot();
        assert_eq!(stale_snapshot.signals.len(), 1);
        assert_eq!(
            stale_snapshot.signals[0].kind,
            SupervisorSignalKind::Stalled
        );

        loop_state.run_health_check(&input, 113);
        assert_eq!(loop_state.snapshot().signals.len(), 1);

        loop_state.run_health_check(&input, 125);
        let disconnected_snapshot = loop_state.snapshot();
        assert_eq!(disconnected_snapshot.signals.len(), 2);
        assert_eq!(
            disconnected_snapshot.signals[0].kind,
            SupervisorSignalKind::Disconnected
        );

        loop_state.run_health_check(&input, 126);
        assert_eq!(loop_state.snapshot().signals.len(), 2);
    }

    #[test]
    fn turn_and_item_events_update_thread_runtime_state() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig::default());

        loop_state.apply_app_server_event(
            "ws-2",
            &json!({
                "method": "turn/started",
                "params": {
                    "threadId": "thread-2",
                    "turnId": "turn-2",
                    "currentTask": "Investigate"
                }
            }),
            10,
        );

        loop_state.apply_app_server_event(
            "ws-2",
            &json!({
                "method": "item/started",
                "params": {
                    "threadId": "thread-2",
                    "itemId": "item-2",
                    "title": "Run tests"
                }
            }),
            11,
        );

        loop_state.apply_app_server_event(
            "ws-2",
            &json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-2",
                    "turnId": "turn-2"
                }
            }),
            20,
        );

        let snapshot = loop_state.snapshot();
        let thread = snapshot
            .threads
            .get("ws-2:thread-2")
            .expect("thread should exist");
        assert_eq!(thread.status, SupervisorThreadStatus::Completed);
        assert_eq!(thread.active_turn_id, None);
        assert_eq!(thread.last_activity_at_ms, Some(20));

        let workspace = snapshot
            .workspaces
            .get("ws-2")
            .expect("workspace should exist");
        assert_eq!(workspace.health, SupervisorHealth::Healthy);
        assert_eq!(workspace.active_thread_id.as_deref(), Some("thread-2"));
    }

    #[test]
    fn child_final_result_is_bridged_into_supervisor_chat() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig::default());
        loop_state.upsert_job(tracked_running_job("job-1", "ws-1", "thread-1"));

        loop_state.apply_app_server_event(
            "ws-1",
            &json!({
                "method": "item/completed",
                "params": {
                    "threadId": "thread-1",
                    "itemId": "item-1",
                    "item": {
                        "id": "item-1",
                        "type": "agentMessage",
                        "text": "Deployment finished successfully"
                    }
                }
            }),
            10,
        );
        loop_state.apply_app_server_event(
            "ws-1",
            &json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "summary": "Deployment finished successfully"
                }
            }),
            11,
        );

        let snapshot = loop_state.snapshot();
        let job = snapshot.jobs.get("job-1").expect("job should exist");
        assert_eq!(job.status, SupervisorJobStatus::Completed);
        assert!(
            job.recent_events
                .iter()
                .any(|event| event.kind == "completed"),
            "expected completed event in recent subtask events"
        );

        let history = loop_state.chat_history();
        assert!(
            history.iter().any(|message| message
                .text
                .contains("Agent response: Deployment finished successfully")),
            "expected bridged agent result in supervisor chat"
        );
        assert!(
            history
                .iter()
                .any(|message| message.text.contains("Subtask completed.")),
            "expected completion summary in supervisor chat"
        );
    }

    #[test]
    fn child_question_is_bridged_and_marks_job_waiting_for_user() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig::default());
        loop_state.upsert_job(tracked_running_job("job-2", "ws-2", "thread-2"));

        loop_state.apply_app_server_event(
            "ws-2",
            &json!({
                "id": "req-7",
                "method": "item/tool/requestUserInput",
                "params": {
                    "threadId": "thread-2",
                    "turnId": "turn-2",
                    "itemId": "item-2",
                    "questions": [
                        { "id": "q-1", "question": "Should I restart the service?" }
                    ]
                }
            }),
            20,
        );

        let snapshot = loop_state.snapshot();
        let job = snapshot.jobs.get("job-2").expect("job should exist");
        assert_eq!(job.status, SupervisorJobStatus::WaitingForUser);
        assert_eq!(job.waiting_request_id.as_ref(), Some(&json!("req-7")));
        assert_eq!(job.waiting_question_ids, vec!["q-1".to_string()]);
        assert!(snapshot.open_questions.contains_key("ws-2:req-7"));

        let history = loop_state.chat_history();
        assert!(
            history.iter().any(|message| message
                .text
                .contains("Child task asks: Should I restart the service?")),
            "expected bridged child clarification in supervisor chat"
        );
    }

    #[test]
    fn child_error_is_bridged_and_marks_job_failed() {
        let mut loop_state = SupervisorLoop::new(SupervisorLoopConfig::default());
        loop_state.upsert_job(tracked_running_job("job-3", "ws-3", "thread-3"));

        loop_state.apply_app_server_event(
            "ws-3",
            &json!({
                "method": "error",
                "params": {
                    "threadId": "thread-3",
                    "turnId": "turn-3",
                    "error": { "message": "Build failed on step test" },
                    "willRetry": false
                }
            }),
            30,
        );

        let snapshot = loop_state.snapshot();
        let job = snapshot.jobs.get("job-3").expect("job should exist");
        assert_eq!(job.status, SupervisorJobStatus::Failed);
        assert_eq!(job.error.as_deref(), Some("Build failed on step test"));

        let history = loop_state.chat_history();
        assert!(
            history.iter().any(|message| message
                .text
                .contains("Child task failed: Build failed on step test")),
            "expected bridged child failure in supervisor chat"
        );
    }
}
