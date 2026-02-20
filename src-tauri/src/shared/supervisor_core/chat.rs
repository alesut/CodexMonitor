use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::contract::SUPERVISOR_ACTION_CONTRACT_VERSION;
use super::dispatch::{SupervisorDispatchBatchResult, SupervisorDispatchStatus};
use super::{SupervisorActivityEntry, SupervisorChatMessage, SupervisorState};

pub(crate) const SUPERVISOR_CHAT_FEED_LIMIT: usize = 20;
const STATUS_THREADS_PER_WORKSPACE_LIMIT: usize = 10;

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
    Ack {
        signal_id: String,
    },
    Status {
        workspace_id: Option<String>,
        thread_id: Option<String>,
    },
    Feed {
        needs_input_only: bool,
    },
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
        "/status" | "/статус" => {
            parse_status_command(&tokens[1..]).map(|(workspace_id, thread_id)| {
                SupervisorChatCommand::Status {
                    workspace_id,
                    thread_id,
                }
            })
        }
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

fn parse_status_command(tokens: &[String]) -> Result<(Option<String>, Option<String>), String> {
    let usage =
        "usage: /status [workspace_id] [thread_id] | /status [workspace_id] --thread <thread_id>";
    if tokens.is_empty() {
        return Ok((None, None));
    }

    if tokens.len() == 1 {
        let workspace_id = tokens[0].trim();
        if workspace_id.is_empty() {
            return Ok((None, None));
        }
        if workspace_id == "--thread" {
            return Err(usage.to_string());
        }
        return Ok((Some(workspace_id.to_string()), None));
    }

    if tokens.len() == 2 {
        let first = tokens[0].trim();
        let second = tokens[1].trim();
        if first == "--thread" {
            if second.is_empty() {
                return Err(usage.to_string());
            }
            return Ok((None, Some(second.to_string())));
        }
        if first.is_empty() || second.is_empty() || second == "--thread" {
            return Err(usage.to_string());
        }
        return Ok((Some(first.to_string()), Some(second.to_string())));
    }

    if tokens.len() == 3 {
        let workspace_id = tokens[0].trim();
        let flag = tokens[1].trim();
        let thread_id = tokens[2].trim();
        if workspace_id.is_empty() || thread_id.is_empty() || flag != "--thread" {
            return Err(usage.to_string());
        }
        return Ok((Some(workspace_id.to_string()), Some(thread_id.to_string())));
    }

    Err(usage.to_string())
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
        "- /status [workspace_id] [thread_id]",
        "- /status [workspace_id] --thread <thread_id>",
        "- /статус [workspace_id] [thread_id] (alias)",
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
    thread_id: Option<&str>,
) -> Result<String, String> {
    let workspace_id = workspace_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let thread_id = thread_id.map(str::trim).filter(|value| !value.is_empty());

    if let Some(thread_id) = thread_id {
        let thread_summary = find_thread_summary(state, workspace_id, thread_id)?;
        let thread_key = super::thread_map_key(&thread_summary.workspace_id, &thread_summary.id);
        let Some(thread_state) = state.threads.get(&thread_key) else {
            return Err(format!(
                "thread `{}` not found in workspace `{}`",
                thread_summary.id, thread_summary.workspace_id
            ));
        };
        let workspace_label = state
            .workspaces
            .get(&thread_summary.workspace_id)
            .map(workspace_summary_label)
            .unwrap_or_else(|| format!("`{}`", thread_summary.workspace_id));
        return Ok(format_thread_status_message(
            &thread_summary,
            thread_state,
            &workspace_label,
        ));
    }

    let pending_signals = state
        .signals
        .iter()
        .filter(|signal| signal.acknowledged_at_ms.is_none())
        .count();

    if let Some(workspace_id) = workspace_id {
        let Some(workspace) = state.workspaces.get(workspace_id) else {
            return Err(format!("workspace `{workspace_id}` not found"));
        };
        let thread_summaries = collect_workspace_thread_summaries(state, workspace_id);
        let thread_count = thread_summaries.len();
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
        let workspace_label = workspace_summary_label(workspace);
        let mut lines = vec![
            format!("Status for workspace {workspace_label}:"),
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
        ];
        append_thread_details_lines(
            &mut lines,
            &thread_summaries,
            STATUS_THREADS_PER_WORKSPACE_LIMIT,
            "",
        );
        lines.extend([
            format!("- jobs: {job_count}"),
            format!("- pending_signals: {workspace_pending_signals}"),
        ]);
        return Ok(lines.join("\n"));
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
            let thread_summaries = collect_workspace_thread_summaries(state, &workspace.id);
            lines.push(format!(
                "  - {} ({}): {}",
                workspace_summary_label(workspace),
                health_label(&workspace.health),
                workspace.current_task.as_deref().unwrap_or("idle")
            ));
            lines.push(format!("    - threads: {} active", thread_summaries.len()));
            append_thread_details_lines(
                &mut lines,
                &thread_summaries,
                STATUS_THREADS_PER_WORKSPACE_LIMIT,
                "    ",
            );
        }
    }

    Ok(lines.join("\n"))
}

