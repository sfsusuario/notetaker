import { useEffect, useRef, useState } from "react";
import { useAutoScroll } from "../../hooks/useAutoScroll";
import { askAboutSession, cancelChat } from "../../services/llm/chatClient";
import { SUMMARY_PROMPT } from "../../services/llm/prompts";
import { PROVIDER_LABELS } from "../../services/llm/providers";
import { useChatStore } from "../../stores/useChatStore";
import { useSettingsStore } from "../../stores/useSettingsStore";
import { useUiStore } from "../../stores/useUiStore";
import type { Segment, SpeakerOverride } from "../../types";
import { IconSend, IconSparkle, IconTrash } from "../common/icons";
import { Markdown } from "../common/Markdown";
import { Button, IconButton, cn } from "../common/ui";

interface Props {
  sessionId: string;
  segments: Segment[];
  speakers?: SpeakerOverride[];
  title?: string | null;
  date?: string;
  live?: boolean;
  className?: string;
}

const QUICK: Array<{ label: string; prompt: string }> = [
  { label: "Resumen", prompt: SUMMARY_PROMPT },
  { label: "Acciones", prompt: "Lista las tareas o acciones acordadas, con responsable y plazo si se mencionan." },
  { label: "Decisiones", prompt: "¿Qué decisiones se tomaron? Enuméralas con una frase de contexto cada una." },
  { label: "Dudas", prompt: "¿Qué preguntas quedaron abiertas o sin respuesta?" },
];

export function ChatPanel({ sessionId, segments, speakers, title, date, live, className }: Props) {
  const messages = useChatStore((s) => s.bySession[sessionId] ?? []);
  const streaming = useChatStore((s) => s.streaming);
  const error = useChatStore((s) => s.error);
  const load = useChatStore((s) => s.load);
  const clear = useChatStore((s) => s.clear);
  const llm = useSettingsStore((s) => s.llm);
  const navigate = useUiStore((s) => s.navigate);
  const [input, setInput] = useState("");
  const taRef = useRef<HTMLTextAreaElement>(null);
  const busy = streaming?.sessionId === sessionId;
  const streamText = busy ? streaming!.text : "";
  const ref = useAutoScroll<HTMLDivElement>(`${messages.length}:${streamText.length}`);

  useEffect(() => {
    void load(sessionId);
  }, [sessionId, load]);

  const send = (text: string) => {
    if (!text.trim() || busy) return;
    setInput("");
    void askAboutSession({
      sessionId,
      question: text,
      segments,
      speakers,
      title,
      date,
      live,
    });
  };

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send(input);
    }
  };

  return (
    <div className={cn("flex h-full flex-col bg-surface/60", className)}>
      <div className="flex items-center justify-between border-b border-line/10 px-3 py-2">
        <div className="flex items-center gap-2 text-xs font-semibold text-fg">
          <IconSparkle width={15} height={15} className="text-indigo-400" />
          Preguntar a la IA
        </div>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => navigate("settings")}
            className="max-w-[160px] truncate rounded-md px-1.5 py-0.5 text-[10px] text-fg-muted hover:bg-line/10 hover:text-fg"
            title="Cambiar proveedor/modelo en Ajustes"
          >
            {PROVIDER_LABELS[llm.provider]} · {llm.model}
          </button>
          {messages.length > 0 && (
            <IconButton title="Borrar conversación" onClick={() => void clear(sessionId)}>
              <IconTrash width={14} height={14} />
            </IconButton>
          )}
        </div>
      </div>

      <div ref={ref} className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {messages.length === 0 && !busy && (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center text-xs text-fg-muted">
            <p>
              Pregunta lo que quieras sobre {live ? "lo hablado hasta ahora" : "esta reunión"}.
            </p>
            <div className="flex flex-wrap justify-center gap-1.5">
              {QUICK.map((q) => (
                <button
                  key={q.label}
                  type="button"
                  onClick={() => send(q.prompt)}
                  className="rounded-full bg-surface-2 px-2.5 py-1 text-[11px] text-fg ring-1 ring-line/10 hover:bg-line/10"
                >
                  {q.label}
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="flex flex-col gap-3">
          {messages.map((m) => (
            <div key={m.id} className={cn("flex", m.role === "user" ? "justify-end" : "justify-start")}>
              <div
                className={cn(
                  "max-w-[92%] rounded-2xl px-3 py-2 text-sm ring-1",
                  m.role === "user"
                    ? "rounded-br-md bg-indigo-600 text-white ring-indigo-500/30"
                    : "rounded-bl-md bg-surface text-fg ring-line/10",
                )}
              >
                {m.role === "user" ? (
                  <span className="whitespace-pre-wrap">{m.content}</span>
                ) : (
                  <Markdown>{m.content}</Markdown>
                )}
              </div>
            </div>
          ))}
          {busy && (
            <div className="flex justify-start">
              <div className="max-w-[92%] rounded-2xl rounded-bl-md bg-surface px-3 py-2 text-sm text-fg ring-1 ring-line/10">
                {streamText ? (
                  <Markdown streaming>{streamText}</Markdown>
                ) : (
                  <span className="nt-typing inline-flex gap-1 py-1 text-fg-muted">
                    <span />
                    <span />
                    <span />
                  </span>
                )}
              </div>
            </div>
          )}
          {error && (
            <div className="rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-400 ring-1 ring-rose-500/30">
              {error}
            </div>
          )}
        </div>
      </div>

      {messages.length > 0 && !busy && (
        <div className="flex flex-wrap gap-1.5 px-3 pb-1">
          {QUICK.map((q) => (
            <button
              key={q.label}
              type="button"
              onClick={() => send(q.prompt)}
              className="rounded-full bg-surface-2 px-2 py-0.5 text-[10px] text-fg-muted ring-1 ring-line/10 hover:text-fg"
            >
              {q.label}
            </button>
          ))}
        </div>
      )}

      <div className="border-t border-line/10 p-2">
        <div className="flex items-end gap-1.5 rounded-xl bg-surface-2 p-1.5 ring-1 ring-line/10 focus-within:ring-indigo-500/50">
          <textarea
            ref={taRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={onKey}
            rows={2}
            placeholder="Escribe una pregunta… (Enter envía, Shift+Enter salto de línea)"
            className="min-h-0 flex-1 resize-none bg-transparent px-1.5 py-1 text-sm text-fg placeholder:text-fg-muted/60 focus:outline-none"
          />
          {busy ? (
            <Button size="sm" onClick={cancelChat}>
              Parar
            </Button>
          ) : (
            <IconButton
              title="Enviar"
              onClick={() => send(input)}
              disabled={!input.trim()}
              className="bg-indigo-600 text-white hover:bg-indigo-500 hover:text-white"
            >
              <IconSend width={15} height={15} />
            </IconButton>
          )}
        </div>
      </div>
    </div>
  );
}
