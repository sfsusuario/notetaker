import { ollamaChat, ollamaGet, ollamaPost, systemProfile } from "../../ipc/native";
import type { Depth } from "../../../types";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS, EFFORT } from "./types";

/** Valor de `model` que delega la elección en `pickBestModel` en cada petición. */
export const AUTO_MODEL = "auto";

/** Mantiene el modelo cargado durante una reunión (por defecto Ollama lo descarga a los 5 min). */
const KEEP_ALIVE = "30m";

export interface OllamaModel {
  name: string;
  size: number;
  details?: {
    family?: string;
    parameter_size?: string;
    quantization_level?: string;
    context_length?: number;
  };
  capabilities?: string[];
}

interface OllamaPs {
  models: { name: string; size: number; size_vram: number; context_length?: number }[];
}

// ── Elección del mejor modelo ───────────────────────────────────────────────

/**
 * Calidad relativa por familia para responder sobre transcripciones en
 * español. Medido en este proyecto (Arc 140V, reunión de 19 min ≈ 4k tokens,
 * 4 preguntas de detalle): qwen3:8b, llama3.1:8b y gpt-oss:20b acertaron 3.5;
 * gemma3:4b, 2.5 (pero responde 2–3 veces más rápido). Las familias de 2025
 * en adelante rinden más que las de 2024 de tamaño parecido.
 */
const FAMILY_TIERS: ReadonlyArray<readonly [RegExp, number]> = [
  [/^(gemma[34]|qwen3|gpt-oss|mistral-small|magistral|phi4)/, 1.6],
  [/^(llama3|llama4|qwen2|mistral|mixtral|granite3|command-r|aya)/, 1.0],
];
/** Modelos de código: responden peor a preguntas sobre una conversación. */
const CODE_MODEL = /(coder|code|starcoder|codestral|sqlcoder)/;
/**
 * Razonan siempre (solo se puede graduar): cada respuesta paga ese tiempo.
 * gpt-oss:20b acertó lo mismo que qwen3:8b pero con el doble de memoria de
 * GPU (compartida con whisper) y en la app mezcló inglés en la respuesta.
 */
const ALWAYS_THINKS = /^gpt-oss/;
/** Modelos que no generan texto (embeddings, rerankers). */
const NON_CHAT = /(embed|bge-|minilm|nomic|rerank)/;

/** Parámetros en miles de millones ("4.3B", "567M"); si falta, se estima por tamaño (Q4 ≈ 0.6 GB/B). */
function paramsB(m: OllamaModel): number {
  const ps = m.details?.parameter_size ?? "";
  const match = /([\d.]+)\s*([BM])/i.exec(ps);
  if (match) return Number(match[1]) / (match[2].toUpperCase() === "M" ? 1000 : 1);
  return m.size / 0.6e9;
}

/** 0 = no sirve para chat. */
export function scoreModel(m: OllamaModel): number {
  const name = m.name.toLowerCase();
  if (NON_CHAT.test(name)) return 0;
  if (m.capabilities && !m.capabilities.includes("completion")) return 0;
  const tier = FAMILY_TIERS.find(([re]) => re.test(name))?.[1] ?? 1.0;
  const kind = CODE_MODEL.test(name) ? 0.4 : ALWAYS_THINKS.test(name) ? 0.7 : 1;
  // Más parámetros ayuda hasta ~14B; por encima apenas mejora para esta tarea
  // y en un portátil se vuelve demasiado lento para chatear en vivo.
  return tier * kind * Math.log2(1 + Math.min(paramsB(m), 14));
}

/** Mejor modelo de chat que cabe en `maxBytes`; a igual puntuación, el más ligero. */
export function pickBestModel(models: OllamaModel[], maxBytes?: number): OllamaModel | null {
  const ranked = models
    .filter((m) => scoreModel(m) > 0 && (!maxBytes || m.size <= maxBytes))
    .sort((a, b) => scoreModel(b) - scoreModel(a) || a.size - b.size);
  return ranked[0] ?? null;
}

