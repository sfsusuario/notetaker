import { Component, useEffect, type ReactNode } from "react";
import { bootstrap } from "../../app/actions";
import { wireNativeEvents } from "../../app/wireEvents";
import { ConfirmHost } from "../../components/common/ConfirmDialog";
import { IconX } from "../../components/common/icons";
import { cn } from "../../components/common/ui";
import { useUiStore } from "../../stores/useUiStore";
import { HistoryView } from "./history/HistoryView";
import { LiveView } from "./live/LiveView";
import { NewSessionView } from "./new/NewSessionView";
import { SessionView } from "./session/SessionView";
import { SettingsView } from "./settings/SettingsView";
import { TitleBar } from "./TitleBar";

class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error, info: { componentStack?: string }) {
    console.error("UI crash", error, info.componentStack);
  }
  render() {
    if (this.state.error) {
      return (
        <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
          <div className="text-sm font-semibold text-rose-400">Se produjo un error en la interfaz</div>
          <pre className="max-h-64 max-w-2xl overflow-auto rounded-lg bg-surface p-3 text-left text-[11px] text-fg-muted ring-1 ring-line/10">
            {String(this.state.error?.stack ?? this.state.error)}
          </pre>
          <button
            type="button"
            className="rounded-lg bg-indigo-600 px-3 py-1.5 text-sm text-white"
            onClick={() => {
              this.setState({ error: null });
              useUiStore.getState().navigate("new");
            }}
          >
            Volver al inicio
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

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
    <div className="flex h-full flex-col bg-bg text-fg">
      <TitleBar />
      <main className="min-h-0 min-w-0 flex-1">
        <ErrorBoundary key={view}>
          {view === "new" && <NewSessionView />}
          {view === "live" && <LiveView />}
          {view === "history" && <HistoryView />}
          {view === "session" && <SessionView />}
          {view === "settings" && <SettingsView />}
        </ErrorBoundary>
      </main>
      <Toasts />
      <ConfirmHost />
    </div>
  );
}
