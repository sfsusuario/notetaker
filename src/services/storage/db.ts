import Database from "@tauri-apps/plugin-sql";
import type {
  ChatMessage,
  Segment,
  SegmentSource,
  Session,
  SessionEngine,
  SessionMode,
  SessionStatus,
  SpeakerOverride,
} from "../../types";

let db: Database | null = null;

async function getDb(): Promise<Database> {
  if (!db) {
    db = await Database.load("sqlite:notetaker.db");
    await db.execute("PRAGMA foreign_keys = ON");
  }
  return db;
}

// ── Sesiones ─────────────────────────────────────────────────────────────────

interface SessionRow {
  id: string;
  title: string | null;
  title_auto: number;
  created_at: number;
  ended_at: number | null;
  mode: string;
  engine: string;
  engine_model: string | null;
  sources: string;
  language: string | null;
  source_file_path: string | null;
  audio_dir: string | null;
  duration_ms: number | null;
  status: string;
  notes: string | null;
  tags: string | null;
  segment_count?: number;
}

function parseJsonArray(s: string | null): string[] {
  if (!s) return [];
  try {
    const v = JSON.parse(s);
    return Array.isArray(v) ? v.map(String) : [];
  } catch {
    return [];
  }
}

function rowToSession(r: SessionRow): Session {
  return {
    id: r.id,
    title: r.title,
    titleAuto: r.title_auto !== 0,
    createdAt: r.created_at,
    endedAt: r.ended_at,
    mode: r.mode as SessionMode,
    engine: r.engine as SessionEngine,
    engineModel: r.engine_model,
    sources: parseJsonArray(r.sources) as SegmentSource[],
    language: r.language,
    sourceFilePath: r.source_file_path,
    audioDir: r.audio_dir,
    durationMs: r.duration_ms,
    status: r.status as SessionStatus,
    notes: r.notes,
    tags: parseJsonArray(r.tags),
    segmentCount: r.segment_count,
  };
}

export async function insertSession(s: {
  id: string;
  mode: SessionMode;
  engine: string;
  engineModel: string | null;
  sources: SegmentSource[];
  language: string | null;
  sourceFilePath?: string | null;
  audioDir?: string | null;
  status: SessionStatus;
  title?: string | null;
}): Promise<void> {
  const d = await getDb();
  await d.execute(
    `INSERT OR IGNORE INTO sessions
      (id, title, title_auto, created_at, mode, engine, engine_model, sources, language, source_file_path, audio_dir, status)
     VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8, $9, $10, $11)`,
    [
      s.id,
      s.title ?? null,
      Date.now(),
      s.mode,
      s.engine,
      s.engineModel,
      JSON.stringify(s.sources),
      s.language,
      s.sourceFilePath ?? null,
      s.audioDir ?? null,
      s.status,
    ],
  );
}

export async function updateSession(
  id: string,
  patch: Partial<{
    endedAt: number | null;
    durationMs: number | null;
    status: SessionStatus;
    audioDir: string | null;
    language: string | null;
    engine: string;
    engineModel: string | null;
    notes: string | null;
    tags: string[];
    sources: SegmentSource[];
  }>,
): Promise<void> {
  const sets: string[] = [];
  const args: unknown[] = [];
  const push = (col: string, v: unknown) => {
    args.push(v);
    sets.push(`${col} = $${args.length}`);
  };
  if (patch.endedAt !== undefined) push("ended_at", patch.endedAt);
  if (patch.durationMs !== undefined) push("duration_ms", patch.durationMs);
  if (patch.status !== undefined) push("status", patch.status);
  if (patch.audioDir !== undefined) push("audio_dir", patch.audioDir);
  if (patch.language !== undefined) push("language", patch.language);
  if (patch.engine !== undefined) push("engine", patch.engine);
  if (patch.engineModel !== undefined) push("engine_model", patch.engineModel);
  if (patch.notes !== undefined) push("notes", patch.notes);
  if (patch.tags !== undefined) push("tags", JSON.stringify(patch.tags));
  if (patch.sources !== undefined) push("sources", JSON.stringify(patch.sources));
  if (sets.length === 0) return;
  args.push(id);
  const d = await getDb();
  await d.execute(
    `UPDATE sessions SET ${sets.join(", ")} WHERE id = $${args.length}`,
    args,
  );
}

/** `auto=false` cuando lo edita el usuario: un título automático posterior no lo pisa. */
export async function setTitle(id: string, title: string, auto: boolean): Promise<void> {
  const d = await getDb();
  await d.execute("UPDATE sessions SET title = $1, title_auto = $2 WHERE id = $3", [
    title,
    auto ? 1 : 0,
    id,
  ]);
}

export async function getSession(id: string): Promise<Session | null> {
  const d = await getDb();
  const rows = await d.select<SessionRow[]>(
    `SELECT s.*, (SELECT COUNT(*) FROM segments WHERE session_id = s.id) AS segment_count
     FROM sessions s WHERE id = $1`,
    [id],
  );
  return rows[0] ? rowToSession(rows[0]) : null;
}

export async function listSessions(args: {
  query?: string;
  limit?: number;
  offset?: number;
} = {}): Promise<Session[]> {
  const d = await getDb();
  const q = args.query?.trim() ?? "";
  const limit = Math.max(1, args.limit ?? 200);
  const offset = Math.max(0, args.offset ?? 0);
  const base = `SELECT s.*, (SELECT COUNT(*) FROM segments WHERE session_id = s.id) AS segment_count FROM sessions s`;
  const rows = q
    ? await d.select<SessionRow[]>(
        `${base}
         WHERE s.title LIKE $1
            OR EXISTS (SELECT 1 FROM segments g WHERE g.session_id = s.id AND g.text LIKE $1)
         ORDER BY s.created_at DESC LIMIT $2 OFFSET $3`,
        [`%${q}%`, limit, offset],
      )
    : await d.select<SessionRow[]>(
        `${base} ORDER BY s.created_at DESC LIMIT $1 OFFSET $2`,
        [limit, offset],
      );
  return rows.map(rowToSession);
}

