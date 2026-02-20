import { useCallback, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
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

const TECHNICAL_DETAIL_LINE_PATTERN =
  /\b(route|reason|fallback|workspace|thread|subtask|request[_\s-]?id|turn[_\s-]?id|item[_\s-]?id|dedupe|access mode|model|effort)\b/i;
const TECHNICAL_DETAIL_ID_PATTERN =
  /`[^`]*(ws-|thread-|job-|turn-|signal-|req-|request-|item-)[^`]*`/i;

function isTechnicalDetailLine(line: string) {
  const trimmed = line.trim();
  if (!trimmed) {
    return false;
  }
  return (
    TECHNICAL_DETAIL_LINE_PATTERN.test(trimmed) ||
    TECHNICAL_DETAIL_ID_PATTERN.test(trimmed)
  );
}

function splitSystemMessageText(text: string) {
  const lines = text
    .split("\n")
    .map((line) => line.trimEnd())
    .filter((line) => line.trim().length > 0);
  if (lines.length <= 1) {
    return { primaryText: text, technicalDetails: null as string | null };
  }

  const primaryLines: string[] = [];
  const technicalLines: string[] = [];
  for (const line of lines) {
    if (isTechnicalDetailLine(line)) {
      technicalLines.push(line);
    } else {
      primaryLines.push(line);
    }
  }

  if (technicalLines.length === 0) {
    return { primaryText: text, technicalDetails: null as string | null };
  }
  if (primaryLines.length === 0) {
    const [headline, ...details] = technicalLines;
    return {
      primaryText: headline,
      technicalDetails: details.length > 0 ? details.join("\n") : null,
    };
  }
  return {
    primaryText: primaryLines.join("\n"),
    technicalDetails: technicalLines.join("\n"),
  };
}

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
  const [expandedSystemDetails, setExpandedSystemDetails] = useState<Set<string>>(new Set());

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
  const toggleSystemDetails = useCallback((messageId: string) => {
    setExpandedSystemDetails((current) => {
      const next = new Set(current);
      if (next.has(messageId)) {
        next.delete(messageId);
      } else {
        next.add(messageId);
      }
      return next;
    });
  }, []);

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
            {messages.map((message) => {
              const isSystemMessage = message.role === "system";
              const { primaryText, technicalDetails } = isSystemMessage
                ? splitSystemMessageText(message.text)
                : { primaryText: message.text, technicalDetails: null };
              const isExpanded = expandedSystemDetails.has(message.id);

              return (
                <li key={message.id} className={`supervisor-chat-item is-${message.role}`}>
                  <div className="supervisor-chat-item-role">{message.role}</div>
                  <pre className="supervisor-chat-item-text">{primaryText}</pre>
                  {technicalDetails ? (
                    <div className="supervisor-chat-item-technical">
                      <button
                        type="button"
                        className="supervisor-chat-item-details-toggle"
                        onClick={() => toggleSystemDetails(message.id)}
                      >
                        {isExpanded ? "Hide technical details" : "Show technical details"}
                      </button>
                      {!isExpanded ? (
                        <span className="supervisor-chat-item-technical-hint">
                          Technical details hidden
                        </span>
                      ) : (
                        <pre className="supervisor-chat-item-details">{technicalDetails}</pre>
                      )}
                    </div>
                  ) : null}
                </li>
              );
            })}
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
      <div className="supervisor-chat-voice-row">
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
      </div>

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
