use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::AppSettings;

use super::SupervisorHealth;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorRouteWorkspaceMetadata {
    pub(crate) workspace_id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) branch: Option<String>,
    pub(crate) connected: bool,
    pub(crate) available: bool,
    pub(crate) health: SupervisorHealth,
    #[serde(default)]
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorRouteKind {
    WorkspaceDelegate,
    LocalTool,
    Clarification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorLocalTool {
    Status,
    Feed,
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorRouteDecision {
    pub(crate) kind: SupervisorRouteKind,
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(default)]
    pub(crate) local_tool: Option<SupervisorLocalTool>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) used_dedicated_workspace: bool,
    #[serde(default)]
    pub(crate) fallback_message: Option<String>,
    #[serde(default)]
    pub(crate) clarification: Option<String>,
    #[serde(default)]
    pub(crate) options: Vec<String>,
    #[serde(default)]
    pub(crate) candidates: Vec<SupervisorRouteWorkspaceMetadata>,
}

#[derive(Debug, Clone)]
struct ScoredCandidate {
    workspace: SupervisorRouteWorkspaceMetadata,
    score: i32,
    explicit_match: bool,
}

pub(crate) fn select_supervisor_route(
    prompt: &str,
    workspaces: &[SupervisorRouteWorkspaceMetadata],
    settings: &AppSettings,
) -> SupervisorRouteDecision {
    let prompt_lower = prompt.trim().to_lowercase();
    let sorted_candidates = sort_candidates(workspaces);

    if let Some(local_tool) = detect_local_tool(&prompt_lower) {
        return SupervisorRouteDecision {
            kind: SupervisorRouteKind::LocalTool,
            reason: "Prompt matched Supervisor local-tool intent.".to_string(),
            workspace_id: None,
            local_tool: Some(local_tool),
            model: None,
            used_dedicated_workspace: false,
            fallback_message: None,
            clarification: None,
            options: Vec::new(),
            candidates: sorted_candidates,
        };
    }

    let available = sorted_candidates
        .iter()
        .filter(|workspace| workspace.connected && workspace.available)
        .filter(|workspace| !matches!(workspace.health, SupervisorHealth::Disconnected))
        .cloned()
        .collect::<Vec<_>>();

    if available.is_empty() {
        return clarification_decision(
            "No connected workspace is currently available for delegation.".to_string(),
            "Connect a workspace or explicitly route with `/dispatch --ws ...`.".to_string(),
            Vec::new(),
            sorted_candidates,
            None,
        );
    }

    if settings.supervisor_dedicated_workspace_enabled {
        let dedicated_model = normalize_model(settings.supervisor_fast_model.as_str());
        if let Some(dedicated_workspace_id) = settings
            .supervisor_dedicated_workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(workspace) = available
                .iter()
                .find(|workspace| workspace.workspace_id == dedicated_workspace_id)
            {
                return workspace_decision(
                    workspace.workspace_id.clone(),
                    format!(
                        "Dedicated Supervisor workspace mode is enabled; routed to `{}`.",
                        workspace.workspace_id
                    ),
                    dedicated_model,
                    true,
                    None,
                    sorted_candidates,
                );
            }

            return select_standard_workspace_route(
                &prompt_lower,
                &available,
                sorted_candidates,
                Some(format!(
                    "Dedicated workspace `{dedicated_workspace_id}` is unavailable; using standard routing fallback."
                )),
            );
        }

        if let Some(workspace) = available.first() {
            return workspace_decision(
                workspace.workspace_id.clone(),
                format!(
                    "Dedicated Supervisor workspace mode is enabled; auto-selected `{}` as the current dedicated target.",
                    workspace.workspace_id
                ),
                dedicated_model,
                true,
                None,
                sorted_candidates,
            );
        }
    }

    select_standard_workspace_route(&prompt_lower, &available, sorted_candidates, None)
}

