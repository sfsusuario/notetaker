import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

const NAV: Array<{ view: View; label: string; icon: React.ReactNode; match: View[] }> = [
  { view: "new", label: "Nueva sesión", icon: <IconPlus width={14} height={14} />, match: ["new"] },
  {
    view: "history",
    label: "Historial",
    icon: <IconHistory width={14} height={14} />,
    match: ["history", "session"],
  },
  { view: "settings", label: "Ajustes", icon: <IconGear width={14} height={14} />, match: ["settings"] },
];

function WinButton({
  onClick,
  title,
  danger,
  children,
}: {
  onClick: () => void;
  title: string;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "flex h-9 w-11 items-center justify-center text-fg-muted transition-colors",
        danger ? "hover:bg-rose-600 hover:text-white" : "hover:bg-line/10 hover:text-fg",
      )}
    >
      {children}
    </button>
  );
}

/**
 * Barra superior propia (la ventana no tiene decoración nativa): logo,
 * navegación, estado de grabación y botones de ventana. Sustituye al sidebar
 * para dejar todo el ancho al contenido.
 */
export function TitleBar() {
  const view = useUiStore((s) => s.view);
  const navigate = useUiStore((s) => s.navigate);
  const liveId = useSessionStore((s) => s.sessionId);
  const paused = useSessionStore((s) => s.paused);
  const metrics = useSessionStore((s) => s.metrics);
  const theme = useSettingsStore((s) => s.theme);
  const setSettings = useSettingsStore((s) => s.set);
  const [maximized, setMaximized] = useState(false);
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));
  const win = getCurrentWindow();

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void win.isMaximized().then(setMaximized).catch(() => {});
    void win
      .onResized(() => {
        void win.isMaximized().then(setMaximized).catch(() => {});
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    setDark(document.documentElement.classList.contains("dark"));
  }, [theme]);

  const toggleTheme = () => {
    const next = dark ? "light" : "dark";
    setSettings({ theme: next });
    applyTheme(next);
    setDark(!dark);
  };

  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center gap-1 border-b border-line/10 bg-surface/80 pl-2.5"
    >
      <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-indigo-600 text-white">
        <IconRecord width={11} height={11} />
      </span>
      <span data-tauri-drag-region className="mr-1 hidden text-xs font-semibold text-fg sm:inline">
        NoteTaker
      </span>

      <nav className="flex items-center gap-0.5">
        {NAV.map((n) => (
          <button
            key={n.view}
            type="button"
            onClick={() => navigate(n.view)}
            className={cn(
              "flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs transition-colors",
              n.match.includes(view)
                ? "bg-indigo-600/15 text-indigo-400"
                : "text-fg-muted hover:bg-line/10 hover:text-fg",
            )}
          >
            {n.icon}
            {n.label}
          </button>
        ))}
      </nav>

      <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center justify-end gap-1 pr-1">
        {liveId && (
          <button
            type="button"
            onClick={() => navigate("live")}
            title="Ver la sesión en curso"
            className={cn(
              "flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] ring-1 transition-colors",
              paused
                ? "bg-amber-500/10 text-amber-500 ring-amber-500/30 hover:bg-amber-500/20"
                : "bg-rose-500/10 text-rose-400 ring-rose-500/30 hover:bg-rose-500/20",
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                paused ? "bg-amber-400" : "animate-pulse bg-rose-500",
              )}
            />
            {paused ? "En pausa" : "Grabando"}
            <span className="font-mono tabular-nums">{fmtMs(livePositionMs(metrics))}</span>
          </button>
        )}
        <button
          type="button"
          onClick={toggleTheme}
          title={`Tema: ${theme}`}
          className="rounded-lg p-1.5 text-fg-muted transition-colors hover:bg-line/10 hover:text-fg"
        >
          {dark ? <IconSun width={14} height={14} /> : <IconMoon width={14} height={14} />}
        </button>
      </div>

      <div className="flex shrink-0">
        <WinButton title="Minimizar" onClick={() => void win.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <path d="M0 5h10" />
          </svg>
        </WinButton>
        <WinButton title={maximized ? "Restaurar" : "Maximizar"} onClick={() => void win.toggleMaximize()}>
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
              <path d="M2.5 0.5h7v7M0.5 2.5h7v7h-7z" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
              <rect x="0.5" y="0.5" width="9" height="9" />
            </svg>
          )}
        </WinButton>
        <WinButton title="Cerrar (sigue en la bandeja)" danger onClick={() => void win.close()}>
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
            <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
          </svg>
        </WinButton>
      </div>
    </div>
  );
}
