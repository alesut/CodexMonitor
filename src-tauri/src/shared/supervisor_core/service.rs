use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::backend::app_server::WorkspaceSession;

use super::chat::{
    build_dispatch_contract, format_ack_message, format_dispatch_message, format_feed_message,
    format_help_message, format_status_message, parse_supervisor_chat_command,
    SupervisorChatCommand, SupervisorChatHistoryResponse, SupervisorChatSendResponse,
    SUPERVISOR_CHAT_FEED_LIMIT,
};
use super::contract::parse_supervisor_action_contract_value;
use super::dispatch::{
    SupervisorDispatchBatchResult, SupervisorDispatchExecutor, SupervisorDispatchStatus,
    WorkspaceSessionDispatchBackend,
};
use super::supervisor_loop::{now_timestamp_ms, SupervisorLoop};
use super::{
    SupervisorActivityEntry, SupervisorChatMessage, SupervisorChatMessageRole, SupervisorState,
};

const SUPERVISOR_FEED_DEFAULT_LIMIT: usize = 100;
const SUPERVISOR_FEED_MAX_LIMIT: usize = 1000;
#[cfg_attr(not(test), allow(dead_code))]
const SUPERVISOR_STATE_FILE_NAME: &str = "supervisor-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SupervisorFeedResponse {
    pub(crate) items: Vec<SupervisorActivityEntry>,
    pub(crate) total: usize,
}

pub(crate) async fn supervisor_snapshot_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
) -> SupervisorState {
    let supervisor_loop = supervisor_loop.lock().await;
    supervisor_loop.snapshot()
}

pub(crate) async fn supervisor_feed_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    limit: Option<usize>,
    needs_input_only: bool,
) -> SupervisorFeedResponse {
    let snapshot = supervisor_snapshot_core(supervisor_loop).await;
    let mut items = snapshot.activity_feed;

    if needs_input_only {
        items.retain(|entry| entry.needs_input);
    }

    let total = items.len();
    let limit = limit
        .unwrap_or(SUPERVISOR_FEED_DEFAULT_LIMIT)
        .min(SUPERVISOR_FEED_MAX_LIMIT);
    items.truncate(limit);

    SupervisorFeedResponse { items, total }
}

pub(crate) async fn supervisor_chat_history_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
) -> SupervisorChatHistoryResponse {
    let supervisor_loop = supervisor_loop.lock().await;
    SupervisorChatHistoryResponse {
        messages: supervisor_loop.chat_history(),
    }
}

pub(crate) async fn supervisor_chat_send_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    command: &str,
    received_at_ms: i64,
) -> Result<SupervisorChatSendResponse, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("command is required".to_string());
    }

    let user_message = SupervisorChatMessage {
        id: format!("chat-user-{}-{}", received_at_ms, Uuid::new_v4().simple()),
        role: SupervisorChatMessageRole::User,
        text: command.to_string(),
        created_at_ms: received_at_ms,
    };

    let response_text = execute_supervisor_chat_command(
        supervisor_loop,
        dispatch_executor,
        sessions,
        command,
        received_at_ms,
    )
    .await;

    let system_message = SupervisorChatMessage {
        id: format!("chat-system-{}-{}", received_at_ms, Uuid::new_v4().simple()),
        role: SupervisorChatMessageRole::System,
        text: response_text,
        created_at_ms: now_timestamp_ms(),
    };

    let mut supervisor_loop = supervisor_loop.lock().await;
    supervisor_loop.append_chat_message(user_message);
    supervisor_loop.append_chat_message(system_message);
    Ok(SupervisorChatSendResponse {
        messages: supervisor_loop.chat_history(),
    })
}

async fn execute_supervisor_chat_command(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    command: &str,
    received_at_ms: i64,
) -> String {
    let execution = async {
        let command = parse_supervisor_chat_command(command)?;
        execute_parsed_supervisor_chat_command(
            supervisor_loop,
            dispatch_executor,
            sessions,
            command,
            received_at_ms,
        )
        .await
    }
    .await;

    match execution {
        Ok(response) => response,
        Err(error) => format!("Error: {error}\nRun `/help` for command usage."),
    }
}

