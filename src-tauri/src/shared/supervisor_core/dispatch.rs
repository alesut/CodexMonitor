use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::backend::app_server::WorkspaceSession;

type DispatchFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorDispatchAction {
    pub(crate) action_id: String,
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) dedupe_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SupervisorDispatchStatus {
    #[default]
    Dispatched,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SupervisorDispatchActionResult {
    pub(crate) action_id: String,
    pub(crate) workspace_id: String,
    pub(crate) dedupe_key: String,
    pub(crate) status: SupervisorDispatchStatus,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    #[serde(default)]
    pub(crate) turn_id: Option<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct SupervisorDispatchBatchResult {
    #[serde(default)]
    pub(crate) results: Vec<SupervisorDispatchActionResult>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SupervisorDispatchIdempotencyStore {
    entries: BTreeMap<String, SupervisorDispatchActionResult>,
}

impl SupervisorDispatchIdempotencyStore {
    pub(crate) fn get(&self, key: &str) -> Option<&SupervisorDispatchActionResult> {
        self.entries.get(key)
    }

    pub(crate) fn insert(&mut self, key: String, value: SupervisorDispatchActionResult) {
        self.entries.insert(key, value);
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<String, SupervisorDispatchActionResult> {
        self.entries.clone()
    }
}

pub(crate) trait SupervisorDispatchBackend {
    fn start_thread<'a>(
        &'a self,
        workspace_id: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>>;
    fn resume_thread<'a>(
        &'a self,
        workspace_id: &'a str,
        thread_id: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>>;
    fn start_turn<'a>(
        &'a self,
        workspace_id: &'a str,
        thread_id: &'a str,
        prompt: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>>;
}

pub(crate) struct WorkspaceSessionDispatchBackend<'a> {
    sessions: &'a Mutex<HashMap<String, Arc<WorkspaceSession>>>,
}

impl<'a> WorkspaceSessionDispatchBackend<'a> {
    pub(crate) fn new(sessions: &'a Mutex<HashMap<String, Arc<WorkspaceSession>>>) -> Self {
        Self { sessions }
    }

    async fn session_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Arc<WorkspaceSession>, String> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(workspace_id)
            .cloned()
            .ok_or_else(|| format!("workspace `{workspace_id}` is not connected"))
    }
}

impl SupervisorDispatchBackend for WorkspaceSessionDispatchBackend<'_> {
    fn start_thread<'a>(
        &'a self,
        workspace_id: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            let session = self.session_for_workspace(workspace_id).await?;
            let params = json!({
                "cwd": session.entry.path,
                "approvalPolicy": "on-request"
            });
            session.send_request("thread/start", params).await
        })
    }

    fn resume_thread<'a>(
        &'a self,
        workspace_id: &'a str,
        thread_id: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            let session = self.session_for_workspace(workspace_id).await?;
            let params = json!({ "threadId": thread_id });
            session.send_request("thread/resume", params).await
        })
    }

    fn start_turn<'a>(
        &'a self,
        workspace_id: &'a str,
        thread_id: &'a str,
        prompt: &'a str,
    ) -> DispatchFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            let session = self.session_for_workspace(workspace_id).await?;
            let params = json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
                "cwd": session.entry.path,
                "approvalPolicy": "on-request"
            });
            session.send_request("turn/start", params).await
        })
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SupervisorDispatchExecutor {
    idempotency: SupervisorDispatchIdempotencyStore,
}