/**
 * Contexto a pedir. Sin `num_ctx`, Ollama usa 4096 tokens y recorta EN
 * SILENCIO el principio del prompt: con una reunión de 19 min, llama3.1:8b
 * recibió 2050 tokens de ~4300 e inventó las respuestas. Se usa ~3 caracteres
 * por token (en español son 3.4–3.9: mejor sobrar que recortar) y escalones
 * fijos para que Ollama no recargue el modelo en cada pregunta.
 */
export function contextFor(promptChars: number, maxOutput: number, modelMax?: number): number {
  const need = Math.ceil(promptChars / 3) + maxOutput + 256;
  const steps = [8192, 16384, 32768];
  const ctx = steps.find((s) => s >= need) ?? steps[steps.length - 1];
  return modelMax ? Math.min(ctx, modelMax) : ctx;
}

// ── Caché de modelos instalados ─────────────────────────────────────────────

let tagsCache: { base: string; at: number; models: OllamaModel[] } | null = null;
let ramBudget: Promise<number | undefined> | null = null;

async function installedModels(baseUrl?: string, fresh = false): Promise<OllamaModel[]> {
  const key = baseUrl ?? "";
  if (!fresh && tagsCache && tagsCache.base === key && Date.now() - tagsCache.at < 60_000) {
    return tagsCache.models;
  }
  const json = await ollamaGet<{ models?: OllamaModel[] }>(baseUrl, "/api/tags");
  const models = json.models ?? [];
  tagsCache = { base: key, at: Date.now(), models };
  return models;
}

/** Modelos de más de la mitad de la RAM dejarían al equipo sin memoria. */
function maxModelBytes(): Promise<number | undefined> {
  ramBudget ??= systemProfile()
    .then((p) => p.ramTotalGb * 0.5 * 1024 ** 3)
    .catch(() => undefined);
  return ramBudget;
}

/** Nombre real del modelo a usar ("auto" → el mejor instalado). */
export async function resolveOllamaModel(model: string, baseUrl?: string, fresh = false): Promise<string> {
  const models = await installedModels(baseUrl, fresh);
  if (model && model !== AUTO_MODEL) {
    const found = models.find((m) => m.name === model || m.name === `${model}:latest`);
    if (found) return found.name;
    throw new Error(`El modelo "${model}" no está instalado en Ollama. Elige "auto" o instálalo con: ollama pull ${model}`);
  }
  const best = pickBestModel(models, await maxModelBytes());
  if (!best) {
    throw new Error("Ollama no tiene modelos de chat instalados. Instala uno, p. ej.: ollama pull gemma3:4b");
  }
  return best.name;
}

/** Contexto con el que Ollama tiene cargado el modelo (0 si no lo está). */
async function loadedContext(model: string, baseUrl?: string): Promise<number> {
  try {
    const ps = await ollamaGet<OllamaPs>(baseUrl, "/api/ps");
    return ps.models.find((m) => m.name === model)?.context_length ?? 0;
  } catch {
    return 0;
  }
}

/** Razonamiento, tokens de salida y contexto para una petición. */
async function requestPlan(model: string, promptChars: number, depth: Depth, baseUrl?: string) {
  const info = (await installedModels(baseUrl)).find((m) => m.name === model);
  // Modelos de razonamiento: sin pensar (la respuesta llega mucho antes);
  // gpt-oss no permite apagarlo, solo graduarlo.
  const think = info?.capabilities?.includes("thinking")
    ? ALWAYS_THINKS.test(model)
      ? EFFORT[depth]
      : false
    : undefined;
  // El razonamiento cuenta contra num_predict: sin margen, un título
  // (200 tokens) puede agotarse pensando y llegar vacío.
  const numPredict = DEPTH_MAX_TOKENS[depth] + (think ? 1024 : 0);
  // Nunca por debajo del contexto ya cargado: cambiarlo obliga a Ollama a
  // recargar el modelo (30–40 s con gpt-oss:20b).
  const numCtx = Math.max(
    contextFor(promptChars, numPredict, info?.details?.context_length),
    await loadedContext(model, baseUrl),
  );
  return { think, numPredict, numCtx };
}