async fn execute_parsed_supervisor_chat_command(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    command: SupervisorChatCommand,
    received_at_ms: i64,
) -> Result<String, String> {
    match command {
        SupervisorChatCommand::Help => Ok(format_help_message()),
        SupervisorChatCommand::Status { workspace_id } => {
            let snapshot = supervisor_snapshot_core(supervisor_loop).await;
            format_status_message(&snapshot, workspace_id.as_deref())
        }
        SupervisorChatCommand::Feed { needs_input_only } => {
            let feed = supervisor_feed_core(
                supervisor_loop,
                Some(SUPERVISOR_CHAT_FEED_LIMIT),
                needs_input_only,
            )
            .await;
            Ok(format_feed_message(
                &feed.items,
                feed.total,
                needs_input_only,
            ))
        }
        SupervisorChatCommand::Ack { signal_id } => {
            supervisor_ack_signal_core(supervisor_loop, &signal_id, now_timestamp_ms()).await?;
            Ok(format_ack_message(&signal_id))
        }
        SupervisorChatCommand::Dispatch(request) => {
            let action_id_prefix = format!(
                "chat-dispatch-{}-{}",
                received_at_ms,
                Uuid::new_v4().simple()
            );
            let contract = build_dispatch_contract(&request, &action_id_prefix);
            let dispatch =
                supervisor_dispatch_core(supervisor_loop, dispatch_executor, sessions, &contract)
                    .await?;
            Ok(format_dispatch_message(&request, &dispatch))
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn supervisor_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SUPERVISOR_STATE_FILE_NAME)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read_supervisor_state(path: &PathBuf) -> Result<SupervisorState, String> {
    if !path.exists() {
        return Ok(SupervisorState::default());
    }
    let data = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&data).map_err(|error| error.to_string())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_supervisor_state(
    path: &PathBuf,
    state: &SupervisorState,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let data = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    std::fs::write(path, data).map_err(|error| error.to_string())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn persist_supervisor_snapshot(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    path: &PathBuf,
) -> Result<(), String> {
    let snapshot = supervisor_snapshot_core(supervisor_loop).await;
    write_supervisor_state(path, &snapshot)
}

pub(crate) async fn supervisor_ack_signal_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    signal_id: &str,
    acknowledged_at_ms: i64,
) -> Result<(), String> {
    let signal_id = signal_id.trim();
    if signal_id.is_empty() {
        return Err("signal_id is required".to_string());
    }

    let mut supervisor_loop = supervisor_loop.lock().await;
    let signal_exists = supervisor_loop
        .snapshot()
        .signals
        .iter()
        .any(|signal| signal.id == signal_id);
    if !signal_exists {
        return Err(format!("signal `{signal_id}` not found"));
    }

    supervisor_loop.ack_signal(signal_id, acknowledged_at_ms);
    Ok(())
}

pub(crate) async fn supervisor_dispatch_core(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    contract: &Value,
) -> Result<SupervisorDispatchBatchResult, String> {
    let validated_contract = parse_supervisor_action_contract_value(contract)?;
    let backend = WorkspaceSessionDispatchBackend::new(sessions);
    let dispatch_result = {
        let mut executor = dispatch_executor.lock().await;
        executor
            .dispatch_batch(&backend, validated_contract.dispatch_actions)
            .await
    };

    apply_dispatch_outcome_events(supervisor_loop, &dispatch_result).await;
    Ok(dispatch_result)
}

async fn apply_dispatch_outcome_events(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_result: &SupervisorDispatchBatchResult,
) {
    if dispatch_result.results.is_empty() {
        return;
    }

    let timestamp_ms = now_timestamp_ms();
    let mut supervisor_loop = supervisor_loop.lock().await;

    for result in &dispatch_result.results {
        match result.status {
            SupervisorDispatchStatus::Dispatched => {
                let Some(thread_id) = result.thread_id.as_ref() else {
                    continue;
                };
                let Some(turn_id) = result.turn_id.as_ref() else {
                    continue;
                };

                supervisor_loop.apply_app_server_event(
                    &result.workspace_id,
                    &json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": thread_id,
                            "turnId": turn_id,
                        }
                    }),
                    timestamp_ms,
                );
            }
            SupervisorDispatchStatus::Failed => {
                let message = result
                    .error
                    .clone()
                    .unwrap_or_else(|| "Supervisor dispatch failed".to_string());
                supervisor_loop.apply_app_server_event(
                    &result.workspace_id,
                    &json!({
                        "method": "error",
                        "params": {
                            "threadId": result.thread_id,
                            "error": {
                                "message": message,
                            }
                        }
                    }),
                    timestamp_ms,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    use crate::shared::supervisor_core::supervisor_loop::SupervisorLoopConfig;

    fn run_async<F>(future: F)
    where
        F: Future<Output = ()>,
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future);
    }

    #[test]
    fn supervisor_feed_core_filters_and_limits() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));

            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "id": 1,
                        "method": "workspace/requestApproval",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                            "itemId": "item-1"
                        }
                    }),
                    100,
                );
                supervisor_loop.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-2"
                        }
                    }),
                    101,
                );
            }

            let feed = supervisor_feed_core(&supervisor_loop, Some(1), false).await;
            assert_eq!(feed.total, 2);
            assert_eq!(feed.items.len(), 1);

            let needs_input_feed = supervisor_feed_core(&supervisor_loop, None, true).await;
            assert_eq!(needs_input_feed.total, 1);
            assert_eq!(needs_input_feed.items.len(), 1);
            assert!(needs_input_feed.items[0].needs_input);
        });
    }

    #[test]
    fn supervisor_ack_signal_core_acknowledges_existing_signal() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "id": 1,
                        "method": "workspace/requestApproval",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                            "itemId": "item-1"
                        }
                    }),
                    100,
                );
            }

            let signal_id = {
                let snapshot = supervisor_snapshot_core(&supervisor_loop).await;
                snapshot
                    .signals
                    .first()
                    .expect("signal should exist")
                    .id
                    .clone()
            };

            supervisor_ack_signal_core(&supervisor_loop, &signal_id, 200)
                .await
                .expect("ack signal");

            let snapshot = supervisor_snapshot_core(&supervisor_loop).await;
            let signal = snapshot
                .signals
                .iter()
                .find(|signal| signal.id == signal_id)
                .expect("signal should remain in snapshot");
            assert_eq!(signal.acknowledged_at_ms, Some(200));
        });
    }

    #[test]
    fn supervisor_ack_signal_core_rejects_unknown_signal() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let error = supervisor_ack_signal_core(&supervisor_loop, "unknown", 100)
                .await
                .expect_err("unknown signal should fail");
            assert_eq!(error, "signal `unknown` not found");
        });
    }

    #[test]
    fn supervisor_state_roundtrip_persists_to_disk() {
        let temp_dir = std::env::temp_dir().join(format!(
            "codex-monitor-supervisor-state-{}",
            uuid::Uuid::new_v4()
        ));
        let path = supervisor_state_path(&temp_dir);

        let mut state = SupervisorState::default();
        state.workspaces.insert(
            "ws-restore".to_string(),
            crate::shared::supervisor_core::SupervisorWorkspaceState {
                id: "ws-restore".to_string(),
                name: "Restore Workspace".to_string(),
                connected: true,
                ..Default::default()
            },
        );

        write_supervisor_state(&path, &state).expect("write supervisor state");
        let restored = read_supervisor_state(&path).expect("read supervisor state");
        assert_eq!(restored.workspaces.len(), 1);
        assert!(restored.workspaces.contains_key("ws-restore"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn persist_supervisor_snapshot_writes_latest_snapshot() {
        run_async(async {
            let temp_dir = std::env::temp_dir().join(format!(
                "codex-monitor-supervisor-persist-{}",
                uuid::Uuid::new_v4()
            ));
            let path = supervisor_state_path(&temp_dir);
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));

            {
                let mut loop_state = supervisor_loop.lock().await;
                loop_state.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                        }
                    }),
                    100,
                );
            }

            persist_supervisor_snapshot(&supervisor_loop, &path)
                .await
                .expect("persist snapshot");
            let restored = read_supervisor_state(&path).expect("read state from disk");
            assert!(restored.workspaces.contains_key("ws-1"));
            assert!(restored.threads.contains_key("ws-1:thread-1"));

            let _ = std::fs::remove_dir_all(&temp_dir);
        });
    }

    #[test]
    fn supervisor_chat_send_core_appends_user_and_system_messages() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                "/help",
                100,
            )
            .await
            .expect("chat send");

            assert_eq!(response.messages.len(), 2);
            assert_eq!(response.messages[0].role, SupervisorChatMessageRole::User);
            assert_eq!(response.messages[0].text, "/help");
            assert_eq!(response.messages[1].role, SupervisorChatMessageRole::System);
            assert!(response.messages[1].text.contains("Supported commands"));

            let history = supervisor_chat_history_core(&supervisor_loop).await;
            assert_eq!(history.messages.len(), 2);
        });
    }

    #[test]
    fn supervisor_chat_send_core_converts_command_errors_to_chat_response() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                "status",
                100,
            )
            .await
            .expect("chat send");

            assert_eq!(response.messages.len(), 2);
            let system_message = response.messages.last().expect("system message");
            assert_eq!(system_message.role, SupervisorChatMessageRole::System);
            assert!(
                system_message.text.contains("commands must start with `/`"),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }
}
