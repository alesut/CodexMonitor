import { useCallback, useMemo, useState } from "react";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import type {
  ApprovalRequest,
  DictationSessionState,
  DictationTranscript,
} from "@/types";
import { getApprovalCommandInfo } from "@/utils/approvalRules";
import { formatRelativeTime } from "../../../utils/time";
import { useSupervisorOperations } from "../hooks/useSupervisorOperations";
import { SupervisorChat } from "./SupervisorChat";

type SupervisorHomeProps = {
  approvals: ApprovalRequest[];
  onApprovalDecision: (
    request: ApprovalRequest,
    decision: "accept" | "decline",
  ) => void | Promise<void>;
  dictationEnabled: boolean;
  dictationState: DictationSessionState;
  dictationLevel: number;
  onToggleDictation: () => void;
  onOpenDictationSettings: () => void;
  dictationError: string | null;
  onDismissDictationError: () => void;
  dictationHint: string | null;
  onDismissDictationHint: () => void;
  dictationTranscript: DictationTranscript | null;
  onDictationTranscriptHandled: (id: string) => void;
};

function formatSupervisorTime(value: number | null) {
  if (value === null) {
    return "No activity";
  }
  return formatRelativeTime(value);
}

function normalizeJobStatusLabel(status: string) {
  const normalized = status === "pending" ? "queued" : status;
  return normalized.replace(/_/g, " ");
}

function normalizeJobStatusClass(status: string) {
  return status === "pending" ? "queued" : status;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function formatApprovalMethod(value: string) {
  const trimmed = value.replace(/^codex\/requestApproval\/?/, "");
  return trimmed || value;
}

type SignalSeverity = "critical" | "attention" | "info";

function signalSeverity(kind: string): SignalSeverity {
  switch (kind) {
    case "failed":
    case "disconnected":
      return "critical";
    case "needs_approval":
    case "stalled":
      return "attention";
    default:
      return "info";
  }
}

function formatSignalKindLabel(kind: string) {
  switch (kind) {
    case "needs_approval":
      return "Needs approval";
    case "failed":
      return "Failure";
    case "stalled":
      return "Stalled";
    case "disconnected":
      return "Disconnected";
    case "completed":
      return "Completed";
    default:
      return kind.replace(/_/g, " ");
  }
}

function formatSignalStatusLabel(kind: string) {
  switch (kind) {
    case "needs_approval":
      return "Awaiting approval decision";
    case "failed":
      return "Needs follow-up";
    case "stalled":
      return "Needs attention";
    case "disconnected":
      return "Workspace disconnected";
    case "completed":
      return "Awaiting acknowledgment";
    default:
      return "Pending review";
  }
}

function formatSeverityLabel(severity: SignalSeverity) {
  switch (severity) {
    case "critical":
      return "Critical";
    case "attention":
      return "Attention";
    default:
      return "Info";
  }
}

function formatSummaryValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "none";
  }
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!trimmed) {
      return "empty";
    }
    return trimmed.length > 72 ? `${trimmed.slice(0, 69)}...` : trimmed;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  if (Array.isArray(value)) {
    const rendered = value
      .slice(0, 3)
      .map((entry) => formatSummaryValue(entry))
      .join(", ");
    return value.length > 3 ? `${rendered}, ...` : rendered;
  }
  return "structured payload";
}

function formatApprovalParamsSummary(params: Record<string, unknown>): string | null {
  const ignoredKeys = new Set([
    "threadId",
    "thread_id",
    "turnId",
    "turn_id",
    "itemId",
    "item_id",
    "requestId",
    "request_id",
  ]);

  const summary = Object.entries(params)
    .filter(([key]) => !ignoredKeys.has(key))
    .slice(0, 3)
    .map(([key, value]) => `${key}: ${formatSummaryValue(value)}`)
    .join(" · ");

  return summary || null;
}