fn select_standard_workspace_route(
    prompt_lower: &str,
    available: &[SupervisorRouteWorkspaceMetadata],
    candidates: Vec<SupervisorRouteWorkspaceMetadata>,
    fallback_message: Option<String>,
) -> SupervisorRouteDecision {
    let mut scored = available
        .iter()
        .cloned()
        .map(|workspace| {
            let explicit_match = prompt_mentions_workspace(prompt_lower, &workspace);
            let mut score = match workspace.health {
                SupervisorHealth::Healthy => 30,
                SupervisorHealth::Stale => 18,
                SupervisorHealth::Disconnected => -100,
            };
            score += if workspace.available { 15 } else { -100 };
            score += if workspace.connected { 10 } else { -100 };
            score += (workspace.capabilities.len() as i32).min(6);
            if explicit_match {
                score += 70;
            }
            ScoredCandidate {
                workspace,
                score,
                explicit_match,
            }
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| {
        right.score.cmp(&left.score).then_with(|| {
            left.workspace
                .workspace_id
                .cmp(&right.workspace.workspace_id)
        })
    });

    let Some(best) = scored.first() else {
        return clarification_decision(
            "No workspace candidates are available for routing.".to_string(),
            "Connect a workspace or use `/dispatch --ws ...`.".to_string(),
            Vec::new(),
            candidates,
            fallback_message,
        );
    };

    let is_ambiguous = scored
        .get(1)
        .map(|next| next.score == best.score && !best.explicit_match && !next.explicit_match)
        .unwrap_or(false);
    if is_ambiguous || best.score < 25 {
        let options = scored
            .iter()
            .take(4)
            .map(|entry| entry.workspace.workspace_id.clone())
            .collect::<Vec<_>>();
        return clarification_decision(
            "Workspace route is ambiguous.".to_string(),
            "Specify target workspace in chat (for example: `workspace ws-1 ...`) or use `/dispatch --ws ...`."
                .to_string(),
            options,
            candidates,
            fallback_message,
        );
    }

    workspace_decision(
        best.workspace.workspace_id.clone(),
        if best.explicit_match {
            format!(
                "Prompt explicitly matched workspace metadata; selected `{}`.",
                best.workspace.workspace_id
            )
        } else {
            format!(
                "Selected `{}` as the highest-ranked available workspace (score {}).",
                best.workspace.workspace_id, best.score
            )
        },
        None,
        false,
        fallback_message,
        candidates,
    )
}

fn workspace_decision(
    workspace_id: String,
    reason: String,
    model: Option<String>,
    used_dedicated_workspace: bool,
    fallback_message: Option<String>,
    candidates: Vec<SupervisorRouteWorkspaceMetadata>,
) -> SupervisorRouteDecision {
    SupervisorRouteDecision {
        kind: SupervisorRouteKind::WorkspaceDelegate,
        reason,
        workspace_id: Some(workspace_id),
        local_tool: None,
        model,
        used_dedicated_workspace,
        fallback_message,
        clarification: None,
        options: Vec::new(),
        candidates,
    }
}

fn clarification_decision(
    reason: String,
    clarification: String,
    options: Vec<String>,
    candidates: Vec<SupervisorRouteWorkspaceMetadata>,
    fallback_message: Option<String>,
) -> SupervisorRouteDecision {
    SupervisorRouteDecision {
        kind: SupervisorRouteKind::Clarification,
        reason,
        workspace_id: None,
        local_tool: None,
        model: None,
        used_dedicated_workspace: false,
        fallback_message,
        clarification: Some(clarification),
        options,
        candidates,
    }
}

fn normalize_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn detect_local_tool(prompt_lower: &str) -> Option<SupervisorLocalTool> {
    let trimmed =
        prompt_lower.trim_matches(|ch: char| ch.is_whitespace() || ch.is_ascii_punctuation());
    if trimmed.starts_with("help")
        || trimmed == "what can you do"
        || trimmed == "what are the available commands"
    {
        return Some(SupervisorLocalTool::Help);
    }
    if trimmed.starts_with("status")
        || trimmed.starts_with("show status")
        || trimmed.starts_with("supervisor status")
        || trimmed == "global status"
    {
        return Some(SupervisorLocalTool::Status);
    }
    if trimmed.starts_with("feed")
        || trimmed.starts_with("activity feed")
        || trimmed.starts_with("show feed")
        || trimmed == "activity"
    {
        return Some(SupervisorLocalTool::Feed);
    }
    None
}

fn prompt_mentions_workspace(
    prompt_lower: &str,
    workspace: &SupervisorRouteWorkspaceMetadata,
) -> bool {
    let id_match = prompt_lower.contains(workspace.workspace_id.to_lowercase().as_str());
    if id_match {
        return true;
    }

    let name_match = workspace
        .name
        .trim()
        .to_lowercase()
        .split_whitespace()
        .filter(|token| token.len() >= 3)
        .any(|token| prompt_lower.contains(token));
    if name_match {
        return true;
    }

    let path_basename_match = Path::new(workspace.path.as_str())
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(|value| value.to_lowercase())
        .filter(|value| !value.is_empty())
        .is_some_and(|value| prompt_lower.contains(value.as_str()));
    if path_basename_match {
        return true;
    }

    workspace
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase())
        .is_some_and(|value| prompt_lower.contains(value.as_str()))
}

