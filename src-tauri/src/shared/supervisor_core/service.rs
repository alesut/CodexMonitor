use std::collections::HashMap;
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
}