#[derive(Debug, Clone)]
struct StatusThreadSummary {
    id: String,
    workspace_id: String,
    name: Option<String>,
    status: super::SupervisorThreadStatus,
    last_activity_at_ms: Option<i64>,
    message_count: usize,
    unread_count: usize,
}

fn collect_workspace_thread_summaries(
    state: &SupervisorState,
    workspace_id: &str,
) -> Vec<StatusThreadSummary> {
    let mut summaries = state
        .threads
        .values()
        .filter(|thread| thread.workspace_id == workspace_id)
        .map(|thread| {
            let message_count = state
                .activity_feed
                .iter()
                .filter(|entry| {
                    entry.workspace_id.as_deref() == Some(workspace_id)
                        && entry.thread_id.as_deref() == Some(thread.id.as_str())
                })
                .count();
            let unread_signals = state
                .signals
                .iter()
                .filter(|signal| {
                    signal.acknowledged_at_ms.is_none()
                        && signal.workspace_id.as_deref() == Some(workspace_id)
                        && signal.thread_id.as_deref() == Some(thread.id.as_str())
                })
                .count();
            let unread_questions = state
                .open_questions
                .values()
                .filter(|question| {
                    question.resolved_at_ms.is_none()
                        && question.workspace_id == workspace_id
                        && question.thread_id == thread.id
                })
                .count();
            let unread_approvals = state
                .pending_approvals
                .values()
                .filter(|approval| {
                    approval.resolved_at_ms.is_none()
                        && approval.workspace_id == workspace_id
                        && approval.thread_id.as_deref() == Some(thread.id.as_str())
                })
                .count();

            StatusThreadSummary {
                id: thread.id.clone(),
                workspace_id: workspace_id.to_string(),
                name: thread
                    .name
                    .as_ref()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string),
                status: thread.status.clone(),
                last_activity_at_ms: thread.last_activity_at_ms,
                message_count,
                unread_count: unread_signals + unread_questions + unread_approvals,
            }
        })
        .collect::<Vec<_>>();

    summaries.sort_by(|left, right| {
        right
            .last_activity_at_ms
            .unwrap_or_default()
            .cmp(&left.last_activity_at_ms.unwrap_or_default())
            .then_with(|| left.id.cmp(&right.id))
    });
    summaries
}

fn find_thread_summary(
    state: &SupervisorState,
    workspace_id: Option<&str>,
    thread_id: &str,
) -> Result<StatusThreadSummary, String> {
    if let Some(workspace_id) = workspace_id {
        return collect_workspace_thread_summaries(state, workspace_id)
            .into_iter()
            .find(|summary| summary.id == thread_id)
            .ok_or_else(|| {
                format!("thread `{thread_id}` not found in workspace `{workspace_id}`")
            });
    }

    let matching_workspace_ids = state
        .threads
        .values()
        .filter(|thread| thread.id == thread_id)
        .map(|thread| thread.workspace_id.clone())
        .collect::<Vec<_>>();
    if matching_workspace_ids.is_empty() {
        return Err(format!("thread `{thread_id}` not found"));
    }
    let mut unique_workspace_ids = matching_workspace_ids
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if unique_workspace_ids.len() > 1 {
        return Err(format!(
            "thread `{thread_id}` exists in multiple workspaces; use `/status <workspace_id> --thread {thread_id}`"
        ));
    }
    let workspace_id = unique_workspace_ids
        .pop_first()
        .ok_or_else(|| format!("thread `{thread_id}` not found"))?;
    collect_workspace_thread_summaries(state, &workspace_id)
        .into_iter()
        .find(|summary| summary.id == thread_id)
        .ok_or_else(|| format!("thread `{thread_id}` not found in workspace `{workspace_id}`"))
}

