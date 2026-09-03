import { useEffect } from "react";
import {
  IconCheck,
  IconChip,
  IconCloud,
  IconUsers,
} from "../../../components/common/icons";
import { Badge, Dropdown, cn } from "../../../components/common/ui";
import { useEnginesStore } from "../../../stores/useEnginesStore";
import { useUiStore } from "../../../stores/useUiStore";
import type { EngineId, EngineInfo } from "../../../types";

interface Props {
  engine: EngineId;
  model: string | null;
  onChange: (engine: EngineId, model: string | null) => void;
  compact?: boolean;
}

export function CapabilityBadges({ info }: { info: EngineInfo }) {
  return (
    <div className="flex flex-wrap gap-1">
      {info.diarization ? (
        <Badge tone="ok">
          <IconUsers width={11} height={11} /> Detecta hablantes
        </Badge>
      ) : (
        <Badge tone="warn">
          <IconUsers width={11} height={11} /> Solo Yo / Otros
        </Badge>
      )}
      {info.offline ? <Badge tone="info">Sin conexión</Badge> : <Badge tone="neutral">Nube</Badge>}
      {info.partials && <Badge tone="neutral">Texto en tiempo real</Badge>}
      {info.needsApiKey && <Badge tone="neutral">Requiere API key</Badge>}
    </div>
  );
}

export function defaultModelFor(info: EngineInfo | undefined, preferred: string | null): string | null {
  if (!info) return preferred;
  const installed = info.models.filter((m) => m.installed);
  if (preferred && installed.some((m) => m.id === preferred)) return preferred;
  return installed[0]?.id ?? preferred;
}

export function EnginePicker({ engine, model, onChange, compact }: Props) {
  const engines = useEnginesStore((s) => s.engines);
  const loaded = useEnginesStore((s) => s.loaded);
  const refresh = useEnginesStore((s) => s.refresh);
  const navigate = useUiStore((s) => s.navigate);

  useEffect(() => {
    if (!loaded) void refresh();
  }, [loaded, refresh]);

  const current = engines.find((e) => e.id === engine);
  const installedModels = (current?.models ?? []).filter((m) => m.installed);

  return (
    <div className="space-y-2">
      <div className={cn("grid gap-2", compact ? "grid-cols-1" : "grid-cols-2")}>
        {engines.map((info) => {
          const selected = info.id === engine;
          return (
            <button
              key={info.id}
              type="button"
              onClick={() => onChange(info.id, defaultModelFor(info, info.id === engine ? model : null))}
              className={cn(
                "flex flex-col gap-1.5 rounded-xl p-3 text-left ring-1 transition-colors",
                selected
                  ? "bg-indigo-600/10 ring-indigo-500/60"
                  : "bg-surface-2/60 ring-line/10 hover:bg-line/10",
              )}
            >
              <div className="flex items-center gap-2">
                <span className={cn("text-fg-muted", selected && "text-indigo-400")}>
                  {info.offline ? <IconChip width={16} height={16} /> : <IconCloud width={16} height={16} />}
                </span>
                <span className="flex-1 text-sm font-medium text-fg">{info.label}</span>
                {selected && <IconCheck width={16} height={16} className="text-indigo-400" />}
              </div>
              {!compact && <p className="text-[11px] text-fg-muted">{info.description}</p>}
              <CapabilityBadges info={info} />
              <div className={cn("text-[11px]", info.ready ? "text-emerald-500" : "text-amber-500")}>
                {info.ready ? "✓ " : "⚠ "}
                {info.readiness}
                {!info.ready && (
                  <span
                    role="link"
                    tabIndex={0}
                    onClick={(e) => {
                      e.stopPropagation();
                      navigate("settings");
                    }}
                    className="ml-1 cursor-pointer underline hover:text-fg"
                  >
                    Configurar
                  </span>
                )}
              </div>
            </button>
          );
        })}
        {engines.length === 0 && (
          <div className="col-span-2 text-xs text-fg-muted">Cargando motores…</div>
        )}
      </div>

      {current && current.id === "whisper" && (
        <div className="flex items-center gap-2 text-xs">
          <span className="w-16 text-fg-muted">Modelo</span>
          <Dropdown
            value={model ?? ""}
            options={installedModels.map((m) => ({ value: m.id, label: m.label }))}
            onSelect={(v) => onChange("whisper", v)}
            placeholder={installedModels.length ? "— elegir modelo —" : "Sin modelos descargados"}
            className="min-w-0 flex-1"
            disabled={installedModels.length === 0}
          />
        </div>
      )}
    </div>
  );
}
