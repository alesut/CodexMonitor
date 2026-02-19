use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::backend::app_server::WorkspaceSession;

use super::contract::parse_supervisor_action_contract_value;
use super::dispatch::{
    SupervisorDispatchBatchResult, SupervisorDispatchExecutor, SupervisorDispatchStatus,
    WorkspaceSessionDispatchBackend,
};
use super::supervisor_loop::{now_timestamp_ms, SupervisorLoop};
use super::{SupervisorActivityEntry, SupervisorState};

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
}
