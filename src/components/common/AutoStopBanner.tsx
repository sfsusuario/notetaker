import { useEffect, useState } from "react";
import { autostopCancel, autostopStopNow } from "../../services/ipc/native";
import type { AutoStopPending } from "../../types";
import { IconClock, IconStop } from "./icons";
import { Button, cn } from "./ui";

/**
 * Segundos que faltan hasta `deadlineMs`. Se recalcula contra el reloj en cada
 * tick en vez de llevar un contador decreciente en el estado: así no se
 * desincroniza si la ventana estuvo minimizada o el equipo suspendido.
 */
export function useCountdown(deadlineMs: number): number {
  const remaining = () => Math.max(0, Math.ceil((deadlineMs - Date.now()) / 1000));
  const [left, setLeft] = useState(remaining);
  useEffect(() => {
    setLeft(remaining());
    const id = setInterval(() => setLeft(remaining()), 1000);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deadlineMs]);
  return left;
}

export function AutoStopBanner({
  pending,
  className,
}: {
  pending: AutoStopPending;
  className?: string;
}) {
  const left = useCountdown(pending.deadlineMs);

  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-3 gap-y-2 rounded-xl bg-amber-500/10 px-3 py-2 text-xs text-amber-500 ring-1 ring-amber-500/30",
        className,
      )}
    >
      <IconClock width={15} height={15} className="shrink-0" />
      <span className="min-w-0 flex-1">
        <b>{pending.detail}</b>{" "}
        {left > 0 ? (
          <>
            La grabación se detendrá en{" "}
            <span className="font-mono tabular-nums">{left}s</span>.
          </>
        ) : (
          "Deteniendo…"
        )}
      </span>
      <div className="flex shrink-0 items-center gap-1.5">
        <Button size="sm" onClick={() => void autostopCancel()}>
          Seguir grabando
        </Button>
        <Button size="sm" variant="danger" onClick={() => void autostopStopNow()}>
          <IconStop width={13} height={13} /> Detener ahora
        </Button>
      </div>
    </div>
  );
}