/**
 * Carga el modelo en memoria sin generar nada, con el contexto que pedirá el
 * chat sobre una transcripción de `promptChars` caracteres.
 */
async function warmup(model: string, baseUrl?: string, promptChars = 0): Promise<void> {
  const name = await resolveOllamaModel(model, baseUrl);
  const { numCtx } = await requestPlan(name, promptChars, "standard", baseUrl);
  await ollamaPost(baseUrl, "/api/generate", { model: name, keep_alive: KEEP_ALIVE, options: { num_ctx: numCtx } });
}

// ── Proveedor ───────────────────────────────────────────────────────────────

/** Ollama es local: no requiere API key (secretName omitido). */
export const ollamaProvider: LlmProvider = {
  id: "ollama",
  models: [AUTO_MODEL],

  async test(model, baseUrl) {
    const { version } = await ollamaGet<{ version: string }>(baseUrl, "/api/version");
    const name = await resolveOllamaModel(model, baseUrl, true);
    await warmup(name, baseUrl);
    const ps = await ollamaGet<OllamaPs>(baseUrl, "/api/ps");
    const loaded = ps.models.find((m) => m.name === name);
    const auto = !model || model === AUTO_MODEL ? " (automático)" : "";
    let where = "";
    if (loaded) {
      const gpu = loaded.size > 0 ? loaded.size_vram / loaded.size : 0;
      where = gpu >= 0.95 ? " · en GPU" : gpu > 0 ? ` · ${Math.round(gpu * 100)} % en GPU` : " · en CPU";
    }
    let msg = `${name}${auto} · Ollama ${version}${where}`;
    if (where === " · en CPU") {
      msg +=
        ". Ollama no está usando la GPU: si es integrada (Intel/AMD), define la variable OLLAMA_IGPU_ENABLE=1 (y GGML_VK_DISABLE_COOPMAT=1 con drivers Intel antiguos) y reinicia Ollama.";
    }
    return msg;
  },

  async listModels(baseUrl) {
    const models = await installedModels(baseUrl, true);
    const chat = models
      .filter((m) => scoreModel(m) > 0)
      .sort((a, b) => scoreModel(b) - scoreModel(a) || a.size - b.size)
      .map((m) => m.name);
    return [AUTO_MODEL, ...chat];
  },

  warmup,

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    const model = await resolveOllamaModel(args.model, args.baseUrl);
    const systemText = args.system.map((b) => b.text).join("\n\n");

    interface OllamaMsg {
      role: string;
      content: string;
      images?: string[];
    }
    const messages: OllamaMsg[] = [
      { role: "system", content: systemText },
      ...args.history.map((h) => ({ role: h.role, content: h.text })),
      {
        role: "user",
        content: args.turn.text,
        ...(args.turn.imageBase64 ? { images: [args.turn.imageBase64] } : {}),
      },
    ];
    const promptChars = messages.reduce((n, m) => n + m.content.length, 0);
    const { think, numPredict, numCtx } = await requestPlan(model, promptChars, args.depth, args.baseUrl);

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };
    await ollamaChat(
      args.baseUrl,
      {
        model,
        messages,
        keep_alive: KEEP_ALIVE,
        ...(think !== undefined ? { think } : {}),
        options: { num_predict: numPredict, num_ctx: numCtx },
      },
      (obj) => {
        const json = obj as {
          message?: { content?: string };
          prompt_eval_count?: number;
          eval_count?: number;
          done?: boolean;
        };
        const delta = json.message?.content;
        if (delta) {
          text += delta;
          args.onDelta(delta);
        }
        if (json.done) {
          usage = {
            inputTokens: json.prompt_eval_count ?? 0,
            outputTokens: json.eval_count ?? 0,
            cacheReadTokens: 0,
          };
        }
      },
      args.signal,
    );

    return { text, usage, refused: false };
  },
};
