import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

export function cn(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

/**
 * Menú desplegable anclado a un disparador. Se renderiza en un portal a
 * document.body con posición fija para escapar de cualquier contenedor con
 * overflow-hidden o backdrop-filter.
 */
export function Menu({
  trigger,
  children,
  align = "right",
  title,
  className,
}: {
  trigger: (open: boolean) => ReactNode;
  children: ReactNode | ((close: () => void) => ReactNode);
  align?: "left" | "right";
  title?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{
    top: number;
    left?: number;
    right?: number;
    maxHeight: number;
  }>({ top: 0, maxHeight: 400 });
  const close = () => setOpen(false);

  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const r = btnRef.current.getBoundingClientRect();
    const top = r.bottom + 4;
    const margin = 8;
    const w = panelRef.current?.offsetWidth ?? 0;
    let side: { left?: number; right?: number };
    if (align === "right") {
      let right = window.innerWidth - r.right;
      if (w && window.innerWidth - right - w < margin) {
        right = Math.max(margin, window.innerWidth - w - margin);
      }
      side = { right };
    } else {
      let left = r.left;
      if (w && left + w > window.innerWidth - margin) {
        left = Math.max(margin, window.innerWidth - w - margin);
      }
      side = { left };
    }
    setPos({ top, maxHeight: window.innerHeight - top - 12, ...side });
  }, [open, align]);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
        title={title}
        className={cn(
          "flex items-center gap-0.5 rounded-lg px-1.5 py-1 text-fg-muted transition-colors hover:bg-line/10 hover:text-fg",
          className,
        )}
      >
        {trigger(open)}
      </button>
      {open &&
        createPortal(
          <>
            <div
              className="fixed inset-0 z-[90]"
              onClick={(e) => {
                e.stopPropagation();
                close();
              }}
            />
            <div
              ref={panelRef}
              style={{
                top: pos.top,
                left: pos.left,
                right: pos.right,
                maxHeight: pos.maxHeight,
              }}
              className="fixed z-[100] min-w-44 overflow-y-auto rounded-xl bg-surface p-1.5 shadow-2xl ring-1 ring-line/15"
            >
              {title && (
                <div className="px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-fg-muted">
                  {title}
                </div>
              )}
              {typeof children === "function" ? children(close) : children}
            </div>
          </>,
          document.body,
        )}
    </>
  );
}

export interface DropdownOption {
  value: string;
  label: string;
}

/**
 * Reemplazo del <select> nativo: lista en un portal con posición fija y
 * z-index por encima de los modales, así no la recorta ningún contenedor.
 */
export function Dropdown({
  value,
  options,
  onSelect,
  placeholder = "— elegir —",
  className,
  disabled,
}: {
  value: string;
  options: Array<string | DropdownOption>;
  onSelect: (value: string) => void;
  placeholder?: string;
  className?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState({ top: 0, left: 0, width: 0, maxHeight: 240 });
  const opts: DropdownOption[] = options.map((o) =>
    typeof o === "string" ? { value: o, label: o } : o,
  );
  const current = opts.find((o) => o.value === value);

  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const r = btnRef.current.getBoundingClientRect();
    const top = r.bottom + 4;
    setPos({
      top,
      left: r.left,
      width: Math.max(r.width, 160),
      maxHeight: Math.max(120, Math.min(280, window.innerHeight - top - 12)),
    });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex items-center justify-between gap-2 rounded-lg bg-surface-2 px-2.5 py-1.5 text-xs text-fg ring-1 ring-line/10 transition-colors hover:bg-line/10 focus:outline-none focus:ring-indigo-500/50 disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
      >
        <span className={cn("truncate", !current && "text-fg-muted")}>
          {current?.label ?? value ?? placeholder}
        </span>
        <span
          className={cn(
            "shrink-0 text-[10px] text-fg-muted transition-transform",
            open && "rotate-180",
          )}
        >
          ▾
        </span>
      </button>
      {open &&
        createPortal(
          <>
            <div
              className="fixed inset-0 z-[125]"
              onClick={() => setOpen(false)}
            />
            <div
              style={{
                top: pos.top,
                left: pos.left,
                width: pos.width,
                maxHeight: pos.maxHeight,
              }}
              className="fixed z-[130] overflow-y-auto rounded-lg bg-surface py-1 shadow-2xl ring-1 ring-line/15"
            >
              {opts.length === 0 && (
                <div className="px-2.5 py-1.5 text-xs text-fg-muted">
                  Sin opciones
                </div>
              )}
              {opts.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => {
                    onSelect(opt.value);
                    setOpen(false);
                  }}
                  className={cn(
                    "flex w-full items-center justify-between gap-2 px-2.5 py-1.5 text-left text-xs transition-colors hover:bg-line/10",
                    opt.value === value ? "text-indigo-400" : "text-fg",
                  )}
                >
                  <span className="truncate">{opt.label}</span>
                  {opt.value === value && <span className="shrink-0">✓</span>}
                </button>
              ))}
            </div>
          </>,
          document.body,
        )}
    </>
  );
}

