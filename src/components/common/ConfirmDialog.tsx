import { useEffect, type ReactNode } from "react";
import { create } from "zustand";
import { Button, Modal } from "./ui";

interface ConfirmOptions {
  title: string;
  message: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

interface ConfirmState {
  req: (ConfirmOptions & { resolve: (ok: boolean) => void }) | null;
  ask: (opts: ConfirmOptions) => Promise<boolean>;
  answer: (ok: boolean) => void;
}

const useConfirmStore = create<ConfirmState>((set, get) => ({
  req: null,
  ask: (opts) =>
    new Promise<boolean>((resolve) => {
      // Si ya había una pregunta abierta se resuelve como cancelada.
      get().req?.resolve(false);
      set({ req: { ...opts, resolve } });
    }),
  answer: (ok) => {
    const req = get().req;
    set({ req: null });
    req?.resolve(ok);
  },
}));

/**
 * Confirmación con el aspecto de la app (el diálogo nativo del sistema
 * desentona con el tema y no respeta el modo claro/oscuro).
 */
export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  return useConfirmStore.getState().ask(opts);
}

/** Se monta una sola vez en la ventana principal. */
export function ConfirmHost() {
  const req = useConfirmStore((s) => s.req);
  const answer = useConfirmStore((s) => s.answer);

  useEffect(() => {
    if (!req) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        answer(true);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [req, answer]);

  if (!req) return null;

  return (
    <Modal
      title={req.title}
      onClose={() => answer(false)}
      footer={
        <>
          <Button onClick={() => answer(false)}>{req.cancelLabel ?? "Cancelar"}</Button>
          <Button
            autoFocus
            variant={req.danger ? "danger" : "primary"}
            onClick={() => answer(true)}
          >
            {req.confirmLabel ?? "Continuar"}
          </Button>
        </>
      }
    >
      <div className="text-sm leading-relaxed text-fg-muted">{req.message}</div>
    </Modal>
  );
}
