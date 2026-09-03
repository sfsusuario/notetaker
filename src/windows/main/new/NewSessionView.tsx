import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { startFromFile, startLive } from "../../../app/actions";
import {
  IconFileAudio,
  IconHeadphones,
  IconMic,
  IconRecord,
  IconRefresh,
} from "../../../components/common/icons";
import { Button, Card, Dropdown, cn } from "../../../components/common/ui";
import { audioListDevices } from "../../../services/ipc/native";
import { useEnginesStore } from "../../../stores/useEnginesStore";
import { useSessionStore } from "../../../stores/useSessionStore";
import { useSettingsStore } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import { LANGUAGES, type AudioSource, type DeviceInfo, type EngineId } from "../../../types";
import { EnginePicker, defaultModelFor } from "./EnginePicker";

type Mode = "live" | "file";

export function SourcePicker({
  sources,
  onChange,
  micDeviceId,
  systemDeviceId,
  onDevices,
  compact,
}: {
  sources: AudioSource[];
  onChange: (s: AudioSource[]) => void;
  micDeviceId?: string | null;
  systemDeviceId?: string | null;
  onDevices?: (mic: string | null, system: string | null) => void;
  compact?: boolean;
}) {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const load = () => void audioListDevices().then(setDevices).catch(() => {});
  useEffect(() => {
    if (!compact) load();
  }, [compact]);

  const opts: Array<{ id: "mic" | "system" | "both"; label: string; hint: string; icon: React.ReactNode }> = [
    { id: "mic", label: "Micrófono", hint: "Solo mi voz", icon: <IconMic width={16} height={16} /> },
    { id: "system", label: "Sistema", hint: "Solo el audio del equipo", icon: <IconHeadphones width={16} height={16} /> },
    { id: "both", label: "Ambos", hint: "Mi voz y los demás por separado", icon: <IconRecord width={16} height={16} /> },
  ];
  const currentId: "mic" | "system" | "both" =
    sources.includes("mic") && sources.includes("system") ? "both" : sources[0] ?? "both";

  const inputs = devices.filter((d) => d.kind === "input");
  const outputs = devices.filter((d) => d.kind === "output");
  const devOpts = (list: DeviceInfo[]) => [
    { value: "", label: "Predeterminado del sistema" },
    ...list.map((d) => ({ value: d.id, label: d.label })),
  ];

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-3 gap-2">
        {opts.map((o) => (
          <button
            key={o.id}
            type="button"
            onClick={() => onChange(o.id === "both" ? ["mic", "system"] : [o.id])}
            className={cn(
              "flex flex-col items-start gap-1 rounded-xl p-2.5 text-left ring-1 transition-colors",
              currentId === o.id
                ? "bg-indigo-600/10 ring-indigo-500/60"
                : "bg-surface-2/60 ring-line/10 hover:bg-line/10",
            )}
          >
            <span className={cn("flex items-center gap-1.5 text-sm font-medium text-fg", currentId === o.id && "text-indigo-400")}>
              {o.icon}
              {o.label}
            </span>
            {!compact && <span className="text-[11px] text-fg-muted">{o.hint}</span>}
          </button>
        ))}
      </div>
      {!compact && onDevices && (
        <div className="grid grid-cols-2 gap-2 text-xs">
          {sources.includes("mic") && (
            <div className="flex items-center gap-2">
              <span className="w-16 shrink-0 text-fg-muted">Micrófono</span>
              <Dropdown
                value={micDeviceId ?? ""}
                options={devOpts(inputs)}
                onSelect={(v) => onDevices(v || null, systemDeviceId ?? null)}
                className="min-w-0 flex-1"
              />
            </div>
          )}
          {sources.includes("system") && (
            <div className="flex items-center gap-2">
              <span className="w-16 shrink-0 text-fg-muted">Salida</span>
              <Dropdown
                value={systemDeviceId ?? ""}
                options={devOpts(outputs)}
                onSelect={(v) => onDevices(micDeviceId ?? null, v || null)}
                className="min-w-0 flex-1"
              />
            </div>
          )}
          <button
            type="button"
            onClick={load}
            className="col-span-2 flex items-center gap-1 self-start text-[11px] text-fg-muted hover:text-fg"
          >
            <IconRefresh width={12} height={12} /> Actualizar dispositivos
          </button>
        </div>
      )}
    </div>
  );
}

