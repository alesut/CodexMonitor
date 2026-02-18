use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::backend::events::{AppServerEvent, EventSink, TerminalExit, TerminalOutput};
use crate::shared::supervisor_core::supervisor_loop::{now_timestamp_ms, SupervisorLoop};

#[derive(Clone)]
pub(crate) struct TauriEventSink {
    app: AppHandle,
    supervisor_loop: Option<Arc<Mutex<SupervisorLoop>>>,
}

impl TauriEventSink {
    pub(crate) fn new(app: AppHandle, supervisor_loop: Option<Arc<Mutex<SupervisorLoop>>>) -> Self {
        Self {
            app,
            supervisor_loop,
        }
    }
}

impl EventSink for TauriEventSink {
    fn emit_app_server_event(&self, event: AppServerEvent) {
        if let Some(supervisor_loop) = self.supervisor_loop.as_ref().map(Arc::clone) {
            let workspace_id = event.workspace_id.clone();
            let message = event.message.clone();
            tauri::async_runtime::spawn(async move {
                let mut supervisor_loop = supervisor_loop.lock().await;
                supervisor_loop.apply_app_server_event(&workspace_id, &message, now_timestamp_ms());
            });
        }
        let _ = self.app.emit("app-server-event", event);
    }

    fn emit_terminal_output(&self, event: TerminalOutput) {
        let _ = self.app.emit("terminal-output", event);
    }

    fn emit_terminal_exit(&self, event: TerminalExit) {
        let _ = self.app.emit("terminal-exit", event);
    }
}
