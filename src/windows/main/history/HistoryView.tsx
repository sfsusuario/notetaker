import { useEffect, useState } from "react";
import { deleteSessionFully, openSession } from "../../../app/actions";
import { fmtDate, fmtDuration, fmtRelative } from "../../../app/format";
import {
  IconEdit,
  IconFileAudio,
  IconHeadphones,
  IconMic,
  IconMore,
  IconSearch,
  IconTrash,
} from "../../../components/common/icons";
import { Badge, Input, Menu, MenuItem, Spinner, cn } from "../../../components/common/ui";
import { exportSession } from "../../../services/storage/export";
import * as db from "../../../services/storage/db";
import { useHistoryStore } from "../../../stores/useHistoryStore";
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
  const toast = useUiStore((x) => x.toast);
  const liveId = useSessionStore((x) => x.sessionId);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(s.title ?? "");
  const isLive = liveId === s.id;

  const commit = async () => {
    setEditing(false);
    if (draft.trim() && draft.trim() !== s.title) await rename(s.id, draft);
  };

  const doExport = async () => {
    const [segments, speakers, chat] = await Promise.all([
      db.getSegments(s.id),
      db.listSpeakers(s.id),
      db.listChatMessages(s.id),
    ]);
    const p = await exportSession({ session: s, segments, speakers, chat }, exportFormat);
    if (p) toast(`Exportado a ${p}`, "ok");
  };

  return (
    <div
      onClick={() => !editing && void openSession(s.id)}
      className={cn(
        "group flex cursor-pointer items-center gap-3 rounded-xl bg-surface px-4 py-3 ring-1 ring-line/10 transition-colors hover:ring-indigo-500/40",
      )}
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
            <span className="truncate text-sm font-medium text-fg">{s.title ?? `Sesión ${fmtDate(s.createdAt)}`}</span>
            {isLive && <Badge tone="bad">Grabando</Badge>}
            {s.status === "processing" && !isLive && (
              <Badge tone="warn">
                <Spinner className="h-2.5 w-2.5" /> Procesando
              </Badge>
            )}
            {s.status === "error" && <Badge tone="bad">Error</Badge>}
            {s.titleAuto && s.title && <span className="text-[10px] text-fg-muted" title="Título generado por IA">✨</span>}
          </div>
        )}
        <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-fg-muted">
          <span title={fmtDate(s.createdAt)}>{fmtRelative(s.createdAt)}</span>
          <span>{fmtDuration(s.durationMs)}</span>
          <SourceIcons s={s} />
          <span>{s.engine === "whisper" ? "Whisper local" : "Deepgram"}</span>
          <span>{s.segmentCount ?? 0} fragmentos</span>
        </div>
      </div>
      <div onClick={(e) => e.stopPropagation()}>
        <Menu trigger={() => <IconMore width={16} height={16} />} title="Sesión">
          {(close) => (
            <>
              <MenuItem onClick={() => { close(); void openSession(s.id); }}>Abrir</MenuItem>
              <MenuItem onClick={() => { close(); setDraft(s.title ?? ""); setEditing(true); }}>
                <span className="flex items-center gap-2"><IconEdit width={13} height={13} /> Renombrar</span>
              </MenuItem>
              <MenuItem onClick={() => { close(); void doExport(); }}>Exportar ({exportFormat})</MenuItem>
              <MenuItem danger disabled={isLive} onClick={() => { close(); void deleteSessionFully(s.id); }}>
                <span className="flex items-center gap-2"><IconTrash width={13} height={13} /> Eliminar</span>
              </MenuItem>
            </>
          )}
        </Menu>
      </div>
    </div>
  );
}

export function HistoryView() {
  const sessions = useHistoryStore((h) => h.sessions);
  const query = useHistoryStore((h) => h.query);
  const setQuery = useHistoryStore((h) => h.setQuery);
  const loading = useHistoryStore((h) => h.loading);
  const load = useHistoryStore((h) => h.load);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line/10 px-6 py-3">
        <h1 className="text-base font-semibold text-fg">Historial</h1>
        <div className="relative ml-auto w-72">
          <IconSearch width={14} height={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-muted" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar por título o contenido…"
            className="w-full pl-8"
          />
        </div>
        {loading && <Spinner />}
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
        {sessions.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-fg-muted">
            {query ? "Sin resultados." : "Aún no hay sesiones. Crea una desde “Nueva sesión”."}
          </div>
        ) : (
          <div className="mx-auto flex max-w-3xl flex-col gap-2">
            {sessions.map((s) => (
              <SessionRow key={s.id} s={s} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
