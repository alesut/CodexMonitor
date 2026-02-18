use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SupervisorEvent {
    TurnStarted {
        workspace_id: String,
        thread_id: String,
        turn_id: String,
        task: Option<String>,
        received_at_ms: i64,
    },
    TurnCompleted {
        workspace_id: String,
        thread_id: String,
        turn_id: String,
        task: Option<String>,
        received_at_ms: i64,
    },
    ItemStarted {
        workspace_id: String,
        thread_id: String,
        item_id: String,
        item_type: Option<String>,
        task: Option<String>,
        received_at_ms: i64,
    },
    ItemCompleted {
        workspace_id: String,
        thread_id: String,
        item_id: String,
        item_type: Option<String>,
        task: Option<String>,
        received_at_ms: i64,
    },
    ApprovalRequested {
        workspace_id: String,
        request_key: String,
        request_id: String,
        method: String,
        thread_id: Option<String>,
        turn_id: Option<String>,
        item_id: Option<String>,
        params: Value,
        received_at_ms: i64,
    },
    Error {
        workspace_id: String,
        thread_id: Option<String>,
        turn_id: Option<String>,
        message: String,
        will_retry: bool,
        received_at_ms: i64,
    },
}

pub(crate) fn normalize_app_server_event(
    workspace_id: &str,
    message: &Value,
    received_at_ms: i64,
) -> Option<SupervisorEvent> {
    let message = message.as_object()?;
    let method = message.get("method")?.as_str()?.trim();
    if method.is_empty() {
        return None;
    }

    let params = message
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    match method {
        "turn/started" => normalize_turn_event(workspace_id, &params, received_at_ms, true),
        "turn/completed" => normalize_turn_event(workspace_id, &params, received_at_ms, false),
        "item/started" => normalize_item_event(workspace_id, &params, received_at_ms, true),
        "item/completed" => normalize_item_event(workspace_id, &params, received_at_ms, false),
        "error" => normalize_error_event(workspace_id, &params, received_at_ms),
        _ if method.ends_with("requestApproval") => {
            normalize_approval_event(workspace_id, method, message, &params, received_at_ms)
        }
        _ => None,
    }
}

fn normalize_turn_event(
    workspace_id: &str,
    params: &Map<String, Value>,
    received_at_ms: i64,
    started: bool,
) -> Option<SupervisorEvent> {
    let turn = params.get("turn").and_then(Value::as_object);
    let thread_id = extract_field(params, &["threadId", "thread_id"])
        .or_else(|| turn.and_then(|value| extract_field(value, &["threadId", "thread_id"])))?;
    let turn_id = extract_field(params, &["turnId", "turn_id"])
        .or_else(|| turn.and_then(|value| extract_field(value, &["id"])))?;
    let task = extract_task(params).or_else(|| turn.and_then(extract_task));

    if started {
        Some(SupervisorEvent::TurnStarted {
            workspace_id: workspace_id.to_string(),
            thread_id,
            turn_id,
            task,
            received_at_ms,
        })
    } else {
        Some(SupervisorEvent::TurnCompleted {
            workspace_id: workspace_id.to_string(),
            thread_id,
            turn_id,
            task,
            received_at_ms,
        })
    }
}

fn normalize_item_event(
    workspace_id: &str,
    params: &Map<String, Value>,
    received_at_ms: i64,
    started: bool,
) -> Option<SupervisorEvent> {
    let item = params.get("item").and_then(Value::as_object);
    let thread_id = extract_field(params, &["threadId", "thread_id"])
        .or_else(|| item.and_then(|value| extract_field(value, &["threadId", "thread_id"])))?;
    let item_id = extract_field(params, &["itemId", "item_id"])
        .or_else(|| item.and_then(|value| extract_field(value, &["id"])))?;
    let item_type = item.and_then(|value| extract_field(value, &["type"]));
    let task = extract_task(params).or_else(|| item.and_then(extract_task));

    if started {
        Some(SupervisorEvent::ItemStarted {
            workspace_id: workspace_id.to_string(),
            thread_id,
            item_id,
            item_type,
            task,
            received_at_ms,
        })
    } else {
        Some(SupervisorEvent::ItemCompleted {
            workspace_id: workspace_id.to_string(),
            thread_id,
            item_id,
            item_type,
            task,
            received_at_ms,
        })
    }
}

fn normalize_approval_event(
    workspace_id: &str,
    method: &str,
    message: &Map<String, Value>,
    params: &Map<String, Value>,
    received_at_ms: i64,
) -> Option<SupervisorEvent> {
    let request_id = extract_request_id(message)?;
    let thread_id = extract_field(params, &["threadId", "thread_id"]);
    let turn_id = extract_field(params, &["turnId", "turn_id"]);
    let item_id = extract_field(params, &["itemId", "item_id"]);

    Some(SupervisorEvent::ApprovalRequested {
        workspace_id: workspace_id.to_string(),
        request_key: format!("{workspace_id}:{request_id}"),
        request_id,
        method: method.to_string(),
        thread_id,
        turn_id,
        item_id,
        params: Value::Object(params.clone()),
        received_at_ms,
    })
}

