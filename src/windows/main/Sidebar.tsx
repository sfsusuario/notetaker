import { fmtMs } from "../../app/format";
import {
  IconGear,
  IconHistory,
  IconMoon,
  IconPlus,
  IconRecord,
  IconSun,
} from "../../components/common/icons";
import { cn } from "../../components/common/ui";
import { livePositionMs, useSessionStore } from "../../stores/useSessionStore";
import { applyTheme, useSettingsStore } from "../../stores/useSettingsStore";
import { useUiStore, type View } from "../../stores/useUiStore";

function NavItem({
  view,
  icon,
  label,
  active,
}: {
  view: View;
  icon: React.ReactNode;
  label: string;
  active: boolean;
}) {
  const navigate = useUiStore((s) => s.navigate);
  return (
    <button
      type="button"
      onClick={() => navigate(view)}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-sm transition-colors",
        active ? "bg-indigo-600/15 text-indigo-400" : "text-fg-muted hover:bg-line/10 hover:text-fg",
      )}
    >
      {icon}
      {label}
    </button>
  );
}

export function Sidebar() {
  const view = useUiStore((s) => s.view);
  const navigate = useUiStore((s) => s.navigate);
  const liveId = useSessionStore((s) => s.sessionId);
  const metrics = useSessionStore((s) => s.metrics);
  const paused = useSessionStore((s) => s.paused);
  const theme = useSettingsStore((s) => s.theme);
  const setSettings = useSettingsStore((s) => s.set);
  const isDark = document.documentElement.classList.contains("dark");

  const toggleTheme = () => {
    const next = isDark ? "light" : "dark";
    setSettings({ theme: next });
    applyTheme(next);
  };

  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-line/10 bg-surface/50 p-3">
      <div className="mb-4 flex items-center gap-2 px-2 pt-1">
        <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-indigo-600 text-white">
          <IconRecord width={16} height={16} />
        </div>
        <div>
          <div className="text-sm font-semibold leading-tight text-fg">NoteTaker</div>
          <div className="text-[10px] text-fg-muted">Notas de reuniones</div>
        </div>
      </div>

      <nav className="flex flex-col gap-1">
        <NavItem view="new" icon={<IconPlus width={16} height={16} />} label="Nueva sesión" active={view === "new"} />
        <NavItem
          view="history"
          icon={<IconHistory width={16} height={16} />}
          label="Historial"
          active={view === "history" || view === "session"}
        />
        <NavItem view="settings" icon={<IconGear width={16} height={16} />} label="Ajustes" active={view === "settings"} />
      </nav>

      {liveId && (
        <button
          type="button"
          onClick={() => navigate("live")}
          className={cn(
            "mt-4 flex items-center gap-2 rounded-xl px-3 py-2 text-left text-xs ring-1 transition-colors",
            view === "live"
              ? "bg-rose-500/15 text-rose-400 ring-rose-500/30"
              : "bg-surface text-fg ring-line/10 hover:bg-line/10",
          )}
        >
          <span
            className={cn(
              "h-2 w-2 rounded-full",
              paused ? "bg-amber-400" : "animate-pulse bg-rose-500",
            )}
          />
          <span className="flex-1">
            <span className="block font-medium">{paused ? "En pausa" : "Grabando"}</span>
            <span className="block font-mono text-[11px] text-fg-muted">
              {fmtMs(livePositionMs(metrics))}
            </span>
          </span>
        </button>
      )}

      <div className="mt-auto flex items-center justify-between px-1 pt-3">
        <span className="text-[10px] text-fg-muted">v0.1.0</span>
        <button
          type="button"
          onClick={toggleTheme}
          className="rounded-lg p-1.5 text-fg-muted hover:bg-line/10 hover:text-fg"
          title={`Tema: ${theme}`}
        >
          {isDark ? <IconSun width={16} height={16} /> : <IconMoon width={16} height={16} />}
        </button>
      </div>
    </aside>
  );
}
