// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppServerEvent } from "../../../types";
import { subscribeSupervisorEvents } from "@services/events";
import { getSupervisorSnapshot } from "@services/tauri";
import { pushErrorToast } from "@services/toasts";
import { useSupervisorSignalAlerts } from "./useSupervisorSignalAlerts";

vi.mock("@services/events", () => ({
  subscribeSupervisorEvents: vi.fn(),
}));

vi.mock("@services/tauri", () => ({
  getSupervisorSnapshot: vi.fn(),
}));

vi.mock("@services/toasts", () => ({
  pushErrorToast: vi.fn(),
}));

function baseSnapshot() {
  return {
    workspaces: {},
    threads: {},
    jobs: {},
    signals: [],
    activity_feed: [],
    open_questions: {},
    pending_approvals: {},
    chat_history: [],
  };
}

let supervisorListener: ((event: AppServerEvent) => void) | null = null;
const unlisten = vi.fn();

describe("useSupervisorSignalAlerts", () => {
  beforeEach(() => {
    supervisorListener = null;
    unlisten.mockReset();
    vi.mocked(subscribeSupervisorEvents).mockImplementation((listener) => {
      supervisorListener = listener as unknown as (event: AppServerEvent) => void;
      return unlisten;
    });
    vi.mocked(pushErrorToast).mockReset();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("emits toasts for pending critical signals and tracks badge count", async () => {
    vi.mocked(getSupervisorSnapshot).mockResolvedValueOnce({
      ...baseSnapshot(),
      signals: [
        {
          id: "s-1",
          kind: "needs_approval",
          workspace_id: "ws-1",
          thread_id: "thread-1",
          job_id: null,
          message: "Need approval for apply_patch",
          created_at_ms: Date.now(),
          acknowledged_at_ms: null,
          context: null,
        },
        {
          id: "s-2",
          kind: "failed",
          workspace_id: "ws-2",
          thread_id: null,
          job_id: null,
          message: "Dispatch failed",
          created_at_ms: Date.now(),
          acknowledged_at_ms: null,
          context: null,
        },
        {
          id: "s-3",
          kind: "failed",
          workspace_id: "ws-3",
          thread_id: null,
          job_id: null,
          message: "Already acknowledged",
          created_at_ms: Date.now(),
          acknowledged_at_ms: Date.now(),
          context: null,
        },
      ],
    });

    const { result } = renderHook(() => useSupervisorSignalAlerts());

    await waitFor(() => {
      expect(result.current.pendingCriticalSignalsCount).toBe(2);
    });

    expect(pushErrorToast).toHaveBeenCalledTimes(2);
    expect(pushErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "supervisor-signal-s-1",
        title: "Supervisor approval needed",
        message: expect.stringContaining("workspace ws-1"),
      }),
    );
  });

  it("uses pending approval details to replace generic approval signal text", async () => {
    const now = Date.now();
    vi.mocked(getSupervisorSnapshot).mockResolvedValueOnce({
      ...baseSnapshot(),
      workspaces: {
        "ws-1": {
          id: "ws-1",
          name: "Primary",
          connected: true,
          current_task: null,
          last_activity_at_ms: null,
          next_expected_step: null,
          blockers: [],
          health: "healthy",
          active_thread_id: null,
        },
      },
      pending_approvals: {
        "ws-1:42": {
          request_key: "ws-1:42",
          workspace_id: "ws-1",
          thread_id: "019c7b1f-6c51-7ad3-b2da-d02da5937f74",
          turn_id: null,
          item_id: null,
          request_id: "42",
          method: "codex/requestApproval/shell",
          params: {
            command: ["npm", "run", "typecheck"],
          },
          created_at_ms: now,
          resolved_at_ms: null,
        },
      },
      signals: [
        {
          id: "s-approval",
          kind: "needs_approval",
          workspace_id: "ws-1",
          thread_id: "019c7b1f-6c51-7ad3-b2da-d02da5937f74",
          job_id: null,
          message: "Action requires approval",
          created_at_ms: now,
          acknowledged_at_ms: null,
          context: { requestKey: "ws-1:42" },
        },
      ],
    });

    const { result } = renderHook(() => useSupervisorSignalAlerts());

    await waitFor(() => {
      expect(result.current.pendingCriticalSignalsCount).toBe(1);
    });

    expect(pushErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "supervisor-signal-s-approval",
        title: "Supervisor approval needed - Primary",
        message: expect.stringContaining("Command awaiting approval: npm run typecheck"),
      }),
    );
    expect(pushErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({
        message: expect.not.stringContaining("Action requires approval"),
      }),
    );
    expect(pushErrorToast).toHaveBeenCalledWith(
      expect.objectContaining({
        message: expect.stringContaining("thread 019c7b1f...7f74"),
      }),
    );
  });

  it("does not duplicate toast for already announced signals on event refresh", async () => {
    vi.mocked(getSupervisorSnapshot)
      .mockResolvedValueOnce({
        ...baseSnapshot(),
        signals: [
          {
            id: "s-1",
            kind: "failed",
            workspace_id: "ws-1",
            thread_id: "thread-1",
            job_id: null,
            message: "Initial failure",
            created_at_ms: Date.now(),
            acknowledged_at_ms: null,
            context: null,
          },
        ],
      })
      .mockResolvedValueOnce({
        ...baseSnapshot(),
        signals: [
          {
            id: "s-1",
            kind: "failed",
            workspace_id: "ws-1",
            thread_id: "thread-1",
            job_id: null,
            message: "Initial failure",
            created_at_ms: Date.now(),
            acknowledged_at_ms: null,
            context: null,
          },
          {
            id: "s-2",
            kind: "stalled",
            workspace_id: "ws-2",
            thread_id: "thread-2",
            job_id: null,
            message: "Run has stalled",
            created_at_ms: Date.now(),
            acknowledged_at_ms: null,
            context: null,
          },
        ],
      });

    const { result } = renderHook(() => useSupervisorSignalAlerts());

    await waitFor(() => {
      expect(result.current.pendingCriticalSignalsCount).toBe(1);
    });
    expect(pushErrorToast).toHaveBeenCalledTimes(1);

    act(() => {
      supervisorListener?.({
        workspace_id: "ws-1",
        message: { method: "turn/completed" },
      });
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 300));
    });

    await waitFor(() => {
      expect(result.current.pendingCriticalSignalsCount).toBe(2);
    });
    expect(pushErrorToast).toHaveBeenCalledTimes(2);
    expect(pushErrorToast).toHaveBeenLastCalledWith(
      expect.objectContaining({
        id: "supervisor-signal-s-2",
        title: "Supervisor task stalled",
      }),
    );
  });
});