impl SupervisorDispatchExecutor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_idempotency_store(idempotency: SupervisorDispatchIdempotencyStore) -> Self {
        Self { idempotency }
    }

    pub(crate) fn idempotency_snapshot(&self) -> BTreeMap<String, SupervisorDispatchActionResult> {
        self.idempotency.snapshot()
    }

    pub(crate) async fn dispatch_batch<B>(
        &mut self,
        backend: &B,
        actions: Vec<SupervisorDispatchAction>,
    ) -> SupervisorDispatchBatchResult
    where
        B: SupervisorDispatchBackend,
    {
        let mut results = Vec::with_capacity(actions.len());
        for action in actions {
            results.push(self.dispatch_action(backend, action).await);
        }
        SupervisorDispatchBatchResult { results }
    }

    async fn dispatch_action<B>(
        &mut self,
        backend: &B,
        action: SupervisorDispatchAction,
    ) -> SupervisorDispatchActionResult
    where
        B: SupervisorDispatchBackend,
    {
        let normalized = match NormalizedDispatchAction::try_from(action.clone()) {
            Ok(value) => value,
            Err(error) => {
                return SupervisorDispatchActionResult {
                    action_id: action.action_id,
                    workspace_id: action.workspace_id,
                    dedupe_key: action.dedupe_key.unwrap_or_default(),
                    status: SupervisorDispatchStatus::Failed,
                    thread_id: None,
                    turn_id: None,
                    error: Some(error),
                    idempotent_replay: false,
                };
            }
        };

        let idempotency_key = normalized.idempotency_key();
        if let Some(cached) = self.idempotency.get(&idempotency_key) {
            let mut replay = cached.clone();
            replay.action_id = normalized.action_id;
            replay.idempotent_replay = true;
            return replay;
        }

        let result = self.dispatch_normalized(backend, &normalized).await;
        self.idempotency.insert(idempotency_key, result.clone());
        result
    }

    async fn dispatch_normalized<B>(
        &self,
        backend: &B,
        action: &NormalizedDispatchAction,
    ) -> SupervisorDispatchActionResult
    where
        B: SupervisorDispatchBackend,
    {
        let thread_id = match self.ensure_thread(backend, action).await {
            Ok(value) => value,
            Err(error) => {
                return failed_dispatch_result(action, error, None, None, false);
            }
        };

        let turn_response = match backend
            .start_turn(&action.workspace_id, &thread_id, &action.prompt)
            .await
        {
            Ok(value) => value,
            Err(error) => {
                return failed_dispatch_result(action, error, Some(thread_id), None, false);
            }
        };

        if let Some(error) = response_error_message(&turn_response) {
            return failed_dispatch_result(action, error, Some(thread_id), None, false);
        }

        SupervisorDispatchActionResult {
            action_id: action.action_id.clone(),
            workspace_id: action.workspace_id.clone(),
            dedupe_key: action.dedupe_token.clone(),
            status: SupervisorDispatchStatus::Dispatched,
            thread_id: Some(thread_id),
            turn_id: extract_turn_id(&turn_response),
            error: None,
            idempotent_replay: false,
        }
    }

    async fn ensure_thread<B>(
        &self,
        backend: &B,
        action: &NormalizedDispatchAction,
    ) -> Result<String, String>
    where
        B: SupervisorDispatchBackend,
    {
        if let Some(thread_id) = action.thread_id.as_deref() {
            let response = backend
                .resume_thread(&action.workspace_id, thread_id)
                .await?;
            if let Some(error) = response_error_message(&response) {
                return Err(error);
            }
            return Ok(extract_thread_id(&response).unwrap_or_else(|| thread_id.to_string()));
        }

        let response = backend.start_thread(&action.workspace_id).await?;
        if let Some(error) = response_error_message(&response) {
            return Err(error);
        }

        extract_thread_id(&response).ok_or_else(|| {
            format!(
                "thread/start response did not include threadId for workspace `{}`",
                action.workspace_id
            )
        })
    }
}

#[derive(Debug, Clone)]
struct NormalizedDispatchAction {
    action_id: String,
    workspace_id: String,
    thread_id: Option<String>,
    prompt: String,
    dedupe_token: String,
}

impl NormalizedDispatchAction {
    fn idempotency_key(&self) -> String {
        format!("{}:{}", self.workspace_id, self.dedupe_token)
    }
}

impl TryFrom<SupervisorDispatchAction> for NormalizedDispatchAction {
    type Error = String;

