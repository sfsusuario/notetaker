import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cancelFileJob,
  deleteSessionFully,
  maybeGenerateTitle,
  retranscribe,
} from "../../../app/actions";
import { fmtDate, fmtDuration, fmtMs } from "../../../app/format";
import { ChatPanel } from "../../../components/chat/ChatPanel";
import {
  IconArrowLeft,
  IconEdit,
  IconFolder,
  IconMore,
  IconRefresh,
  IconSparkle,
  IconTrash,
} from "../../../components/common/icons";
import {
  Badge,
  Button,
  IconButton,
  Input,
  Menu,
  MenuItem,
  Modal,
  Spinner,
  Dropdown,
  cn,
} from "../../../components/common/ui";
import { TranscriptFeed } from "../../../components/transcript/TranscriptFeed";
import { openPath } from "../../../services/ipc/native";
import * as db from "../../../services/storage/db";
import { exportSession } from "../../../services/storage/export";
import { useHistoryStore } from "../../../stores/useHistoryStore";
import type { ExportFormat } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import { LANGUAGES, type EngineId } from "../../../types";
import { EnginePicker } from "../new/EnginePicker";

function RetranscribeModal({
  onClose,
  sessionId,
  engine0,
  model0,
  language0,
}: {
  onClose: () => void;
  sessionId: string;
  engine0: EngineId;
  model0: string | null;
  language0: string;
}) {
  const [engine, setEngine] = useState<EngineId>(engine0);
  const [model, setModel] = useState<string | null>(model0);
  const [language, setLanguage] = useState(language0);
  return (
    <Modal
      title="Retranscribir con…"
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button
            variant="primary"
            onClick={() => {
              onClose();
              void retranscribe(sessionId, { engine, model, language });
            }}
          >
            Retranscribir
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <EnginePicker engine={engine} model={model} onChange={(e, m) => { setEngine(e); setModel(m); }} />
        <div className="flex items-center gap-2 text-xs">
          <span className="w-16 text-fg-muted">Idioma</span>
          <Dropdown value={language} options={LANGUAGES.map(([v, l]) => ({ value: v, label: l }))} onSelect={setLanguage} className="w-56" />
        </div>
        <p className="text-[11px] text-fg-muted">
          Se usará el audio guardado de la sesión. La transcripción y las etiquetas de hablante actuales se reemplazan.
        </p>
      </div>
    </Modal>
  );
}

function SpeakerModal({
  speakerKey,
  current,
  onClose,
  onSave,
}: {
  speakerKey: string;
  current: string;
  onClose: () => void;
  onSave: (label: string) => void;
}) {
  const [v, setV] = useState(current);
  return (
    <Modal
      title="Renombrar hablante"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancelar</Button>
          <Button variant="primary" disabled={!v.trim()} onClick={() => onSave(v)}>
            Guardar
          </Button>
        </>
      }
    >
      <p className="mb-2 text-xs text-fg-muted">
        Etiqueta actual: <b>{current}</b> <span className="text-fg-muted/60">({speakerKey})</span>
      </p>
      <Input autoFocus value={v} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => e.key === "Enter" && v.trim() && onSave(v)} className="w-full" placeholder="Nombre de la persona" />
    </Modal>
  );
}