export function NewSessionView() {
  const settings = useSettingsStore();
  const engines = useEnginesStore((s) => s.engines);
  const liveId = useSessionStore((s) => s.sessionId);
  const navigate = useUiStore((s) => s.navigate);
  const [mode, setMode] = useState<Mode>("live");
  const [engine, setEngine] = useState<EngineId>(settings.defaultEngine);
  const [model, setModel] = useState<string | null>(
    settings.defaultEngine === "whisper" ? settings.whisperModel : "nova-3",
  );
  const [sources, setSources] = useState<AudioSource[]>(settings.defaultSources);
  const [language, setLanguage] = useState(settings.language);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Cuando llegan los motores, asegura un modelo válido para whisper
  useEffect(() => {
    if (engine === "whisper") {
      const info = engines.find((e) => e.id === "whisper");
      setModel((m) => defaultModelFor(info, m ?? settings.whisperModel));
    }
  }, [engines, engine, settings.whisperModel]);

  const info = engines.find((e) => e.id === engine);
  const canStart =
    !!info?.ready && (engine !== "whisper" || !!model) && (mode === "live" ? sources.length > 0 : !!filePath);

  const pickFile = async () => {
    const p = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: ["wav", "mp3", "m4a", "aac", "ogg", "oga", "flac", "mp4"] }],
    });
    if (typeof p === "string") setFilePath(p);
  };

  const start = async () => {
    setBusy(true);
    try {
      if (mode === "live") {
        await startLive({ engine, model, sources, language });
      } else if (filePath) {
        await startFromFile(filePath, { engine, model, language });
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-6">
        <h1 className="text-lg font-semibold text-fg">Nueva sesión</h1>
        <p className="mb-5 text-sm text-fg-muted">
          Transcribe una reunión en curso o un archivo de audio ya grabado.
        </p>

        {liveId && (
          <div className="mb-4 flex items-center justify-between rounded-xl bg-rose-500/10 px-3 py-2 text-xs text-rose-400 ring-1 ring-rose-500/30">
            Ya hay una sesión grabando. Iniciar otra la detendrá.
            <Button size="sm" onClick={() => navigate("live")}>
              Ver sesión
            </Button>
          </div>
        )}

        <div className="mb-4 grid grid-cols-2 gap-2">
          {(
            [
              { id: "live", label: "En vivo", hint: "Micrófono y/o audio del sistema", icon: <IconRecord width={18} height={18} /> },
              { id: "file", label: "Desde grabación", hint: "wav, mp3, m4a, ogg, flac", icon: <IconFileAudio width={18} height={18} /> },
            ] as const
          ).map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => setMode(m.id)}
              className={cn(
                "flex items-center gap-3 rounded-2xl p-4 text-left ring-1 transition-colors",
                mode === m.id ? "bg-indigo-600/10 ring-indigo-500/60" : "bg-surface ring-line/10 hover:bg-line/10",
              )}
            >
              <span className={cn("rounded-xl bg-surface-2 p-2 text-fg-muted", mode === m.id && "text-indigo-400")}>{m.icon}</span>
              <span>
                <span className="block text-sm font-medium text-fg">{m.label}</span>
                <span className="block text-[11px] text-fg-muted">{m.hint}</span>
              </span>
            </button>
          ))}
        </div>

        <div className="space-y-3">
          <Card title="Motor de transcripción">
            <EnginePicker engine={engine} model={model} onChange={(e, m) => { setEngine(e); setModel(m); }} />
          </Card>

          {mode === "live" ? (
            <Card title="Fuentes de audio">
              <SourcePicker
                sources={sources}
                onChange={setSources}
                micDeviceId={settings.micDeviceId}
                systemDeviceId={settings.systemDeviceId}
                onDevices={(mic, sys) => settings.set({ micDeviceId: mic, systemDeviceId: sys })}
              />
              {sources.length === 2 && (
                <p className="mt-2 text-[11px] text-fg-muted">
                  Con ambas fuentes, tu voz se etiqueta como <b>Yo</b> y el audio del equipo como{" "}
                  <b>Otros</b>{info?.diarization ? " (o Hablante 1, 2… si el motor los distingue)" : ""}.
                </p>
              )}
            </Card>
          ) : (
            <Card title="Archivo de audio">
              <div className="flex items-center gap-2">
                <Button onClick={() => void pickFile()}>
                  <IconFileAudio width={15} height={15} /> Elegir archivo…
                </Button>
                <span className="min-w-0 flex-1 truncate text-xs text-fg-muted" title={filePath ?? ""}>
                  {filePath ?? "Ningún archivo seleccionado"}
                </span>
              </div>
              <p className="mt-2 text-[11px] text-fg-muted">
                El audio se decodifica a 16 kHz y se guarda una copia para poder reproducirlo desde el historial.
              </p>
            </Card>
          )}

          <Card title="Idioma">
            <div className="flex items-center gap-3">
              <Dropdown
                value={language}
                options={LANGUAGES.map(([v, l]) => ({ value: v, label: l }))}
                onSelect={setLanguage}
                className="w-64"
              />
              <span className="text-[11px] text-fg-muted">
                "Automático" detecta el idioma (y mezclas) por sí solo.
              </span>
            </div>
          </Card>

          <div className="flex items-center justify-end gap-3 pt-2">
            {!info?.ready && info && (
              <span className="text-xs text-amber-500">{info.readiness}</span>
            )}
            <Button size="lg" variant="primary" disabled={!canStart || busy} onClick={() => void start()}>
              {mode === "live" ? <IconRecord width={16} height={16} /> : <IconFileAudio width={16} height={16} />}
              {busy ? "Iniciando…" : mode === "live" ? "Iniciar transcripción" : "Transcribir archivo"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
