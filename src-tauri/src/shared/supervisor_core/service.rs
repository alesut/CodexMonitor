use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::backend::app_server::WorkspaceSession;
use crate::types::{AppSettings, WorkspaceEntry};

use super::chat::{
    build_dispatch_contract, format_ack_message, format_dispatch_message, format_feed_message,
    format_help_message, format_status_message, parse_supervisor_chat_command,
    SupervisorChatCommand, SupervisorChatDispatchRequest, SupervisorChatHistoryResponse,
    SupervisorChatSendResponse, SUPERVISOR_CHAT_FEED_LIMIT,
};
use super::contract::parse_supervisor_action_contract_value;
use super::dispatch::{
    SupervisorDispatchAction, SupervisorDispatchBatchResult, SupervisorDispatchExecutor,
    SupervisorDispatchStatus, WorkspaceSessionDispatchBackend,
};
use super::routing::{
    select_supervisor_route, SupervisorLocalTool, SupervisorRouteDecision, SupervisorRouteKind,
    SupervisorRouteWorkspaceMetadata,
};
use super::supervisor_loop::{now_timestamp_ms, SupervisorLoop};
use super::{
    SupervisorActivityEntry, SupervisorChatMessage, SupervisorChatMessageRole, SupervisorJobState,
    SupervisorJobStatus, SupervisorState,
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
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    app_settings: &Mutex<AppSettings>,
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
        workspaces,
        app_settings,
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
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    app_settings: &Mutex<AppSettings>,
    command: &str,
    received_at_ms: i64,
) -> String {
    if command.starts_with('/') {
        let execution = async {
            let command = parse_supervisor_chat_command(command)?;
            execute_parsed_supervisor_chat_command(
                supervisor_loop,
                dispatch_executor,
                sessions,
                workspaces,
                command,
                received_at_ms,
            )
            .await
        }
        .await;

        return match execution {
            Ok(response) => response,
            Err(error) => format!("Error: {error}\nRun `/help` for command usage."),
        };
    }

    let execution = execute_freeform_supervisor_chat(
        supervisor_loop,
        dispatch_executor,
        sessions,
        workspaces,
        app_settings,
        command,
        received_at_ms,
    )
    .await;

    match execution {
        Ok(response) => response,
        Err(error) => format!("Unable to route chat message: {error}"),
    }
}

#[derive(Debug)]
enum FreeformDispatchOutcome {
    Dispatch(SupervisorChatDispatchRequest),
    LocalTool(SupervisorRouteDecision),
    Clarification(SupervisorRouteDecision),
}

async fn execute_freeform_supervisor_chat(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    app_settings: &Mutex<AppSettings>,
    command: &str,
    received_at_ms: i64,
) -> Result<String, String> {
    if let Some(reply_result) =
        try_route_reply_to_waiting_subtask(supervisor_loop, sessions, command, received_at_ms)
            .await?
    {
        return Ok(reply_result);
    }

    match build_freeform_dispatch_request(
        supervisor_loop,
        sessions,
        workspaces,
        app_settings,
        command,
        received_at_ms,
    )
    .await?
    {
        FreeformDispatchOutcome::Dispatch(request) => {
            execute_parsed_supervisor_chat_command(
                supervisor_loop,
                dispatch_executor,
                sessions,
                workspaces,
                SupervisorChatCommand::Dispatch(request),
                received_at_ms,
            )
            .await
        }
        FreeformDispatchOutcome::LocalTool(route) => {
            execute_local_tool_route(
                supervisor_loop,
                route
                    .local_tool
                    .ok_or_else(|| "local route decision is missing tool type".to_string())?,
            )
            .await
        }
        FreeformDispatchOutcome::Clarification(route) => Ok(format_route_clarification(&route)),
    }
}

