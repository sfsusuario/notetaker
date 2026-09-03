import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { startFromFile, startLive } from "../../../app/actions";
import {
  IconChip,
  IconCloud,
  IconFileAudio,
  IconGlobe,
  IconHeadphones,
  IconMic,
  IconRecord,
  IconRefresh,
  IconUsers,
} from "../../../components/common/icons";
import { Badge, Button, Collapsible, Dropdown, cn } from "../../../components/common/ui";
import { audioListDevices } from "../../../services/ipc/native";
import { useEnginesStore } from "../../../stores/useEnginesStore";
import { useSessionStore } from "../../../stores/useSessionStore";
import { useSettingsStore } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import { LANGUAGES, type AudioSource, type DeviceInfo, type EngineId } from "../../../types";
import { EnginePicker, defaultModelFor } from "./EnginePicker";

/** live = graba y transcribe · record = solo graba · file = archivo existente */
type Mode = "live" | "record" | "file";
type Section = "engine" | "sources" | "language";

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
    { id: "mic", label: "Micrófono", hint: "Solo mi voz", icon: <IconMic width={15} height={15} /> },
    { id: "system", label: "Sistema", hint: "Solo el audio del equipo", icon: <IconHeadphones width={15} height={15} /> },
    { id: "both", label: "Ambos", hint: "Mi voz y los demás por separado", icon: <IconRecord width={15} height={15} /> },
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
              "flex flex-col items-start gap-0.5 rounded-xl px-2.5 py-2 text-left ring-1 transition-colors",
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

function sourcesLabel(sources: AudioSource[]): string {
  if (sources.includes("mic") && sources.includes("system")) return "Micrófono + sistema";
  if (sources[0] === "mic") return "Solo micrófono";
  if (sources[0] === "system") return "Solo sistema";
  return "Sin fuente";
}

function languageLabel(code: string): string {
  return LANGUAGES.find(([v]) => v === code)?.[1] ?? code;
}