    fn try_from(value: SupervisorDispatchAction) -> Result<Self, Self::Error> {
        let action_id = value.action_id.trim().to_string();
        if action_id.is_empty() {
            return Err("action_id is required".to_string());
        }

        let workspace_id = value.workspace_id.trim().to_string();
        if workspace_id.is_empty() {
            return Err("workspace_id is required".to_string());
        }

        let prompt = value.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("prompt is required".to_string());
        }

        let thread_id = value.thread_id.and_then(|thread_id| {
            let trimmed = thread_id.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        let dedupe_token = value
            .dedupe_key
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .unwrap_or(action_id.as_str())
            .to_string();

        Ok(Self {
            action_id,
            workspace_id,
            thread_id,
            prompt,
            dedupe_token,
        })
    }
}

fn failed_dispatch_result(
    action: &NormalizedDispatchAction,
    error: String,
    thread_id: Option<String>,
    turn_id: Option<String>,
    idempotent_replay: bool,
) -> SupervisorDispatchActionResult {
    SupervisorDispatchActionResult {
        action_id: action.action_id.clone(),
        workspace_id: action.workspace_id.clone(),
        dedupe_key: action.dedupe_token.clone(),
        status: SupervisorDispatchStatus::Failed,
        thread_id,
        turn_id,
        error: Some(error),
        idempotent_replay,
    }
}

fn response_error_message(response: &Value) -> Option<String> {
    let error = response.get("error")?;

    if let Some(message) = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
    {
        return Some(message.to_string());
    }

    if let Some(message) = error
        .as_str()
        .map(str::trim)
        .filter(|message| !message.is_empty())
    {
        return Some(message.to_string());
    }

    Some(error.to_string())
}

fn extract_thread_id(response: &Value) -> Option<String> {
    response
        .get("result")
        .and_then(|result| {
            result
                .get("threadId")
                .or_else(|| result.get("thread").and_then(|thread| thread.get("id")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            response
                .get("threadId")
                .or_else(|| response.get("thread").and_then(|thread| thread.get("id")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn extract_turn_id(response: &Value) -> Option<String> {
    response
        .get("result")
        .and_then(|result| {
            result
                .get("turnId")
                .or_else(|| result.get("turn").and_then(|turn| turn.get("id")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            response
                .get("turnId")
                .or_else(|| response.get("turn").and_then(|turn| turn.get("id")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::future::Future;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct MockDispatchBackend {
        calls: StdMutex<Vec<String>>,
        resume_failures: StdMutex<HashSet<String>>,
    }

    impl MockDispatchBackend {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock poisoned").clone()
        }

        fn fail_resume_for(&self, workspace_id: &str) {
            self.resume_failures
                .lock()
                .expect("resume failures lock poisoned")
                .insert(workspace_id.to_string());
        }

        fn push_call(&self, call: String) {
            self.calls.lock().expect("calls lock poisoned").push(call);
        }
    }

    impl SupervisorDispatchBackend for MockDispatchBackend {
        fn start_thread<'a>(
            &'a self,
            workspace_id: &'a str,
        ) -> DispatchFuture<'a, Result<Value, String>> {
            Box::pin(async move {
                self.push_call(format!("thread/start:{workspace_id}"));
                Ok(json!({ "result": { "threadId": format!("thread-{workspace_id}") } }))
            })
        }

        fn resume_thread<'a>(
            &'a self,
            workspace_id: &'a str,
            thread_id: &'a str,
        ) -> DispatchFuture<'a, Result<Value, String>> {
            Box::pin(async move {
                self.push_call(format!("thread/resume:{workspace_id}:{thread_id}"));
                if self
                    .resume_failures
                    .lock()
                    .expect("resume failures lock poisoned")
                    .contains(workspace_id)
                {
                    return Ok(json!({ "error": { "message": "resume failed" } }));
                }
                Ok(json!({ "result": { "threadId": thread_id } }))
            })
        }

        fn start_turn<'a>(
            &'a self,
            workspace_id: &'a str,
            thread_id: &'a str,
            _prompt: &'a str,
        ) -> DispatchFuture<'a, Result<Value, String>> {
            Box::pin(async move {
                self.push_call(format!("turn/start:{workspace_id}:{thread_id}"));
                Ok(json!({
                    "result": { "turnId": format!("turn-{workspace_id}-{thread_id}") }
                }))
            })
        }
    }

    fn action(
        action_id: &str,
        workspace_id: &str,
        thread_id: Option<&str>,
        prompt: &str,
        dedupe_key: Option<&str>,
    ) -> SupervisorDispatchAction {
        SupervisorDispatchAction {
            action_id: action_id.to_string(),
            workspace_id: workspace_id.to_string(),
            thread_id: thread_id.map(ToOwned::to_owned),
            prompt: prompt.to_string(),
            dedupe_key: dedupe_key.map(ToOwned::to_owned),
        }
    }

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

    #[test]
    fn dispatches_to_multiple_workspaces_in_one_batch() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![
                        action("action-1", "ws-1", None, "Review workspace one", None),
                        action("action-2", "ws-2", None, "Review workspace two", None),
                    ],
                )
                .await;

            assert_eq!(result.results.len(), 2);
            assert!(result
                .results
                .iter()
                .all(|entry| entry.status == SupervisorDispatchStatus::Dispatched));
            assert_eq!(
                backend.calls(),
                vec![
                    "thread/start:ws-1",
                    "turn/start:ws-1:thread-ws-1",
                    "thread/start:ws-2",
                    "turn/start:ws-2:thread-ws-2",
                ]
            );
        });
    }

    #[test]
    fn uses_thread_resume_when_thread_id_is_provided() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![action(
                        "action-1",
                        "ws-1",
                        Some("thread-existing"),
                        "Continue task",
                        None,
                    )],
                )
                .await;

            assert_eq!(result.results.len(), 1);
            assert_eq!(
                result.results[0].thread_id.as_deref(),
                Some("thread-existing")
            );
            assert_eq!(
                backend.calls(),
                vec![
                    "thread/resume:ws-1:thread-existing",
                    "turn/start:ws-1:thread-existing"
                ]
            );
        });
    }

    #[test]
    fn deduplicates_actions_by_workspace_and_dedupe_key() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![
                        action("action-1", "ws-1", None, "Run check", Some("dispatch-1")),
                        action(
                            "action-2",
                            "ws-1",
                            None,
                            "Run check duplicate",
                            Some("dispatch-1"),
                        ),
                    ],
                )
                .await;

            assert_eq!(result.results.len(), 2);
            assert!(!result.results[0].idempotent_replay);
            assert!(result.results[1].idempotent_replay);
            assert_eq!(
                result.results[1].status,
                SupervisorDispatchStatus::Dispatched
            );
            assert_eq!(result.results[0].thread_id, result.results[1].thread_id);
            assert_eq!(result.results[0].turn_id, result.results[1].turn_id);
            assert_eq!(
                backend.calls(),
                vec!["thread/start:ws-1", "turn/start:ws-1:thread-ws-1",]
            );
        });
    }

    #[test]
    fn same_dedupe_key_is_not_shared_between_workspaces() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![
                        action("action-1", "ws-1", None, "Task one", Some("dispatch")),
                        action("action-2", "ws-2", None, "Task two", Some("dispatch")),
                    ],
                )
                .await;

            assert_eq!(result.results.len(), 2);
            assert!(!result.results[0].idempotent_replay);
            assert!(!result.results[1].idempotent_replay);
            assert_eq!(
                backend.calls(),
                vec![
                    "thread/start:ws-1",
                    "turn/start:ws-1:thread-ws-1",
                    "thread/start:ws-2",
                    "turn/start:ws-2:thread-ws-2",
                ]
            );
        });
    }

    #[test]
    fn returns_failed_result_for_invalid_action_input() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![action("action-1", "ws-1", None, "   ", None)],
                )
                .await;

            assert_eq!(result.results.len(), 1);
            assert_eq!(result.results[0].status, SupervisorDispatchStatus::Failed);
            assert_eq!(
                result.results[0].error.as_deref(),
                Some("prompt is required")
            );
            assert!(backend.calls().is_empty());
        });
    }

    #[test]
    fn propagates_resume_errors() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            backend.fail_resume_for("ws-1");
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![action(
                        "action-1",
                        "ws-1",
                        Some("thread-existing"),
                        "Continue task",
                        None,
                    )],
                )
                .await;

            assert_eq!(result.results.len(), 1);
            assert_eq!(result.results[0].status, SupervisorDispatchStatus::Failed);
            assert_eq!(result.results[0].error.as_deref(), Some("resume failed"));
            assert_eq!(backend.calls(), vec!["thread/resume:ws-1:thread-existing"]);
        });
    }

    #[test]
    fn reuses_action_id_as_default_dedupe_key() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let _ = executor
                .dispatch_batch(
                    &backend,
                    vec![action("action-1", "ws-1", None, "Task", None)],
                )
                .await;

            let snapshot = executor.idempotency_snapshot();
            assert!(snapshot.contains_key("ws-1:action-1"));
        });
    }

    #[test]
    fn can_bootstrap_executor_with_existing_idempotency_store() {
        run_async(async {
            let mut store = SupervisorDispatchIdempotencyStore::default();
            store.insert(
                "ws-1:dispatch".to_string(),
                SupervisorDispatchActionResult {
                    action_id: "action-1".to_string(),
                    workspace_id: "ws-1".to_string(),
                    dedupe_key: "dispatch".to_string(),
                    status: SupervisorDispatchStatus::Dispatched,
                    thread_id: Some("thread-ws-1".to_string()),
                    turn_id: Some("turn-ws-1-thread-ws-1".to_string()),
                    error: None,
                    idempotent_replay: false,
                },
            );
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::with_idempotency_store(store);

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![action("action-2", "ws-1", None, "Task", Some("dispatch"))],
                )
                .await;

            assert_eq!(result.results.len(), 1);
            assert!(result.results[0].idempotent_replay);
            assert!(backend.calls().is_empty());
        });
    }

    #[test]
    fn response_extractors_support_nested_and_flat_shapes() {
        let nested =
            json!({ "result": { "thread": { "id": "thread-1" }, "turn": { "id": "turn-1" } } });
        let flat = json!({ "threadId": "thread-2", "turnId": "turn-2" });

        assert_eq!(extract_thread_id(&nested).as_deref(), Some("thread-1"));
        assert_eq!(extract_turn_id(&nested).as_deref(), Some("turn-1"));
        assert_eq!(extract_thread_id(&flat).as_deref(), Some("thread-2"));
        assert_eq!(extract_turn_id(&flat).as_deref(), Some("turn-2"));
    }

    #[test]
    fn idempotency_store_snapshot_returns_copy() {
        let mut store = SupervisorDispatchIdempotencyStore::default();
        store.insert(
            "ws-1:dispatch".to_string(),
            SupervisorDispatchActionResult {
                action_id: "action-1".to_string(),
                workspace_id: "ws-1".to_string(),
                dedupe_key: "dispatch".to_string(),
                status: SupervisorDispatchStatus::Dispatched,
                thread_id: Some("thread-ws-1".to_string()),
                turn_id: Some("turn-ws-1-thread-ws-1".to_string()),
                error: None,
                idempotent_replay: false,
            },
        );

        let mut snapshot = store.snapshot();
        snapshot.insert(
            "ws-2:dispatch".to_string(),
            SupervisorDispatchActionResult {
                action_id: "action-2".to_string(),
                workspace_id: "ws-2".to_string(),
                dedupe_key: "dispatch".to_string(),
                status: SupervisorDispatchStatus::Dispatched,
                thread_id: None,
                turn_id: None,
                error: None,
                idempotent_replay: false,
            },
        );

        let original_snapshot = store.snapshot();
        assert_eq!(original_snapshot.len(), 1);
        assert!(original_snapshot.contains_key("ws-1:dispatch"));
        assert!(!original_snapshot.contains_key("ws-2:dispatch"));
    }

    #[test]
    fn normalization_strips_whitespace_and_optional_thread() {
        let normalized = NormalizedDispatchAction::try_from(SupervisorDispatchAction {
            action_id: " action-1 ".to_string(),
            workspace_id: " ws-1 ".to_string(),
            thread_id: Some("   ".to_string()),
            prompt: " do it ".to_string(),
            dedupe_key: Some(" dispatch ".to_string()),
        })
        .expect("normalized action");

        assert_eq!(normalized.action_id, "action-1");
        assert_eq!(normalized.workspace_id, "ws-1");
        assert_eq!(normalized.thread_id, None);
        assert_eq!(normalized.prompt, "do it");
        assert_eq!(normalized.dedupe_token, "dispatch");
    }

    #[test]
    fn normalization_rejects_missing_action_id() {
        let error = NormalizedDispatchAction::try_from(SupervisorDispatchAction {
            action_id: "   ".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: None,
            prompt: "Run".to_string(),
            dedupe_key: None,
        })
        .expect_err("missing action id should fail");

        assert_eq!(error, "action_id is required");
    }

    #[test]
    fn normalization_rejects_missing_workspace_id() {
        let error = NormalizedDispatchAction::try_from(SupervisorDispatchAction {
            action_id: "action-1".to_string(),
            workspace_id: "\n".to_string(),
            thread_id: None,
            prompt: "Run".to_string(),
            dedupe_key: None,
        })
        .expect_err("missing workspace id should fail");

        assert_eq!(error, "workspace_id is required");
    }

    #[test]
    fn normalization_rejects_missing_prompt() {
        let error = NormalizedDispatchAction::try_from(SupervisorDispatchAction {
            action_id: "action-1".to_string(),
            workspace_id: "ws-1".to_string(),
            thread_id: None,
            prompt: "\t".to_string(),
            dedupe_key: None,
        })
        .expect_err("missing prompt should fail");

        assert_eq!(error, "prompt is required");
    }

    #[test]
    fn response_error_message_falls_back_to_stringified_error() {
        let response = json!({ "error": { "code": -32000 } });
        assert_eq!(
            response_error_message(&response),
            Some("{\"code\":-32000}".to_string())
        );
    }

    #[test]
    fn response_error_message_uses_string_error_shape() {
        let response = json!({ "error": "plain string" });
        assert_eq!(
            response_error_message(&response),
            Some("plain string".to_string())
        );
    }

    #[test]
    fn response_error_message_returns_none_for_success() {
        let response = json!({ "result": { "ok": true } });
        assert_eq!(response_error_message(&response), None);
    }

    #[test]
    fn dedupe_uses_action_id_when_no_dedupe_key_is_present() {
        run_async(async {
            let backend = MockDispatchBackend::default();
            let mut executor = SupervisorDispatchExecutor::new();

            let result = executor
                .dispatch_batch(
                    &backend,
                    vec![
                        action("same", "ws-1", None, "Task one", None),
                        action("same", "ws-1", None, "Task duplicate", None),
                    ],
                )
                .await;

            assert_eq!(result.results.len(), 2);
            assert!(result.results[1].idempotent_replay);
            assert_eq!(
                backend.calls(),
                vec!["thread/start:ws-1", "turn/start:ws-1:thread-ws-1",]
            );
        });
    }

    #[test]
    fn idempotency_key_is_workspace_scoped() {
        let a = NormalizedDispatchAction::try_from(action(
            "action-1",
            "ws-1",
            None,
            "Task",
            Some("dup"),
        ))
        .expect("normalize a");
        let b = NormalizedDispatchAction::try_from(action(
            "action-1",
            "ws-2",
            None,
            "Task",
            Some("dup"),
        ))
        .expect("normalize b");

        let mut keys = HashMap::new();
        keys.insert(a.idempotency_key(), 1u8);
        keys.insert(b.idempotency_key(), 2u8);
        assert_eq!(keys.len(), 2);
    }
}