/**
 * Cierra sesiones que quedaron en "recording" porque la app terminó de forma
 * abrupta (cierre forzado, reinicio en desarrollo). El audio ya está en disco,
 * así que se marcan como transcritas o pendientes según tengan segmentos.
 */
export async function closeOrphanSessions(): Promise<number> {
  const d = await getDb();
  const rows = await d.select<Array<{ n: number }>>(
    "SELECT COUNT(*) AS n FROM sessions WHERE status = 'recording'",
  );
  const n = rows[0]?.n ?? 0;
  if (n === 0) return 0;
  await d.execute(`
    UPDATE sessions
       SET status = CASE
             WHEN (SELECT COUNT(*) FROM segments WHERE session_id = sessions.id) > 0
             THEN 'done' ELSE 'recorded' END,
           ended_at = COALESCE(ended_at, created_at)
     WHERE status = 'recording'`);
  return n;
}

/** Total de sesiones (con el mismo filtro que `listSessions`), para paginar. */
export async function countSessions(query?: string): Promise<number> {
  const d = await getDb();
  const q = query?.trim() ?? "";
  const rows = q
    ? await d.select<Array<{ n: number }>>(
        `SELECT COUNT(*) AS n FROM sessions s
          WHERE s.title LIKE $1
             OR EXISTS (SELECT 1 FROM segments g WHERE g.session_id = s.id AND g.text LIKE $1)`,
        [`%${q}%`],
      )
    : await d.select<Array<{ n: number }>>("SELECT COUNT(*) AS n FROM sessions");
  return rows[0]?.n ?? 0;
}

export async function deleteSession(id: string): Promise<void> {
  const d = await getDb();
  await d.execute("DELETE FROM segments WHERE session_id = $1", [id]);
  await d.execute("DELETE FROM chat_messages WHERE session_id = $1", [id]);
  await d.execute("DELETE FROM speakers WHERE session_id = $1", [id]);
  await d.execute("DELETE FROM sessions WHERE id = $1", [id]);
}

// ── Segmentos ────────────────────────────────────────────────────────────────

interface SegmentRow {
  id: string;
  session_id: string;
  source: string;
  speaker: string | null;
  text: string;
  start_ms: number;
  end_ms: number;
  received_at: number;
  language: string | null;
}

function rowToSegment(r: SegmentRow): Segment {
  return {
    id: r.id,
    sessionId: r.session_id,
    source: r.source as SegmentSource,
    speaker: r.speaker,
    text: r.text,
    startMs: r.start_ms,
    endMs: r.end_ms,
    receivedAt: r.received_at,
    isFinal: true,
    language: r.language,
  };
}

export async function insertSegment(seg: Segment): Promise<void> {
  const d = await getDb();
  await d.execute(
    `INSERT OR REPLACE INTO segments (id, session_id, source, speaker, text, start_ms, end_ms, received_at, language)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
    [
      seg.id,
      seg.sessionId,
      seg.source,
      seg.speaker,
      seg.text,
      seg.startMs,
      seg.endMs,
      seg.receivedAt,
      seg.language,
    ],
  );
}

export async function deleteSegments(sessionId: string): Promise<void> {
  const d = await getDb();
  await d.execute("DELETE FROM segments WHERE session_id = $1", [sessionId]);
}

export async function getSegments(sessionId: string): Promise<Segment[]> {
  const d = await getDb();
  const rows = await d.select<SegmentRow[]>(
    "SELECT * FROM segments WHERE session_id = $1 ORDER BY received_at, start_ms",
    [sessionId],
  );
  return rows.map(rowToSegment);
}

// ── Chat ─────────────────────────────────────────────────────────────────────

export async function insertChatMessage(m: ChatMessage): Promise<void> {
  const d = await getDb();
  await d.execute(
    "INSERT OR REPLACE INTO chat_messages (id, session_id, role, content, created_at) VALUES ($1, $2, $3, $4, $5)",
    [m.id, m.sessionId, m.role, m.content, m.createdAt],
  );
}

export async function listChatMessages(sessionId: string): Promise<ChatMessage[]> {
  const d = await getDb();
  const rows = await d.select<
    Array<{ id: string; session_id: string; role: string; content: string; created_at: number }>
  >("SELECT * FROM chat_messages WHERE session_id = $1 ORDER BY created_at", [sessionId]);
  return rows.map((r) => ({
    id: r.id,
    sessionId: r.session_id,
    role: r.role as ChatMessage["role"],
    content: r.content,
    createdAt: r.created_at,
  }));
}

export async function clearChat(sessionId: string): Promise<void> {
  const d = await getDb();
  await d.execute("DELETE FROM chat_messages WHERE session_id = $1", [sessionId]);
}

// ── Hablantes (etiquetas personalizadas) ─────────────────────────────────────

export async function upsertSpeaker(sessionId: string, speakerKey: string, label: string) {
  const d = await getDb();
  await d.execute(
    "INSERT OR REPLACE INTO speakers (session_id, speaker_key, label) VALUES ($1, $2, $3)",
    [sessionId, speakerKey, label],
  );
}

export async function listSpeakers(sessionId: string): Promise<SpeakerOverride[]> {
  const d = await getDb();
  const rows = await d.select<Array<{ speaker_key: string; label: string }>>(
    "SELECT speaker_key, label FROM speakers WHERE session_id = $1",
    [sessionId],
  );
  return rows.map((r) => ({ speakerKey: r.speaker_key, label: r.label }));
}
