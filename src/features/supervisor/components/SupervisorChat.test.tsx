// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupervisorChat } from "./SupervisorChat";
import {
  getSupervisorChatHistory,
  sendSupervisorChatCommand,
} from "@services/tauri";
import { subscribeSupervisorEvents } from "@services/events";

vi.mock("@services/tauri", () => ({
  getSupervisorChatHistory: vi.fn(),
  sendSupervisorChatCommand: vi.fn(),
}));

vi.mock("@services/events", () => ({
  subscribeSupervisorEvents: vi.fn(),
}));

const baseProps = {
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

describe("SupervisorChat", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(subscribeSupervisorEvents).mockImplementation(() => () => {});
    vi.mocked(getSupervisorChatHistory).mockResolvedValue({
      messages: [
        {
          id: "msg-1",
          role: "system",
          text: "Supported commands",
          created_at_ms: Date.now(),
        },
      ],
    });
    vi.mocked(sendSupervisorChatCommand).mockResolvedValue({
      messages: [
        {
          id: "msg-1",
          role: "system",
          text: "Supported commands",
          created_at_ms: Date.now(),
        },
        {
          id: "msg-2",
          role: "user",
          text: "/status",
          created_at_ms: Date.now(),
        },
        {
          id: "msg-3",
          role: "system",
          text: "Global supervisor status:",
          created_at_ms: Date.now(),
        },
      ],
    });
  });

  it("loads history and sends command", async () => {
    render(<SupervisorChat {...baseProps} />);
    expect(await screen.findByText("Supported commands")).toBeTruthy();

    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "/status" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(sendSupervisorChatCommand).toHaveBeenCalledWith("/status");
      expect(screen.getByText("Global supervisor status:")).toBeTruthy();
    });
  });

  it("inserts dictation transcript into the command input", async () => {
    const onDictationTranscriptHandled = vi.fn();
    const { rerender } = render(
      <SupervisorChat
        {...baseProps}
        onDictationTranscriptHandled={onDictationTranscriptHandled}
      />,
    );
    await screen.findByText("Supported commands");

    const input = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "/status " } });

    rerender(
      <SupervisorChat
        {...baseProps}
        onDictationTranscriptHandled={onDictationTranscriptHandled}
        dictationTranscript={{ id: "dict-1", text: "ws-1" }}
      />,
    );

    await waitFor(() => {
      expect(onDictationTranscriptHandled).toHaveBeenCalledWith("dict-1");
      expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toContain(
        "/status ws-1",
      );
    });
  });

  it("collapses technical system details until expanded", async () => {
    vi.mocked(getSupervisorChatHistory).mockResolvedValueOnce({
      messages: [
        {
          id: "msg-technical",
          role: "system",
          text: "Child task completed.\nRoute: workspace_delegate\nReason: explicit route",
          created_at_ms: Date.now(),
        },
      ],
    });

    render(<SupervisorChat {...baseProps} />);

    expect(await screen.findByText("Child task completed.")).toBeTruthy();
    expect(screen.getByText("Technical details hidden")).toBeTruthy();
    expect(screen.queryByText(/Route: workspace_delegate/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Show technical details" }));

    expect(await screen.findByText(/Route: workspace_delegate/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hide technical details" })).toBeTruthy();
  });
});