fn append_thread_details_lines(
    lines: &mut Vec<String>,
    thread_summaries: &[StatusThreadSummary],
    limit: usize,
    indent: &str,
) {
    if thread_summaries.is_empty() {
        lines.push(format!("{indent}- threads_detail: none"));
        return;
    }

    let show_count = thread_summaries.len().min(limit);
    lines.push(format!(
        "{indent}- threads_detail (showing {show_count} of {}):",
        thread_summaries.len()
    ));
    for summary in thread_summaries.iter().take(limit) {
        lines.push(format!(
            "{indent}  - {} | status: {} | last_activity_at_ms: {} | messages: {} | unread: {}",
            thread_summary_label(summary),
            thread_status_label(&summary.status),
            summary
                .last_activity_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            summary.message_count,
            summary.unread_count
        ));
    }
    let hidden = thread_summaries.len().saturating_sub(show_count);
    if hidden > 0 {
        lines.push(format!(
            "{indent}  - ... and {hidden} more active thread(s)"
        ));
    }
}

fn thread_summary_label(thread: &StatusThreadSummary) -> String {
    let name = thread.name.as_deref().unwrap_or_default().trim();
    if name.is_empty() || name == thread.id {
        format!("`{}`", thread.id)
    } else {
        format!("`{}` (`{}`)", name, thread.id)
    }
}

fn thread_status_label(status: &super::SupervisorThreadStatus) -> &'static str {
    match status {
        super::SupervisorThreadStatus::Idle => "idle",
        super::SupervisorThreadStatus::Running => "running",
        super::SupervisorThreadStatus::WaitingInput => "waiting_input",
        super::SupervisorThreadStatus::Failed => "failed",
        super::SupervisorThreadStatus::Completed => "completed",
        super::SupervisorThreadStatus::Stalled => "stalled",
    }
}

