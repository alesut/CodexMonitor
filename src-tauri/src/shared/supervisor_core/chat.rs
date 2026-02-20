use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::contract::SUPERVISOR_ACTION_CONTRACT_VERSION;
use super::dispatch::{SupervisorDispatchBatchResult, SupervisorDispatchStatus};
use super::{SupervisorActivityEntry, SupervisorChatMessage, SupervisorState};

pub(crate) const SUPERVISOR_CHAT_FEED_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SupervisorChatDispatchRequest {
    pub(crate) workspace_ids: Vec<String>,
    pub(crate) prompt: String,
    pub(crate) thread_id: Option<String>,
    pub(crate) dedupe_key: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) access_mode: Option<String>,
    pub(crate) route_kind: Option<String>,
    pub(crate) route_reason: Option<String>,
    pub(crate) route_fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SupervisorChatCommand {
    Dispatch(SupervisorChatDispatchRequest),
    Ack { signal_id: String },
    Status { workspace_id: Option<String> },
    Feed { needs_input_only: bool },
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorChatHistoryResponse {
    pub(crate) messages: Vec<SupervisorChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorChatSendResponse {
    pub(crate) messages: Vec<SupervisorChatMessage>,
}

pub(crate) fn parse_supervisor_chat_command(input: &str) -> Result<SupervisorChatCommand, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("command is required".to_string());
    }
    if !raw.starts_with('/') {
        return Err("commands must start with `/` (run `/help` for usage)".to_string());
    }

    let tokens =
        shell_words::split(raw).map_err(|error| format!("invalid command syntax: {error}"))?;
    if tokens.is_empty() {
        return Err("command is required".to_string());
    }

    match tokens[0].as_str() {
        "/dispatch" => parse_dispatch_command(&tokens[1..]).map(SupervisorChatCommand::Dispatch),
        "/ack" => parse_ack_command(&tokens[1..])
            .map(|signal_id| SupervisorChatCommand::Ack { signal_id }),
        "/status" => parse_status_command(&tokens[1..])
            .map(|workspace_id| SupervisorChatCommand::Status { workspace_id }),
        "/feed" => parse_feed_command(&tokens[1..])
            .map(|needs_input_only| SupervisorChatCommand::Feed { needs_input_only }),
        "/help" => {
            ensure_no_extra_args("/help", &tokens[1..])?;
            Ok(SupervisorChatCommand::Help)
        }
        unknown => Err(format!(
            "unknown command `{unknown}` (run `/help` for usage)"
        )),
    }
}

fn parse_dispatch_command(tokens: &[String]) -> Result<SupervisorChatDispatchRequest, String> {
    let mut workspace_ids: Option<Vec<String>> = None;
    let mut prompt: Option<String> = None;
    let mut thread_id: Option<String> = None;
    let mut dedupe_key: Option<String> = None;
    let mut model: Option<String> = None;
    let mut effort: Option<String> = None;
    let mut access_mode: Option<String> = None;
    let mut index = 0usize;

    while index < tokens.len() {
        let flag = tokens[index].as_str();
        index += 1;
        let value = tokens
            .get(index)
            .ok_or_else(|| format!("missing value for `{flag}`"))?;

        match flag {
            "--ws" => {
                let parsed = value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                if parsed.is_empty() {
                    return Err("`--ws` requires at least one workspace id".to_string());
                }
                workspace_ids = Some(parsed);
            }
            "--prompt" => {
                let next_prompt = value.trim();
                if next_prompt.is_empty() {
                    return Err("`--prompt` cannot be empty".to_string());
                }
                prompt = Some(next_prompt.to_string());
            }
            "--thread" => {
                let next_thread = value.trim();
                if next_thread.is_empty() {
                    return Err("`--thread` cannot be empty".to_string());
                }
                thread_id = Some(next_thread.to_string());
            }
            "--dedupe" => {
                let next_dedupe = value.trim();
                if next_dedupe.is_empty() {
                    return Err("`--dedupe` cannot be empty".to_string());
                }
                dedupe_key = Some(next_dedupe.to_string());
            }
            "--model" => {
                let next_model = value.trim();
                if next_model.is_empty() {
                    return Err("`--model` cannot be empty".to_string());
                }
                model = Some(next_model.to_string());
            }
            "--effort" => {
                let next_effort = value.trim();
                if next_effort.is_empty() {
                    return Err("`--effort` cannot be empty".to_string());
                }
                effort = Some(next_effort.to_string());
            }
            "--access-mode" | "--access" => {
                let next_access_mode = value.trim();
                if next_access_mode.is_empty() {
                    return Err("`--access-mode` cannot be empty".to_string());
                }
                access_mode = Some(parse_access_mode(next_access_mode)?);
            }
            unknown => {
                return Err(format!(
                    "unknown `/dispatch` flag `{unknown}` (supported: --ws --prompt --thread --dedupe --model --effort --access-mode)"
                ));
            }
        }

        index += 1;
    }

    let workspace_ids = workspace_ids.ok_or_else(|| "`--ws` is required".to_string())?;
    let prompt = prompt.ok_or_else(|| "`--prompt` is required".to_string())?;
    Ok(SupervisorChatDispatchRequest {
        workspace_ids,
        prompt,
        thread_id,
        dedupe_key,
        model,
        effort,
        access_mode,
        route_kind: None,
        route_reason: None,
        route_fallback: None,
    })
}

