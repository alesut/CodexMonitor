use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::shared::supervisor_core::{SupervisorSignal, SupervisorSignalKind};
use crate::shared::supervisor_core::service as supervisor_service;

use super::DaemonState;

const DEFAULT_POLL_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_SYNC_INTERVAL_SECONDS: u64 = 5;

#[derive(Debug, Clone)]
pub(crate) struct TelegramBridgeConfig {
    bot_token: String,
    allowed_user_id: i64,
    allowed_chat_id: Option<i64>,
    poll_timeout_seconds: u64,
    sync_interval_seconds: u64,
}

impl TelegramBridgeConfig {
    pub(crate) fn from_env() -> Option<Self> {
        let bot_token = std::env::var("SUPERVISOR_TELEGRAM_BOT_TOKEN").ok()?;
        let allowed_user_id = std::env::var("SUPERVISOR_TELEGRAM_ALLOWED_USER_ID")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())?;
        let allowed_chat_id = std::env::var("SUPERVISOR_TELEGRAM_ALLOWED_CHAT_ID")
            .ok()
            .and_then(|value| value.parse::<i64>().ok());
        let poll_timeout_seconds = std::env::var("SUPERVISOR_TELEGRAM_POLL_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_POLL_TIMEOUT_SECONDS);
        let sync_interval_seconds = std::env::var("SUPERVISOR_TELEGRAM_SYNC_INTERVAL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_SYNC_INTERVAL_SECONDS);

        Some(Self {
            bot_token,
            allowed_user_id,
            allowed_chat_id,
            poll_timeout_seconds,
            sync_interval_seconds,
        })
    }

    fn api_base(&self) -> String {
        format!("https://api.telegram.org/bot{}", self.bot_token)
    }
}

#[derive(Debug, Deserialize)]
struct TelegramGetUpdatesResponse {
    ok: bool,
    #[serde(default)]
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    from: Option<TelegramUser>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
}

#[derive(Debug, Serialize)]
struct SendMessagePayload<'a> {
    chat_id: i64,
    text: &'a str,
}

pub(crate) async fn run(state: Arc<DaemonState>, config: TelegramBridgeConfig) {
    let client = Client::builder()
        .timeout(Duration::from_secs(config.poll_timeout_seconds + 10))
        .build();

    let client = match client {
        Ok(client) => client,
        Err(error) => {
            eprintln!("telegram bridge disabled: failed to build HTTP client: {error}");
            return;
        }
    };

    eprintln!(
        "telegram bridge enabled for user {}{}",
        config.allowed_user_id,
        config
            .allowed_chat_id
            .map(|chat_id| format!(", chat {chat_id}"))
            .unwrap_or_default()
    );

    let mut offset: Option<i64> = None;
    let mut notified_signal_ids: HashSet<String> = HashSet::new();

    loop {
        if let Err(error) = sync_notifications(&state, &config, &client, &mut notified_signal_ids).await {
            eprintln!("telegram notification sync error: {error}");
        }

        match poll_updates(&config, &client, offset).await {
            Ok(updates) => {
                for update in updates {
                    offset = Some(update.update_id + 1);
                    if let Err(error) =
                        handle_update(&state, &config, &client, update).await
                    {
                        eprintln!("telegram update handling error: {error}");
                    }
                }
            }
            Err(error) => {
                eprintln!("telegram getUpdates failed: {error}");
                tokio::time::sleep(Duration::from_secs(config.sync_interval_seconds.max(1))).await;
            }
        }
    }
}

async fn poll_updates(
    config: &TelegramBridgeConfig,
    client: &Client,
    offset: Option<i64>,
) -> Result<Vec<TelegramUpdate>, String> {
    let mut payload = json!({
        "timeout": config.poll_timeout_seconds,
        "allowed_updates": ["message"],
    });
    if let Some(offset) = offset {
        payload["offset"] = json!(offset);
    }

    let response = client
        .post(format!("{}/getUpdates", config.api_base()))
        .json(&payload)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("status {status}: {body}"));
    }

    let parsed: TelegramGetUpdatesResponse = response.json().await.map_err(|error| error.to_string())?;
    if !parsed.ok {
        return Err("Telegram API returned ok=false for getUpdates".to_string());
    }

    Ok(parsed.result)
}

