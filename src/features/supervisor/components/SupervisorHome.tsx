import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { subscribeSupervisorEvents } from "@services/events";
import {
  ackSupervisorSignal,
  getSupervisorSnapshot,
  type SupervisorSignal,
  type SupervisorSnapshot,
} from "@services/tauri";
import { formatRelativeTime } from "../../../utils/time";

function formatSupervisorTime(value: number | null) {
  if (value === null) {
    return "No activity";
  }
  return formatRelativeTime(value);
}

export function SupervisorHome() {
  const [snapshot, setSnapshot] = useState<SupervisorSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [ackError, setAckError] = useState<string | null>(null);
  const [ackingSignalId, setAckingSignalId] = useState<string | null>(null);
  const isMountedRef = useRef(true);

  const loadSnapshot = useCallback(async (showRefreshing: boolean) => {
    if (!showRefreshing) {
      setIsLoading(true);
    } else {
      setIsRefreshing(true);
    }
    setLoadError(null);
    try {
      const next = await getSupervisorSnapshot();
      if (!isMountedRef.current) {
        return;
      }
      setSnapshot(next);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      setLoadError(error instanceof Error ? error.message : "Failed to load supervisor snapshot.");
    } finally {
      if (!isMountedRef.current) {
        return;
      }
      setIsLoading(false);
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    isMountedRef.current = true;
    void loadSnapshot(false);

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeSupervisorEvents(() => {
      if (refreshTimer !== null) {
        return;
      }
      refreshTimer = setTimeout(() => {
        refreshTimer = null;
        void loadSnapshot(true);
      }, 250);
    });

    return () => {
      isMountedRef.current = false;
      unsubscribe();
      if (refreshTimer !== null) {
        clearTimeout(refreshTimer);
      }
    };
  }, [loadSnapshot]);

  const workspaceList = useMemo(
    () => Object.values(snapshot?.workspaces ?? {}),
    [snapshot],
  );
  const threadList = useMemo(() => Object.values(snapshot?.threads ?? {}), [snapshot]);
  const jobList = useMemo(() => Object.values(snapshot?.jobs ?? {}), [snapshot]);
  const signalList = snapshot?.signals ?? [];
  const pendingSignals = signalList.filter((signal) => signal.acknowledged_at_ms === null);
  const activityFeed = snapshot?.activity_feed ?? [];
  const openQuestionsCount = Object.keys(snapshot?.open_questions ?? {}).length;
  const pendingApprovalsCount = Object.keys(snapshot?.pending_approvals ?? {}).length;

  const handleAckSignal = useCallback(
    async (signal: SupervisorSignal) => {
      setAckError(null);
      setAckingSignalId(signal.id);
      try {
        await ackSupervisorSignal(signal.id);
        await loadSnapshot(true);
      } catch (error) {
        setAckError(error instanceof Error ? error.message : "Failed to acknowledge signal.");
      } finally {
        setAckingSignalId(null);
      }
    },
    [loadSnapshot],
  );

  return (
    <div className="supervisor-home">
      <div className="supervisor-home-header">
        <div>
          <h1 className="supervisor-home-title">Supervisor</h1>
          <p className="supervisor-home-subtitle">
            Global operations view across all workspaces and threads.
          </p>
        </div>
        <button
          type="button"
          className="supervisor-home-refresh"
          onClick={() => {
            void loadSnapshot(true);
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
          <span className="supervisor-kpi-label">Jobs</span>
          <strong>{jobList.length}</strong>
        </div>
        <div className="supervisor-kpi">
          <span className="supervisor-kpi-label">Needs input</span>
          <strong>{activityFeed.filter((entry) => entry.needs_input).length}</strong>
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
      {isLoading && !snapshot ? (
        <div className="supervisor-home-empty">Loading supervisor snapshot...</div>
      ) : null}

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
                            void handleAckSignal(signal);
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