fn sort_candidates(
    workspaces: &[SupervisorRouteWorkspaceMetadata],
) -> Vec<SupervisorRouteWorkspaceMetadata> {
    let mut candidates = workspaces.to_vec();
    candidates.sort_by(|left, right| left.workspace_id.cmp(&right.workspace_id));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(
        workspace_id: &str,
        health: SupervisorHealth,
        connected: bool,
        available: bool,
    ) -> SupervisorRouteWorkspaceMetadata {
        SupervisorRouteWorkspaceMetadata {
            workspace_id: workspace_id.to_string(),
            name: format!("Workspace {workspace_id}"),
            path: format!("/tmp/{workspace_id}"),
            branch: Some("main".to_string()),
            connected,
            available,
            health,
            capabilities: vec!["thread_start".to_string(), "turn_start".to_string()],
        }
    }

    #[test]
    fn routes_local_tool_intents_without_delegation() {
        let settings = AppSettings::default();
        let route = select_supervisor_route(
            "show status",
            &[workspace("ws-1", SupervisorHealth::Healthy, true, true)],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::LocalTool);
        assert_eq!(route.local_tool, Some(SupervisorLocalTool::Status));
        assert!(route.workspace_id.is_none());
    }

    #[test]
    fn routes_to_explicitly_mentioned_workspace() {
        let settings = AppSettings::default();
        let route = select_supervisor_route(
            "Run smoke tests in ws-2",
            &[
                workspace("ws-1", SupervisorHealth::Healthy, true, true),
                workspace("ws-2", SupervisorHealth::Healthy, true, true),
            ],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::WorkspaceDelegate);
        assert_eq!(route.workspace_id.as_deref(), Some("ws-2"));
    }

    #[test]
    fn asks_for_clarification_when_route_is_ambiguous() {
        let settings = AppSettings::default();
        let route = select_supervisor_route(
            "Please handle this task",
            &[
                workspace("ws-a", SupervisorHealth::Healthy, true, true),
                workspace("ws-b", SupervisorHealth::Healthy, true, true),
            ],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::Clarification);
        assert!(route
            .clarification
            .as_deref()
            .is_some_and(|message| message.contains("Specify target workspace")));
        assert!(route.options.contains(&"ws-a".to_string()));
        assert!(route.options.contains(&"ws-b".to_string()));
    }

    #[test]
    fn uses_configured_dedicated_workspace_when_enabled() {
        let mut settings = AppSettings::default();
        settings.supervisor_dedicated_workspace_enabled = true;
        settings.supervisor_dedicated_workspace_id = Some("ws-2".to_string());
        settings.supervisor_fast_model = "gpt-5-mini".to_string();

        let route = select_supervisor_route(
            "Deploy latest change",
            &[
                workspace("ws-1", SupervisorHealth::Healthy, true, true),
                workspace("ws-2", SupervisorHealth::Healthy, true, true),
            ],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::WorkspaceDelegate);
        assert_eq!(route.workspace_id.as_deref(), Some("ws-2"));
        assert_eq!(route.model.as_deref(), Some("gpt-5-mini"));
        assert!(route.used_dedicated_workspace);
    }

    #[test]
    fn falls_back_when_configured_dedicated_workspace_is_unavailable() {
        let mut settings = AppSettings::default();
        settings.supervisor_dedicated_workspace_enabled = true;
        settings.supervisor_dedicated_workspace_id = Some("ws-missing".to_string());

        let route = select_supervisor_route(
            "Deploy latest change",
            &[workspace("ws-1", SupervisorHealth::Healthy, true, true)],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::WorkspaceDelegate);
        assert_eq!(route.workspace_id.as_deref(), Some("ws-1"));
        assert!(!route.used_dedicated_workspace);
        assert!(route
            .fallback_message
            .as_deref()
            .is_some_and(|message| message.contains("ws-missing")));
    }

    #[test]
    fn asks_for_clarification_when_no_workspace_is_available() {
        let settings = AppSettings::default();
        let route = select_supervisor_route(
            "Run tests",
            &[workspace(
                "ws-1",
                SupervisorHealth::Disconnected,
                true,
                false,
            )],
            &settings,
        );
        assert_eq!(route.kind, SupervisorRouteKind::Clarification);
        assert!(route.options.is_empty());
    }
}
