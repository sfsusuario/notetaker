import { useEffect, useState } from "react";
import {
  deleteSessionFully,
  openSession,
  transcribeSession,
} from "../../../app/actions";
import { fmtDate, fmtDuration, fmtRelative } from "../../../app/format";
import {
  IconBraces,
  IconChevronLeft,
  IconChevronRight,
  IconCopy,
  IconDownload,
  IconEdit,
  IconFileAudio,
  IconFileText,
  IconFolder,
  IconHeadphones,
  IconMic,
  IconMore,
  IconOpen,
  IconRefresh,
  IconSearch,
  IconTrash,
} from "../../../components/common/icons";
import {
  Badge,
  IconButton,
  Input,
  Menu,
  MenuItem,
  Spinner,
  cn,
} from "../../../components/common/ui";
import { openPath, recordingPaths } from "../../../services/ipc/native";
import * as db from "../../../services/storage/db";
import { copyTranscript, exportSession } from "../../../services/storage/export";
import { PAGE_SIZE, useHistoryStore } from "../../../stores/useHistoryStore";
import { useSessionStore } from "../../../stores/useSessionStore";
import { useSettingsStore } from "../../../stores/useSettingsStore";
import { useUiStore } from "../../../stores/useUiStore";
import type { Session } from "../../../types";

function SourceIcons({ s }: { s: Session }) {
  return (
    <span className="flex items-center gap-1 text-fg-muted">
      {s.sources.includes("mic") && <IconMic width={13} height={13} />}
      {s.sources.includes("system") && <IconHeadphones width={13} height={13} />}
      {s.sources.includes("file") && <IconFileAudio width={13} height={13} />}
    </span>
  );
}