function extractSignalRequestKey(signalId: string, context: unknown): string | null {
  if (signalId.startsWith("approval:")) {
    const requestKey = signalId.slice("approval:".length).trim();
    if (requestKey) {
      return requestKey;
    }
  }

  const contextRecord = asRecord(context);
  const contextRequestKey = contextRecord?.requestKey;
  if (typeof contextRequestKey === "string") {
    const trimmed = contextRequestKey.trim();
    if (trimmed) {
      return trimmed;
    }
  }

  return null;
}

export function SupervisorHome({
  approvals,
  onApprovalDecision,
  dictationEnabled,
  dictationState,
  dictationLevel,
  onToggleDictation,
  onOpenDictationSettings,
  dictationError,
  onDismissDictationError,
  dictationHint,
  onDismissDictationHint,
  dictationTranscript,
  onDictationTranscriptHandled,
}: SupervisorHomeProps) {
  const {
    workspaceList,
    threadList,
    jobList,
    signalList,
    pendingSignals,
    feedItems,
    feedTotal,
    openQuestionsCount,
    pendingApprovalsCount,
    pendingApprovals,
    activityNeedsInputCount,
    needsInputOnly,
    setNeedsInputOnly,
    isLoading,
    isRefreshing,
    lastRefreshedAtMs,
    loadError,
    ackError,
    ackingSignalId,
    refresh,
    acknowledgeSignal,
  } = useSupervisorOperations();
  const [decidingApprovalKey, setDecidingApprovalKey] = useState<string | null>(null);

  const workspacesById = useMemo(
    () => new Map(workspaceList.map((workspace) => [workspace.id, workspace.name])),
    [workspaceList],
  );
  const approvalRequestsByKey = useMemo(() => {
    const map = new Map<string, ApprovalRequest>();
    for (const approval of approvals) {
      map.set(`${approval.workspace_id}:${String(approval.request_id)}`, approval);
    }
    return map;
  }, [approvals]);
  const pendingApprovalsByKey = useMemo(() => {
    const map = new Map<string, (typeof pendingApprovals)[number]>();
    for (const approval of pendingApprovals) {
      map.set(approval.request_key, approval);
    }
    return map;
  }, [pendingApprovals]);
  const handleSignalApprovalDecision = useCallback(
    async (request: ApprovalRequest, decision: "accept" | "decline") => {
      const approvalKey = `${request.workspace_id}:${String(request.request_id)}`;
      setDecidingApprovalKey(approvalKey);
      try {
        await Promise.resolve(onApprovalDecision(request, decision));
      } finally {
        setDecidingApprovalKey((current) => (current === approvalKey ? null : current));
      }
    },
    [onApprovalDecision],
  );
  const formatSignalLocation = useCallback(
    (workspaceId: string | null, threadId: string | null) => {
      const workspaceLabel = workspaceId
        ? workspacesById.get(workspaceId) ?? workspaceId
        : "global";
      return threadId ? `${workspaceLabel} · ${threadId}` : workspaceLabel;
    },
    [workspacesById],
  );
  const controlCounters = useMemo(() => {
    type ControlCounter = {
      key: string;
      label: string;
      value: number;
      isAttention?: boolean;
    };
    const primary: ControlCounter[] = [
      { key: "workspaces", label: "Workspaces", value: workspaceList.length },
      { key: "threads", label: "Threads", value: threadList.length },
      { key: "jobs", label: "Jobs", value: jobList.length },
    ];
    const secondary: ControlCounter[] = [
      {
        key: "signals",
        label: "Pending signals",
        value: pendingSignals.length,
        isAttention: true,
      },
      {
        key: "needs-input",
        label: "Needs input",
        value: activityNeedsInputCount,
        isAttention: true,
      },
      {
        key: "approvals",
        label: "Approvals",
        value: pendingApprovalsCount,
        isAttention: true,
      },
      {
        key: "questions",
        label: "Open questions",
        value: openQuestionsCount,
        isAttention: true,
      },
    ].filter((counter) => counter.value > 0);
    return [...primary, ...secondary];
  }, [
    activityNeedsInputCount,
    jobList.length,
    openQuestionsCount,
    pendingApprovalsCount,
    pendingSignals.length,
    threadList.length,
    workspaceList.length,
  ]);
  const lastUpdatedLabel =
    lastRefreshedAtMs === null ? "syncing..." : formatSupervisorTime(lastRefreshedAtMs);
  const actionCenterChips = useMemo(() => {
    const criticalCount = pendingSignals.filter(
      (signal) => signalSeverity(signal.kind) === "critical",
    ).length;
    return [
      {
        key: "critical",
        label: "Critical",
        value: criticalCount,
        severity: "critical" as const,
      },
      {
        key: "needs-input",
        label: "Needs input",
        value: openQuestionsCount,
        severity: "attention" as const,
      },
      {
        key: "approvals",
        label: "Approvals",
        value: pendingApprovalsCount,
        severity: "attention" as const,
      },
      {
        key: "pending-signals",
        label: "Pending signals",
        value: pendingSignals.length,
        severity: "info" as const,
      },
    ].filter((chip) => chip.value > 0);
  }, [openQuestionsCount, pendingApprovalsCount, pendingSignals]);

  return (
    <div className="supervisor-home">
      <div className="supervisor-home-header">
        <div>
          <h1 className="supervisor-home-title">Supervisor</h1>
          <p className="supervisor-home-subtitle">
            Live operations state across workspaces, threads, and dispatch jobs.
          </p>
        </div>
        <div className="supervisor-home-control-bar">
          <div className="supervisor-home-control-meta">
            <span className="supervisor-home-last-updated">Last updated {lastUpdatedLabel}</span>
            <button
              type="button"
              className="supervisor-home-refresh"
              onClick={() => {
                void refresh("manual");
              }}
              disabled={isLoading || isRefreshing}
              aria-label="Refresh supervisor snapshot"
            >
              <RefreshCw className={isRefreshing ? "spinning" : ""} size={14} aria-hidden />
              Refresh
            </button>
          </div>
          <div className="supervisor-home-control-counters">
            {controlCounters.map((counter) => (
              <span
                key={counter.key}
                className={`supervisor-home-counter-pill${counter.isAttention ? " is-attention" : ""}`}
              >
                <strong>{counter.value}</strong> {counter.label}
              </span>
            ))}
          </div>
        </div>
      </div>

      {loadError ? <div className="supervisor-home-error">{loadError}</div> : null}
      {ackError ? <div className="supervisor-home-error">{ackError}</div> : null}
      {isLoading && workspaceList.length === 0 ? (
        <div className="supervisor-home-empty">Loading supervisor snapshot...</div>
      ) : null}

      <section className="supervisor-section supervisor-section-priority">
        <div className="supervisor-section-header">
          <h2>Action center</h2>
          <span>{pendingSignals.length} pending</span>
        </div>
        {actionCenterChips.length > 0 ? (
          <div className="supervisor-action-chips">
            {actionCenterChips.map((chip) => (
              <span
                key={chip.key}
                className={`supervisor-action-chip is-${chip.severity}`}
              >
                <strong>{chip.value}</strong> {chip.label}
              </span>
            ))}
          </div>
        ) : null}
        {pendingSignals.length === 0 ? (
          <p className="supervisor-home-empty">No actions waiting for your input.</p>
        ) : (
          <ul className="supervisor-signal-list supervisor-signal-list-priority">
            {pendingSignals.map((signal) => {
              const severity = signalSeverity(signal.kind);
              const requestKey =
                signal.kind === "needs_approval"
                  ? extractSignalRequestKey(signal.id, signal.context)
                  : null;
              const request = requestKey ? approvalRequestsByKey.get(requestKey) ?? null : null;
              const pendingApproval = requestKey
                ? pendingApprovalsByKey.get(requestKey) ?? null
                : null;
              const params = request?.params ?? asRecord(pendingApproval?.params) ?? {};
              const commandInfo = getApprovalCommandInfo(params);
              const approvalMethod = request?.method ?? pendingApproval?.method ?? null;
              const approvalSummary = formatApprovalParamsSummary(params);
              const locationWorkspaceId = signal.workspace_id ?? pendingApproval?.workspace_id ?? null;
              const locationThreadId = signal.thread_id ?? pendingApproval?.thread_id ?? null;
              const approvalKey = request
                ? `${request.workspace_id}:${String(request.request_id)}`
                : null;
              const isApprovalActionBusy =
                approvalKey !== null && decidingApprovalKey === approvalKey;
              const kindLabel = formatSignalKindLabel(signal.kind);
              const statusLabel = formatSignalStatusLabel(signal.kind);

              return (
                <li key={signal.id} className="supervisor-signal-item">
                  <div className="supervisor-signal-main">
                    <span className={`supervisor-signal-severity is-${severity}`}>
                      {formatSeverityLabel(severity)}
                    </span>
                    <span className={`supervisor-signal-kind is-${severity}`}>
                      {kindLabel}
                    </span>
                    <span className="supervisor-signal-message">{signal.message}</span>
                  </div>
                  <div className="supervisor-signal-status-text">{statusLabel}</div>

                  {signal.kind === "needs_approval" ? (
                    <div className="supervisor-signal-approval">
                      {approvalMethod ? (
                        <div className="supervisor-signal-approval-method">
                          Method: {formatApprovalMethod(approvalMethod)}
                        </div>
                      ) : null}
                      {commandInfo ? (
                        <code className="supervisor-signal-command">{commandInfo.preview}</code>
                      ) : approvalSummary ? (
                        <div className="supervisor-signal-approval-summary">{approvalSummary}</div>
                      ) : (
                        <div className="supervisor-signal-approval-summary">
                          No additional approval details.
                        </div>
                      )}
                    </div>
                  ) : null}

                  <div className="supervisor-signal-meta">
                    <span>
                      {formatSignalLocation(locationWorkspaceId, locationThreadId)} ·{" "}
                      {formatSupervisorTime(signal.created_at_ms)}
                    </span>

                    {signal.kind === "needs_approval" && request ? (
                      <div className="supervisor-signal-actions">
                        <button
                          type="button"
                          className="secondary"
                          onClick={() => {
                            void handleSignalApprovalDecision(request, "decline");
                          }}
                          disabled={isApprovalActionBusy}
                        >
                          Decline
                        </button>
                        <button
                          type="button"
                          className="primary"
                          onClick={() => {
                            void handleSignalApprovalDecision(request, "accept");
                          }}
                          disabled={isApprovalActionBusy}
                        >
                          Approve
                        </button>
                      </div>
                    ) : (
                      <button
                        type="button"
                        className="supervisor-signal-ack"
                        onClick={() => {
                          void acknowledgeSignal(signal);
                        }}
                        disabled={ackingSignalId === signal.id}
                      >
                        Acknowledge
                      </button>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <SupervisorChat
        dictationEnabled={dictationEnabled}
        dictationState={dictationState}
        dictationLevel={dictationLevel}
        onToggleDictation={onToggleDictation}
        onOpenDictationSettings={onOpenDictationSettings}
        dictationError={dictationError}
        onDismissDictationError={onDismissDictationError}
        dictationHint={dictationHint}
        onDismissDictationHint={onDismissDictationHint}
        dictationTranscript={dictationTranscript}
        onDictationTranscriptHandled={onDictationTranscriptHandled}
      />

      <div className="supervisor-grid">
        <section className="supervisor-section">
          <div className="supervisor-section-header">
            <h2>Workspace status</h2>
          </div>
          {workspaceList.length === 0 ? (
            <p className="supervisor-home-empty">No workspace status yet.</p>
          ) : (
            <div className="supervisor-workspace-list">
              {workspaceList.map((workspace) => (
                <article className="supervisor-workspace-card" key={workspace.id}>
                  <header className="supervisor-workspace-header">
                    <div className="supervisor-workspace-name">{workspace.name}</div>
                    <span className={`supervisor-health is-${workspace.health}`}>
                      {workspace.health}
                    </span>
                  </header>
                  <dl className="supervisor-workspace-meta">
                    <div>
                      <dt>Current task</dt>
                      <dd>{workspace.current_task || "None"}</dd>
                    </div>
                    <div>
                      <dt>Next step</dt>
                      <dd>{workspace.next_expected_step || "Pending update"}</dd>
                    </div>
                    <div>
                      <dt>Last activity</dt>
                      <dd>{formatSupervisorTime(workspace.last_activity_at_ms)}</dd>
                    </div>
                  </dl>
                  <div className="supervisor-blockers">
                    {workspace.blockers.length > 0
                      ? `Blockers: ${workspace.blockers.join(", ")}`
                      : "Blockers: none"}
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>

        <section className="supervisor-section">
          <div className="supervisor-section-header">
            <h2>Thread activity</h2>
            <span>{threadList.length} total</span>
          </div>
          {threadList.length === 0 ? (
            <p className="supervisor-home-empty">No active thread telemetry yet.</p>
          ) : (
            <ul className="supervisor-thread-list">
              {threadList.map((thread) => (
                <li key={thread.id} className="supervisor-thread-item">
                  <div className="supervisor-thread-top">
                    <span className="supervisor-thread-name">
                      {thread.name?.trim() || `Thread ${thread.id}`}
                    </span>
                    <span className={`supervisor-thread-status is-${thread.status}`}>
                      {thread.status}
                    </span>
                  </div>
                  <div className="supervisor-thread-line">
                    <strong>Task:</strong> {thread.current_task || "None"}
                  </div>
                  <div className="supervisor-thread-line">
                    <strong>Next:</strong> {thread.next_expected_step || "Pending update"}
                  </div>
                  <div className="supervisor-thread-line">
                    <strong>Last:</strong> {formatSupervisorTime(thread.last_activity_at_ms)}
                  </div>
                  <div className="supervisor-thread-line">
                    <strong>Blockers:</strong>{" "}
                    {thread.blockers.length > 0 ? thread.blockers.join(", ") : "none"}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="supervisor-section">
          <div className="supervisor-section-header">
            <h2>Dispatch jobs</h2>
            <span>{jobList.length} tracked</span>
          </div>
          {jobList.length === 0 ? (
            <p className="supervisor-home-empty">No jobs dispatched yet.</p>
          ) : (
            <ul className="supervisor-job-list">
              {jobList.map((job) => {
                const normalizedStatusClass = normalizeJobStatusClass(job.status);
                const normalizedStatusLabel = normalizeJobStatusLabel(job.status);
                const recentEvents = [...(job.recent_events ?? [])]
                  .slice(-4)
                  .reverse();
                return (
                  <li key={job.id} className="supervisor-job-item">
                    <div className="supervisor-job-top">
                      <span className="supervisor-job-name">{job.description}</span>
                      <span className={`supervisor-job-status is-${normalizedStatusClass}`}>
                        {normalizedStatusLabel}
                      </span>
                    </div>
                    <div className="supervisor-job-meta">
                      Workspace: {job.workspace_id} ·{" "}
                      {job.thread_id ? `Thread ${job.thread_id}` : "Thread pending"}
                    </div>
                    <div className="supervisor-job-meta">
                      Route: {job.route_kind ?? "workspace_delegate"} · Target:{" "}
                      {job.route_target ?? job.workspace_id}
                      {job.model ? ` · Model ${job.model}` : ""}
                      {job.effort ? ` · Effort ${job.effort}` : ""}
                      {job.access_mode ? ` · Access ${job.access_mode}` : ""}
                    </div>
                    {job.route_reason ? (
                      <div className="supervisor-job-meta">Reason: {job.route_reason}</div>
                    ) : null}
                    {job.route_fallback ? (
                      <div className="supervisor-job-meta">Fallback: {job.route_fallback}</div>
                    ) : null}
                    {recentEvents.length > 0 ? (
                      <ul className="supervisor-job-events">
                        {recentEvents.map((event) => (
                          <li key={event.id} className="supervisor-job-event">
                            <span className="supervisor-job-event-kind">{event.kind}</span>
                            <span className="supervisor-job-event-message">{event.message}</span>
                            <span className="supervisor-job-event-time">
                              {formatSupervisorTime(event.created_at_ms)}
                            </span>
                          </li>
                        ))}
                      </ul>
                    ) : null}
                    {job.error ? (
                      <div className="supervisor-job-error">Error: {job.error}</div>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        <section className="supervisor-section">
          <div className="supervisor-section-header">
            <h2>Live activity feed</h2>
            <span>{feedTotal} entries</span>
          </div>
          <div className="supervisor-feed-filter" role="group" aria-label="Feed filter">
            <button
              type="button"
              className={`supervisor-feed-filter-button${!needsInputOnly ? " is-active" : ""}`}
              onClick={() => setNeedsInputOnly(false)}
            >
              All activity
            </button>
            <button
              type="button"
              className={`supervisor-feed-filter-button${needsInputOnly ? " is-active" : ""}`}
              onClick={() => setNeedsInputOnly(true)}
            >
              Needs my input
            </button>
          </div>
          {feedItems.length === 0 ? (
            <p className="supervisor-home-empty">No feed items for this filter.</p>
          ) : (
            <ul className="supervisor-feed-list">
              {feedItems.map((entry) => (
                <li key={entry.id} className="supervisor-feed-item">
                  <div className="supervisor-feed-main">
                    <span className="supervisor-feed-message">{entry.message}</span>
                    <span className="supervisor-feed-time">
                      {formatSupervisorTime(entry.created_at_ms)}
                    </span>
                  </div>
                  <div className="supervisor-feed-meta">
                    <span className="supervisor-feed-kind">{entry.kind}</span>
                    <span>
                      {entry.workspace_id ?? "global"}
                      {entry.thread_id ? ` · ${entry.thread_id}` : ""}
                    </span>
                    {entry.needs_input ? (
                      <span className="supervisor-feed-needs-input">Needs input</span>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="supervisor-section">
          <div className="supervisor-section-header">
            <h2>Recent signals</h2>
            <span>{signalList.length} total</span>
          </div>
          {signalList.length === 0 ? (
            <p className="supervisor-home-empty">No supervisor signals.</p>
          ) : (
            <ul className="supervisor-signal-list">
              {signalList.map((signal) => {
                const isPending = signal.acknowledged_at_ms === null;
                const severity = signalSeverity(signal.kind);
                return (
                  <li key={signal.id} className="supervisor-signal-item">
                    <div className="supervisor-signal-main">
                      <span className={`supervisor-signal-severity is-${severity}`}>
                        {formatSeverityLabel(severity)}
                      </span>
                      <span className={`supervisor-signal-kind is-${severity}`}>
                        {formatSignalKindLabel(signal.kind)}
                      </span>
                      <span className="supervisor-signal-message">{signal.message}</span>
                    </div>
                    <div className="supervisor-signal-meta">
                      <span>
                        {formatSignalLocation(signal.workspace_id, signal.thread_id)} ·{" "}
                        {formatSupervisorTime(signal.created_at_ms)}
                      </span>
                      <span
                        className={
                          isPending
                            ? "supervisor-signal-pending"
                            : "supervisor-signal-acked"
                        }
                      >
                        {isPending ? "Pending" : "Acknowledged"}
                      </span>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