async fn handle_update(
    state: &Arc<DaemonState>,
    config: &TelegramBridgeConfig,
    client: &Client,
    update: TelegramUpdate,
) -> Result<(), String> {
    let message = match update.message {
        Some(message) => message,
        None => return Ok(()),
    };

    let from = message
        .from
        .ok_or_else(|| "message has no sender".to_string())?;

    let allowed = from.id == config.allowed_user_id
        && config
            .allowed_chat_id
            .map(|chat_id| chat_id == message.chat.id)
            .unwrap_or(true);

    if !allowed {
        if config.allowed_chat_id.is_none() {
            let _ = send_message(
                config,
                client,
                message.chat.id,
                "Access denied. Разрешен только администраторский Telegram-аккаунт.",
            )
            .await;
        }
        return Ok(());
    }

    let text = message
        .text
        .map(|value| value.trim().to_string())
        .unwrap_or_default();

    if text.is_empty() {
        send_message(
            config,
            client,
            message.chat.id,
            "Please send text commands only. Пожалуйста, отправьте текстовую команду.",
        )
        .await?;
        return Ok(());
    }

    if text.eq_ignore_ascii_case("/start") || text.eq_ignore_ascii_case("start") {
        send_message(
            config,
            client,
            message.chat.id,
            "Supervisor bot online ✅\nUse /help or plain language (EN/RU):\n- status\n- feed\n- dispatch task to workspace ...\n\nБот Supervisor в сети ✅\nМожно писать команды в свободной форме.",
        )
        .await?;
        return Ok(());
    }

    let response = state.supervisor_chat_send(text).await?;
    let rendered = extract_chat_response_text(&response)
        .unwrap_or_else(|| "Command accepted. Команда принята.".to_string());
    send_message(config, client, message.chat.id, rendered.as_str()).await
}

fn extract_chat_response_text(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("messages")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|message| {
            if message.get("role").and_then(|role| role.as_str()) == Some("system") {
                return message
                    .get("text")
                    .and_then(|text| text.as_str())
                    .map(ToString::to_string);
            }
            None
        })
}

async fn sync_notifications(
    state: &Arc<DaemonState>,
    config: &TelegramBridgeConfig,
    client: &Client,
    notified_signal_ids: &mut HashSet<String>,
) -> Result<(), String> {
    let snapshot = supervisor_service::supervisor_snapshot_core(&state.supervisor_loop).await;
    let Some(chat_id) = config.allowed_chat_id else {
        return Ok(());
    };

    let mut pending = snapshot
        .signals
        .iter()
        .filter(|signal| signal.acknowledged_at_ms.is_none())
        .filter(|signal| !notified_signal_ids.contains(&signal.id))
        .collect::<Vec<_>>();
    pending.sort_by_key(|signal| signal.created_at_ms);

    for signal in pending {
        let text = format_signal_message(signal);
        send_message(config, client, chat_id, text.as_str()).await?;
        notified_signal_ids.insert(signal.id.clone());
    }

    tokio::time::sleep(Duration::from_secs(config.sync_interval_seconds)).await;
    Ok(())
}

fn format_signal_message(signal: &SupervisorSignal) -> String {
    let kind = match signal.kind {
        SupervisorSignalKind::NeedsApproval => "Needs approval / Требуется подтверждение",
        SupervisorSignalKind::Failed => "Failed / Ошибка",
        SupervisorSignalKind::Completed => "Completed / Выполнено",
        SupervisorSignalKind::Stalled => "Stalled / Застопорилось",
        SupervisorSignalKind::Disconnected => "Disconnected / Отключено",
    };

    format!(
        "🔔 Supervisor signal\nType: {kind}\nMessage: {}\nWorkspace: {}\nThread: {}\n\nСигнал Supervisor: {kind}",
        signal.message,
        signal.workspace_id.as_deref().unwrap_or("-"),
        signal.thread_id.as_deref().unwrap_or("-")
    )
}

async fn send_message(
    config: &TelegramBridgeConfig,
    client: &Client,
    chat_id: i64,
    text: &str,
) -> Result<(), String> {
    let response = client
        .post(format!("{}/sendMessage", config.api_base()))
        .json(&SendMessagePayload { chat_id, text })
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("status {status}: {body}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_system_message_text() {
        let payload = json!({
            "messages": [
                {"role": "user", "text": "ping"},
                {"role": "system", "text": "pong"},
                {"role": "user", "text": "status"},
                {"role": "system", "text": "ok"}
            ]
        });
        assert_eq!(extract_chat_response_text(&payload).as_deref(), Some("ok"));
    }
}