fn normalize_error_event(
    workspace_id: &str,
    params: &Map<String, Value>,
    received_at_ms: i64,
) -> Option<SupervisorEvent> {
    let message = params
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| extract_field(error, &["message"]))
        .or_else(|| extract_field(params, &["message"]))
        .unwrap_or_default();

    if message.is_empty() {
        return None;
    }

    let thread_id = extract_field(params, &["threadId", "thread_id"]);
    let turn_id = extract_field(params, &["turnId", "turn_id"]);
    let will_retry = extract_bool(params, &["willRetry", "will_retry"]).unwrap_or(false);

    Some(SupervisorEvent::Error {
        workspace_id: workspace_id.to_string(),
        thread_id,
        turn_id,
        message,
        will_retry,
        received_at_ms,
    })
}

fn extract_field(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let value = map.get(*key).and_then(Value::as_str).map(str::trim);
        if let Some(value) = value {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_bool(map: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(value) = map.get(*key).and_then(Value::as_bool) {
            return Some(value);
        }
    }
    None
}

fn extract_request_id(message: &Map<String, Value>) -> Option<String> {
    let id = message.get("id")?;
    if let Some(number) = id.as_i64() {
        return Some(number.to_string());
    }
    if let Some(number) = id.as_u64() {
        return Some(number.to_string());
    }
    id.as_str().map(|value| value.trim().to_string())
}

fn extract_task(map: &Map<String, Value>) -> Option<String> {
    extract_field(
        map,
        &["currentTask", "current_task", "summary", "preview", "title"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_turn_started_event() {
        let event = normalize_app_server_event(
            "ws-1",
            &json!({
                "method": "turn/started",
                "params": {
                    "turn": {
                        "id": "turn-1",
                        "threadId": "thread-1",
                        "summary": "Implement feature"
                    }
                }
            }),
            100,
        );

        assert_eq!(
            event,
            Some(SupervisorEvent::TurnStarted {
                workspace_id: "ws-1".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                task: Some("Implement feature".to_string()),
                received_at_ms: 100,
            })
        );
    }

    #[test]
    fn normalizes_item_completed_event() {
        let event = normalize_app_server_event(
            "ws-2",
            &json!({
                "method": "item/completed",
                "params": {
                    "thread_id": "thread-2",
                    "item": {
                        "id": "item-2",
                        "type": "agentMessage",
                        "title": "Answer user"
                    }
                }
            }),
            222,
        );

        assert_eq!(
            event,
            Some(SupervisorEvent::ItemCompleted {
                workspace_id: "ws-2".to_string(),
                thread_id: "thread-2".to_string(),
                item_id: "item-2".to_string(),
                item_type: Some("agentMessage".to_string()),
                task: Some("Answer user".to_string()),
                received_at_ms: 222,
            })
        );
    }

    #[test]
    fn normalizes_approval_request_event() {
        let event = normalize_app_server_event(
            "ws-3",
            &json!({
                "id": 7,
                "method": "workspace/requestApproval",
                "params": {
                    "threadId": "thread-3",
                    "turnId": "turn-3",
                    "itemId": "item-3",
                    "mode": "full"
                }
            }),
            333,
        );

        assert_eq!(
            event,
            Some(SupervisorEvent::ApprovalRequested {
                workspace_id: "ws-3".to_string(),
                request_key: "ws-3:7".to_string(),
                request_id: "7".to_string(),
                method: "workspace/requestApproval".to_string(),
                thread_id: Some("thread-3".to_string()),
                turn_id: Some("turn-3".to_string()),
                item_id: Some("item-3".to_string()),
                params: json!({
                    "threadId": "thread-3",
                    "turnId": "turn-3",
                    "itemId": "item-3",
                    "mode": "full"
                }),
                received_at_ms: 333,
            })
        );
    }

    #[test]
    fn normalizes_error_event() {
        let event = normalize_app_server_event(
            "ws-4",
            &json!({
                "method": "error",
                "params": {
                    "threadId": "thread-4",
                    "turnId": "turn-4",
                    "willRetry": true,
                    "error": {
                        "message": "network timeout"
                    }
                }
            }),
            444,
        );

        assert_eq!(
            event,
            Some(SupervisorEvent::Error {
                workspace_id: "ws-4".to_string(),
                thread_id: Some("thread-4".to_string()),
                turn_id: Some("turn-4".to_string()),
                message: "network timeout".to_string(),
                will_retry: true,
                received_at_ms: 444,
            })
        );
    }

    #[test]
    fn ignores_unmapped_events() {
        let event = normalize_app_server_event(
            "ws-9",
            &json!({
                "method": "codex/connected",
                "params": { "workspaceId": "ws-9" }
            }),
            999,
        );

        assert_eq!(event, None);
    }
}
