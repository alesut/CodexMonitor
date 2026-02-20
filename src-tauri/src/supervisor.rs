use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::backend::app_server::WorkspaceSession;
use crate::codex::spawn_workspace_session;
use crate::remote_backend;
use crate::shared::supervisor_core::service as supervisor_service;
use crate::shared::supervisor_core::supervisor_loop;
use crate::shared::workspaces_core;
use crate::state::AppState;
use crate::types::WorkspaceEntry;

fn spawn_supervisor_workspace_session(
    app: &AppHandle,
    entry: WorkspaceEntry,
    default_bin: Option<String>,
    codex_args: Option<String>,
    codex_home: Option<PathBuf>,
) -> impl std::future::Future<Output = Result<Arc<WorkspaceSession>, String>> {
    spawn_workspace_session(entry, default_bin, codex_args, app.clone(), codex_home)
}

#[tauri::command]
pub(crate) async fn supervisor_snapshot(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(&*state, app, "supervisor_snapshot", json!({})).await;
    }

    let snapshot = supervisor_service::supervisor_snapshot_core(&state.supervisor_loop).await;
    serde_json::to_value(snapshot).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn supervisor_feed(
    limit: Option<u32>,
    needs_input_only: Option<bool>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(
            &*state,
            app,
            "supervisor_feed",
            json!({
                "limit": limit,
                "needsInputOnly": needs_input_only,
            }),
        )
        .await;
    }

    let response = supervisor_service::supervisor_feed_core(
        &state.supervisor_loop,
        limit.map(|value| value as usize),
        needs_input_only.unwrap_or(false),
    )
    .await;
    serde_json::to_value(response).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn supervisor_dispatch(
    contract: Value,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(
            &*state,
            app,
            "supervisor_dispatch",
            json!({ "contract": contract }),
        )
        .await;
    }

    let response = supervisor_service::supervisor_dispatch_core(
        &state.supervisor_loop,
        &state.supervisor_dispatch_executor,
        &state.sessions,
        &contract,
    )
    .await?;
    serde_json::to_value(response).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn supervisor_ack_signal(
    signal_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(
            &*state,
            app,
            "supervisor_ack_signal",
            json!({ "signalId": signal_id }),
        )
        .await;
    }

    supervisor_service::supervisor_ack_signal_core(
        &state.supervisor_loop,
        signal_id.as_str(),
        supervisor_loop::now_timestamp_ms(),
    )
    .await?;
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub(crate) async fn supervisor_chat_history(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(&*state, app, "supervisor_chat_history", json!({}))
            .await;
    }

    let response = supervisor_service::supervisor_chat_history_core(&state.supervisor_loop).await;
    serde_json::to_value(response).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn supervisor_chat_send(
    command: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    if remote_backend::is_remote_mode(&*state).await {
        return remote_backend::call_remote(
            &*state,
            app,
            "supervisor_chat_send",
            json!({ "command": command }),
        )
        .await;
    }

    if let Some(workspace_id) = supervisor_service::supervisor_chat_autoconnect_target_core(
        &state.supervisor_loop,
        &state.sessions,
        &state.workspaces,
        &state.app_settings,
        &command,
    )
    .await
    {
        let _ = workspaces_core::connect_workspace_core(
            workspace_id,
            &state.workspaces,
            &state.sessions,
            &state.app_settings,
            |entry, default_bin, codex_args, codex_home| {
                spawn_supervisor_workspace_session(&app, entry, default_bin, codex_args, codex_home)
            },
        )
        .await;
    }

    let response = supervisor_service::supervisor_chat_send_core(
        &state.supervisor_loop,
        &state.supervisor_dispatch_executor,
        &state.sessions,
        &state.workspaces,
        &state.app_settings,
        &command,
        supervisor_loop::now_timestamp_ms(),
    )
    .await?;
    serde_json::to_value(response).map_err(|error| error.to_string())
}