async fn build_freeform_dispatch_request(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    app_settings: &Mutex<AppSettings>,
    prompt: &str,
    received_at_ms: i64,
) -> Result<FreeformDispatchOutcome, String> {
    let workspace_metadata =
        collect_route_workspace_metadata(supervisor_loop, sessions, workspaces).await;
    let settings = app_settings.lock().await.clone();
    let route = select_supervisor_route(prompt, &workspace_metadata, &settings);

    {
        let mut supervisor_loop = supervisor_loop.lock().await;
        let decision_metadata = serde_json::to_value(&route).unwrap_or(Value::Null);
        supervisor_loop.record_route_decision(
            &format!("{}-{}", received_at_ms, Uuid::new_v4().simple()),
            format!(
                "Free-form route decision: {} ({})",
                route_kind_label(&route.kind),
                route.reason
            ),
            received_at_ms,
            decision_metadata,
        );
    }

    match route.kind {
        SupervisorRouteKind::LocalTool => Ok(FreeformDispatchOutcome::LocalTool(route)),
        SupervisorRouteKind::Clarification => Ok(FreeformDispatchOutcome::Clarification(route)),
        SupervisorRouteKind::WorkspaceDelegate => {
            let workspace_id = route
                .workspace_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "route decision did not include target workspace".to_string())?;
            Ok(FreeformDispatchOutcome::Dispatch(
                SupervisorChatDispatchRequest {
                    workspace_ids: vec![workspace_id.to_string()],
                    prompt: prompt.trim().to_string(),
                    thread_id: None,
                    dedupe_key: Some(format!(
                        "freeform:{}:{}",
                        workspace_id,
                        Uuid::new_v4().simple()
                    )),
                    model: route.model.clone(),
                    effort: None,
                    access_mode: None,
                    route_kind: Some("workspace_delegate".to_string()),
                    route_reason: Some(route.reason.clone()),
                    route_fallback: route.fallback_message.clone(),
                },
            ))
        }
    }
}

async fn execute_local_tool_route(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    tool: SupervisorLocalTool,
) -> Result<String, String> {
    match tool {
        SupervisorLocalTool::Status => {
            let snapshot = supervisor_snapshot_core(supervisor_loop).await;
            format_status_message(&snapshot, None)
        }
        SupervisorLocalTool::Feed => {
            let feed =
                supervisor_feed_core(supervisor_loop, Some(SUPERVISOR_CHAT_FEED_LIMIT), false)
                    .await;
            Ok(format_feed_message(&feed.items, feed.total, false))
        }
        SupervisorLocalTool::Help => Ok(format_help_message()),
    }
}

fn format_route_clarification(route: &SupervisorRouteDecision) -> String {
    let mut lines = vec![
        "Need clarification before dispatching.".to_string(),
        format!("Reason: {}", route.reason),
    ];
    if let Some(fallback_message) = route.fallback_message.as_deref() {
        lines.push(format!("Fallback note: {fallback_message}"));
    }
    if let Some(clarification) = route.clarification.as_deref() {
        lines.push(format!("Question: {clarification}"));
    }
    if !route.options.is_empty() {
        lines.push(format!(
            "Candidate workspaces: {}",
            route.options.join(", ")
        ));
    }
    lines.join("\n")
}

fn route_kind_label(kind: &SupervisorRouteKind) -> &'static str {
    match kind {
        SupervisorRouteKind::WorkspaceDelegate => "workspace_delegate",
        SupervisorRouteKind::LocalTool => "local_tool",
        SupervisorRouteKind::Clarification => "clarification",
    }
}

