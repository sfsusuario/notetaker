import { useMemo } from "react";
import { fmtMs } from "../../app/format";
import { initials, isMe, speakerColor, speakerKey, speakerLabel } from "../../app/speakers";
import { useAutoScroll } from "../../hooks/useAutoScroll";
import type { Segment, SpeakerOverride } from "../../types";
import { cn } from "../common/ui";

interface Props {
  segments: Segment[];
  /** parciales en curso (uno por fuente) */
  partials?: Segment[];
  speakers?: SpeakerOverride[];
  /** posición del reproductor (ms) para resaltar la burbuja activa */
  activeMs?: number | null;
  onSeek?: (ms: number) => void;
  onSpeakerClick?: (key: string, label: string) => void;
  emptyHint?: string;
  autoScroll?: boolean;
}

function Avatar({ label, colorKey }: { label: string; colorKey: string }) {
  return (
    <div
      className={cn(
        "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-[10px] font-semibold text-white",
        speakerColor(colorKey),
      )}
      title={label}
    >
      {initials(label)}
    </div>
  );
}

export function TypingBubble({ seg, speakers }: { seg: Segment; speakers?: SpeakerOverride[] }) {
  const me = isMe(seg);
  const label = speakerLabel(seg, speakers ?? []);
  const key = speakerKey(seg);
  const dots = seg.text.trim() === "" || seg.text.trim() === "…";
  return (
    <div className={cn("flex items-end gap-2", me ? "flex-row-reverse" : "flex-row")}>
      <Avatar label={label} colorKey={key} />
      <div
        className={cn(
          "max-w-[70%] rounded-2xl px-3.5 py-2 text-sm ring-1",
          me
            ? "rounded-br-md bg-indigo-600/60 text-white ring-indigo-500/30"
            : "rounded-bl-md bg-surface-2 text-fg ring-line/10",
          "animate-pulse",
        )}
      >
        {dots ? (
          <span className="nt-typing inline-flex gap-1 py-1 text-fg-muted">
            <span />
            <span />
            <span />
          </span>
        ) : (
          <span className="opacity-80">{seg.text}</span>
        )}
      </div>
    </div>
  );
}

export function TranscriptFeed({
  segments,
  partials = [],
  speakers = [],
  activeMs = null,
  onSeek,
  onSpeakerClick,
  emptyHint = "La transcripción aparecerá aquí.",
  autoScroll = true,
}: Props) {
  const dep = `${segments.length}:${segments[segments.length - 1]?.id ?? ""}:${partials
    .map((p) => p.text.length)
    .join(",")}`;
  const ref = useAutoScroll<HTMLDivElement>(autoScroll ? dep : null);

  const activeId = useMemo(() => {
    if (activeMs == null) return null;
    let best: Segment | null = null;
    for (const s of segments) {
      if (s.startMs <= activeMs && (best == null || s.startMs >= best.startMs)) best = s;
    }
    return best?.id ?? null;
  }, [segments, activeMs]);

  return (
    <div ref={ref} className="h-full overflow-y-auto px-3 py-3">
      {segments.length === 0 && partials.length === 0 && (
        <div className="flex h-full items-center justify-center text-sm text-fg-muted">
          {emptyHint}
        </div>
      )}
      <div className="mx-auto flex max-w-3xl flex-col gap-1">
        {segments.map((seg, i) => {
          const prev = segments[i - 1];
          const me = isMe(seg);
          const key = speakerKey(seg);
          const label = speakerLabel(seg, speakers);
          const sameAsPrev =
            prev && speakerKey(prev) === key && seg.receivedAt - prev.receivedAt < 60_000;
          const active = seg.id === activeId;
          return (
            <div
              key={seg.id}
              className={cn(
                "flex items-end gap-2",
                me ? "flex-row-reverse" : "flex-row",
                !sameAsPrev && i > 0 && "mt-3",
              )}
            >
              <div className="w-7 shrink-0">
                {!sameAsPrev && <Avatar label={label} colorKey={key} />}
              </div>
              <div className={cn("flex max-w-[72%] flex-col", me ? "items-end" : "items-start")}>
                {!sameAsPrev && (
                  <div className={cn("mb-0.5 flex items-center gap-2 px-1 text-[11px]", me && "flex-row-reverse")}>
                    <button
                      type="button"
                      onClick={() => onSpeakerClick?.(key, label)}
                      className={cn(
                        "font-medium text-fg-muted",
                        onSpeakerClick && "hover:text-fg hover:underline",
                      )}
                      title={onSpeakerClick ? "Renombrar hablante" : undefined}
                    >
                      {label}
                    </button>
                    <span className="text-fg-muted/60">{fmtMs(seg.startMs)}</span>
                  </div>
                )}
                <div
                  onClick={onSeek ? () => onSeek(seg.startMs) : undefined}
                  title={onSeek ? `Ir a ${fmtMs(seg.startMs)}` : fmtMs(seg.startMs)}
                  className={cn(
                    "rounded-2xl px-3 py-1.5 text-[13px] leading-relaxed ring-1 transition-colors",
                    me
                      ? "rounded-br-md bg-indigo-600 text-white ring-indigo-500/30"
                      : "rounded-bl-md bg-surface text-fg ring-line/10",
                    onSeek && "cursor-pointer hover:ring-indigo-400/60",
                    active && "ring-2 ring-amber-400/80",
                  )}
                >
                  {seg.text}
                </div>
              </div>
            </div>
          );
        })}
        {partials.map((p) => (
          <div key={`p-${p.source}`} className="mt-2">
            <TypingBubble seg={p} speakers={speakers} />
          </div>
        ))}
      </div>
    </div>
  );
}