fn parse_access_mode(value: &str) -> Result<String, String> {
    match value {
        "read-only" | "current" | "full-access" => Ok(value.to_string()),
        _ => Err(
            "`--access-mode` must be one of `read-only`, `current`, or `full-access`".to_string(),
        ),
    }
}

fn parse_ack_command(tokens: &[String]) -> Result<String, String> {
    if tokens.is_empty() {
        return Err("usage: /ack <signal_id>".to_string());
    }
    if tokens.len() > 1 {
        return Err("usage: /ack <signal_id>".to_string());
    }
    let signal_id = tokens[0].trim();
    if signal_id.is_empty() {
        return Err("usage: /ack <signal_id>".to_string());
    }
    Ok(signal_id.to_string())
}

fn parse_status_command(tokens: &[String]) -> Result<Option<String>, String> {
    if tokens.is_empty() {
        return Ok(None);
    }
    if tokens.len() > 1 {
        return Err("usage: /status [workspace_id]".to_string());
    }
    let workspace_id = tokens[0].trim();
    if workspace_id.is_empty() {
        return Ok(None);
    }
    Ok(Some(workspace_id.to_string()))
}

fn parse_feed_command(tokens: &[String]) -> Result<bool, String> {
    if tokens.is_empty() {
        return Ok(false);
    }
    if tokens.len() > 1 {
        return Err("usage: /feed [needs_input]".to_string());
    }
    match tokens[0].trim() {
        "needs_input" => Ok(true),
        "" => Ok(false),
        _ => Err("usage: /feed [needs_input]".to_string()),
    }
}

fn ensure_no_extra_args(command: &str, tokens: &[String]) -> Result<(), String> {
    if tokens.is_empty() {
        return Ok(());
    }
    Err(format!("usage: {command}"))
}