export function MenuItem({
  active,
  disabled,
  danger,
  onClick,
  children,
}: {
  active?: boolean;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      disabled={disabled}
      className={cn(
        "flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent",
        active
          ? "bg-indigo-600/25 text-indigo-300"
          : danger
            ? "text-rose-400 hover:bg-rose-500/10"
            : "text-fg hover:bg-line/10",
      )}
    >
      {children}
      {active && <span className="text-indigo-400">✓</span>}
    </button>
  );
}

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "ghost" | "danger" | "subtle";
  size?: "sm" | "md" | "lg";
}

export function Button({
  variant = "ghost",
  size = "md",
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors",
        "disabled:cursor-not-allowed disabled:opacity-40",
        size === "sm" && "px-2 py-1 text-xs",
        size === "md" && "px-3 py-1.5 text-sm",
        size === "lg" && "px-4 py-2.5 text-sm",
        variant === "primary" &&
          "bg-indigo-600 text-white hover:bg-indigo-500 active:bg-indigo-700",
        variant === "ghost" &&
          "bg-surface-2 text-fg ring-1 ring-line/10 hover:bg-line/10",
        variant === "subtle" && "text-fg-muted hover:bg-line/10 hover:text-fg",
        variant === "danger" &&
          "bg-rose-600/15 text-rose-400 ring-1 ring-rose-500/30 hover:bg-rose-600/25",
        className,
      )}
      {...rest}
    />
  );
}

export function IconButton({
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex h-8 w-8 items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-line/10 hover:text-fg disabled:cursor-not-allowed disabled:opacity-40",
        className,
      )}
      {...rest}
    />
  );
}

export function Input({
  className,
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "min-w-0 rounded-lg bg-surface-2 px-2.5 py-1.5 text-xs text-fg ring-1 ring-line/10 placeholder:text-fg-muted/60 focus:outline-none focus:ring-indigo-500/50",
        className,
      )}
      {...rest}
    />
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-line/20 bg-surface-2 px-1.5 py-0.5 font-mono text-[10px] text-fg">
      {children}
    </kbd>
  );
}

export type DotStatus =
  | "connected"
  | "degraded"
  | "reconnecting"
  | "disconnected"
  | "loading";

export function StatusDot({ status }: { status: DotStatus }) {
  const color =
    status === "connected"
      ? "bg-emerald-400"
      : status === "disconnected"
        ? "bg-rose-400"
        : "bg-amber-400";
  return (
    <span
      className={cn(
        "inline-block h-2 w-2 rounded-full",
        color,
        (status === "reconnecting" || status === "loading") && "animate-pulse",
      )}
    />
  );
}

export function Badge({
  tone = "neutral",
  children,
  className,
}: {
  tone?: "neutral" | "ok" | "warn" | "bad" | "info";
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ring-1",
        tone === "neutral" && "bg-line/5 text-fg-muted ring-line/10",
        tone === "ok" && "bg-emerald-500/10 text-emerald-500 ring-emerald-500/30",
        tone === "warn" && "bg-amber-500/10 text-amber-500 ring-amber-500/30",
        tone === "bad" && "bg-rose-500/10 text-rose-500 ring-rose-500/30",
        tone === "info" && "bg-indigo-500/10 text-indigo-400 ring-indigo-500/30",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40",
        checked ? "bg-indigo-600" : "bg-line/20",
      )}
    >
      <span
        className={cn(
          "inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform",
          checked ? "translate-x-4" : "translate-x-0.5",
        )}
      />
    </button>
  );
}

/** Etiqueta + control en fila (o columna con `stack`). */
export function Field({
  label,
  hint,
  children,
  stack,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  stack?: boolean;
}) {
  return (
    <div
      className={cn(
        "gap-3 py-2",
        stack ? "flex flex-col gap-1.5" : "flex items-center justify-between",
      )}
    >
      <div className="min-w-0">
        <div className="text-xs font-medium text-fg">{label}</div>
        {hint && <div className="text-[11px] text-fg-muted">{hint}</div>}
      </div>
      <div className={cn("shrink-0", stack && "w-full")}>{children}</div>
    </div>
  );
}

export function Card({
  title,
  action,
  children,
  className,
}: {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={cn("rounded-2xl bg-surface p-4 ring-1 ring-line/10", className)}
    >
      {(title || action) && (
        <div className="mb-3 flex items-center justify-between gap-2">
          {title && <h3 className="text-sm font-semibold text-fg">{title}</h3>}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-line/20 border-t-indigo-500",
        className,
      )}
    />
  );
}

export function Modal({
  title,
  onClose,
  children,
  wide,
  scrollBody = true,
  footer,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
  scrollBody?: boolean;
  footer?: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return createPortal(
    <div
      className="fixed inset-0 z-[120] flex items-center justify-center bg-black/60 p-6"
      onClick={onClose}
    >
      <div
        className={cn(
          "flex max-h-[90vh] w-full flex-col rounded-2xl bg-surface p-5 shadow-2xl ring-1 ring-line/10",
          wide ? "max-w-2xl" : "max-w-md",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex shrink-0 items-center justify-between">
          <h2 className="text-sm font-semibold text-fg">{title}</h2>
          <Button size="sm" variant="subtle" onClick={onClose} aria-label="Cerrar">
            ✕
          </Button>
        </div>
        <div
          className={cn(
            "min-h-0 flex-1",
            scrollBody ? "overflow-y-auto" : "flex flex-col",
          )}
        >
          {children}
        </div>
        {footer && (
          <div className="mt-4 flex shrink-0 items-center justify-end gap-2">
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
