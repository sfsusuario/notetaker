import { useEffect, useState } from "react";
import { syncAutoStopToBackend, syncDetectionToBackend } from "../../../app/actions";
import { fmtBytes } from "../../../app/format";
import { IconDownload, IconFolder, IconTrash } from "../../../components/common/icons";
import {
  Badge,
  Button,
  Collapsible,
  Dropdown,
  Field,
  Input,
  Spinner,
  Toggle,
  cn,
} from "../../../components/common/ui";
import {
  dataDir,
  openPath,
  secretDelete,
  secretGet,
  secretSet,
  whisperDeleteModel,
  whisperInstall,
  whisperStopServer,
  type SecretName,
} from "../../../services/ipc/native";
import { testProvider } from "../../../services/llm/chatClient";
import { PROVIDERS, PROVIDER_LABELS } from "../../../services/llm/providers";
import { useEnginesStore } from "../../../stores/useEnginesStore";
import { applyTheme, useSettingsStore, type ExportFormat, type Theme } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import { LANGUAGES, type EngineId, type LlmProviderId } from "../../../types";
import { SourcePicker } from "../new/NewSessionView";

function KeyField({ name, label }: { name: SecretName; label: string }) {
  const [value, setValue] = useState("");
  const [stored, setStored] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const refresh = useEnginesStore((s) => s.refresh);

  useEffect(() => {
    void secretGet(name).then((v) => setStored(v !== null && v !== ""));
  }, [name]);

  const saveKey = async () => {
    if (!value.trim()) return;
    await secretSet(name, value.trim());
    setStored(true);
    setValue("");
    setStatus("Guardada en el llavero del sistema.");
    void refresh();
  };
  const remove = async () => {
    await secretDelete(name);
    setStored(false);
    setStatus("Eliminada.");
    void refresh();
  };

  return (
    <div className="space-y-1 py-1.5">
      <label className="text-xs text-fg-muted">
        {label} {stored && <span className="text-emerald-500">(configurada)</span>}
      </label>
      <div className="flex gap-1.5">
        <Input
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={stored ? "••••••••  (reemplazar)" : "Pega la API key"}
          className="flex-1"
          onKeyDown={(e) => e.key === "Enter" && void saveKey()}
        />
        <Button size="sm" variant="primary" onClick={() => void saveKey()} disabled={!value.trim()}>
          Guardar
        </Button>
        {stored && (
          <Button size="sm" variant="danger" onClick={() => void remove()}>
            Borrar
          </Button>
        )}
      </div>
      {status && <p className="text-[11px] text-fg-muted">{status}</p>}
    </div>
  );
}