fn format_thread_status_message(
    summary: &StatusThreadSummary,
    thread: &super::SupervisorThreadState,
    workspace_label: &str,
) -> String {
    [
        format!(
            "Status for thread {} in workspace {}:",
            thread_summary_label(summary),
            workspace_label
        ),
        format!("- status: {}", thread_status_label(&summary.status)),
        format!(
            "- current_task: {}",
            thread.current_task.as_deref().unwrap_or("none")
        ),
        format!(
            "- next_step: {}",
            thread
                .next_expected_step
                .as_deref()
                .unwrap_or("pending update")
        ),
        format!(
            "- last_activity_at_ms: {}",
            summary
                .last_activity_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("- blockers: {}", blockers_label(&thread.blockers)),
        format!("- messages: {}", summary.message_count),
        format!("- unread: {}", summary.unread_count),
        format!(
            "- active_turn_id: {}",
            thread.active_turn_id.as_deref().unwrap_or("none")
        ),
    ]
    .join("\n")
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
            "Sent task to {} workspace(s): {} succeeded, {} failed.",
            request.workspace_ids.len(),
            dispatched,
            failed
        ),
        format!(
            "Task: {}",
            request.prompt.trim().chars().take(140).collect::<String>()
        ),
    ];

    for item in &dispatch.results {
        match item.status {
            SupervisorDispatchStatus::Dispatched => lines.push(format!(
                "- {}: started{}",
                item.workspace_id,
                if item.idempotent_replay {
                    " (reused existing run)"
                } else {
                    ""
                }
            )),
            SupervisorDispatchStatus::Failed => lines.push(format!(
                "- {}: failed to start ({})",
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

fn workspace_summary_label(workspace: &super::SupervisorWorkspaceState) -> String {
    let name = workspace.name.trim();
    if name.is_empty() || name == workspace.id {
        format!("`{}`", workspace.id)
    } else {
        format!("`{}` (`{}`)", name, workspace.id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::dispatch::SupervisorDispatchActionResult;
    use super::super::SupervisorHealth;
    use super::super::SupervisorOpenQuestion;
    use super::super::SupervisorPendingApproval;
    use super::super::SupervisorSignal;
    use super::super::SupervisorSignalKind;
    use super::super::SupervisorThreadState;
    use super::super::SupervisorThreadStatus;
    use super::super::SupervisorWorkspaceState;
    use super::*;
    use serde_json::Value;

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
                thread_id: None,
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/статус ws-1").expect("status alias"),
            SupervisorChatCommand::Status {
                workspace_id: Some("ws-1".to_string()),
                thread_id: None,
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/status ws-1 thread-2").expect("thread status"),
            SupervisorChatCommand::Status {
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-2".to_string()),
            }
        );
        assert_eq!(
            parse_supervisor_chat_command("/status --thread thread-2")
                .expect("thread status without workspace"),
            SupervisorChatCommand::Status {
                workspace_id: None,
                thread_id: Some("thread-2".to_string()),
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
    fn formats_dispatch_message_with_user_facing_copy() {
        let request = SupervisorChatDispatchRequest {
            workspace_ids: vec!["ws-1".to_string(), "ws-2".to_string()],
            prompt: "Run smoke tests and share a concise summary.".to_string(),
            thread_id: Some("thread-1".to_string()),
            dedupe_key: Some("dispatch-1".to_string()),
            model: Some("gpt-5-mini".to_string()),
            effort: Some("high".to_string()),
            access_mode: Some("full-access".to_string()),
            route_kind: Some("workspace_metadata_match".to_string()),
            route_reason: Some("prompt matched workspace metadata".to_string()),
            route_fallback: Some("manual_dispatch".to_string()),
        };
        let dispatch = SupervisorDispatchBatchResult {
            results: vec![
                SupervisorDispatchActionResult {
                    action_id: "action-1".to_string(),
                    workspace_id: "ws-1".to_string(),
                    dedupe_key: "ws-1:dispatch-1".to_string(),
                    status: SupervisorDispatchStatus::Dispatched,
                    thread_id: Some("thread-1".to_string()),
                    turn_id: Some("turn-1".to_string()),
                    error: None,
                    idempotent_replay: true,
                },
                SupervisorDispatchActionResult {
                    action_id: "action-2".to_string(),
                    workspace_id: "ws-2".to_string(),
                    dedupe_key: "ws-2:dispatch-1".to_string(),
                    status: SupervisorDispatchStatus::Failed,
                    thread_id: None,
                    turn_id: None,
                    error: Some("workspace is not connected".to_string()),
                    idempotent_replay: false,
                },
            ],
        };

        let message = format_dispatch_message(&request, &dispatch);
        assert!(message.contains("Sent task to 2 workspace(s): 1 succeeded, 1 failed."));
        assert!(message.contains("Task: Run smoke tests and share a concise summary."));
        assert!(message.contains("- ws-1: started (reused existing run)"));
        assert!(message.contains("- ws-2: failed to start (workspace is not connected)"));
        assert!(!message.contains("Dispatch completed"));
        assert!(!message.contains("Route kind"));
        assert!(!message.contains("Route reason"));
        assert!(!message.contains("Route fallback"));
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

        let message = format_status_message(&state, None, None).expect("status");
        assert!(message.contains("Global supervisor status:"));
        assert!(message.contains("ws-1"));
        assert!(message.contains("Workspace 1"));
        assert!(message.contains("- threads: 0 active"));
        assert!(message.contains("- threads_detail: none"));
    }

    #[test]
    fn formats_status_message_with_workspace_thread_details_and_limit() {
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
        for index in 0..12 {
            state.threads.insert(
                format!("ws-1:thread-{index}"),
                SupervisorThreadState {
                    id: format!("thread-{index}"),
                    workspace_id: "ws-1".to_string(),
                    name: Some(format!("Thread {index}")),
                    status: if index == 11 {
                        SupervisorThreadStatus::WaitingInput
                    } else {
                        SupervisorThreadStatus::Running
                    },
                    last_activity_at_ms: Some(1_000 + index as i64),
                    ..Default::default()
                },
            );
        }
        state.activity_feed = vec![
            SupervisorActivityEntry {
                id: "activity-1".to_string(),
                kind: "turn_started".to_string(),
                message: "started".to_string(),
                created_at_ms: 10,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-11".to_string()),
                needs_input: false,
                metadata: Value::Null,
            },
            SupervisorActivityEntry {
                id: "activity-2".to_string(),
                kind: "turn_completed".to_string(),
                message: "completed".to_string(),
                created_at_ms: 11,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-11".to_string()),
                needs_input: false,
                metadata: Value::Null,
            },
            SupervisorActivityEntry {
                id: "activity-3".to_string(),
                kind: "item_started".to_string(),
                message: "item".to_string(),
                created_at_ms: 12,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-10".to_string()),
                needs_input: false,
                metadata: Value::Null,
            },
        ];
        state.signals = vec![
            SupervisorSignal {
                id: "signal-1".to_string(),
                kind: SupervisorSignalKind::NeedsApproval,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-11".to_string()),
                job_id: None,
                message: "approval".to_string(),
                created_at_ms: 12,
                acknowledged_at_ms: None,
                context: Value::Null,
            },
            SupervisorSignal {
                id: "signal-2".to_string(),
                kind: SupervisorSignalKind::NeedsApproval,
                workspace_id: Some("ws-1".to_string()),
                thread_id: Some("thread-10".to_string()),
                job_id: None,
                message: "acknowledged".to_string(),
                created_at_ms: 11,
                acknowledged_at_ms: Some(13),
                context: Value::Null,
            },
        ];
        state.open_questions.insert(
            "q-1".to_string(),
            SupervisorOpenQuestion {
                id: "q-1".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: "thread-11".to_string(),
                question: "Proceed?".to_string(),
                created_at_ms: 13,
                resolved_at_ms: None,
                context: Value::Null,
            },
        );
        state.pending_approvals.insert(
            "ws-1:1".to_string(),
            SupervisorPendingApproval {
                request_key: "ws-1:1".to_string(),
                workspace_id: "ws-1".to_string(),
                thread_id: Some("thread-11".to_string()),
                turn_id: None,
                item_id: None,
                request_id: "1".to_string(),
                method: "workspace/requestApproval".to_string(),
                params: Value::Null,
                created_at_ms: 13,
                resolved_at_ms: None,
            },
        );

        let message = format_status_message(&state, None, None).expect("status");
        assert!(message.contains("- threads: 12 active"));
        assert!(message.contains("- threads_detail (showing 10 of 12):"));
        assert!(message.contains(
            "`Thread 11` (`thread-11`) | status: waiting_input | last_activity_at_ms: 1011 | messages: 2 | unread: 3"
        ));
        assert!(message.contains("... and 2 more active thread(s)"));
        let newest = message.find("`Thread 11` (`thread-11`)").expect("newest");
        let older = message.find("`Thread 10` (`thread-10`)").expect("older");
        assert!(newest < older);
    }

    #[test]
    fn formats_workspace_status_message_with_thread_details() {
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
        state.threads.insert(
            "ws-1:thread-1".to_string(),
            SupervisorThreadState {
                id: "thread-1".to_string(),
                workspace_id: "ws-1".to_string(),
                name: Some("Ops".to_string()),
                status: SupervisorThreadStatus::Running,
                last_activity_at_ms: Some(100),
                ..Default::default()
            },
        );
        state.activity_feed = vec![SupervisorActivityEntry {
            id: "activity-1".to_string(),
            kind: "turn_started".to_string(),
            message: "started".to_string(),
            created_at_ms: 10,
            workspace_id: Some("ws-1".to_string()),
            thread_id: Some("thread-1".to_string()),
            needs_input: false,
            metadata: Value::Null,
        }];

        let message = format_status_message(&state, Some("ws-1"), None).expect("status");
        assert!(message.contains("Status for workspace"));
        assert!(message.contains("- threads: 1"));
        assert!(message.contains("- threads_detail (showing 1 of 1):"));
        assert!(message.contains(
            "`Ops` (`thread-1`) | status: running | last_activity_at_ms: 100 | messages: 1 | unread: 0"
        ));
    }

    #[test]
    fn formats_thread_status_message() {
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
        state.threads.insert(
            "ws-1:thread-7".to_string(),
            SupervisorThreadState {
                id: "thread-7".to_string(),
                workspace_id: "ws-1".to_string(),
                name: Some("Ops".to_string()),
                status: SupervisorThreadStatus::WaitingInput,
                current_task: Some("Need approval".to_string()),
                last_activity_at_ms: Some(105),
                next_expected_step: Some("Reply to prompt".to_string()),
                blockers: vec!["human input".to_string()],
                active_turn_id: Some("turn-7".to_string()),
            },
        );
        state.activity_feed = vec![SupervisorActivityEntry {
            id: "activity-1".to_string(),
            kind: "turn_started".to_string(),
            message: "started".to_string(),
            created_at_ms: 10,
            workspace_id: Some("ws-1".to_string()),
            thread_id: Some("thread-7".to_string()),
            needs_input: false,
            metadata: Value::Null,
        }];

        let message =
            format_status_message(&state, Some("ws-1"), Some("thread-7")).expect("thread status");
        assert!(message.contains("Status for thread `Ops` (`thread-7`)"));
        assert!(message.contains("- status: waiting_input"));
        assert!(message.contains("- current_task: Need approval"));
        assert!(message.contains("- next_step: Reply to prompt"));
        assert!(message.contains("- last_activity_at_ms: 105"));
        assert!(message.contains("- blockers: human input"));
        assert!(message.contains("- messages: 1"));
        assert!(message.contains("- unread: 0"));
        assert!(message.contains("- active_turn_id: turn-7"));
    }
}
