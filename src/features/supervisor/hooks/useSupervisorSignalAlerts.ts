import { useCallback, useEffect, useRef, useState } from "react";
import { subscribeSupervisorEvents } from "@services/events";
import {
  getSupervisorSnapshot,
  type SupervisorSignal,
  type SupervisorSignalKind,
} from "@services/tauri";
import { pushErrorToast } from "@services/toasts";

const CRITICAL_SIGNAL_KINDS = new Set<SupervisorSignalKind>([
  "needs_approval",
  "failed",
  "stalled",
  "disconnected",
]);

const SIGNAL_TITLE_BY_KIND: Record<SupervisorSignalKind, string> = {
  needs_approval: "Supervisor approval needed",
  failed: "Supervisor task failed",
  completed: "Supervisor task completed",
  stalled: "Supervisor task stalled",
  disconnected: "Supervisor disconnected",
};

function buildSignalToastMessage(signal: SupervisorSignal) {
  const context: string[] = [];
  if (signal.workspace_id) {
    context.push(`workspace ${signal.workspace_id}`);
  }
  if (signal.thread_id) {
    context.push(`thread ${signal.thread_id}`);
  }
  if (signal.job_id) {
    context.push(`job ${signal.job_id}`);
  }
  const contextLabel = context.length ? ` (${context.join(" · ")})` : "";
  return `${signal.message}${contextLabel}`;
}

export function useSupervisorSignalAlerts() {
  const [pendingCriticalSignalsCount, setPendingCriticalSignalsCount] = useState(0);
  const isMountedRef = useRef(true);
  const announcedSignalIdsRef = useRef(new Set<string>());

  const refreshSignals = useCallback(async () => {
    try {
      const snapshot = await getSupervisorSnapshot();
      if (!isMountedRef.current) {
        return;
      }
      const pendingCriticalSignals = snapshot.signals.filter(
        (signal) =>
          signal.acknowledged_at_ms === null &&
          CRITICAL_SIGNAL_KINDS.has(signal.kind),
      );
      setPendingCriticalSignalsCount(pendingCriticalSignals.length);

      for (const signal of pendingCriticalSignals) {
        if (announcedSignalIdsRef.current.has(signal.id)) {
          continue;
        }
        announcedSignalIdsRef.current.add(signal.id);
        pushErrorToast({
          id: `supervisor-signal-${signal.id}`,
          title: SIGNAL_TITLE_BY_KIND[signal.kind],
          message: buildSignalToastMessage(signal),
          durationMs: 10000,
        });
      }
    } catch {
      if (!isMountedRef.current) {
        return;
      }
    }
  }, []);

  useEffect(() => {
    isMountedRef.current = true;
    void refreshSignals();

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeSupervisorEvents(() => {
      if (refreshTimer !== null) {
        return;
      }
      refreshTimer = setTimeout(() => {
        refreshTimer = null;
        void refreshSignals();
      }, 250);
    });

    const intervalId = setInterval(() => {
      void refreshSignals();
    }, 20000);

    return () => {
      isMountedRef.current = false;
      unsubscribe();
      if (refreshTimer !== null) {
        clearTimeout(refreshTimer);
      }
      clearInterval(intervalId);
    };
  }, [refreshSignals]);

  return {
    pendingCriticalSignalsCount,
  };
}
