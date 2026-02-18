use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const DEFAULT_ACTIVITY_FEED_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorHealth {
    #[default]
    Healthy,
    Stale,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorThreadStatus {
    #[default]
    Idle,
    Running,
    WaitingInput,
    Failed,
    Completed,
    Stalled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorJobStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorSignalKind {
    NeedsApproval,
    Failed,
    Completed,
    Stalled,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct SupervisorWorkspaceState {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) connected: bool,
    #[serde(default)]
    pub(crate) current_task: Option<String>,
    #[serde(default)]
    pub(crate) last_activity_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) next_expected_step: Option<String>,
    #[serde(default)]
    pub(crate) blockers: Vec<String>,
    #[serde(default)]
    pub(crate) health: SupervisorHealth,
    #[serde(default)]
    pub(crate) active_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct SupervisorThreadState {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) status: SupervisorThreadStatus,
    #[serde(default)]
    pub(crate) current_task: Option<String>,
    #[serde(default)]
    pub(crate) last_activity_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) next_expected_step: Option<String>,
    #[serde(default)]
    pub(crate) blockers: Vec<String>,
    #[serde(default)]
    pub(crate) active_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct SupervisorJobState {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) dedupe_key: Option<String>,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) status: SupervisorJobStatus,
    #[serde(default)]
    pub(crate) requested_at_ms: i64,
    #[serde(default)]
    pub(crate) started_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) completed_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SupervisorSignal {
    pub(crate) id: String,
    pub(crate) kind: SupervisorSignalKind,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) job_id: Option<String>,
    #[serde(default)]
    pub(crate) message: String,
    pub(crate) created_at_ms: i64,
    #[serde(default)]
    pub(crate) acknowledged_at_ms: Option<i64>,
    #[serde(default = "default_json_null")]
    pub(crate) context: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SupervisorActivityEntry {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) message: String,
    pub(crate) created_at_ms: i64,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) needs_input: bool,
    #[serde(default = "default_json_null")]
    pub(crate) metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SupervisorOpenQuestion {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    pub(crate) thread_id: String,
    pub(crate) question: String,
    pub(crate) created_at_ms: i64,
    #[serde(default)]
    pub(crate) resolved_at_ms: Option<i64>,
    #[serde(default = "default_json_null")]
    pub(crate) context: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SupervisorPendingApproval {
    pub(crate) request_key: String,
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) turn_id: Option<String>,
    #[serde(default)]
    pub(crate) item_id: Option<String>,
    pub(crate) request_id: String,
    pub(crate) method: String,
    #[serde(default = "default_json_null")]
    pub(crate) params: Value,
    pub(crate) created_at_ms: i64,
    #[serde(default)]
    pub(crate) resolved_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct SupervisorState {
    #[serde(default)]
    pub(crate) workspaces: BTreeMap<String, SupervisorWorkspaceState>,
    #[serde(default)]
    pub(crate) threads: BTreeMap<String, SupervisorThreadState>,
    #[serde(default)]
    pub(crate) jobs: BTreeMap<String, SupervisorJobState>,
    #[serde(default)]
    pub(crate) signals: Vec<SupervisorSignal>,
    #[serde(default)]
    pub(crate) activity_feed: Vec<SupervisorActivityEntry>,
    #[serde(default)]
    pub(crate) open_questions: BTreeMap<String, SupervisorOpenQuestion>,
    #[serde(default)]
    pub(crate) pending_approvals: BTreeMap<String, SupervisorPendingApproval>,
}

#[derive(Debug, Clone)]
pub(crate) enum SupervisorStateUpdate {
    UpsertWorkspace(SupervisorWorkspaceState),
    RemoveWorkspace {
        workspace_id: String,
    },
    UpsertThread(SupervisorThreadState),
    RemoveThread {
        workspace_id: String,
        thread_id: String,
    },
    UpsertJob(SupervisorJobState),
    RemoveJob {
        job_id: String,
    },
    UpdateJobStatus {
        job_id: String,
        status: SupervisorJobStatus,
        at_ms: i64,
        error: Option<String>,
    },
    PushSignal(SupervisorSignal),
    AckSignal {
        signal_id: String,
        acknowledged_at_ms: i64,
    },
    PushActivity {
        entry: SupervisorActivityEntry,
        max_items: usize,
    },
    UpsertOpenQuestion(SupervisorOpenQuestion),
    ResolveOpenQuestion {
        question_id: String,
        resolved_at_ms: i64,
    },
    UpsertPendingApproval(SupervisorPendingApproval),
    ResolvePendingApproval {
        request_key: String,
        resolved_at_ms: i64,
    },
}

pub(crate) fn thread_map_key(workspace_id: &str, thread_id: &str) -> String {
    format!("{workspace_id}:{thread_id}")
}

pub(crate) fn reduce(
    state: &SupervisorState,
    updates: impl IntoIterator<Item = SupervisorStateUpdate>,
) -> SupervisorState {
    let mut next = state.clone();
    apply_updates(&mut next, updates);
    next
}

pub(crate) fn apply_updates(
    state: &mut SupervisorState,
    updates: impl IntoIterator<Item = SupervisorStateUpdate>,
) {
    for update in updates {
        apply_update(state, update);
    }
}

pub(crate) fn apply_update(state: &mut SupervisorState, update: SupervisorStateUpdate) {
    match update {
        SupervisorStateUpdate::UpsertWorkspace(workspace) => {
            state.workspaces.insert(workspace.id.clone(), workspace);
        }
        SupervisorStateUpdate::RemoveWorkspace { workspace_id } => {
            state.workspaces.remove(&workspace_id);
            state
                .threads
                .retain(|_, thread| thread.workspace_id != workspace_id);
            state.jobs.retain(|_, job| job.workspace_id != workspace_id);
            state
                .open_questions
                .retain(|_, question| question.workspace_id != workspace_id);
            state
                .pending_approvals
                .retain(|_, approval| approval.workspace_id != workspace_id);
        }
        SupervisorStateUpdate::UpsertThread(thread) => {
            let key = thread_map_key(&thread.workspace_id, &thread.id);
            state.threads.insert(key, thread);
        }
        SupervisorStateUpdate::RemoveThread {
            workspace_id,
            thread_id,
        } => {
            state.threads.remove(&thread_map_key(&workspace_id, &thread_id));
            state.jobs.retain(|_, job| {
                !(job.workspace_id == workspace_id
                    && job.thread_id.as_deref() == Some(thread_id.as_str()))
            });
            state.open_questions.retain(|_, question| {
                !(question.workspace_id == workspace_id && question.thread_id == thread_id)
            });
            state.pending_approvals.retain(|_, approval| {
                !(approval.workspace_id == workspace_id
                    && approval.thread_id.as_deref() == Some(thread_id.as_str()))
            });
        }
        SupervisorStateUpdate::UpsertJob(job) => {
            state.jobs.insert(job.id.clone(), job);
        }
        SupervisorStateUpdate::RemoveJob { job_id } => {
            state.jobs.remove(&job_id);
        }
        SupervisorStateUpdate::UpdateJobStatus {
            job_id,
            status,
            at_ms,
            error,
        } => {
            if let Some(job) = state.jobs.get_mut(&job_id) {
                job.status = status.clone();
                match status {
                    SupervisorJobStatus::Running => {
                        job.started_at_ms = Some(at_ms);
                        job.completed_at_ms = None;
                    }
                    SupervisorJobStatus::Completed | SupervisorJobStatus::Failed => {
                        job.completed_at_ms = Some(at_ms);
                    }
                    SupervisorJobStatus::Pending => {
                        job.started_at_ms = None;
                        job.completed_at_ms = None;
                    }
                }
                job.error = error;
            }
        }
        SupervisorStateUpdate::PushSignal(signal) => {
            if let Some(existing_idx) = state.signals.iter().position(|item| item.id == signal.id) {
                state.signals.remove(existing_idx);
            }
            state.signals.insert(0, signal);
        }
        SupervisorStateUpdate::AckSignal {
            signal_id,
            acknowledged_at_ms,
        } => {
            if let Some(signal) = state.signals.iter_mut().find(|item| item.id == signal_id) {
                signal.acknowledged_at_ms = Some(acknowledged_at_ms);
            }
        }
        SupervisorStateUpdate::PushActivity { entry, max_items } => {
            if let Some(existing_idx) = state.activity_feed.iter().position(|item| item.id == entry.id)
            {
                state.activity_feed.remove(existing_idx);
            }
            state.activity_feed.insert(0, entry);
            let limit = if max_items == 0 {
                DEFAULT_ACTIVITY_FEED_LIMIT
            } else {
                max_items
            };
            if state.activity_feed.len() > limit {
                state.activity_feed.truncate(limit);
            }
        }
        SupervisorStateUpdate::UpsertOpenQuestion(question) => {
            state.open_questions.insert(question.id.clone(), question);
        }
        SupervisorStateUpdate::ResolveOpenQuestion {
            question_id,
            resolved_at_ms,
        } => {
            if let Some(question) = state.open_questions.get_mut(&question_id) {
                question.resolved_at_ms = Some(resolved_at_ms);
            }
        }
        SupervisorStateUpdate::UpsertPendingApproval(approval) => {
            state
                .pending_approvals
                .insert(approval.request_key.clone(), approval);
        }
        SupervisorStateUpdate::ResolvePendingApproval {
            request_key,
            resolved_at_ms,
        } => {
            if let Some(approval) = state.pending_approvals.get_mut(&request_key) {
                approval.resolved_at_ms = Some(resolved_at_ms);
            }
        }
    }
}

fn default_json_null() -> Value {
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reduce_upserts_workspace_thread_and_job() {
        let state = SupervisorState::default();
        let updates = vec![
            SupervisorStateUpdate::UpsertWorkspace(SupervisorWorkspaceState {
                id: "ws-1".to_string(),
                name: "Workspace One".to_string(),
                connected: true,
                current_task: Some("Implement CM-001".to_string()),
                last_activity_at_ms: Some(1000),
                next_expected_step: Some("Wait for review".to_string()),
                blockers: vec![],
                health: SupervisorHealth::Healthy,
                active_thread_id: Some("thread-1".to_string()),
            }),
            SupervisorStateUpdate::UpsertThread(SupervisorThreadState {
                id: "thread-1".to_string(),
                workspace_id: "ws-1".to_string(),
                name: Some("Main".to_string()),
                status: SupervisorThreadStatus::Running,
                current_task: Some("Implement CM-001".to_string()),
                last_activity_at_ms: Some(1000),
                next_expected_step: Some("Write tests".to_string()),
                blockers: vec![],
                active_turn_id: Some("turn-1".to_string()),
            }),
            SupervisorStateUpdate::UpsertJob(SupervisorJobState {
                id: "job-1".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                dedupe_key: Some("ws-1:cm-001".to_string()),
                description: "Implement CM-001".to_string(),
                status: SupervisorJobStatus::Pending,
                requested_at_ms: 1000,
                started_at_ms: None,
                completed_at_ms: None,
                error: None,
            }),
        ];

        let next = reduce(&state, updates);

        assert_eq!(next.workspaces.len(), 1);
        assert_eq!(next.threads.len(), 1);
        assert_eq!(next.jobs.len(), 1);
        assert_eq!(
            next.threads
                .get("ws-1:thread-1")
                .and_then(|thread| thread.active_turn_id.as_deref()),
            Some("turn-1")
        );
    }

    #[test]
    fn push_activity_deduplicates_and_caps() {
        let mut state = SupervisorState::default();

        apply_update(
            &mut state,
            SupervisorStateUpdate::PushActivity {
                entry: SupervisorActivityEntry {
                    id: "a-1".to_string(),
                    kind: "turn_started".to_string(),
                    message: "started".to_string(),
                    created_at_ms: 10,
                    workspace_id: Some("ws-1".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    needs_input: false,
                    metadata: Value::Null,
                },
                max_items: 2,
            },
        );
        apply_update(
            &mut state,
            SupervisorStateUpdate::PushActivity {
                entry: SupervisorActivityEntry {
                    id: "a-2".to_string(),
                    kind: "turn_completed".to_string(),
                    message: "completed".to_string(),
                    created_at_ms: 11,
                    workspace_id: Some("ws-1".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    needs_input: false,
                    metadata: Value::Null,
                },
                max_items: 2,
            },
        );
        apply_update(
            &mut state,
            SupervisorStateUpdate::PushActivity {
                entry: SupervisorActivityEntry {
                    id: "a-1".to_string(),
                    kind: "turn_started".to_string(),
                    message: "started-again".to_string(),
                    created_at_ms: 12,
                    workspace_id: Some("ws-1".to_string()),
                    thread_id: Some("thread-1".to_string()),
                    needs_input: false,
                    metadata: Value::Null,
                },
                max_items: 2,
            },
        );

        assert_eq!(state.activity_feed.len(), 2);
        assert_eq!(state.activity_feed[0].id, "a-1");
        assert_eq!(state.activity_feed[0].message, "started-again");
        assert_eq!(state.activity_feed[1].id, "a-2");
    }

    #[test]
    fn ack_signal_marks_acknowledged_timestamp() {
        let mut state = SupervisorState::default();
        apply_update(
            &mut state,
            SupervisorStateUpdate::PushSignal(SupervisorSignal {
                id: "signal-1".to_string(),
                kind: SupervisorSignalKind::NeedsApproval,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-1".to_string()),
                job_id: None,
                message: "Approval required".to_string(),
                created_at_ms: 100,
                acknowledged_at_ms: None,
                context: json!({ "requestId": "42" }),
            }),
        );

        apply_update(
            &mut state,
            SupervisorStateUpdate::AckSignal {
                signal_id: "signal-1".to_string(),
                acknowledged_at_ms: 120,
            },
        );

        assert_eq!(state.signals.len(), 1);
        assert_eq!(state.signals[0].acknowledged_at_ms, Some(120));
    }

    #[test]
    fn resolve_open_question_and_pending_approval() {
        let mut state = SupervisorState::default();

        apply_update(
            &mut state,
            SupervisorStateUpdate::UpsertOpenQuestion(SupervisorOpenQuestion {
                id: "q-1".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: "thread-1".to_string(),
                question: "Proceed?".to_string(),
                created_at_ms: 10,
                resolved_at_ms: None,
                context: Value::Null,
            }),
        );
        apply_update(
            &mut state,
            SupervisorStateUpdate::UpsertPendingApproval(SupervisorPendingApproval {
                request_key: "ws-1:42".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: Some("thread-1".to_string()),
                turn_id: Some("turn-1".to_string()),
                item_id: Some("item-1".to_string()),
                request_id: "42".to_string(),
                method: "workspace/requestApproval".to_string(),
                params: json!({ "mode": "full" }),
                created_at_ms: 20,
                resolved_at_ms: None,
            }),
        );

        apply_update(
            &mut state,
            SupervisorStateUpdate::ResolveOpenQuestion {
                question_id: "q-1".to_string(),
                resolved_at_ms: 30,
            },
        );
        apply_update(
            &mut state,
            SupervisorStateUpdate::ResolvePendingApproval {
                request_key: "ws-1:42".to_string(),
                resolved_at_ms: 40,
            },
        );

        assert_eq!(
            state
                .open_questions
                .get("q-1")
                .and_then(|question| question.resolved_at_ms),
            Some(30)
        );
        assert_eq!(
            state
                .pending_approvals
                .get("ws-1:42")
                .and_then(|approval| approval.resolved_at_ms),
            Some(40)
        );
    }
}