export function SessionView() {
  const id = useUiStore((u) => u.openSessionId);
  const navigate = useUiStore((u) => u.navigate);
  const toast = useUiStore((u) => u.toast);
  const current = useHistoryStore((h) => h.current);
  const progress = useHistoryStore((h) => (id ? h.progress[id] : undefined));
  const open = useHistoryStore((h) => h.open);
  const rename = useHistoryStore((h) => h.rename);
  const setSpeakerLabel = useHistoryStore((h) => h.setSpeakerLabel);
  const patchSession = useHistoryStore((h) => h.patchSession);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [retrans, setRetrans] = useState(false);
  const [speakerEdit, setSpeakerEdit] = useState<{ key: string; label: string } | null>(null);
  const [notes, setNotes] = useState("");
  const [activeMs, setActiveMs] = useState<number | null>(null);
  const audioRef = useRef<HTMLAudioElement>(null);

  useEffect(() => {
    if (id && current?.session.id !== id) void open(id);
  }, [id, current?.session.id, open]);

  useEffect(() => {
    setNotes(current?.session.notes ?? "");
  }, [current?.session.id, current?.session.notes]);

  useEffect(() => {
    if (!id) navigate("history");
  }, [id, navigate]);

  if (!id) return null;
  if (!current || current.session.id !== id) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-fg-muted">
        <Spinner /> <span className="ml-2">Cargando…</span>
      </div>
    );
  }

  const { session, segments, speakers, paths } = current;
  const audioSrc = paths?.mix ?? paths?.file ?? paths?.system ?? paths?.mic ?? null;
  const processing = session.status === "processing";
  const titleText = session.title ?? `Sesión ${fmtDate(session.createdAt)}`;

  const commitTitle = async () => {
    setEditing(false);
    if (draft.trim() && draft.trim() !== session.title) await rename(session.id, draft);
  };

  const doExport = async (format: ExportFormat) => {
    const chat = await db.listChatMessages(session.id);
    const p = await exportSession({ session, segments, speakers, chat }, format);
    if (p) toast(`Exportado a ${p}`, "ok");
  };

  const saveNotes = async () => {
    if (notes === (session.notes ?? "")) return;
    await db.updateSession(session.id, { notes });
    patchSession(session.id, { notes });
  };

  const seek = (ms: number) => {
    const a = audioRef.current;
    if (!a) return;
    a.currentTime = ms / 1000;
    void a.play().catch(() => {});
  };

  return (
    <div className="flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="border-b border-line/10 px-4 py-2.5">
          <div className="flex items-center gap-2">
            <IconButton title="Volver al historial" onClick={() => navigate("history")}>
              <IconArrowLeft width={16} height={16} />
            </IconButton>
            {editing ? (
              <Input
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => void commitTitle()}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void commitTitle();
                  if (e.key === "Escape") setEditing(false);
                }}
                className="min-w-0 flex-1 text-sm"
              />
            ) : (
              <button
                type="button"
                onClick={() => { setDraft(session.title ?? ""); setEditing(true); }}
                className="group flex min-w-0 flex-1 items-center gap-2 text-left"
                title="Editar título"
              >
                <span className="truncate text-base font-semibold text-fg">{titleText}</span>
                <IconEdit width={14} height={14} className="shrink-0 text-fg-muted opacity-0 group-hover:opacity-100" />
              </button>
            )}
            {processing && (
              <Badge tone="warn">
                <Spinner className="h-2.5 w-2.5" /> Procesando
              </Badge>
            )}
            <Menu trigger={() => <IconMore width={16} height={16} />} title="Acciones">
              {(close) => (
                <>
                  <MenuItem onClick={() => { close(); void doExport("md"); }}>Exportar Markdown</MenuItem>
                  <MenuItem onClick={() => { close(); void doExport("txt"); }}>Exportar texto</MenuItem>
                  <MenuItem onClick={() => { close(); void doExport("json"); }}>Exportar JSON</MenuItem>
                  <MenuItem disabled={processing} onClick={() => { close(); setRetrans(true); }}>
                    <span className="flex items-center gap-2"><IconRefresh width={13} height={13} /> Retranscribir con…</span>
                  </MenuItem>
                  <MenuItem disabled={processing || segments.length === 0} onClick={() => { close(); void maybeGenerateTitle(session.id, true); }}>
                    <span className="flex items-center gap-2"><IconSparkle width={13} height={13} /> Generar título con IA</span>
                  </MenuItem>
                  {paths?.dir && (
                    <MenuItem onClick={() => { close(); void openPath(paths.dir); }}>
                      <span className="flex items-center gap-2"><IconFolder width={13} height={13} /> Abrir carpeta de audio</span>
                    </MenuItem>
                  )}
                  <MenuItem danger disabled={processing} onClick={() => { close(); void deleteSessionFully(session.id); }}>
                    <span className="flex items-center gap-2"><IconTrash width={13} height={13} /> Eliminar sesión</span>
                  </MenuItem>
                </>
              )}
            </Menu>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 pl-10 text-[11px] text-fg-muted">
            <span>{fmtDate(session.createdAt)}</span>
            <span>{fmtDuration(session.durationMs ?? paths?.durationMs ?? null)}</span>
            <span>{session.engine === "whisper" ? "Whisper local" : "Deepgram"}{session.engineModel ? ` · ${session.engineModel}` : ""}</span>
            <span>{segments.length} fragmentos</span>
            {session.language && session.language !== "auto" && <span>{session.language}</span>}
            {session.sourceFilePath && (
              <span className="truncate" title={session.sourceFilePath}>{session.sourceFilePath.split(/[\\/]/).pop()}</span>
            )}
          </div>
        </header>

        {processing && progress && progress.phase !== "done" && (
          <div className="mx-4 mt-3 rounded-xl bg-surface p-3 ring-1 ring-line/10">
            <div className="mb-1.5 flex items-center justify-between text-xs">
              <span className="text-fg">
                {progress.phase === "decoding" && "Decodificando audio…"}
                {progress.phase === "uploading" && "Subiendo a Deepgram…"}
                {progress.phase === "transcribing" && "Transcribiendo…"}
                {progress.phase === "error" && `Error: ${progress.message}`}
                {progress.phase === "cancelled" && "Cancelado"}
                {progress.message && progress.phase !== "error" && (
                  <span className="ml-2 text-fg-muted">{progress.message}</span>
                )}
              </span>
              <Button size="sm" onClick={() => void cancelFileJob()}>Cancelar</Button>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-line/10">
              <div className="h-full rounded-full bg-indigo-500 transition-[width]" style={{ width: `${Math.min(100, Math.max(2, progress.percent))}%` }} />
            </div>
          </div>
        )}

        {audioSrc && (
          <div className="mx-4 mt-3 flex items-center gap-3 rounded-xl bg-surface px-3 py-2 ring-1 ring-line/10">
            <audio
              ref={audioRef}
              controls
              preload="metadata"
              src={convertFileSrc(audioSrc)}
              onTimeUpdate={(e) => setActiveMs(e.currentTarget.currentTime * 1000)}
              onEnded={() => setActiveMs(null)}
              className="h-9 w-full"
            />
            {activeMs != null && <span className="font-mono text-[11px] tabular-nums text-fg-muted">{fmtMs(activeMs)}</span>}
          </div>
        )}

        <div className="min-h-0 flex-1">
          <TranscriptFeed
            segments={segments}
            speakers={speakers}
            activeMs={activeMs}
            onSeek={audioSrc ? seek : undefined}
            onSpeakerClick={(key, label) => setSpeakerEdit({ key, label })}
            autoScroll={processing}
            emptyHint={processing ? "Esperando los primeros fragmentos…" : "Esta sesión no tiene transcripción."}
          />
        </div>

        <div className="border-t border-line/10 px-4 py-2">
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            onBlur={() => void saveNotes()}
            rows={2}
            placeholder="Notas propias de la sesión (se guardan al salir del campo)…"
            className={cn("w-full resize-none rounded-lg bg-surface-2 px-2.5 py-1.5 text-xs text-fg ring-1 ring-line/10 placeholder:text-fg-muted/60 focus:outline-none focus:ring-indigo-500/50")}
          />
        </div>
      </div>

      <aside className="w-[360px] shrink-0 border-l border-line/10">
        <ChatPanel
          sessionId={session.id}
          segments={segments}
          speakers={speakers}
          title={session.title}
          date={fmtDate(session.createdAt)}
          live={processing}
        />
      </aside>

      {retrans && (
        <RetranscribeModal
          onClose={() => setRetrans(false)}
          sessionId={session.id}
          engine0={session.engine}
          model0={session.engineModel}
          language0={session.language ?? "auto"}
        />
      )}
      {speakerEdit && (
        <SpeakerModal
          speakerKey={speakerEdit.key}
          current={speakerEdit.label}
          onClose={() => setSpeakerEdit(null)}
          onSave={(label) => {
            void setSpeakerLabel(session.id, speakerEdit.key, label);
            setSpeakerEdit(null);
          }}
        />
      )}
    </div>
  );
}
