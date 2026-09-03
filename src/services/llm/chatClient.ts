import { newId } from "../../app/format";
import * as db from "../storage/db";
import { useChatStore } from "../../stores/useChatStore";
import { useSettingsStore } from "../../stores/useSettingsStore";
import type { ChatMessage, Segment, SpeakerOverride } from "../../types";
import { getProvider, PROVIDERS } from "./providers";
import type { HistoryTurn } from "./providers/types";
import {
  clampTranscript,
  formatTranscript,
  systemQa,
  titleTurn,
  TITLE_SYSTEM,
} from "./prompts";

/**
 * Agrupa los deltas de streaming y refresca el store a ~15 fps en vez de por
 * token: evita re-parsear el Markdown en cada carácter.
 */
function makeThrottledAppender(sink: (s: string) => void, intervalMs = 66) {
  let buf = "";
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    timer = null;
    if (buf) {
      sink(buf);
      buf = "";
    }
  };
  return {
    push(delta: string) {
      buf += delta;
      if (!timer) timer = setTimeout(flush, intervalMs);
    },
    flush() {
      if (timer) clearTimeout(timer);
      flush();
    },
  };
}

let current: AbortController | null = null;

export function cancelChat(): void {
  current?.abort();
  current = null;
  useChatStore.getState().endStreaming();
}

export async function testProvider(): Promise<string> {
  const { llm } = useSettingsStore.getState();
  return getProvider(llm.provider).test(llm.model, llm.baseUrl);
}

/** Pregunta sobre una sesión; persiste pregunta y respuesta en la DB. */
export async function askAboutSession(args: {
  sessionId: string;
  question: string;
  segments: Segment[];
  speakers?: SpeakerOverride[];
  title?: string | null;
  date?: string;
  live?: boolean;
}): Promise<void> {
  const chat = useChatStore.getState();
  const { llm } = useSettingsStore.getState();
  const provider = PROVIDERS[llm.provider];
  const question = args.question.trim();
  if (!question) return;

  cancelChat();
  const controller = new AbortController();
  current = controller;
  chat.setError(null);

  const userMsg: ChatMessage = {
    id: newId(),
    sessionId: args.sessionId,
    role: "user",
    content: question,
    createdAt: Date.now(),
  };
  chat.append(userMsg);
  void db.insertChatMessage(userMsg);

  const history: HistoryTurn[] = (chat.bySession[args.sessionId] ?? [])
    .filter((m) => m.id !== userMsg.id)
    .slice(-12)
    .map((m) => ({ role: m.role, text: m.content }));

  const { text: transcript, truncated } = clampTranscript(
    formatTranscript(args.segments, args.speakers ?? []),
  );

  chat.setStreaming(args.sessionId, "");
  let acc = "";
  const appender = makeThrottledAppender((s) => {
    acc += s;
    if (current === controller) useChatStore.getState().setStreaming(args.sessionId, acc);
  });

  try {
    const result = await provider.generate({
      model: llm.model,
      depth: "standard",
      system: [
        {
          text: systemQa(transcript, truncated, {
            title: args.title,
            date: args.date,
            live: args.live,
          }),
          cache: true,
        },
      ],
      history,
      turn: { text: question, imageBase64: null },
      baseUrl: llm.baseUrl,
      signal: controller.signal,
      onDelta: (d) => appender.push(d),
    });
    appender.flush();
    if (current !== controller) return;
    const content = result.refused
      ? "El modelo rechazó responder a esta pregunta."
      : result.text.trim() || acc.trim() || "(respuesta vacía)";
    const assistantMsg: ChatMessage = {
      id: newId(),
      sessionId: args.sessionId,
      role: "assistant",
      content,
      createdAt: Date.now(),
    };
    useChatStore.getState().append(assistantMsg);
    void db.insertChatMessage(assistantMsg);
  } catch (e) {
    if (controller.signal.aborted) return;
    useChatStore
      .getState()
      .setError(e instanceof Error ? e.message : String(e));
  } finally {
    if (current === controller) {
      current = null;
      useChatStore.getState().endStreaming();
    }
  }
}

/** Título corto (≤ 8 palabras) a partir de la transcripción. */
export async function generateTitle(
  segments: Segment[],
  speakers: SpeakerOverride[] = [],
): Promise<string | null> {
  const { llm } = useSettingsStore.getState();
  const provider = PROVIDERS[llm.provider];
  const transcript = formatTranscript(segments, speakers);
  if (transcript.trim().length < 80) return null;
  const result = await provider.generate({
    model: llm.model,
    depth: "minimal",
    system: [{ text: TITLE_SYSTEM }],
    history: [],
    turn: { text: titleTurn(transcript), imageBase64: null },
    baseUrl: llm.baseUrl,
    onDelta: () => {},
  });
  const t = result.text
    .split("\n")[0]
    .replace(/^["'“”«»\s]+|["'“”«»\s.]+$/g, "")
    .trim();
  if (!t || t.length > 90) return null;
  return t;
}
