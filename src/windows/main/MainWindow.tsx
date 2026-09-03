import { useEffect } from "react";
import { bootstrap } from "../../app/actions";
import { wireNativeEvents } from "../../app/wireEvents";
import { IconX } from "../../components/common/icons";
import { cn } from "../../components/common/ui";
import { useUiStore } from "../../stores/useUiStore";
import { HistoryView } from "./history/HistoryView";
import { LiveView } from "./live/LiveView";
import { NewSessionView } from "./new/NewSessionView";
import { SessionView } from "./session/SessionView";
import { SettingsView } from "./settings/SettingsView";
import { Sidebar } from "./Sidebar";

function Toasts() {
  const toasts = useUiStore((s) => s.toasts);
  const dismiss = useUiStore((s) => s.dismissToast);
  if (toasts.length === 0) return null;
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[200] flex w-80 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={cn(
            "pointer-events-auto flex items-start gap-2 rounded-xl px-3 py-2 text-xs shadow-2xl ring-1",
            t.kind === "error" && "bg-rose-600/90 text-white ring-rose-400/40",
            t.kind === "ok" && "bg-emerald-600/90 text-white ring-emerald-400/40",
            t.kind === "info" && "bg-surface text-fg ring-line/15",
          )}
        >
          <span className="flex-1">{t.text}</span>
          <button type="button" onClick={() => dismiss(t.id)} className="opacity-70 hover:opacity-100">
            <IconX width={14} height={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function MainWindow() {
  const view = useUiStore((s) => s.view);

  useEffect(() => {
    let unlisteners: Array<() => void> = [];
    void (async () => {
      unlisteners = await wireNativeEvents();
      await bootstrap();
    })();
    return () => unlisteners.forEach((u) => u());
  }, []);

  return (
    <div className="flex h-full bg-bg text-fg">
      <Sidebar />
      <main className="min-w-0 flex-1">
        {view === "new" && <NewSessionView />}
        {view === "live" && <LiveView />}
        {view === "history" && <HistoryView />}
        {view === "session" && <SessionView />}
        {view === "settings" && <SettingsView />}
      </main>
      <Toasts />
    </div>
  );
}