function SessionRow({ s }: { s: Session }) {
  const rename = useHistoryStore((h) => h.rename);
  const exportFormat = useSettingsStore((x) => x.exportFormat);
  const defaultEngine = useSettingsStore((x) => x.defaultEngine);
  const whisperModel = useSettingsStore((x) => x.whisperModel);
  const toast = useUiStore((x) => x.toast);
  const liveId = useSessionStore((x) => x.sessionId);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(s.title ?? "");
  const isLive = liveId === s.id;
  const untranscribed = !isLive && s.status === "recorded";
  const busy = s.status === "processing";
  const hasText = (s.segmentCount ?? 0) > 0;
  // En las sesiones grabadas la transcripción es una acción habitual (aunque ya
  // se haya hecho una vez), así que se ofrece siempre a un clic.
  const canTranscribe = !isLive && !busy && (untranscribed || s.mode === "record");

  const commit = async () => {
    setEditing(false);
    if (draft.trim() && draft.trim() !== s.title) await rename(s.id, draft);
  };

  const doExport = async (format: typeof exportFormat) => {
    const [segments, speakers, chat] = await Promise.all([
      db.getSegments(s.id),
      db.listSpeakers(s.id),
      db.listChatMessages(s.id),
    ]);
    const p = await exportSession({ session: s, segments, speakers, chat }, format);
    if (p) toast(`Exportado a ${p}`, "ok");
  };

  const doCopy = async () => {
    const [segments, speakers] = await Promise.all([db.getSegments(s.id), db.listSpeakers(s.id)]);
    if (segments.length === 0) {
      toast("Esta sesión no tiene transcripción que copiar.", "error");
      return;
    }
    const ok = await copyTranscript({ session: s, segments, speakers });
    toast(ok ? "Transcripción copiada en Markdown" : "No se pudo copiar al portapapeles", ok ? "ok" : "error");
  };

  const openFolder = async () => {
    const paths = await recordingPaths(s.id).catch(() => null);
    if (paths?.dir) await openPath(paths.dir).catch(() => {});
    else toast("Esta sesión no tiene audio guardado.", "error");
  };

  const transcribeNow = () =>
    void transcribeSession(s.id, {
      engine: defaultEngine,
      model: defaultEngine === "whisper" ? whisperModel : "nova-3",
      language: s.language ?? "auto",
    });

  const engineLabel =
    s.engine === "none" ? "Solo audio" : s.engine === "whisper" ? "Whisper local" : "Deepgram";
  const defaultEngineLabel = defaultEngine === "whisper" ? "Whisper local" : "Deepgram";

  return (
    <div
      onClick={() => !editing && void openSession(s.id)}
      className="group flex cursor-pointer items-center gap-3 rounded-xl bg-surface px-3.5 py-2.5 ring-1 ring-line/10 transition-colors hover:ring-indigo-500/40"
    >
      <div className="min-w-0 flex-1">
        {editing ? (
          <Input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => void commit()}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commit();
              if (e.key === "Escape") setEditing(false);
            }}
            onClick={(e) => e.stopPropagation()}
            className="w-full text-sm"
          />
        ) : (
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-medium text-fg">
              {s.title ?? `Sesión ${fmtDate(s.createdAt)}`}
            </span>
            {isLive && <Badge tone="bad">Grabando</Badge>}
            {busy && !isLive && (
              <Badge tone="warn">
                <Spinner className="h-2.5 w-2.5" /> Procesando
              </Badge>
            )}
            {untranscribed && <Badge tone="info">Sin transcribir</Badge>}
            {s.status === "error" && <Badge tone="bad">Error</Badge>}
            {s.titleAuto && s.title && (
              <span className="text-[10px] text-fg-muted" title="Título generado por IA">
                ✨
              </span>
            )}
          </div>
        )}
        <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-fg-muted">
          <span title={fmtDate(s.createdAt)}>{fmtRelative(s.createdAt)}</span>
          <span>{fmtDuration(s.durationMs)}</span>
          <SourceIcons s={s} />
          <span>{engineLabel}</span>
          <span>{s.segmentCount ?? 0} fragmentos</span>
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-0.5" onClick={(e) => e.stopPropagation()}>
        {hasText && (
          <IconButton title="Copiar transcripción (Markdown)" onClick={() => void doCopy()}>
            <IconCopy width={15} height={15} />
          </IconButton>
        )}
        {canTranscribe && (
          <IconButton
            title={`${untranscribed ? "Transcribir" : "Volver a transcribir"} con ${defaultEngineLabel}`}
            onClick={transcribeNow}
            className={cn(untranscribed && "text-indigo-400")}
          >
            <IconRefresh width={15} height={15} />
          </IconButton>
        )}
        <Menu
          trigger={() => <IconMore width={16} height={16} />}
          title="Sesión"
          className="hover:bg-line/10"
        >
          {(close) => (
            <>
              <MenuItem
                icon={<IconOpen width={14} height={14} />}
                onClick={() => {
                  close();
                  void openSession(s.id);
                }}
              >
                Abrir
              </MenuItem>
              <MenuItem
                icon={<IconEdit width={14} height={14} />}
                onClick={() => {
                  close();
                  setDraft(s.title ?? "");
                  setEditing(true);
                }}
              >
                Renombrar
              </MenuItem>
              <MenuItem
                icon={<IconRefresh width={14} height={14} />}
                disabled={busy || isLive}
                onClick={() => {
                  close();
                  transcribeNow();
                }}
              >
                {untranscribed
                  ? `Transcribir con ${defaultEngineLabel}`
                  : `Retranscribir con ${defaultEngineLabel}`}
              </MenuItem>
              <MenuItem
                icon={<IconCopy width={14} height={14} />}
                disabled={(s.segmentCount ?? 0) === 0}
                onClick={() => {
                  close();
                  void doCopy();
                }}
              >
                Copiar transcripción
              </MenuItem>
              <MenuItem
                icon={<IconDownload width={14} height={14} />}
                onClick={() => {
                  close();
                  void doExport("md");
                }}
              >
                Exportar Markdown
              </MenuItem>
              <MenuItem
                icon={<IconFileText width={14} height={14} />}
                onClick={() => {
                  close();
                  void doExport("txt");
                }}
              >
                Exportar texto
              </MenuItem>
              <MenuItem
                icon={<IconBraces width={14} height={14} />}
                onClick={() => {
                  close();
                  void doExport("json");
                }}
              >
                Exportar JSON
              </MenuItem>
              <MenuItem
                icon={<IconFolder width={14} height={14} />}
                onClick={() => {
                  close();
                  void openFolder();
                }}
              >
                Abrir carpeta de audio
              </MenuItem>
              <MenuItem
                danger
                icon={<IconTrash width={14} height={14} />}
                disabled={isLive}
                onClick={() => {
                  close();
                  void deleteSessionFully(s.id);
                }}
              >
                Eliminar
              </MenuItem>
            </>
          )}
        </Menu>
      </div>
    </div>
  );
}

