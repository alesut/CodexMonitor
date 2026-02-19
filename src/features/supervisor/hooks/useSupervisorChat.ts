import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { subscribeSupervisorEvents } from "@services/events";
import {
  getSupervisorChatHistory,
  sendSupervisorChatCommand,
  type SupervisorChatMessage,
} from "@services/tauri";
import type { DictationTranscript } from "@/types";
import { computeDictationInsertion } from "@/utils/dictation";

type UseSupervisorChatOptions = {
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  dictationTranscript: DictationTranscript | null;
  onDictationTranscriptHandled: (id: string) => void;
};

function normalizeError(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export function useSupervisorChat({
  textareaRef,
  dictationTranscript,
  onDictationTranscriptHandled,
}: UseSupervisorChatOptions) {
  const [messages, setMessages] = useState<SupervisorChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSending, setIsSending] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const isMountedRef = useRef(true);
  const hasLoadedRef = useRef(false);

  const refreshHistory = useCallback(
    async (mode: "initial" | "background" = "initial") => {
      if (mode === "initial") {
        setIsLoading(true);
        setLoadError(null);
      }
      try {
        const response = await getSupervisorChatHistory();
        if (!isMountedRef.current) {
          return;
        }
        setMessages(response.messages);
      } catch (error) {
        if (!isMountedRef.current) {
          return;
        }
        if (mode === "initial") {
          setLoadError(normalizeError(error, "Failed to load supervisor chat history."));
        }
      } finally {
        if (!isMountedRef.current) {
          return;
        }
        if (mode === "initial") {
          setIsLoading(false);
        }
      }
    },
    [],
  );

  const sendCommand = useCallback(async () => {
    const command = draft.trim();
    if (!command || isSending) {
      return false;
    }

    setSendError(null);
    setIsSending(true);
    let sent = false;
    try {
      const response = await sendSupervisorChatCommand(command);
      if (!isMountedRef.current) {
        return false;
      }
      setMessages(response.messages);
      setDraft("");
      sent = true;
    } catch (error) {
      if (!isMountedRef.current) {
        return false;
      }
      setSendError(normalizeError(error, "Failed to send supervisor command."));
    } finally {
      if (isMountedRef.current) {
        setIsSending(false);
      }
    }
    return sent;
  }, [draft, isSending]);

  useEffect(() => {
    isMountedRef.current = true;
    if (!hasLoadedRef.current) {
      hasLoadedRef.current = true;
      void refreshHistory("initial");
      return;
    }
    void refreshHistory("background");
  }, [refreshHistory]);

  useEffect(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeSupervisorEvents(() => {
      if (refreshTimer !== null) {
        return;
      }
      refreshTimer = setTimeout(() => {
        refreshTimer = null;
        void refreshHistory("background");
      }, 250);
    });

    return () => {
      isMountedRef.current = false;
      unsubscribe();
      if (refreshTimer !== null) {
        clearTimeout(refreshTimer);
      }
    };
  }, [refreshHistory]);

  useEffect(() => {
    if (!dictationTranscript) {
      return;
    }

    const textToInsert = dictationTranscript.text.trim();
    if (!textToInsert) {
      onDictationTranscriptHandled(dictationTranscript.id);
      return;
    }

    setDraft((previous) => {
      const textarea = textareaRef.current;
      const start = textarea?.selectionStart ?? previous.length;
      const end = textarea?.selectionEnd ?? start;
      const { nextText, nextCursor } = computeDictationInsertion(
        previous,
        textToInsert,
        start,
        end,
      );

      requestAnimationFrame(() => {
        if (!textareaRef.current) {
          return;
        }
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(nextCursor, nextCursor);
      });

      return nextText;
    });

    onDictationTranscriptHandled(dictationTranscript.id);
  }, [dictationTranscript, onDictationTranscriptHandled, textareaRef]);

  return {
    messages,
    draft,
    setDraft,
    isLoading,
    isSending,
    loadError,
    sendError,
    sendCommand,
    refreshHistory,
  };
}