/** Chip resumen: al pulsarlo abre la sección correspondiente. */
function SummaryChip({
  icon,
  label,
  onClick,
  tone,
}: {
  icon: React.ReactNode;
  label: React.ReactNode;
  onClick: () => void;
  tone?: "ok" | "warn";
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-lg bg-surface-2/70 px-2 py-1 text-[11px] text-fg ring-1 ring-line/10 transition-colors hover:bg-line/10",
        tone === "warn" && "text-amber-500 ring-amber-500/30",
      )}
      title="Cambiar"
    >
      <span className="text-fg-muted">{icon}</span>
      <span className="truncate">{label}</span>
    </button>
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
  const [openSections, setOpenSections] = useState<Record<Section, boolean>>({
    engine: false,
    sources: false,
    language: false,
  });
  const toggle = (k: Section, v?: boolean) =>
    setOpenSections((o) => ({ ...o, [k]: v ?? !o[k] }));

  useEffect(() => {
    if (engine === "whisper") {
      const info = engines.find((e) => e.id === "whisper");
      setModel((m) => defaultModelFor(info, m ?? settings.whisperModel));
    }
  }, [engines, engine, settings.whisperModel]);

  const info = engines.find((e) => e.id === engine);
  const needsEngine = mode !== "record";
  const canStart =
    (!needsEngine || (!!info?.ready && (engine !== "whisper" || !!model))) &&
    (mode === "file" ? !!filePath : sources.length > 0);

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
      if (mode === "file") {
        if (filePath) await startFromFile(filePath, { engine, model, language });
      } else {
        // record: engine null → el backend solo escribe el WAV
        await startLive({ engine: mode === "record" ? null : engine, model, sources, language });
      }
    } finally {
      setBusy(false);
    }
  };

  const engineLabel = info ? `${info.label}${engine === "whisper" && model ? ` · ${model}` : ""}` : engine;
  const startLabel =
    mode === "live" ? "Iniciar" : mode === "record" ? "Grabar" : "Transcribir";

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl space-y-2.5 px-5 py-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h1 className="text-base font-semibold text-fg">Nueva sesión</h1>
            <p className="text-[11px] text-fg-muted">Elige el modo, revisa el resumen y arranca. Despliega solo lo que quieras cambiar.</p>
          </div>
          <div className="flex shrink-0 rounded-lg bg-surface-2 p-0.5 ring-1 ring-line/10">
            {(
              [
                { id: "live", label: "En vivo", icon: <IconRecord width={13} height={13} />, hint: "Graba y transcribe a la vez" },
                { id: "record", label: "Solo grabar", icon: <IconMic width={13} height={13} />, hint: "Guarda el audio; transcríbelo después" },
                { id: "file", label: "Desde grabación", icon: <IconFileAudio width={13} height={13} />, hint: "Transcribe un archivo de audio" },
              ] as const
            ).map((m) => (
              <button
                key={m.id}
                type="button"
                title={m.hint}
                onClick={() => setMode(m.id)}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs transition-colors",
                  mode === m.id ? "bg-indigo-600 text-white" : "text-fg-muted hover:text-fg",
                )}
              >
                {m.icon}
                {m.label}
              </button>
            ))}
          </div>
        </div>

        {liveId && (
          <div className="flex items-center justify-between rounded-xl bg-rose-500/10 px-3 py-1.5 text-xs text-rose-400 ring-1 ring-rose-500/30">
            Ya hay una sesión grabando. Iniciar otra la detendrá.
            <Button size="sm" onClick={() => navigate("live")}>
              Ver sesión
            </Button>
          </div>
        )}

        {/* Tarjeta de inicio: resumen + botón */}
        <section className="rounded-2xl bg-surface p-3 ring-1 ring-indigo-500/30">
          <div className="flex items-center gap-3">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
              {needsEngine ? (
                <>
                  <SummaryChip
                    icon={engine === "whisper" ? <IconChip width={13} height={13} /> : <IconCloud width={13} height={13} />}
                    label={engineLabel}
                    onClick={() => toggle("engine", true)}
                    tone={info && !info.ready ? "warn" : undefined}
                  />
                  {info && (
                    info.diarization ? (
                      <Badge tone="ok"><IconUsers width={10} height={10} /> Hablantes</Badge>
                    ) : (
                      <Badge tone="warn"><IconUsers width={10} height={10} /> Yo / Otros</Badge>
                    )
                  )}
                </>
              ) : (
                <Badge tone="info">Sin transcripción</Badge>
              )}
              {mode === "file" ? (
                <SummaryChip
                  icon={<IconFileAudio width={13} height={13} />}
                  label={filePath ? (filePath.split(/[\\/]/).pop() ?? filePath) : "Elegir archivo…"}
                  onClick={() => void pickFile()}
                  tone={filePath ? undefined : "warn"}
                />
              ) : (
                <SummaryChip
                  icon={<IconHeadphones width={13} height={13} />}
                  label={sourcesLabel(sources)}
                  onClick={() => toggle("sources", true)}
                />
              )}
              {needsEngine && (
                <SummaryChip
                  icon={<IconGlobe width={13} height={13} />}
                  label={languageLabel(language)}
                  onClick={() => toggle("language", true)}
                />
              )}
            </div>
            <Button size="lg" variant="primary" disabled={!canStart || busy} onClick={() => void start()} className="shrink-0">
              {mode === "file" ? <IconFileAudio width={16} height={16} /> : <IconRecord width={16} height={16} />}
              {busy ? "Iniciando…" : startLabel}
            </Button>
          </div>
          {mode === "record" ? (
            <p className="mt-2 text-[11px] text-fg-muted">
              Solo se guarda el audio, sin coste ni conexión. Al terminar, la sesión queda como{" "}
              <b>Sin transcribir</b> y puedes transcribirla cuando quieras con el motor que prefieras.
            </p>
          ) : (
            <>
              {info && !info.ready && (
                <p className="mt-2 text-[11px] text-amber-500">
                  ⚠ {info.readiness}.{" "}
                  <button type="button" className="underline hover:text-fg" onClick={() => navigate("settings")}>
                    Ir a Ajustes
                  </button>
                  {" · "}
                  <button type="button" className="underline hover:text-fg" onClick={() => setMode("record")}>
                    o graba ahora y transcribe después
                  </button>
                </p>
              )}
              {mode === "live" && sources.length === 2 && info && (
                <p className="mt-2 text-[11px] text-fg-muted">
                  Tu voz se etiqueta como <b>Yo</b> y el audio del equipo como <b>Otros</b>
                  {info.diarization ? " (o Hablante 1, 2… cuando el motor los distingue)" : ""}.
                </p>
              )}
            </>
          )}
        </section>

        {needsEngine && (
          <Collapsible
            title="Motor"
            summary={engineLabel}
            open={openSections.engine}
            onToggle={(v) => toggle("engine", v)}
          >
            <EnginePicker engine={engine} model={model} onChange={(e, m) => { setEngine(e); setModel(m); }} compact />
          </Collapsible>
        )}

        {mode === "file" ? (
          <Collapsible title="Archivo" summary={filePath ?? "Ninguno"} defaultOpen>
            <div className="flex items-center gap-2">
              <Button size="sm" onClick={() => void pickFile()}>
                <IconFileAudio width={14} height={14} /> Elegir archivo…
              </Button>
              <span className="min-w-0 flex-1 truncate text-xs text-fg-muted" title={filePath ?? ""}>
                {filePath ?? "wav, mp3, m4a, ogg, flac"}
              </span>
            </div>
            <p className="mt-2 text-[11px] text-fg-muted">
              Se decodifica a 16 kHz y se guarda una copia para reproducirlo desde el historial.
            </p>
          </Collapsible>
        ) : (
          <Collapsible
            title="Fuentes de audio"
            summary={sourcesLabel(sources)}
            open={openSections.sources}
            onToggle={(v) => toggle("sources", v)}
          >
            <SourcePicker
              sources={sources}
              onChange={setSources}
              micDeviceId={settings.micDeviceId}
              systemDeviceId={settings.systemDeviceId}
              onDevices={(mic, sys) => settings.set({ micDeviceId: mic, systemDeviceId: sys })}
            />
          </Collapsible>
        )}

        {needsEngine && (
          <Collapsible
            title="Idioma"
            summary={languageLabel(language)}
            open={openSections.language}
            onToggle={(v) => toggle("language", v)}
          >
            <div className="flex items-center gap-3">
              <Dropdown
                value={language}
                options={LANGUAGES.map(([v, l]) => ({ value: v, label: l }))}
                onSelect={setLanguage}
                className="w-56"
              />
              <span className="text-[11px] text-fg-muted">"Automático" detecta el idioma y las mezclas por sí solo.</span>
            </div>
          </Collapsible>
        )}
      </div>
    </div>
  );
}
