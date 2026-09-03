import { useMemo } from "react";
import { pauseLive, resumeLive, stopLive } from "../../../app/actions";
import { fmtMs } from "../../../app/format";
import { ChatPanel } from "../../../components/chat/ChatPanel";
import {
  IconHeadphones,
  IconMic,
  IconPause,
  IconPlay,
  IconSparkle,
  IconStop,
} from "../../../components/common/icons";
import { Button, IconButton, StatusDot, cn } from "../../../components/common/ui";
import { TranscriptFeed } from "../../../components/transcript/TranscriptFeed";
import { useEnginesStore } from "../../../stores/useEnginesStore";
import { livePositionMs, useSessionStore } from "../../../stores/useSessionStore";
import { useSettingsStore } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import type { AudioMetrics, ConnectionState, SegmentSource } from "../../../types";

function LevelMeter({ rms }: { rms: number }) {
  // rms 0–1 → escala logarítmica aproximada para que la voz normal llene ~60 %
  const level = Math.min(1, Math.max(0, Math.log10(1 + rms * 30)));
  return (
    <div className="h-1 w-14 overflow-hidden rounded-full bg-line/10">
      <div
        className={cn("h-full rounded-full transition-[width] duration-100", level > 0.85 ? "bg-rose-400" : "bg-emerald-400")}
        style={{ width: `${level * 100}%` }}
      />
    </div>
  );
}

const STATUS_LABEL: Record<ConnectionState["status"], string> = {
  connected: "Conectado",
  degraded: "Lento",
  reconnecting: "Reconectando",
  disconnected: "Desconectado",
  loading: "Cargando modelo",
};

function SourceChip({
  source,
  status,
  metrics,
}: {
  source: SegmentSource;
  status?: ConnectionState;
  metrics?: AudioMetrics;
}) {
  const st = status?.status ?? "reconnecting";
  return (
    <div
      className="flex items-center gap-1.5 rounded-md bg-surface-2/70 px-1.5 py-0.5 ring-1 ring-line/10"
      title={`${source === "mic" ? "Micrófono" : "Audio del sistema"} · ${STATUS_LABEL[st]}${metrics?.deviceLabel ? ` · ${metrics.deviceLabel}` : ""}${status?.message ? ` · ${status.message}` : ""}`}
    >
      <span className="text-fg-muted">
        {source === "mic" ? <IconMic width={13} height={13} /> : <IconHeadphones width={13} height={13} />}
      </span>
      <LevelMeter rms={metrics?.rms ?? 0} />
      <StatusDot status={st} />
    </div>
  );
}

export function LiveView() {
  const s = useSessionStore();
  const navigate = useUiStore((n) => n.navigate);
  const chatOpen = useSettingsStore((x) => x.liveChatOpen);
  const setSettings = useSettingsStore((x) => x.set);
  const engines = useEnginesStore((x) => x.engines);
  const server = useEnginesStore((x) => x.server);
  const partials = useMemo(
    () => Object.values(s.partials).filter((p): p is NonNullable<typeof p> => !!p),
    [s.partials],
  );

  if (!s.sessionId || !s.config) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-sm text-fg-muted">
        No hay ninguna sesión en curso.
        <Button variant="primary" onClick={() => navigate("new")}>
          Nueva sesión
        </Button>
      </div>
    );
  }

  const recordOnly = s.config.engine === null;
  const engineInfo = engines.find((e) => e.id === s.config!.engine);
  const elapsed = livePositionMs(s.metrics);
  const loadingModel = s.config.engine === "whisper" && server?.status === "starting";
  const worst = Object.values(s.status).find((st) => st && st.status !== "connected");

  return (
    <div className="flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-line/10 px-3 py-1.5">
          <span className={cn("h-2 w-2 shrink-0 rounded-full", s.paused ? "bg-amber-400" : "animate-pulse bg-rose-500")} />
          <span className="text-xs font-semibold text-fg">{s.paused ? "En pausa" : "Grabando"}</span>
          <span className="font-mono text-xs tabular-nums text-fg-muted">{fmtMs(elapsed)}</span>
          <span className="hidden text-[11px] text-fg-muted sm:inline">
            {recordOnly
              ? "· Solo grabación"
              : `· ${engineInfo?.label ?? s.config.engine}${
                  s.config.model && s.config.engine === "whisper" ? ` (${s.config.model})` : ""
                }`}
          </span>
          {loadingModel && <span className="text-[11px] text-amber-500">· cargando modelo…</span>}
          {worst?.message && !loadingModel && (
            <span className="truncate text-[11px] text-amber-500" title={worst.message}>
              · {worst.message}
            </span>
          )}
          <div className="ml-auto flex items-center gap-1.5">
            {s.config.sources.map((src) => (
              <SourceChip key={src} source={src} status={s.status[src]} metrics={s.metrics[src]} />
            ))}
            {s.paused ? (
              <IconButton title="Reanudar" onClick={() => void resumeLive()}>
                <IconPlay width={15} height={15} />
              </IconButton>
            ) : (
              <IconButton title="Pausar" onClick={() => void pauseLive()}>
                <IconPause width={15} height={15} />
              </IconButton>
            )}
            <Button size="sm" variant="danger" disabled={s.stopping} onClick={() => void stopLive()}>
              <IconStop width={13} height={13} /> {s.stopping ? "Cerrando…" : "Detener"}
            </Button>
            {!recordOnly && (
              <IconButton
                title={chatOpen ? "Ocultar chat de IA" : "Preguntar a la IA"}
                onClick={() => setSettings({ liveChatOpen: !chatOpen })}
                className={cn(chatOpen && "bg-indigo-600/15 text-indigo-400")}
              >
                <IconSparkle width={15} height={15} />
              </IconButton>
            )}
          </div>
        </header>

        {s.error && (
          <div className="mx-3 mt-2 rounded-lg bg-rose-500/10 px-3 py-1 text-xs text-rose-400 ring-1 ring-rose-500/30">
            {s.error}
          </div>
        )}

        <div className="min-h-0 flex-1">
          {recordOnly ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center">
              <span className="flex h-14 w-14 items-center justify-center rounded-full bg-rose-500/10 text-rose-400 ring-1 ring-rose-500/30">
                <IconMic width={26} height={26} />
              </span>
              <p className="text-sm font-medium text-fg">Grabando audio, sin transcribir</p>
              <p className="max-w-sm text-[11px] text-fg-muted">
                Al detener se guarda el audio y la sesión queda en el historial como
                <b> Sin transcribir</b>. Podrás transcribirla con el motor que quieras.
              </p>
              <div className="font-mono text-2xl tabular-nums text-fg-muted">{fmtMs(elapsed)}</div>
            </div>
          ) : (
          <TranscriptFeed
            segments={s.segments}
            partials={partials}
            emptyHint={
              loadingModel
                ? "Cargando el modelo local…"
                : "Escuchando… la transcripción aparecerá aquí en cuanto haya voz."
            }
          />
          )}
        </div>
      </div>

      {chatOpen && !recordOnly && (
        <aside className="w-80 shrink-0 border-l border-line/10">
          <ChatPanel sessionId={s.sessionId} segments={s.segments} live />
        </aside>
      )}
    </div>
  );
}
