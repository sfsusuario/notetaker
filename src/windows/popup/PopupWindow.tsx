import { useEffect, useState } from "react";
import { IconRecord, IconX } from "../../components/common/icons";
import { Badge, Button, Dropdown, cn } from "../../components/common/ui";
import {
  enginesList,
  meetingCurrent,
  onMeetingDetected,
  onMeetingEnded,
  popupAction,
} from "../../services/ipc/native";
import { applyTheme, useSettingsStore } from "../../stores/useSettingsStore";
import {
  LANGUAGES,
  type AudioSource,
  type EngineId,
  type EngineInfo,
  type MeetingInfo,
  type QuickStartConfig,
} from "../../types";

const APP_COLORS: Record<MeetingInfo["app"], string> = {
  teams: "bg-violet-600",
  zoom: "bg-sky-600",
  meet: "bg-emerald-600",
};

export function PopupWindow() {
  const settings = useSettingsStore();
  const [meeting, setMeeting] = useState<MeetingInfo | null>(null);
  const [engines, setEngines] = useState<EngineInfo[]>([]);
  // "none" = solo grabar; útil si falta la API key o el modelo local.
  const [engine, setEngine] = useState<EngineId | "none">(settings.defaultEngine);
  const [model, setModel] = useState<string | null>(null);
  const [sources, setSources] = useState<AudioSource[]>(settings.defaultSources);
  const [language, setLanguage] = useState(settings.language);

  // localStorage es compartido con la ventana principal pero no reactivo:
  // se rehidrata cada vez que aparece una reunión.
  const rehydrate = async () => {
    await useSettingsStore.persist.rehydrate();
    const s = useSettingsStore.getState();
    applyTheme(s.theme);
    setEngine(s.defaultEngine);
    setSources(s.defaultSources);
    setLanguage(s.language);
    try {
      const list = await enginesList();
      setEngines(list);
      const info = list.find((e) => e.id === s.defaultEngine);
      const installed = info?.models.filter((m) => m.installed) ?? [];
      setModel(
        s.defaultEngine === "whisper"
          ? (installed.find((m) => m.id === s.whisperModel)?.id ?? installed[0]?.id ?? null)
          : "nova-3",
      );
    } catch {
      /* ignore */
    }
  };

  useEffect(() => {
    let unl: Array<() => void> = [];
    void (async () => {
      await rehydrate();
      setMeeting(await meetingCurrent().catch(() => null));
      unl.push(
        await onMeetingDetected((m) => {
          setMeeting(m);
          void rehydrate();
        }),
      );
      unl.push(await onMeetingEnded(() => setMeeting(null)));
    })();
    return () => unl.forEach((u) => u());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const recordOnly = engine === "none";
  const info = engines.find((e) => e.id === engine);
  const changeEngine = (id: string) => {
    if (id === "none") {
      setEngine("none");
      setModel(null);
      return;
    }
    const e = id as EngineId;
    setEngine(e);
    const inf = engines.find((x) => x.id === e);
    const installed = inf?.models.filter((m) => m.installed) ?? [];
    setModel(e === "whisper" ? (installed[0]?.id ?? null) : "nova-3");
  };

  const start = () => {
    const config: QuickStartConfig = {
      engine: recordOnly ? null : (engine as EngineId),
      model,
      sources,
      language,
    };
    void popupAction("start", config);
  };

  const srcId = sources.includes("mic") && sources.includes("system") ? "both" : sources[0] ?? "both";

  return (
    <div className="flex h-full flex-col bg-surface text-fg ring-1 ring-line/15">
      <div data-tauri-drag-region className="flex items-center gap-2 px-3 py-2">
        <span className={cn("flex h-6 w-6 items-center justify-center rounded-lg text-white", meeting ? APP_COLORS[meeting.app] : "bg-indigo-600")}>
          <IconRecord width={13} height={13} />
        </span>
        <div data-tauri-drag-region className="min-w-0 flex-1">
          <div data-tauri-drag-region className="text-xs font-semibold">
            Reunión detectada{meeting ? ` en ${meeting.appLabel}` : ""}
          </div>
          <div data-tauri-drag-region className="truncate text-[10px] text-fg-muted" title={meeting?.title}>
            {meeting?.title ?? "¿Quieres transcribirla?"}
          </div>
        </div>
        <button type="button" onClick={() => void popupAction("hide")} className="rounded-md p-1 text-fg-muted hover:bg-line/10 hover:text-fg" title="Cerrar">
          <IconX width={14} height={14} />
        </button>
      </div>

      <div className="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1.5 px-3 text-[11px]">
        <span className="text-fg-muted">Motor</span>
        <div className="flex items-center gap-2">
          <Dropdown
            value={engine}
            options={[
              ...engines.map((e) => ({ value: e.id as string, label: e.label })),
              { value: "none", label: "Solo grabar (transcribir después)" },
            ]}
            onSelect={changeEngine}
            className="min-w-0 flex-1"
          />
          {recordOnly ? (
            <Badge tone="info">Sin IA</Badge>
          ) : (
            info && (info.diarization ? <Badge tone="ok">Hablantes</Badge> : <Badge tone="warn">Yo/Otros</Badge>)
          )}
        </div>
        <span className="text-fg-muted">Audio</span>
        <div className="flex gap-1">
          {(
            [
              ["mic", "Micrófono"],
              ["system", "Sistema"],
              ["both", "Ambos"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => setSources(id === "both" ? ["mic", "system"] : [id])}
              className={cn(
                "flex-1 rounded-lg px-2 py-1 ring-1 transition-colors",
                srcId === id ? "bg-indigo-600/20 text-indigo-400 ring-indigo-500/50" : "bg-surface-2 text-fg-muted ring-line/10 hover:text-fg",
              )}
            >
              {label}
            </button>
          ))}
        </div>
        {!recordOnly && (
          <>
            <span className="text-fg-muted">Idioma</span>
            <Dropdown value={language} options={LANGUAGES.map(([v, l]) => ({ value: v, label: l }))} onSelect={setLanguage} className="min-w-0" />
          </>
        )}
      </div>

      <div className="mt-auto flex items-center justify-between gap-2 px-3 py-2.5">
        <span className={cn("text-[10px]", recordOnly || info?.ready ? "text-fg-muted" : "text-amber-500")}>
          {recordOnly
            ? "Solo se guarda el audio"
            : info
              ? info.ready
                ? info.readiness
                : `⚠ ${info.readiness}`
              : ""}
        </span>
        <div className="flex gap-1.5">
          <Button size="sm" onClick={() => void popupAction("ignore")}>Ignorar</Button>
          <Button
            size="sm"
            variant="primary"
            disabled={(!recordOnly && !info?.ready) || sources.length === 0}
            onClick={start}
          >
            <IconRecord width={13} height={13} /> {recordOnly ? "Grabar" : "Transcribir"}
          </Button>
        </div>
      </div>
    </div>
  );
}
