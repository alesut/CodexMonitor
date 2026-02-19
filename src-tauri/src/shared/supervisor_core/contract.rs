use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::dispatch::SupervisorDispatchAction;

pub(crate) const SUPERVISOR_ACTION_CONTRACT_VERSION: &str = "supervisor.dispatch.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SupervisorActionContract {
    pub(crate) version: String,
    pub(crate) actions: Vec<SupervisorPlannerAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SupervisorPlannerAction {
    DispatchTurn(SupervisorDispatchTurnAction),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SupervisorDispatchTurnAction {
    pub(crate) action_id: String,
    pub(crate) workspace_id: String,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) dedupe_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedSupervisorActionContract {
    pub(crate) version: String,
    pub(crate) dispatch_actions: Vec<SupervisorDispatchAction>,
}

#[cfg(test)]
pub(crate) fn parse_supervisor_action_contract_json(
    raw_json: &str,
) -> Result<ValidatedSupervisorActionContract, String> {
    let value: Value = serde_json::from_str(raw_json)
        .map_err(|error| format!("invalid supervisor action contract JSON: {error}"))?;
    parse_supervisor_action_contract_value(&value)
}

pub(crate) fn parse_supervisor_action_contract_value(
    value: &Value,
) -> Result<ValidatedSupervisorActionContract, String> {
    let contract: SupervisorActionContract = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid supervisor action contract: {error}"))?;
    validate_supervisor_action_contract(contract)
}

pub(crate) fn validate_supervisor_action_contract(
    contract: SupervisorActionContract,
) -> Result<ValidatedSupervisorActionContract, String> {
    let version = contract.version.trim();
    if version != SUPERVISOR_ACTION_CONTRACT_VERSION {
        return Err(format!(
            "unsupported supervisor contract version `{}` (expected `{}`)",
            version, SUPERVISOR_ACTION_CONTRACT_VERSION
        ));
    }

    if contract.actions.is_empty() {
        return Err("actions must contain at least one item".to_string());
    }

    let mut seen_action_ids = HashSet::new();
    let mut seen_dedupe_keys = HashSet::new();
    let mut dispatch_actions = Vec::with_capacity(contract.actions.len());

    for action in contract.actions {
        let dispatch_action = match action {
            SupervisorPlannerAction::DispatchTurn(action) => {
                normalize_dispatch_turn_action(action)?
            }
        };

        if !seen_action_ids.insert(dispatch_action.action_id.clone()) {
            return Err(format!(
                "duplicate action_id `{}` in supervisor contract",
                dispatch_action.action_id
            ));
        }

        let dedupe_token = dispatch_action
            .dedupe_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(dispatch_action.action_id.as_str());
        let scoped_dedupe_key = format!("{}:{dedupe_token}", dispatch_action.workspace_id);
        if !seen_dedupe_keys.insert(scoped_dedupe_key) {
            return Err(format!(
                "duplicate dedupe key `{}` for workspace `{}`",
                dedupe_token, dispatch_action.workspace_id
            ));
        }

        dispatch_actions.push(dispatch_action);
    }

    Ok(ValidatedSupervisorActionContract {
        version: SUPERVISOR_ACTION_CONTRACT_VERSION.to_string(),
        dispatch_actions,
    })
}

fn normalize_dispatch_turn_action(
    action: SupervisorDispatchTurnAction,
) -> Result<SupervisorDispatchAction, String> {
    let action_id = normalize_required("action_id", action.action_id)?;
    let workspace_id = normalize_required("workspace_id", action.workspace_id)?;
    let prompt = normalize_required("prompt", action.prompt)?;

    Ok(SupervisorDispatchAction {
        action_id,
        workspace_id,
        prompt,
        thread_id: normalize_optional(action.thread_id),
        dedupe_key: normalize_optional(action.dedupe_key),
    })
}

fn normalize_required(field_name: &str, value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field_name} is required"));
    }
    Ok(trimmed.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_and_normalizes_dispatch_actions() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "dispatch_turn",
                    "action_id": " action-1 ",
                    "workspace_id": " ws-1 ",
                    "thread_id": " thread-1 ",
                    "prompt": " fix failing tests ",
                    "dedupe_key": " dispatch-1 "
                },
                {
                    "type": "dispatch_turn",
                    "action_id": "action-2",
                    "workspace_id": "ws-2",
                    "prompt": "ship release"
                }
            ]
        });

        let validated = parse_supervisor_action_contract_value(&value).expect("valid contract");

        assert_eq!(
            validated.version,
            SUPERVISOR_ACTION_CONTRACT_VERSION.to_string()
        );
        assert_eq!(validated.dispatch_actions.len(), 2);
        assert_eq!(validated.dispatch_actions[0].action_id, "action-1");
        assert_eq!(validated.dispatch_actions[0].workspace_id, "ws-1");
        assert_eq!(
            validated.dispatch_actions[0].thread_id.as_deref(),
            Some("thread-1")
        );
        assert_eq!(validated.dispatch_actions[0].prompt, "fix failing tests");
        assert_eq!(
            validated.dispatch_actions[0].dedupe_key.as_deref(),
            Some("dispatch-1")
        );
        assert_eq!(validated.dispatch_actions[1].action_id, "action-2");
        assert_eq!(validated.dispatch_actions[1].dedupe_key, None);
    }

    #[test]
    fn rejects_unknown_contract_version() {
        let value = json!({
            "version": "supervisor.dispatch.v0",
            "actions": [{
                "type": "dispatch_turn",
                "action_id": "action-1",
                "workspace_id": "ws-1",
                "prompt": "run"
            }]
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("version mismatch");
        assert!(error.contains("unsupported supervisor contract version"));
    }

    #[test]
    fn rejects_empty_actions_array() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": []
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("empty actions");
        assert_eq!(error, "actions must contain at least one item");
    }

    #[test]
    fn rejects_duplicate_action_id() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "first"
                },
                {
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-2",
                    "prompt": "second"
                }
            ]
        });

        let error =
            parse_supervisor_action_contract_value(&value).expect_err("duplicate action id");
        assert!(error.contains("duplicate action_id `action-1`"));
    }

    #[test]
    fn rejects_duplicate_dedupe_key_within_workspace() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "first",
                    "dedupe_key": "same"
                },
                {
                    "type": "dispatch_turn",
                    "action_id": "action-2",
                    "workspace_id": "ws-1",
                    "prompt": "second",
                    "dedupe_key": "same"
                }
            ]
        });

        let error =
            parse_supervisor_action_contract_value(&value).expect_err("duplicate dedupe key");
        assert!(error.contains("duplicate dedupe key `same` for workspace `ws-1`"));
    }

    #[test]
    fn allows_same_dedupe_key_in_different_workspaces() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "first",
                    "dedupe_key": "same"
                },
                {
                    "type": "dispatch_turn",
                    "action_id": "action-2",
                    "workspace_id": "ws-2",
                    "prompt": "second",
                    "dedupe_key": "same"
                }
            ]
        });

        let validated = parse_supervisor_action_contract_value(&value).expect("valid contract");
        assert_eq!(validated.dispatch_actions.len(), 2);
    }

    #[test]
    fn rejects_blank_required_fields() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "   "
                }
            ]
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("blank prompt");
        assert_eq!(error, "prompt is required");
    }

    #[test]
    fn rejects_unknown_action_type() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [
                {
                    "type": "noop",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "noop"
                }
            ]
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("unknown action");
        assert!(error.contains("unknown variant"));
    }

    #[test]
    fn rejects_unknown_top_level_fields() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [{
                "type": "dispatch_turn",
                "action_id": "action-1",
                "workspace_id": "ws-1",
                "prompt": "run"
            }],
            "unexpected": true
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("unknown field");
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_action_fields() {
        let value = json!({
            "version": SUPERVISOR_ACTION_CONTRACT_VERSION,
            "actions": [{
                "type": "dispatch_turn",
                "action_id": "action-1",
                "workspace_id": "ws-1",
                "prompt": "run",
                "extra": "field"
            }]
        });

        let error = parse_supervisor_action_contract_value(&value).expect_err("unknown field");
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn parses_contract_from_json_string() {
        let validated = parse_supervisor_action_contract_json(
            r#"{
                "version": "supervisor.dispatch.v1",
                "actions": [{
                    "type": "dispatch_turn",
                    "action_id": "action-1",
                    "workspace_id": "ws-1",
                    "prompt": "run"
                }]
            }"#,
        )
        .expect("valid contract json");

        assert_eq!(validated.dispatch_actions.len(), 1);
        assert_eq!(validated.dispatch_actions[0].action_id, "action-1");
    }
}
