import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { subscribeSupervisorEvents } from "@services/events";
import {
  ackSupervisorSignal,
  getSupervisorFeed,
  getSupervisorSnapshot,
  type SupervisorActivityEntry,
  type SupervisorSignal,
  type SupervisorSnapshot,
} from "@services/tauri";

type RefreshMode = "initial" | "manual" | "background";

function normalizeError(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function useSupervisorOperations() {
  const [snapshot, setSnapshot] = useState<SupervisorSnapshot | null>(null);
  const [feedItems, setFeedItems] = useState<SupervisorActivityEntry[]>([]);
  const [feedTotal, setFeedTotal] = useState(0);
  const [needsInputOnly, setNeedsInputOnly] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastRefreshedAtMs, setLastRefreshedAtMs] = useState<number | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [ackError, setAckError] = useState<string | null>(null);
  const [ackingSignalId, setAckingSignalId] = useState<string | null>(null);
  const isMountedRef = useRef(true);
  const hasRequestedInitialRef = useRef(false);

  const refresh = useCallback(
    async (mode: RefreshMode) => {
      if (mode === "initial") {
        setIsLoading(true);
      }
      if (mode === "manual") {
        setIsRefreshing(true);
      }
      if (mode !== "background") {
        setLoadError(null);
      }
      try {
        const [nextSnapshot, nextFeed] = await Promise.all([
          getSupervisorSnapshot(),
          getSupervisorFeed({
            limit: 80,
            needsInputOnly,
          }),
        ]);
        if (!isMountedRef.current) {
          return;
        }
        setSnapshot(nextSnapshot);
        setFeedItems(nextFeed.items);
        setFeedTotal(nextFeed.total);
        setLastRefreshedAtMs(Date.now());
      } catch (error) {
        if (!isMountedRef.current) {
          return;
        }
        setLoadError(normalizeError(error, "Failed to load supervisor operations state."));
      } finally {
        if (!isMountedRef.current) {
          return;
        }
        setIsLoading(false);
        setIsRefreshing(false);
      }
    },
    [needsInputOnly],
  );

  useEffect(() => {
    isMountedRef.current = true;
    const mode: RefreshMode = hasRequestedInitialRef.current
      ? "background"
      : "initial";
    hasRequestedInitialRef.current = true;
    void refresh(mode);
  }, [refresh]);

  useEffect(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeSupervisorEvents(() => {
      if (refreshTimer !== null) {
        return;
      }
      refreshTimer = setTimeout(() => {
        refreshTimer = null;
        void refresh("background");
      }, 250);
    });
    const intervalId = setInterval(() => {
      void refresh("background");
    }, 15000);

    return () => {
      isMountedRef.current = false;
      unsubscribe();
      if (refreshTimer !== null) {
        clearTimeout(refreshTimer);
      }
      clearInterval(intervalId);
    };
  }, [refresh]);

  const acknowledgeSignal = useCallback(
    async (signal: SupervisorSignal) => {
      setAckError(null);
      setAckingSignalId(signal.id);
      try {
        await ackSupervisorSignal(signal.id);
        await refresh("manual");
      } catch (error) {
        if (!isMountedRef.current) {
          return;
        }
        setAckError(normalizeError(error, "Failed to acknowledge signal."));
      } finally {
        if (!isMountedRef.current) {
          return;
        }
        setAckingSignalId(null);
      }
    },
    [refresh],
  );

  const workspaceList = useMemo(
    () => Object.values(snapshot?.workspaces ?? {}),
    [snapshot],
  );
  const threadList = useMemo(
    () =>
      Object.values(snapshot?.threads ?? {}).sort((left, right) => {
        const leftTime = left.last_activity_at_ms ?? 0;
        const rightTime = right.last_activity_at_ms ?? 0;
        return rightTime - leftTime;
      }),
    [snapshot],
  );
  const jobList = useMemo(
    () =>
      Object.values(snapshot?.jobs ?? {}).sort(
        (left, right) => right.requested_at_ms - left.requested_at_ms,
      ),
    [snapshot],
  );
  const signalList = snapshot?.signals ?? [];
  const pendingSignals = useMemo(
    () =>
      signalList
        .filter((signal) => signal.acknowledged_at_ms === null)
        .sort((left, right) => right.created_at_ms - left.created_at_ms),
    [signalList],
  );
  const openQuestionsCount = Object.keys(snapshot?.open_questions ?? {}).length;
  const pendingApprovals = useMemo(
    () =>
      Object.values(snapshot?.pending_approvals ?? {}).sort(
        (left, right) => right.created_at_ms - left.created_at_ms,
      ),
    [snapshot],
  );
  const pendingApprovalsCount = pendingApprovals.length;
  const activityNeedsInputCount = feedItems.filter((entry) => entry.needs_input).length;

  return {
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
  };
}