function Pagination() {
  const page = useHistoryStore((h) => h.page);
  const total = useHistoryStore((h) => h.total);
  const setPage = useHistoryStore((h) => h.setPage);
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  if (total <= PAGE_SIZE) {
    return (
      <span className="text-[11px] text-fg-muted">
        {total} {total === 1 ? "sesión" : "sesiones"}
      </span>
    );
  }
  const from = page * PAGE_SIZE + 1;
  const to = Math.min(total, (page + 1) * PAGE_SIZE);
  return (
    <div className="flex items-center gap-2">
      <span className="text-[11px] text-fg-muted">
        {from}–{to} de {total}
      </span>
      <div className="flex items-center gap-0.5">
        <IconButton
          title="Página anterior"
          disabled={page === 0}
          onClick={() => setPage(page - 1)}
          className="h-7 w-7"
        >
          <IconChevronLeft width={15} height={15} />
        </IconButton>
        {Array.from({ length: pages }, (_, i) => i)
          .filter((i) => i === 0 || i === pages - 1 || Math.abs(i - page) <= 1)
          .map((i, idx, arr) => (
            <span key={i} className="flex items-center">
              {idx > 0 && arr[idx - 1] !== i - 1 && (
                <span className="px-1 text-[11px] text-fg-muted">…</span>
              )}
              <button
                type="button"
                onClick={() => setPage(i)}
                className={cn(
                  "h-7 min-w-7 rounded-lg px-1.5 text-[11px] transition-colors",
                  i === page
                    ? "bg-indigo-600 text-white"
                    : "text-fg-muted hover:bg-line/10 hover:text-fg",
                )}
              >
                {i + 1}
              </button>
            </span>
          ))}
        <IconButton
          title="Página siguiente"
          disabled={page >= pages - 1}
          onClick={() => setPage(page + 1)}
          className="h-7 w-7"
        >
          <IconChevronRight width={15} height={15} />
        </IconButton>
      </div>
    </div>
  );
}

export function HistoryView() {
  const sessions = useHistoryStore((h) => h.sessions);
  const query = useHistoryStore((h) => h.query);
  const setQuery = useHistoryStore((h) => h.setQuery);
  const loading = useHistoryStore((h) => h.loading);
  const total = useHistoryStore((h) => h.total);
  const load = useHistoryStore((h) => h.load);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line/10 px-5 py-2.5">
        <h1 className="text-base font-semibold text-fg">Historial</h1>
        {loading && <Spinner />}
        <div className="relative ml-auto w-64">
          <IconSearch
            width={14}
            height={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted"
          />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar por título o contenido…"
            className="w-full pl-8"
          />
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
        {sessions.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-fg-muted">
            {query ? "Sin resultados." : "Aún no hay sesiones. Crea una desde «Nueva sesión»."}
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-1.5">
            {sessions.map((s) => (
              <SessionRow key={s.id} s={s} />
            ))}
          </div>
        )}
      </div>

      {total > 0 && (
        <footer className="flex items-center justify-end gap-3 border-t border-line/10 px-5 py-1.5">
          <Pagination />
        </footer>
      )}
    </div>
  );
}
