import { useCallback, useRef, type FormEvent, type KeyboardEvent } from "react";
import Mic from "lucide-react/dist/esm/icons/mic";
import SendHorizontal from "lucide-react/dist/esm/icons/send-horizontal";
import { DictationWaveform } from "@/features/dictation/components/DictationWaveform";
import type { DictationSessionState, DictationTranscript } from "@/types";
import { useSupervisorChat } from "../hooks/useSupervisorChat";

type SupervisorChatProps = {
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

export function SupervisorChat({
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
}: SupervisorChatProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const {
    messages,
    draft,
    setDraft,
    isLoading,
    isSending,
    loadError,
    sendError,
    sendCommand,
  } = useSupervisorChat({
    textareaRef,
    dictationTranscript,
    onDictationTranscriptHandled,
  });

  const isDictationBusy = dictationState !== "idle";
  const allowOpenDictationSettings = !dictationEnabled;
  const micDisabled = isSending || dictationState === "processing";

  const handleSubmit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      void sendCommand();
    },
    [sendCommand],
  );

  const handleTextareaKeyDown = useCallback(
    (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) {
        return;
      }
      event.preventDefault();
      void sendCommand();
    },
    [sendCommand],
  );

  const handleDictationClick = useCallback(() => {
    if (allowOpenDictationSettings) {
      onOpenDictationSettings();
      return;
    }
    if (micDisabled) {
      return;
    }
    onToggleDictation();
  }, [
    allowOpenDictationSettings,
    micDisabled,
    onOpenDictationSettings,
    onToggleDictation,
  ]);

  return (
    <section className="supervisor-chat" aria-label="Supervisor chat">
      <div className="supervisor-chat-header">
        <div>
          <h2>Global chat</h2>
          <p>Chat with Supervisor or run structured commands from one control point.</p>
        </div>
      </div>

      <div className="supervisor-chat-history" role="log" aria-live="polite">
        {isLoading && messages.length === 0 ? (
          <p className="supervisor-home-empty">Loading chat history...</p>
        ) : null}
        {!isLoading && messages.length === 0 ? (
          <p className="supervisor-home-empty">
            No messages yet. Ask a question or try <code>/help</code>.
          </p>
        ) : null}
        {messages.length > 0 ? (
          <ul className="supervisor-chat-list">
            {messages.map((message) => (
              <li
                key={message.id}
                className={`supervisor-chat-item is-${message.role}`}
              >
                <div className="supervisor-chat-item-role">{message.role}</div>
                <pre className="supervisor-chat-item-text">{message.text}</pre>
              </li>
            ))}
          </ul>
        ) : null}
      </div>

      {loadError ? <div className="supervisor-home-error">{loadError}</div> : null}
      {sendError ? <div className="supervisor-home-error">{sendError}</div> : null}

      {dictationError ? (
        <div className="supervisor-chat-alert is-error" role="alert">
          <span>{dictationError}</span>
          <button type="button" onClick={onDismissDictationError}>
            Dismiss
          </button>
        </div>
      ) : null}
      {dictationHint ? (
        <div className="supervisor-chat-alert is-hint" role="status">
          <span>{dictationHint}</span>
          <button type="button" onClick={onDismissDictationHint}>
            Dismiss
          </button>
        </div>
      ) : null}
      {isDictationBusy ? (
        <DictationWaveform
          active={dictationState === "listening"}
          processing={dictationState === "processing"}
          level={dictationLevel}
        />
      ) : null}

      <form className="supervisor-chat-form" onSubmit={handleSubmit}>
        <textarea
          ref={textareaRef}
          className="supervisor-chat-input"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleTextareaKeyDown}
          placeholder='Type a message or command (e.g. "Run smoke tests" or /status)'
          rows={3}
          disabled={isSending}
        />
        <div className="supervisor-chat-actions">
          <button
            type="button"
            className={`supervisor-chat-mic${isDictationBusy ? " is-active" : ""}`}
            onClick={handleDictationClick}
            disabled={micDisabled}
            aria-label={
              allowOpenDictationSettings
                ? "Open dictation settings"
                : isDictationBusy
                  ? "Stop dictation"
                  : "Start dictation"
            }
            title={
              allowOpenDictationSettings
                ? "Dictation disabled. Open settings"
                : isDictationBusy
                  ? "Stop dictation"
                  : "Start dictation"
            }
          >
            <Mic size={15} aria-hidden />
            {allowOpenDictationSettings ? "Dictation settings" : "Dictation"}
          </button>
          <button
            type="submit"
            className="supervisor-chat-send"
            disabled={isSending || draft.trim().length === 0 || isDictationBusy}
          >
            <SendHorizontal size={15} aria-hidden />
            Send
          </button>
        </div>
      </form>
    </section>
  );
}
