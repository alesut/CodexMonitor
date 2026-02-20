import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import type { DictationSessionState, DictationTranscript } from "@/types";
import { formatRelativeTime } from "../../../utils/time";
import { useSupervisorOperations } from "../hooks/useSupervisorOperations";
import { SupervisorChat } from "./SupervisorChat";

type SupervisorHomeProps = {
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

export function SupervisorHome({
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
    activityNeedsInputCount,
    needsInputOnly,
    setNeedsInputOnly,
    isLoading,
    isRefreshing,
    loadError,
    ackError,
    ackingSignalId,
    refresh,
    acknowledgeSignal,
  } = useSupervisorOperations();

  return (
    <div className="supervisor-home">
      <div className="supervisor-home-header">
        <div>
          <h1 className="supervisor-home-title">Supervisor</h1>
          <p className="supervisor-home-subtitle">
            Live operations state across workspaces, threads, and dispatch jobs.
          </p>
        </div>
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

      <div className="supervisor-home-kpis">
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Workspaces</span>
          <strong>{workspaceList.length}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Threads</span>
          <strong>{threadList.length}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Dispatch jobs</span>
          <strong>{jobList.length}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Pending signals</span>
          <strong>{pendingSignals.length}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Needs input</span>
          <strong>{activityNeedsInputCount}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Approvals</span>
          <strong>{pendingApprovalsCount}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Open questions</span>
          <strong>{openQuestionsCount}</strong>
        </div>
      </div>

      {loadError ? <div className="supervisor-home-error">{loadError}</div> : null}
      {ackError ? <div className="supervisor-home-error">{ackError}</div> : null}
      {isLoading && workspaceList.length === 0 ? (
        <div className="supervisor-home-empty">Loading supervisor snapshot...</div>
      ) : null}

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
            <h2>Signals</h2>
            <span>{pendingSignals.length} pending</span>
          </div>
          {signalList.length === 0 ? (
            <p className="supervisor-home-empty">No supervisor signals.</p>
          ) : (
            <ul className="supervisor-signal-list">
              {signalList.map((signal) => {
                const isPending = signal.acknowledged_at_ms === null;
                return (
                  <li key={signal.id} className="supervisor-signal-item">
                    <div className="supervisor-signal-main">
                      <span className={`supervisor-signal-kind is-${signal.kind}`}>
                        {signal.kind}
                      </span>
                      <span className="supervisor-signal-message">{signal.message}</span>
                    </div>
                    <div className="supervisor-signal-meta">
                      <span>{formatSupervisorTime(signal.created_at_ms)}</span>
                      {isPending ? (
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
                      ) : (
                        <span className="supervisor-signal-acked">Acknowledged</span>
                      )}
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