async fn collect_route_workspace_metadata(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
) -> Vec<SupervisorRouteWorkspaceMetadata> {
    let snapshot = supervisor_snapshot_core(supervisor_loop).await;
    let connected_workspace_ids = {
        let sessions = sessions.lock().await;
        sessions.keys().cloned().collect::<HashSet<_>>()
    };
    let workspace_entries = {
        let workspaces = workspaces.lock().await;
        workspaces.values().cloned().collect::<Vec<_>>()
    };

    let mut by_id = workspace_entries
        .into_iter()
        .map(|entry| {
            let workspace_state = snapshot.workspaces.get(&entry.id);
            let connected = connected_workspace_ids.contains(&entry.id)
                || workspace_state
                    .map(|state| state.connected)
                    .unwrap_or(false);
            let health = workspace_state
                .map(|state| state.health.clone())
                .unwrap_or_else(|| {
                    if connected {
                        super::SupervisorHealth::Healthy
                    } else {
                        super::SupervisorHealth::Disconnected
                    }
                });
            let available = connected_workspace_ids.contains(&entry.id);
            let mut capabilities = Vec::new();
            if available {
                capabilities.push("thread_start".to_string());
                capabilities.push("thread_resume".to_string());
                capabilities.push("turn_start".to_string());
                capabilities.push("respond_to_server_request".to_string());
            }
            if !entry.path.trim().is_empty() {
                capabilities.push("workspace_fs".to_string());
            }
            if entry
                .worktree
                .as_ref()
                .is_some_and(|worktree| !worktree.branch.trim().is_empty())
            {
                capabilities.push("branch_context".to_string());
            }

            (
                entry.id.clone(),
                SupervisorRouteWorkspaceMetadata {
                    workspace_id: entry.id.clone(),
                    name: entry.name.clone(),
                    path: entry.path.clone(),
                    branch: entry.worktree.map(|worktree| worktree.branch),
                    connected,
                    available,
                    health,
                    capabilities,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    for workspace in snapshot.workspaces.values() {
        if by_id.contains_key(&workspace.id) {
            continue;
        }
        let available = connected_workspace_ids.contains(&workspace.id);
        let mut capabilities = Vec::new();
        if available {
            capabilities.push("thread_start".to_string());
            capabilities.push("thread_resume".to_string());
            capabilities.push("turn_start".to_string());
            capabilities.push("respond_to_server_request".to_string());
        }
        by_id.insert(
            workspace.id.clone(),
            SupervisorRouteWorkspaceMetadata {
                workspace_id: workspace.id.clone(),
                name: workspace.name.clone(),
                path: String::new(),
                branch: None,
                connected: workspace.connected || available,
                available,
                health: workspace.health.clone(),
                capabilities,
            },
        );
    }

    let mut metadata = by_id.into_values().collect::<Vec<_>>();
    metadata.sort_by(|left, right| left.workspace_id.cmp(&right.workspace_id));
    metadata
}

async fn try_route_reply_to_waiting_subtask(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    command: &str,
    received_at_ms: i64,
) -> Result<Option<String>, String> {
    let targeted_reply = parse_targeted_subtask_reply(command);
    let waiting_jobs = {
        let supervisor_loop = supervisor_loop.lock().await;
        supervisor_loop.waiting_jobs()
    };

    if waiting_jobs.is_empty() {
        if let Some((job_id, _)) = targeted_reply {
            let guidance = {
                let supervisor_loop = supervisor_loop.lock().await;
                let snapshot = supervisor_loop.snapshot();
                if snapshot.jobs.contains_key(job_id.as_str()) {
                    format!(
                        "Subtask `{job_id}` is no longer waiting for user input. Run `/status` and submit a new instruction."
                    )
                } else {
                    format!(
                        "Subtask `{job_id}` is unknown. Run `/status` to inspect active subtasks."
                    )
                }
            };
            return Ok(Some(guidance));
        }
        return Ok(None);
    }

    if let Some((job_id, reply_text)) = targeted_reply {
        let Some(job) = waiting_jobs
            .iter()
            .find(|entry| entry.id == job_id)
            .cloned()
        else {
            let available = waiting_jobs
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(Some(format!(
                "Subtask `{job_id}` is not waiting for input. Waiting subtasks: {available}"
            )));
        };
        let result = deliver_reply_to_waiting_subtask(
            supervisor_loop,
            sessions,
            &job,
            &reply_text,
            received_at_ms,
        )
        .await?;
        return Ok(Some(result));
    }

    if waiting_jobs.len() > 1 {
        let hints = waiting_jobs
            .iter()
            .map(|job| {
                format!(
                    "- {} (workspace: {}, thread: {})",
                    job.id,
                    job.workspace_id,
                    job.thread_id.as_deref().unwrap_or("-")
                )
            })
            .collect::<Vec<_>>();
        return Ok(Some(
            [
                "Multiple child subtasks are waiting for input.".to_string(),
                "Reply with an explicit target: `@<subtask_id> your reply`.".to_string(),
                "Waiting subtasks:".to_string(),
                hints.join("\n"),
            ]
            .join("\n"),
        ));
    }

    let job = waiting_jobs
        .into_iter()
        .next()
        .ok_or_else(|| "waiting subtask list became empty".to_string())?;
    let result =
        deliver_reply_to_waiting_subtask(supervisor_loop, sessions, &job, command, received_at_ms)
            .await?;
    Ok(Some(result))
}

fn parse_targeted_subtask_reply(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('@') {
        return None;
    }
    let remainder = trimmed.get(1..)?.trim();
    let mut parts = remainder.splitn(2, char::is_whitespace);
    let subtask_id = parts.next()?.trim();
    let reply_text = parts.next()?.trim();
    if subtask_id.is_empty() || reply_text.is_empty() {
        return None;
    }
    Some((subtask_id.to_string(), reply_text.to_string()))
}

async fn deliver_reply_to_waiting_subtask(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    job: &SupervisorJobState,
    reply_text: &str,
    delivered_at_ms: i64,
) -> Result<String, String> {
    let request_id = job
        .waiting_request_id
        .clone()
        .ok_or_else(|| format!("subtask `{}` no longer has a waiting request", job.id))?;
    let reply_text = reply_text.trim();
    if reply_text.is_empty() {
        return Err("reply text is required".to_string());
    }

    let answers = if job.waiting_question_ids.is_empty() {
        let mut map = serde_json::Map::new();
        map.insert("response".to_string(), json!({ "answers": [reply_text] }));
        map
    } else {
        let mut map = serde_json::Map::new();
        for question_id in &job.waiting_question_ids {
            map.insert(question_id.clone(), json!({ "answers": [reply_text] }));
        }
        map
    };

    let session = {
        let sessions = sessions.lock().await;
        sessions
            .get(job.workspace_id.as_str())
            .cloned()
            .ok_or_else(|| format!("workspace `{}` is not connected", job.workspace_id))?
    };

    let send_result = session
        .send_response(
            request_id.clone(),
            json!({
                "answers": Value::Object(answers),
            }),
        )
        .await;

    match send_result {
        Ok(_) => {
            let mut supervisor_loop = supervisor_loop.lock().await;
            supervisor_loop.mark_reply_delivered(
                &job.id,
                &request_id,
                reply_text,
                delivered_at_ms,
            )?;
            Ok(format!(
                "Reply routed to subtask `{}` in workspace `{}`.",
                job.id, job.workspace_id
            ))
        }
        Err(error) => {
            let mut supervisor_loop = supervisor_loop.lock().await;
            supervisor_loop.mark_reply_delivery_failed(
                &job.id,
                &request_id,
                error.as_str(),
                delivered_at_ms,
            );
            Err(format!(
                "failed to deliver reply to subtask `{}`: {}",
                job.id, error
            ))
        }
    }
}

async fn execute_parsed_supervisor_chat_command(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_executor: &Arc<Mutex<SupervisorDispatchExecutor>>,
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    command: SupervisorChatCommand,
    received_at_ms: i64,
) -> Result<String, String> {
    match command {
        SupervisorChatCommand::Help => Ok(format_help_message()),
        SupervisorChatCommand::Status { workspace_id } => {
            let mut snapshot = supervisor_snapshot_core(supervisor_loop).await;
            let workspaces = workspaces.lock().await;
            for entry in workspaces.values() {
                if let Some(workspace) = snapshot.workspaces.get_mut(&entry.id) {
                    workspace.name = entry.name.clone();
                }
            }
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
    let dispatch_actions = validated_contract.dispatch_actions;
    let backend = WorkspaceSessionDispatchBackend::new(sessions);
    let dispatch_result = {
        let mut executor = dispatch_executor.lock().await;
        executor
            .dispatch_batch(&backend, dispatch_actions.clone())
            .await
    };

    apply_dispatch_outcome_events(supervisor_loop, &dispatch_result, &dispatch_actions).await;
    Ok(dispatch_result)
}

async fn apply_dispatch_outcome_events(
    supervisor_loop: &Arc<Mutex<SupervisorLoop>>,
    dispatch_result: &SupervisorDispatchBatchResult,
    dispatch_actions: &[SupervisorDispatchAction],
) {
    if dispatch_result.results.is_empty() {
        return;
    }

    let timestamp_ms = now_timestamp_ms();
    let mut supervisor_loop = supervisor_loop.lock().await;
    let actions_by_id = dispatch_actions
        .iter()
        .map(|action| (action.action_id.as_str(), action))
        .collect::<HashMap<_, _>>();

    for result in &dispatch_result.results {
        let action = actions_by_id.get(result.action_id.as_str()).copied();
        let mut job = SupervisorJobState {
            id: result.action_id.clone(),
            workspace_id: result.workspace_id.clone(),
            thread_id: result.thread_id.clone(),
            dedupe_key: Some(result.dedupe_key.clone()),
            description: action
                .map(|entry| entry.prompt.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| "Delegated subtask".to_string()),
            status: SupervisorJobStatus::Queued,
            requested_at_ms: timestamp_ms,
            started_at_ms: None,
            completed_at_ms: None,
            error: None,
            route_kind: action.and_then(|entry| entry.route_kind.clone()),
            route_target: Some(result.workspace_id.clone()),
            route_reason: action.and_then(|entry| entry.route_reason.clone()),
            route_fallback: action.and_then(|entry| entry.route_fallback.clone()),
            model: action.and_then(|entry| entry.model.clone()),
            effort: action.and_then(|entry| entry.effort.clone()),
            access_mode: action.and_then(|entry| entry.access_mode.clone()),
            waiting_request_id: None,
            waiting_question_ids: Vec::new(),
            recent_events: Vec::new(),
        };

        match result.status {
            SupervisorDispatchStatus::Dispatched => {
                job.status = SupervisorJobStatus::Running;
                job.started_at_ms = Some(timestamp_ms);
                job.recent_events.push(super::SupervisorSubtaskEvent {
                    id: format!("queued:{}:{}", job.id, timestamp_ms),
                    kind: "queued".to_string(),
                    message: "Subtask queued for execution".to_string(),
                    created_at_ms: timestamp_ms,
                    metadata: json!({
                        "workspaceId": result.workspace_id,
                        "threadId": result.thread_id,
                    }),
                });
                job.recent_events.push(super::SupervisorSubtaskEvent {
                    id: format!("running:{}:{}", job.id, timestamp_ms),
                    kind: "running".to_string(),
                    message: "Subtask dispatch accepted".to_string(),
                    created_at_ms: timestamp_ms,
                    metadata: json!({
                        "workspaceId": result.workspace_id,
                        "threadId": result.thread_id,
                        "turnId": result.turn_id,
                    }),
                });
                supervisor_loop.upsert_job(job);

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
                job.status = SupervisorJobStatus::Failed;
                job.error = Some(message.clone());
                job.completed_at_ms = Some(timestamp_ms);
                job.recent_events.push(super::SupervisorSubtaskEvent {
                    id: format!("queued:{}:{}", job.id, timestamp_ms),
                    kind: "queued".to_string(),
                    message: "Subtask queued for execution".to_string(),
                    created_at_ms: timestamp_ms,
                    metadata: json!({
                        "workspaceId": result.workspace_id,
                    }),
                });
                job.recent_events.push(super::SupervisorSubtaskEvent {
                    id: format!("failed:{}:{}", job.id, timestamp_ms),
                    kind: "failed".to_string(),
                    message: message.clone(),
                    created_at_ms: timestamp_ms,
                    metadata: json!({
                        "workspaceId": result.workspace_id,
                        "threadId": result.thread_id,
                    }),
                });
                supervisor_loop.upsert_job(job);
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
    use std::process::Stdio;
    use std::sync::atomic::AtomicU64;

    use tokio::process::Command;
    use tokio::sync::mpsc;

    use crate::backend::app_server::WorkspaceSession;
    use crate::shared::supervisor_core::dispatch::SupervisorDispatchActionResult;
    use crate::shared::supervisor_core::supervisor_loop::SupervisorLoopConfig;
    use crate::types::{AppSettings, WorkspaceEntry, WorkspaceKind, WorkspaceSettings};

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

    fn waiting_job(
        id: &str,
        workspace_id: &str,
        thread_id: &str,
        request_id: &str,
    ) -> SupervisorJobState {
        SupervisorJobState {
            id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            thread_id: Some(thread_id.to_string()),
            description: "Waiting subtask".to_string(),
            status: SupervisorJobStatus::WaitingForUser,
            requested_at_ms: 1,
            waiting_request_id: Some(json!(request_id)),
            waiting_question_ids: vec!["question-1".to_string()],
            ..Default::default()
        }
    }

    async fn spawn_reply_session(workspace_id: &str) -> Arc<WorkspaceSession> {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "more"]);
            command
        };
        #[cfg(not(target_os = "windows"))]
        let mut command = Command::new("cat");

        command.stdin(Stdio::piped());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        let mut child = command.spawn().expect("spawn test session process");
        let stdin = child.stdin.take().expect("test session stdin");

        Arc::new(WorkspaceSession {
            entry: WorkspaceEntry {
                id: workspace_id.to_string(),
                name: format!("Workspace {workspace_id}"),
                path: ".".to_string(),
                codex_bin: None,
                kind: WorkspaceKind::Main,
                parent_id: None,
                worktree: None,
                settings: WorkspaceSettings::default(),
            },
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            background_thread_callbacks: Mutex::new(
                HashMap::<String, mpsc::UnboundedSender<Value>>::new(),
            ),
        })
    }

    async fn stop_reply_session(session: &Arc<WorkspaceSession>) {
        let mut child = session.child.lock().await;
        let _ = child.kill().await;
        let _ = child.wait().await;
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
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
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
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "/dispatch --ws ws-1",
                100,
            )
            .await
            .expect("chat send");

            assert_eq!(response.messages.len(), 2);
            let system_message = response.messages.last().expect("system message");
            assert_eq!(system_message.role, SupervisorChatMessageRole::System);
            assert!(
                system_message.text.contains("`--prompt` is required"),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }

    #[test]
    fn supervisor_chat_send_core_routes_freeform_chat_without_slash() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "status",
                200,
            )
            .await
            .expect("chat send");

            let system_message = response.messages.last().expect("system message");
            assert_eq!(system_message.role, SupervisorChatMessageRole::System);
            assert!(
                system_message.text.contains("Global supervisor status:"),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }

    #[test]
    fn supervisor_chat_send_core_status_uses_workspace_names_from_registry() {
        run_async(async {
            let mut state = SupervisorState::default();
            state.workspaces.insert(
                "ws-1".to_string(),
                crate::shared::supervisor_core::SupervisorWorkspaceState {
                    id: "ws-1".to_string(),
                    name: String::new(),
                    ..Default::default()
                },
            );
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::from_state(
                SupervisorLoopConfig::default(),
                state,
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());
            let workspaces = Mutex::new(HashMap::from([(
                "ws-1".to_string(),
                WorkspaceEntry {
                    id: "ws-1".to_string(),
                    name: "CodexMonitor".to_string(),
                    path: "/tmp/codex-monitor".to_string(),
                    codex_bin: None,
                    kind: WorkspaceKind::Main,
                    parent_id: None,
                    worktree: None,
                    settings: WorkspaceSettings::default(),
                },
            )]));
            let app_settings = Mutex::new(AppSettings::default());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "/status",
                300,
            )
            .await
            .expect("chat send");

            let system_message = response.messages.last().expect("system message");
            assert_eq!(system_message.role, SupervisorChatMessageRole::System);
            assert!(
                system_message.text.contains("`CodexMonitor` (`ws-1`)"),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }

    #[test]
    fn supervisor_chat_send_core_reports_when_freeform_has_no_connected_workspaces() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "run smoke tests",
                100,
            )
            .await
            .expect("chat send");

            let system_message = response.messages.last().expect("system message");
            assert_eq!(system_message.role, SupervisorChatMessageRole::System);
            assert!(
                system_message
                    .text
                    .contains("Need clarification before dispatching"),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }

    #[test]
    fn apply_dispatch_outcome_events_bridges_final_result_into_supervisor_chat() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));

            let dispatch_actions = vec![SupervisorDispatchAction {
                action_id: "job-1".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: None,
                prompt: "Run smoke tests".to_string(),
                dedupe_key: Some("dedupe-1".to_string()),
                model: Some("gpt-5-mini".to_string()),
                effort: Some("high".to_string()),
                access_mode: Some("full-access".to_string()),
                route_kind: Some("workspace_delegate".to_string()),
                route_reason: Some("selected by routing score".to_string()),
                route_fallback: None,
            }];
            let dispatch_result = SupervisorDispatchBatchResult {
                results: vec![SupervisorDispatchActionResult {
                    action_id: "job-1".to_string(),
                    workspace_id: "ws-1".to_string(),
                    dedupe_key: "dedupe-1".to_string(),
                    status: SupervisorDispatchStatus::Dispatched,
                    thread_id: Some("thread-1".to_string()),
                    turn_id: Some("turn-1".to_string()),
                    error: None,
                    idempotent_replay: false,
                }],
            };

            apply_dispatch_outcome_events(&supervisor_loop, &dispatch_result, &dispatch_actions)
                .await;

            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": "thread-1",
                            "itemId": "item-1",
                            "item": {
                                "id": "item-1",
                                "type": "agentMessage",
                                "text": "Tests passed in workspace ws-1"
                            }
                        }
                    }),
                    110,
                );
                supervisor_loop.apply_app_server_event(
                    "ws-1",
                    &json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                            "summary": "All checks completed"
                        }
                    }),
                    111,
                );
            }

            let snapshot = supervisor_snapshot_core(&supervisor_loop).await;
            let job = snapshot
                .jobs
                .get("job-1")
                .expect("delegated job should exist");
            assert_eq!(job.status, SupervisorJobStatus::Completed);
            assert_eq!(job.route_kind.as_deref(), Some("workspace_delegate"));
            assert_eq!(job.model.as_deref(), Some("gpt-5-mini"));
            assert_eq!(job.effort.as_deref(), Some("high"));
            assert_eq!(job.access_mode.as_deref(), Some("full-access"));

            let chat = supervisor_chat_history_core(&supervisor_loop).await;
            assert!(
                chat.messages.iter().any(|message| message
                    .text
                    .contains("Agent response: Tests passed in workspace ws-1")),
                "missing bridged agent response in supervisor chat"
            );
            assert!(
                chat.messages
                    .iter()
                    .any(|message| message.text.contains("Subtask completed.")),
                "missing completion summary in supervisor chat"
            );
        });
    }

    #[test]
    fn supervisor_chat_send_core_disambiguates_reply_when_multiple_subtasks_wait() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.upsert_job(waiting_job("job-a", "ws-1", "thread-a", "request-a"));
                supervisor_loop.upsert_job(waiting_job("job-b", "ws-2", "thread-b", "request-b"));
            }

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "Use staging environment",
                200,
            )
            .await
            .expect("chat send");

            let system_message = response.messages.last().expect("system message");
            assert!(
                system_message
                    .text
                    .contains("Multiple child subtasks are waiting for input."),
                "unexpected response text: {}",
                system_message.text
            );
            assert!(
                system_message
                    .text
                    .contains("Reply with an explicit target: `@<subtask_id> your reply`."),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }

    #[test]
    fn supervisor_chat_send_core_routes_targeted_reply_to_waiting_subtask() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let reply_session = spawn_reply_session("ws-1").await;
            let sessions = Mutex::new(HashMap::from([(
                "ws-1".to_string(),
                Arc::clone(&reply_session),
            )]));
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.upsert_job(waiting_job("job-1", "ws-1", "thread-1", "request-1"));
            }

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "@job-1 Use staging endpoint",
                300,
            )
            .await
            .expect("chat send");
            stop_reply_session(&reply_session).await;

            let system_message = response.messages.last().expect("system message");
            assert!(
                system_message
                    .text
                    .contains("Reply routed to subtask `job-1` in workspace `ws-1`."),
                "unexpected response text: {}",
                system_message.text
            );

            let snapshot = supervisor_snapshot_core(&supervisor_loop).await;
            let job = snapshot.jobs.get("job-1").expect("job should exist");
            assert_eq!(job.status, SupervisorJobStatus::Running);
            assert!(job.waiting_request_id.is_none());
            assert!(job.waiting_question_ids.is_empty());
        });
    }

    #[test]
    fn supervisor_chat_send_core_guides_when_targeted_subtask_is_not_waiting() {
        run_async(async {
            let supervisor_loop = Arc::new(Mutex::new(SupervisorLoop::new(
                SupervisorLoopConfig::default(),
            )));
            let dispatch_executor = Arc::new(Mutex::new(SupervisorDispatchExecutor::new()));
            let sessions = Mutex::new(HashMap::new());
            let workspaces = Mutex::new(HashMap::new());
            let app_settings = Mutex::new(AppSettings::default());

            {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.upsert_job(SupervisorJobState {
                    id: "job-1".to_string(),
                    workspace_id: "ws-1".to_string(),
                    thread_id: Some("thread-1".to_string()),
                    description: "Completed subtask".to_string(),
                    status: SupervisorJobStatus::Completed,
                    requested_at_ms: 1,
                    completed_at_ms: Some(2),
                    ..Default::default()
                });
            }

            let response = supervisor_chat_send_core(
                &supervisor_loop,
                &dispatch_executor,
                &sessions,
                &workspaces,
                &app_settings,
                "@job-1 Continue with rollout",
                400,
            )
            .await
            .expect("chat send");

            let system_message = response.messages.last().expect("system message");
            assert!(
                system_message
                    .text
                    .contains("Subtask `job-1` is no longer waiting for user input."),
                "unexpected response text: {}",
                system_message.text
            );
        });
    }
}
