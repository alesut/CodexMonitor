// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupervisorHome } from "./SupervisorHome";
import {
  ackSupervisorSignal,
  getSupervisorFeed,
  getSupervisorSnapshot,
} from "@services/tauri";
import { subscribeSupervisorEvents } from "@services/events";

vi.mock("@services/tauri", () => ({
  getSupervisorSnapshot: vi.fn(),
  getSupervisorFeed: vi.fn(),
  ackSupervisorSignal: vi.fn(),
}));

vi.mock("@services/events", () => ({
  subscribeSupervisorEvents: vi.fn(),
}));

vi.mock("./SupervisorChat", () => ({
  SupervisorChat: () => <div data-testid="supervisor-chat">Supervisor chat</div>,
}));

const snapshotFixture = {
  workspaces: {
    "ws-1": {
      id: "ws-1",
      name: "Workspace Alpha",
      connected: true,
      current_task: "Fix alert routing",
      last_activity_at_ms: Date.now(),
      next_expected_step: "Ship hotfix",
      blockers: ["Waiting for approval"],
      health: "healthy" as const,
      active_thread_id: "thread-1",
    },
  },
  threads: {
    "thread-1": {
      id: "thread-1",
      workspace_id: "ws-1",
      name: "Ops thread",
      status: "waiting_input" as const,
      current_task: "Wait for approval",
      last_activity_at_ms: Date.now(),
      next_expected_step: "Resume turn",
      blockers: [],
      active_turn_id: "turn-1",
    },
  },
  jobs: {},
  signals: [
    {
      id: "signal-1",
      kind: "needs_approval" as const,
      workspace_id: "ws-1",
      thread_id: "thread-1",
      job_id: null,
      message: "Approval required for deployment",
      created_at_ms: Date.now(),
      acknowledged_at_ms: null,
      context: null,
    },
  ],
  activity_feed: [],
  open_questions: {},
  pending_approvals: {},
  chat_history: [],
};

function cloneSnapshot() {
  return JSON.parse(JSON.stringify(snapshotFixture));
}

function cloneFeed() {
  return {
    items: [
      {
        id: "activity-1",
        kind: "turn/completed",
        message: "Deployment run completed",
        created_at_ms: Date.now(),
        workspace_id: "ws-1",
        thread_id: "thread-1",
        needs_input: true,
        metadata: null,
      },
    ],
    total: 1,
  };
}

const supervisorHomeProps = {
  dictationEnabled: true,
  dictationState: "idle" as const,
  dictationLevel: 0,
  onToggleDictation: vi.fn(),
  onOpenDictationSettings: vi.fn(),
  dictationError: null,
  onDismissDictationError: vi.fn(),
  dictationHint: null,
  onDismissDictationHint: vi.fn(),
  dictationTranscript: null,
  onDictationTranscriptHandled: vi.fn(),
};

describe("SupervisorHome", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    vi.mocked(getSupervisorSnapshot).mockResolvedValue(cloneSnapshot());
    vi.mocked(getSupervisorFeed).mockResolvedValue(cloneFeed());
    vi.mocked(ackSupervisorSignal).mockResolvedValue({ ok: true });
    vi.mocked(subscribeSupervisorEvents).mockImplementation(() => () => {});
  });

  it("renders snapshot workspace and pending signal", async () => {
    render(<SupervisorHome {...supervisorHomeProps} />);

    expect(await screen.findByText("Supervisor")).toBeTruthy();
    expect(await screen.findByText("Workspace Alpha")).toBeTruthy();
    expect(await screen.findByText("Approval required for deployment")).toBeTruthy();
    expect(await screen.findByText("Deployment run completed")).toBeTruthy();
    expect(screen.getByText("Workspaces")).toBeTruthy();
    expect(vi.mocked(getSupervisorSnapshot)).toHaveBeenCalled();
    expect(vi.mocked(getSupervisorFeed)).toHaveBeenCalledWith({
      limit: 80,
      needsInputOnly: false,
    });
  });

  it("acknowledges signal and refreshes snapshot", async () => {
    const acknowledgedSnapshot = cloneSnapshot();
    acknowledgedSnapshot.signals[0].acknowledged_at_ms = Date.now();

    vi.mocked(getSupervisorSnapshot)
      .mockResolvedValueOnce(cloneSnapshot())
      .mockResolvedValueOnce(acknowledgedSnapshot);

    render(<SupervisorHome {...supervisorHomeProps} />);
    const ackButton = await screen.findByRole("button", { name: "Acknowledge" });
    fireEvent.click(ackButton);

    await waitFor(() => {
      expect(ackSupervisorSignal).toHaveBeenCalledWith("signal-1");
      expect(getSupervisorSnapshot).toHaveBeenCalledTimes(2);
      expect(getSupervisorFeed).toHaveBeenCalledTimes(2);
    });
  });

  it("filters feed to only needs-input entries", async () => {
    render(<SupervisorHome {...supervisorHomeProps} />);
    await screen.findByText("Deployment run completed");

    const filterButton = screen.getByRole("button", { name: "Needs my input" });
    fireEvent.click(filterButton);

    await waitFor(() => {
      expect(getSupervisorFeed).toHaveBeenLastCalledWith({
        limit: 80,
        needsInputOnly: true,
      });
    });
  });

  it("renders dispatch job model/effort/access metadata", async () => {
    const snapshotWithJob = cloneSnapshot();
    snapshotWithJob.jobs = {
      "job-1": {
        id: "job-1",
        workspace_id: "ws-1",
        thread_id: "thread-1",
        dedupe_key: "dedupe-1",
        description: "Run smoke tests",
        status: "running",
        requested_at_ms: Date.now(),
        started_at_ms: Date.now(),
        completed_at_ms: null,
        error: null,
        route_kind: "workspace_delegate",
        route_target: "ws-1",
        route_reason: "Explicit route",
        route_fallback: null,
        model: "gpt-5-mini",
        effort: "high",
        access_mode: "full-access",
        waiting_request_id: null,
        waiting_question_ids: [],
        recent_events: [],
      },
    };
    vi.mocked(getSupervisorSnapshot).mockResolvedValueOnce(snapshotWithJob);

    render(<SupervisorHome {...supervisorHomeProps} />);

    expect(await screen.findByText(/Model gpt-5-mini/)).toBeTruthy();
    expect(screen.getByText(/Effort high/)).toBeTruthy();
    expect(screen.getByText(/Access full-access/)).toBeTruthy();
  });
});
