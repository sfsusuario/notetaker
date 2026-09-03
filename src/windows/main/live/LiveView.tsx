import { useMemo } from "react";
import { pauseLive, resumeLive, stopLive } from "../../../app/actions";
import { fmtMs } from "../../../app/format";
import { ChatPanel } from "../../../components/chat/ChatPanel";
import {
  IconHeadphones,
  IconMic,
  IconPanel,
  IconPause,
  IconPlay,
  IconStop,
} from "../../../components/common/icons";
import { Badge, Button, IconButton, StatusDot, cn } from "../../../components/common/ui";
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
    <div className="h-1.5 w-20 overflow-hidden rounded-full bg-line/10">
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
      className="flex items-center gap-2 rounded-lg bg-surface-2/70 px-2 py-1 ring-1 ring-line/10"
      title={`${metrics?.deviceLabel ?? ""}${status?.message ? ` · ${status.message}` : ""}`}
    >
      <span className="text-fg-muted">
        {source === "mic" ? <IconMic width={14} height={14} /> : <IconHeadphones width={14} height={14} />}
      </span>
      <span className="text-[11px] text-fg">{source === "mic" ? "Yo" : "Sistema"}</span>
      <LevelMeter rms={metrics?.rms ?? 0} />
      <StatusDot status={st} />
      <span className="text-[10px] text-fg-muted">{STATUS_LABEL[st]}</span>
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

  const engineInfo = engines.find((e) => e.id === s.config!.engine);
  const elapsed = livePositionMs(s.metrics);
  const loadingModel = s.config.engine === "whisper" && server?.status === "starting";

  return (
    <div className="flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex flex-wrap items-center gap-3 border-b border-line/10 px-4 py-2.5">
          <div className="flex items-center gap-2">
            <span className={cn("h-2.5 w-2.5 rounded-full", s.paused ? "bg-amber-400" : "animate-pulse bg-rose-500")} />
            <span className="text-sm font-semibold text-fg">{s.paused ? "En pausa" : "Grabando"}</span>
            <span className="font-mono text-sm tabular-nums text-fg-muted">{fmtMs(elapsed)}</span>
          </div>
          <Badge tone="info">{engineInfo?.label ?? s.config.engine}{s.config.model ? ` · ${s.config.model}` : ""}</Badge>
          {loadingModel && <Badge tone="warn">Cargando modelo…</Badge>}
          <div className="flex flex-wrap items-center gap-2">
            {s.config.sources.map((src) => (
              <SourceChip key={src} source={src} status={s.status[src]} metrics={s.metrics[src]} />
            ))}
          </div>
          <div className="ml-auto flex items-center gap-1.5">
            {s.paused ? (
              <Button size="sm" onClick={() => void resumeLive()}>
                <IconPlay width={14} height={14} /> Reanudar
              </Button>
            ) : (
              <Button size="sm" onClick={() => void pauseLive()}>
                <IconPause width={14} height={14} /> Pausar
              </Button>
            )}
            <Button size="sm" variant="danger" disabled={s.stopping} onClick={() => void stopLive()}>
              <IconStop width={14} height={14} /> {s.stopping ? "Cerrando…" : "Detener"}
            </Button>
            <IconButton
              title={chatOpen ? "Ocultar chat" : "Mostrar chat"}
              onClick={() => setSettings({ liveChatOpen: !chatOpen })}
              className={cn(chatOpen && "text-indigo-400")}
            >
              <IconPanel width={16} height={16} />
            </IconButton>
          </div>
        </header>

        {s.error && (
          <div className="mx-4 mt-2 rounded-lg bg-rose-500/10 px-3 py-1.5 text-xs text-rose-400 ring-1 ring-rose-500/30">
            {s.error}
          </div>
        )}

        <div className="min-h-0 flex-1">
          <TranscriptFeed
            segments={s.segments}
            partials={partials}
            emptyHint={
              loadingModel
                ? "Cargando el modelo local…"
                : "Escuchando… la transcripción aparecerá aquí en cuanto haya voz."
            }
          />
        </div>
      </div>

      {chatOpen && (
        <aside className="w-[360px] shrink-0 border-l border-line/10">
          <ChatPanel sessionId={s.sessionId} segments={s.segments} live />
        </aside>
      )}
    </div>
  );
}