function ModelPicker() {
  const llm = useSettingsStore((s) => s.llm);
  const setLlm = useSettingsStore((s) => s.setLlm);
  const provider = PROVIDERS[llm.provider];
  const [fetched, setFetched] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [custom, setCustom] = useState(false);

  useEffect(() => {
    setFetched(null);
    setErr(null);
    setCustom(false);
  }, [llm.provider]);

  const load = async () => {
    setLoading(true);
    setErr(null);
    try {
      const list = await provider.listModels(llm.baseUrl);
      setFetched(list);
      if (list.length > 0 && !list.includes(llm.model)) setLlm({ model: list[0] });
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const options = Array.from(new Set([...(fetched ?? provider.models), llm.model].filter(Boolean)));

  return (
    <div className="space-y-1 py-1.5">
      <div className="flex items-center gap-1.5">
        <span className="w-16 text-xs text-fg-muted">Modelo</span>
        {custom ? (
          <Input value={llm.model} onChange={(e) => setLlm({ model: e.target.value })} placeholder="id del modelo" className="flex-1" />
        ) : (
          <Dropdown value={llm.model} options={options} onSelect={(model) => setLlm({ model })} className="min-w-0 flex-1" />
        )}
        <Button size="sm" onClick={() => void load()} disabled={loading} title="Cargar la lista oficial de modelos del proveedor">
          {loading ? <Spinner /> : "↻ Lista oficial"}
        </Button>
        <Button size="sm" onClick={() => setCustom((v) => !v)} title="Escribir un id manualmente">
          {custom ? "Lista" : "Otro"}
        </Button>
      </div>
      {err && <p className="pl-16 text-[11px] text-rose-400">{err}</p>}
      {fetched && !err && <p className="pl-16 text-[10px] text-fg-muted">{fetched.length} modelos disponibles en la API</p>}
    </div>
  );
}

function LlmSection() {
  const llm = useSettingsStore((s) => s.llm);
  const setLlm = useSettingsStore((s) => s.setLlm);
  const [status, setStatus] = useState<string | null>(null);
  const provider = PROVIDERS[llm.provider];

  const test = async () => {
    setStatus("Probando…");
    try {
      setStatus(`✓ OK — ${await testProvider()}`);
    } catch (e) {
      setStatus(`✗ ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <Collapsible
      title="Proveedor de IA"
      summary={`${PROVIDER_LABELS[llm.provider]} · ${llm.model}`}
      defaultOpen={false}
    >
      <div className="flex flex-wrap gap-1">
        {(Object.keys(PROVIDER_LABELS) as LlmProviderId[]).map((id) => (
          <button
            key={id}
            type="button"
            onClick={() => { setLlm({ provider: id, model: PROVIDERS[id].models[0] }); setStatus(null); }}
            className={cn(
              "rounded-full px-2.5 py-1 text-xs ring-1 transition-colors",
              llm.provider === id ? "bg-indigo-600/20 text-indigo-400 ring-indigo-500/50" : "text-fg-muted ring-line/10 hover:text-fg",
            )}
          >
            {PROVIDER_LABELS[id]}
          </button>
        ))}
      </div>
      <ModelPicker />
      {llm.provider === "ollama" && (
        <Field label="URL de Ollama">
          <Input value={llm.baseUrl ?? "http://localhost:11434"} onChange={(e) => setLlm({ baseUrl: e.target.value })} className="w-64" />
        </Field>
      )}
      {provider.secretName && <KeyField name={provider.secretName} label={`API key — ${PROVIDER_LABELS[llm.provider]}`} />}
      <div className="flex items-center gap-2 pt-1">
        <Button size="sm" onClick={() => void test()}>Probar conexión</Button>
        {status && <span className="text-[11px] text-fg-muted">{status}</span>}
      </div>
    </Collapsible>
  );
}

function WhisperSection() {
  const whisper = useEnginesStore((s) => s.whisper);
  const downloads = useEnginesStore((s) => s.downloads);
  const server = useEnginesStore((s) => s.server);
  const refresh = useEnginesStore((s) => s.refresh);
  const settings = useSettingsStore();
  const toast = useUiStore((u) => u.toast);
  const [busy, setBusy] = useState<string | null>(null);

  const install = async (model: string | null, includeServer: boolean) => {
    setBusy(model ?? "server");
    try {
      await whisperInstall({ model, includeServer });
      toast(model ? `Modelo ${model} listo` : "Servidor whisper instalado", "ok");
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      setBusy(null);
      void refresh();
    }
  };
  const del = async (model: string) => {
    try {
      await whisperDeleteModel(model);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
    }
    void refresh();
  };

  const serverDl = downloads["server"];
  return (
    <Collapsible
      title="Whisper local"
      summary={
        whisper
          ? whisper.serverInstalled
            ? `Servidor instalado · modelos: ${whisper.models.filter((m) => m.installed).map((m) => m.id).join(", ") || "ninguno"} · por defecto ${settings.whisperModel}`
            : "Servidor no instalado"
          : "…"
      }
      action={
        whisper?.running ? (
          <span className="flex items-center gap-2 text-[11px] text-emerald-500">
            ● Servidor activo · {whisper.running.model}
            <Button size="sm" onClick={() => void whisperStopServer().then(refresh)}>Parar</Button>
          </span>
        ) : server?.status === "starting" ? (
          <span className="text-[11px] text-amber-500">Cargando modelo…</span>
        ) : null
      }
    >
      <Field
        label="Servidor whisper-server.exe"
        hint={
          whisper?.serverInstalled
            ? `Instalado (${whisper.releaseTag})`
            : "Binario oficial precompilado de whisper.cpp (~8 MB, CPU). Sin dependencias adicionales."
        }
      >
        {serverDl && serverDl.phase !== "done" ? (
          <span className="text-[11px] text-fg-muted">
            {serverDl.phase === "extracting" ? "Extrayendo…" : `${serverDl.percent.toFixed(0)} %`}
          </span>
        ) : (
          <Button size="sm" onClick={() => void install(null, true)} disabled={busy !== null}>
            <IconDownload width={13} height={13} /> {whisper?.serverInstalled ? "Reinstalar" : "Instalar"}
          </Button>
        )}
      </Field>

      <div className="mt-2 overflow-hidden rounded-xl ring-1 ring-line/10">
        {(whisper?.models ?? []).map((m) => {
          const dl = downloads[m.id];
          const selected = settings.whisperModel === m.id;
          return (
            <div key={m.id} className="flex items-center gap-3 border-b border-line/10 px-3 py-2 text-xs last:border-b-0">
              <button
                type="button"
                disabled={!m.installed}
                onClick={() => settings.set({ whisperModel: m.id })}
                className={cn(
                  "h-4 w-4 shrink-0 rounded-full ring-1",
                  selected ? "bg-indigo-500 ring-indigo-400" : "ring-line/30",
                  !m.installed && "opacity-30",
                )}
                title="Usar por defecto"
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-fg">{m.id}</span>
                  <span className="text-fg-muted">{fmtBytes(m.sizeMb * 1048576)}</span>
                  {m.installed && <Badge tone="ok">instalado</Badge>}
                </div>
                <div className="text-[11px] text-fg-muted">{m.label}</div>
                {dl && dl.phase !== "done" && (
                  <div className="mt-1 h-1 w-48 overflow-hidden rounded-full bg-line/10">
                    <div className="h-full bg-indigo-500" style={{ width: `${dl.percent}%` }} />
                  </div>
                )}
              </div>
              {m.installed ? (
                <Button size="sm" variant="danger" onClick={() => void del(m.id)} disabled={whisper?.running?.model === m.id}>
                  <IconTrash width={13} height={13} />
                </Button>
              ) : (
                <Button size="sm" onClick={() => void install(m.id, false)} disabled={busy !== null || (dl && dl.phase === "downloading")}>
                  <IconDownload width={13} height={13} /> {dl && dl.phase === "downloading" ? `${dl.percent.toFixed(0)} %` : "Descargar"}
                </Button>
              )}
            </div>
          );
        })}
      </div>

      <Field label="Hilos de CPU" hint="Más hilos = más rápido, hasta el número de núcleos.">
        <Input
          type="number"
          min={1}
          max={32}
          value={settings.whisperThreads}
          onChange={(e) => settings.set({ whisperThreads: Math.max(1, Math.min(32, Number(e.target.value) || 1)) })}
          className="w-20"
        />
      </Field>
      {whisper && (
        <p className="text-[10px] text-fg-muted">
          Carpeta: {whisper.modelsDir}{" "}
          <button type="button" className="underline hover:text-fg" onClick={() => void openPath(whisper.modelsDir).catch(() => {})}>
            abrir
          </button>
          {" · "}También puedes instalar con <code>scripts\setup-whisper.ps1</code>.
        </p>
      )}
    </Collapsible>
  );
}

export function SettingsView() {
  const settings = useSettingsStore();
  const [dir, setDir] = useState<string | null>(null);
  useEffect(() => {
    void dataDir().then(setDir).catch(() => {});
  }, []);

  const setDetection = (patch: { meetingDetection?: boolean; meetingApps?: typeof settings.meetingApps }) => {
    settings.set(patch);
    void syncDetectionToBackend();
  };

  const setAutoStop = (patch: Parameters<typeof settings.set>[0]) => {
    settings.set(patch);
    void syncAutoStopToBackend();
  };

  const autoStopSummary = [
    settings.autoStopOnMeetingEnd && "fin de reunión",
    settings.autoStopOnSilence && `${settings.autoStopSilenceMin} min sin audio`,
    settings.autoStopMaxHours > 0 && `${settings.autoStopMaxHours} h máximo`,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl space-y-2.5 px-5 py-4">
        <h1 className="text-base font-semibold text-fg">Ajustes</h1>
        <p className="-mt-1 text-[11px] text-fg-muted">Despliega la sección que quieras configurar.</p>

        <LlmSection />

        <Collapsible
          title="Transcripción"
          summary={`${settings.defaultEngine === "whisper" ? "Whisper local" : "Deepgram"} · ${settings.defaultSources.length === 2 ? "mic + sistema" : settings.defaultSources[0] === "mic" ? "micrófono" : "sistema"} · ${LANGUAGES.find(([v]) => v === settings.language)?.[1] ?? settings.language}`}
        >
          <KeyField name="deepgram" label="Deepgram API key (nova-3, streaming y archivos)" />
          <Field label="Motor por defecto">
            <Dropdown
              value={settings.defaultEngine}
              options={[{ value: "deepgram", label: "Deepgram (nube)" }, { value: "whisper", label: "Whisper local" }]}
              onSelect={(v) => settings.set({ defaultEngine: v as EngineId })}
              className="w-56"
            />
          </Field>
          <Field label="Idioma por defecto">
            <Dropdown value={settings.language} options={LANGUAGES.map(([v, l]) => ({ value: v, label: l }))} onSelect={(v) => settings.set({ language: v })} className="w-56" />
          </Field>
          <Field label="Fuentes por defecto" stack>
            <SourcePicker
              sources={settings.defaultSources}
              onChange={(s) => settings.set({ defaultSources: s })}
              micDeviceId={settings.micDeviceId}
              systemDeviceId={settings.systemDeviceId}
              onDevices={(mic, sys) => settings.set({ micDeviceId: mic, systemDeviceId: sys })}
            />
          </Field>
          <Field label="Título automático con IA" hint="Genera un título a los ~90 s de reunión o al terminar un archivo. Nunca pisa un título editado a mano.">
            <Toggle checked={settings.autoTitle} onChange={(v) => settings.set({ autoTitle: v })} />
          </Field>
        </Collapsible>

        <WhisperSection />

        <Collapsible
          title="Detener automáticamente"
          summary={autoStopSummary || "Desactivado"}
        >
          <p className="pb-1 text-[11px] text-fg-muted">
            Antes de detener aparece un aviso con cuenta atrás y la opción de seguir grabando.
          </p>
          <Field
            label="Cuando termina la reunión detectada"
            hint={
              settings.meetingDetection
                ? "Usa el detector de Teams, Zoom y Meet. Espera a confirmar que la llamada acabó de verdad."
                : "Requiere la detección de reuniones activada (sección siguiente)."
            }
          >
            <Toggle
              checked={settings.autoStopOnMeetingEnd}
              disabled={!settings.meetingDetection}
              onChange={(v) => setAutoStop({ autoStopOnMeetingEnd: v })}
            />
          </Field>
          <Field label="Cuando no se oye nada" hint="Red de seguridad para reuniones presenciales o apps no detectadas. No cuenta mientras la sesión está en pausa ni durante una reunión en curso.">
            <div className="flex items-center gap-2">
              {settings.autoStopOnSilence && (
                <Dropdown
                  value={String(settings.autoStopSilenceMin)}
                  options={[1, 5, 10, 20, 30].map((m) => ({
                    value: String(m),
                    label: `${m} min`,
                  }))}
                  onSelect={(v) => setAutoStop({ autoStopSilenceMin: Number(v) })}
                  className="w-24"
                />
              )}
              <Toggle
                checked={settings.autoStopOnSilence}
                onChange={(v) => setAutoStop({ autoStopOnSilence: v })}
              />
            </div>
          </Field>
          <Field label="Duración máxima" hint="Tope duro por si fallan las demás señales. Cuenta el tiempo total, pausas incluidas.">
            <Dropdown
              value={String(settings.autoStopMaxHours)}
              options={[
                { value: "0", label: "Sin límite" },
                ...[1, 2, 3, 4, 6, 8].map((h) => ({ value: String(h), label: `${h} h` })),
              ]}
              onSelect={(v) => setAutoStop({ autoStopMaxHours: Number(v) })}
              className="w-32"
            />
          </Field>
          <Field label="Tiempo para cancelar" hint="Cuánto espera el aviso antes de detener si no respondes.">
            <Dropdown
              value={String(settings.autoStopGraceSec)}
              options={[10, 30, 60, 120].map((s) => ({
                value: String(s),
                label: s >= 60 ? `${s / 60} min` : `${s} s`,
              }))}
              onSelect={(v) => setAutoStop({ autoStopGraceSec: Number(v) })}
              className="w-32"
            />
          </Field>
        </Collapsible>

        <Collapsible
          title="Detección de reuniones"
          summary={settings.meetingDetection ? `Activada · ${[settings.meetingApps.teams && "Teams", settings.meetingApps.zoom && "Zoom", settings.meetingApps.meet && "Meet"].filter(Boolean).join(", ") || "ninguna app"}` : "Desactivada"}
        >
          <Field label="Detectar reuniones en curso" hint="Muestra un aviso con inicio rápido cuando Teams, Zoom o Google Meet están en una llamada. Nunca graba sin tu confirmación.">
            <Toggle checked={settings.meetingDetection} onChange={(v) => setDetection({ meetingDetection: v })} />
          </Field>
          <div className={cn("grid grid-cols-3 gap-2", !settings.meetingDetection && "opacity-50")}>
            {(
              [
                ["teams", "Microsoft Teams", "Proceso de Teams usando el micrófono"],
                ["zoom", "Zoom", "Ventana de reunión o micrófono en uso"],
                ["meet", "Google Meet", "Pestaña del navegador con código de reunión"],
              ] as const
            ).map(([k, label, hint]) => (
              <label key={k} className="flex cursor-pointer items-start gap-2 rounded-xl bg-surface-2/60 p-2.5 ring-1 ring-line/10">
                <input
                  type="checkbox"
                  checked={settings.meetingApps[k]}
                  disabled={!settings.meetingDetection}
                  onChange={(e) => setDetection({ meetingApps: { ...settings.meetingApps, [k]: e.target.checked } })}
                  className="mt-0.5 accent-indigo-500"
                />
                <span>
                  <span className="block text-xs font-medium text-fg">{label}</span>
                  <span className="block text-[10px] text-fg-muted">{hint}</span>
                </span>
              </label>
            ))}
          </div>
        </Collapsible>

        <Collapsible
          title="General"
          summary={`Tema ${settings.theme === "dark" ? "oscuro" : settings.theme === "light" ? "claro" : "sistema"} · exportar ${settings.exportFormat}${settings.startMinimized ? " · inicia en bandeja" : ""}`}
        >
          <Field label="Iniciar minimizado en la bandeja">
            <Toggle checked={settings.startMinimized} onChange={(v) => settings.set({ startMinimized: v })} />
          </Field>
          <Field label="Tema">
            <Dropdown
              value={settings.theme}
              options={[{ value: "dark", label: "Oscuro" }, { value: "light", label: "Claro" }, { value: "system", label: "Sistema" }]}
              onSelect={(v) => { settings.set({ theme: v as Theme }); applyTheme(v as Theme); }}
              className="w-40"
            />
          </Field>
          <Field label="Formato de exportación por defecto">
            <Dropdown
              value={settings.exportFormat}
              options={[{ value: "md", label: "Markdown" }, { value: "txt", label: "Texto" }, { value: "json", label: "JSON" }]}
              onSelect={(v) => settings.set({ exportFormat: v as ExportFormat })}
              className="w-40"
            />
          </Field>
          <Field label="Carpeta de datos" hint={dir ?? "…"}>
            <Button size="sm" onClick={() => dir && void openPath(dir)}>
              <IconFolder width={13} height={13} /> Abrir
            </Button>
          </Field>
          <p className="text-[10px] text-fg-muted">
            Las API keys se guardan en el Administrador de credenciales de Windows; las grabaciones y modelos en la carpeta de datos local; la base de datos SQLite en la carpeta de configuración de la app.
          </p>
        </Collapsible>
      </div>
    </div>
  );
}
