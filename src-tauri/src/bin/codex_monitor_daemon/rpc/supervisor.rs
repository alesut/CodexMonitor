use super::*;

pub(super) async fn try_handle(
    state: &DaemonState,
    method: &str,
    params: &Value,
    client_version: &str,
) -> Option<Result<Value, String>> {
    match method {
        "supervisor_snapshot" => Some(state.supervisor_snapshot().await),
        "supervisor_feed" => {
            let limit = parse_optional_u32(params, "limit");
            let needs_input_only = parse_optional_bool(params, "needsInputOnly").unwrap_or(false);
            Some(state.supervisor_feed(limit, needs_input_only).await)
        }
        "supervisor_dispatch" => {
            let contract = match parse_optional_value(params, "contract") {
                Some(value) => value,
                None => return Some(Err("missing `contract`".to_string())),
            };
            Some(state.supervisor_dispatch(contract).await)
        }
        "supervisor_ack_signal" => {
            let signal_id = match parse_string(params, "signalId") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            Some(state.supervisor_ack_signal(signal_id).await)
        }
        "supervisor_chat_history" => Some(state.supervisor_chat_history().await),
        "supervisor_chat_send" => {
            let command = match parse_string(params, "command") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            Some(
                state
                    .supervisor_chat_send(command, client_version.to_string())
                    .await,
            )
        }
        _ => None,
    }
}
