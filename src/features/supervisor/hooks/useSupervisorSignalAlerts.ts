import { useCallback, useEffect, useRef, useState } from "react";
import { subscribeSupervisorEvents } from "@services/events";
import {
  getSupervisorSnapshot,
  type SupervisorPendingApproval,
  type SupervisorSignal,
  type SupervisorSignalKind,
  type SupervisorSnapshot,
} from "@services/tauri";
import { pushErrorToast } from "@services/toasts";
import { getApprovalCommandInfo } from "@utils/approvalRules";

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

const GENERIC_APPROVAL_MESSAGE = "Action requires approval";
const APPROVAL_METHOD_PREFIX = /^codex\/requestApproval\/?/;
const MAX_TOAST_MESSAGE_LENGTH = 200;
const CONTEXT_ID_SHRINK_THRESHOLD = 24;

function truncateText(text: string, maxLength: number) {
  if (text.length <= maxLength) {
    return text;
  }
  return `${text.slice(0, Math.max(0, maxLength - 3))}...`;
}

function shortenContextId(value: string) {
  const normalized = value.trim();
  if (normalized.length <= CONTEXT_ID_SHRINK_THRESHOLD) {
    return normalized;
  }
  return `${normalized.slice(0, 8)}...${normalized.slice(-4)}`;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function formatApprovalMethod(method: string) {
  const trimmed = method.replace(APPROVAL_METHOD_PREFIX, "");
  return trimmed || method;
}

function getWorkspaceName(
  snapshot: SupervisorSnapshot,
  workspaceId: string | null,
): string | null {
  if (!workspaceId) {
    return null;
  }
  const workspaceName = snapshot.workspaces[workspaceId]?.name?.trim();
  return workspaceName || null;
}

function getWorkspaceContextLabel(
  snapshot: SupervisorSnapshot,
  workspaceId: string | null,
): string | null {
  if (!workspaceId) {
    return null;
  }
  const workspaceName = getWorkspaceName(snapshot, workspaceId);
  if (workspaceName) {
    return workspaceName;
  }
  return shortenContextId(workspaceId);
}

function getSignalApprovalRequestKey(signal: SupervisorSignal): string | null {
  const contextRecord = asRecord(signal.context);
  const requestKeyValue = contextRecord?.requestKey ?? contextRecord?.request_key;
  if (typeof requestKeyValue !== "string") {
    return null;
  }
  const requestKey = requestKeyValue.trim();
  return requestKey || null;
}

function getSignalPendingApproval(
  signal: SupervisorSignal,
  snapshot: SupervisorSnapshot,
): SupervisorPendingApproval | null {
  const requestKey = getSignalApprovalRequestKey(signal);
  if (!requestKey) {
    return null;
  }
  return snapshot.pending_approvals[requestKey] ?? null;
}

function buildNeedsApprovalToastMessage(
  signal: SupervisorSignal,
  snapshot: SupervisorSnapshot,
): string {
  const pendingApproval = getSignalPendingApproval(signal, snapshot);
  if (pendingApproval) {
    const params = asRecord(pendingApproval.params);
    const commandInfo = params ? getApprovalCommandInfo(params) : null;
    if (commandInfo?.preview) {
      return truncateText(
        `Command awaiting approval: ${commandInfo.preview}`,
        MAX_TOAST_MESSAGE_LENGTH,
      );
    }
    const method = formatApprovalMethod(pendingApproval.method).trim();
    if (method) {
      return truncateText(`Action awaiting approval: ${method}`, MAX_TOAST_MESSAGE_LENGTH);
    }
  }
  if (signal.message === GENERIC_APPROVAL_MESSAGE) {
    return "Action awaiting approval";
  }
  return signal.message;
}

function buildSignalContextLabel(signal: SupervisorSignal, snapshot: SupervisorSnapshot) {
  const context: string[] = [];
  const workspaceLabel = getWorkspaceContextLabel(snapshot, signal.workspace_id);
  if (workspaceLabel) {
    context.push(`workspace ${workspaceLabel}`);
  }
  if (signal.thread_id) {
    context.push(`thread ${shortenContextId(signal.thread_id)}`);
  }
  if (signal.job_id) {
    context.push(`job ${shortenContextId(signal.job_id)}`);
  }
  return context.length ? ` (${context.join(" · ")})` : "";
}

function buildSignalToastMessage(signal: SupervisorSignal, snapshot: SupervisorSnapshot) {
  const message =
    signal.kind === "needs_approval"
      ? buildNeedsApprovalToastMessage(signal, snapshot)
      : signal.message;
  return `${message}${buildSignalContextLabel(signal, snapshot)}`;
}

function buildSignalToastTitle(signal: SupervisorSignal, snapshot: SupervisorSnapshot) {
  const title = SIGNAL_TITLE_BY_KIND[signal.kind];
  if (signal.kind !== "needs_approval") {
    return title;
  }
  const workspaceName = getWorkspaceName(snapshot, signal.workspace_id);
  return workspaceName ? `${title} - ${workspaceName}` : title;
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
          title: buildSignalToastTitle(signal, snapshot),
          message: buildSignalToastMessage(signal, snapshot),
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