pub(crate) fn build_dispatch_contract(
    request: &SupervisorChatDispatchRequest,
    action_id_prefix: &str,
) -> Value {
    let actions = request
        .workspace_ids
        .iter()
        .enumerate()
        .map(|(index, workspace_id)| {
            json!({
                "type": "dispatch_turn",
                "action_id": format!("{action_id_prefix}-{}", index + 1),
                "workspace_id": workspace_id,
                "prompt": request.prompt,
                "thread_id": request.thread_id,
                "dedupe_key": request.dedupe_key,
                "model": request.model,
                "effort": request.effort,
                "access_mode": request.access_mode,
                "route_kind": request.route_kind,
                "route_reason": request.route_reason,
                "route_fallback": request.route_fallback,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
        "actions": actions,
    })
}

pub(crate) fn format_help_message() -> String {
    [
        "Supported commands:",
        "- /dispatch --ws ws-1,ws-2 --prompt \"...\" [--thread ...] [--dedupe ...] [--model ...] [--effort ...] [--access-mode read-only|current|full-access]",
        "- /ack <signal_id>",
        "- /status [workspace_id]",
        "- /feed [needs_input]",
        "- /help",
        "",
        "Free-form chat:",
        "- Any message without `/` is routed by Supervisor (local tool vs delegated workspace).",
        "- When a child subtask is waiting for input, reply directly or target it with `@<subtask_id> ...`.",
    ]
    .join("\n")
}

pub(crate) fn format_status_message(
    state: &SupervisorState,
    workspace_id: Option<&str>,
) -> Result<String, String> {
    let pending_signals = state
        .signals
        .iter()
        .filter(|signal| signal.acknowledged_at_ms.is_none())
        .count();

    if let Some(workspace_id) = workspace_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let Some(workspace) = state.workspaces.get(workspace_id) else {
            return Err(format!("workspace `{workspace_id}` not found"));
        };
        let thread_count = state
            .threads
            .values()
            .filter(|thread| thread.workspace_id == workspace_id)
            .count();
        let job_count = state
            .jobs
            .values()
            .filter(|job| job.workspace_id == workspace_id)
            .count();
        let workspace_pending_signals = state
            .signals
            .iter()
            .filter(|signal| {
                signal.acknowledged_at_ms.is_none()
                    && signal.workspace_id.as_deref() == Some(workspace_id)
            })
            .count();
        return Ok([
            format!("Status for workspace `{workspace_id}`:"),
            format!(
                "- connected: {}",
                if workspace.connected { "yes" } else { "no" }
            ),
            format!("- health: {}", health_label(&workspace.health)),
            format!(
                "- current_task: {}",
                workspace.current_task.as_deref().unwrap_or("none")
            ),
            format!(
                "- next_step: {}",
                workspace
                    .next_expected_step
                    .as_deref()
                    .unwrap_or("pending update")
            ),
            format!(
                "- last_activity_at_ms: {}",
                workspace
                    .last_activity_at_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            format!("- blockers: {}", blockers_label(&workspace.blockers)),
            format!("- threads: {thread_count}"),
            format!("- jobs: {job_count}"),
            format!("- pending_signals: {workspace_pending_signals}"),
        ]
        .join("\n"));
    }

    let mut lines = vec![
        "Global supervisor status:".to_string(),
        format!("- workspaces: {}", state.workspaces.len()),
        format!("- threads: {}", state.threads.len()),
        format!("- jobs: {}", state.jobs.len()),
        format!("- pending_signals: {pending_signals}"),
        format!("- pending_approvals: {}", state.pending_approvals.len()),
        format!("- open_questions: {}", state.open_questions.len()),
    ];

    if state.workspaces.is_empty() {
        lines.push("- workspaces_detail: none".to_string());
    } else {
        lines.push("- workspaces_detail:".to_string());
        for workspace in state.workspaces.values() {
            lines.push(format!(
                "  - {} ({}): {}",
                workspace.id,
                health_label(&workspace.health),
                workspace.current_task.as_deref().unwrap_or("idle")
            ));
        }
    }

    Ok(lines.join("\n"))
}

pub(crate) fn format_feed_message(
    items: &[SupervisorActivityEntry],
    total: usize,
    needs_input_only: bool,
) -> String {
    let mut lines = vec![format!(
        "Activity feed{}: showing {} of {}",
        if needs_input_only {
            " (needs input)"
        } else {
            ""
        },
        items.len(),
        total
    )];

    if items.is_empty() {
        lines.push("- no activity entries".to_string());
        return lines.join("\n");
    }

    for entry in items {
        let workspace = entry.workspace_id.as_deref().unwrap_or("global");
        let thread = entry.thread_id.as_deref().unwrap_or("-");
        lines.push(format!(
            "- [{}] {} (ws: {}, thread: {}, at: {}){}",
            entry.kind,
            entry.message,
            workspace,
            thread,
            entry.created_at_ms,
            if entry.needs_input {
                " [needs_input]"
            } else {
                ""
            }
        ));
    }
    lines.join("\n")
}

pub(crate) fn format_ack_message(signal_id: &str) -> String {
    format!("Signal `{signal_id}` acknowledged.")
}

pub(crate) fn format_dispatch_message(
    request: &SupervisorChatDispatchRequest,
    dispatch: &SupervisorDispatchBatchResult,
) -> String {
    let total = dispatch.results.len();
    let dispatched = dispatch
        .results
        .iter()
        .filter(|item| item.status == SupervisorDispatchStatus::Dispatched)
        .count();
    let failed = total.saturating_sub(dispatched);
    let mut lines = vec![
        format!(
            "Dispatch completed for {} workspace(s): {} dispatched, {} failed.",
            request.workspace_ids.len(),
            dispatched,
            failed
        ),
        format!(
            "Prompt: {}",
            request.prompt.trim().chars().take(140).collect::<String>()
        ),
    ];
    if let Some(route_kind) = request.route_kind.as_deref() {
        lines.push(format!("Route kind: {route_kind}"));
    }
    if let Some(route_reason) = request.route_reason.as_deref() {
        lines.push(format!("Route reason: {route_reason}"));
    }
    if let Some(route_fallback) = request.route_fallback.as_deref() {
        lines.push(format!("Route fallback: {route_fallback}"));
    }
    if let Some(model) = request.model.as_deref() {
        lines.push(format!("Model: {model}"));
    }
    if let Some(effort) = request.effort.as_deref() {
        lines.push(format!("Reasoning effort: {effort}"));
    }
    if let Some(access_mode) = request.access_mode.as_deref() {
        lines.push(format!("Access mode: {access_mode}"));
    }

    for item in &dispatch.results {
        match item.status {
            SupervisorDispatchStatus::Dispatched => lines.push(format!(
                "- {}: dispatched (thread: {}, turn: {}){}",
                item.workspace_id,
                item.thread_id.as_deref().unwrap_or("n/a"),
                item.turn_id.as_deref().unwrap_or("n/a"),
                if item.idempotent_replay {
                    " [idempotent_replay]"
                } else {
                    ""
                }
            )),
            SupervisorDispatchStatus::Failed => lines.push(format!(
                "- {}: failed ({})",
                item.workspace_id,
                item.error.as_deref().unwrap_or("unknown error")
            )),
        }
    }

    lines.join("\n")
}

fn blockers_label(blockers: &[String]) -> String {
    if blockers.is_empty() {
        "none".to_string()
    } else {
        blockers.join(", ")
    }
}

fn health_label(health: &super::SupervisorHealth) -> &'static str {
    match health {
        super::SupervisorHealth::Healthy => "healthy",
        super::SupervisorHealth::Stale => "stale",
        super::SupervisorHealth::Disconnected => "disconnected",
    }
}

#[cfg(test)]
mod tests {
    use super::super::SupervisorHealth;
    use super::super::SupervisorWorkspaceState;
    use super::*;

    #[test]
    fn parses_dispatch_command() {
        let command = parse_supervisor_chat_command(
            "/dispatch --ws ws-1,ws-2 --prompt \"run tests\" --thread thread-7 --dedupe d-1 --model gpt-5-mini --effort high --access-mode full-access",
        )
        .expect("parse command");

        let SupervisorChatCommand::Dispatch(payload) = command else {
            panic!("expected dispatch command");
        };

        assert_eq!(
            payload.workspace_ids,
            vec!["ws-1".to_string(), "ws-2".to_string()]
        );
        assert_eq!(payload.prompt, "run tests");
        assert_eq!(payload.thread_id.as_deref(), Some("thread-7"));
        assert_eq!(payload.dedupe_key.as_deref(), Some("d-1"));
        assert_eq!(payload.model.as_deref(), Some("gpt-5-mini"));
        assert_eq!(payload.effort.as_deref(), Some("high"));
        assert_eq!(payload.access_mode.as_deref(), Some("full-access"));
        assert!(payload.route_kind.is_none());
        assert!(payload.route_reason.is_none());
        assert!(payload.route_fallback.is_none());
    }

    #[test]
    fn parses_ack_status_feed_and_help_commands() {
        assert_eq!(
            parse_supervisor_chat_command("/ack signal-1").expect("ack"),
            SupervisorChatCommand::Ack {
                signal_id: "signal-1".to_string(),
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/status ws-1").expect("status"),
            SupervisorChatCommand::Status {
                workspace_id: Some("ws-1".to_string()),
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/feed needs_input").expect("feed"),
            SupervisorChatCommand::Feed {
                needs_input_only: true,
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/help").expect("help"),
            SupervisorChatCommand::Help
        );
    }

    #[test]
    fn rejects_invalid_commands() {
        let error = parse_supervisor_chat_command("status")
            .expect_err("commands without slash should fail");
        assert!(error.contains("commands must start with `/`"));

        let error = parse_supervisor_chat_command("/dispatch --ws ws-1")
            .expect_err("dispatch without prompt should fail");
        assert!(error.contains("`--prompt` is required"));

        let error =
            parse_supervisor_chat_command("/dispatch --ws ws-1 --prompt run --access-mode admin")
                .expect_err("dispatch with invalid access mode should fail");
        assert!(error.contains("`--access-mode` must be one of"));

        let error =
            parse_supervisor_chat_command("/feed unknown").expect_err("invalid feed argument");
        assert!(error.contains("usage: /feed [needs_input]"));
    }

    #[test]
    fn formats_global_status_message() {
        let mut state = SupervisorState::default();
        state.workspaces.insert(
            "ws-1".to_string(),
            SupervisorWorkspaceState {
                id: "ws-1".to_string(),
                name: "Workspace 1".to_string(),
                connected: true,
                current_task: Some("Handle alert".to_string()),
                health: SupervisorHealth::Healthy,
                ..Default::default()
            },
        );

        let message = format_status_message(&state, None).expect("status");
        assert!(message.contains("Global supervisor status:"));
        assert!(message.contains("ws-1"));
    }
}
