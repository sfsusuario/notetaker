import { secretGet } from "../../ipc/native";
import { streamSse } from "./sse";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS } from "./types";

const BASE = "https://generativelanguage.googleapis.com/v1beta/models";

async function key(): Promise<string> {
  const k = await secretGet("gemini");
  if (!k) throw new Error("Falta la API key de Gemini.");
  return k;
}

export const geminiProvider: LlmProvider = {
  id: "gemini",
  secretName: "gemini",
  // OJO: no listar aquí ids especulativos sin verificar contra la API real —
  // un id inexistente que quede primero se vuelve el modelo por DEFECTO al
  // cambiar de proveedor (PROVIDERS[id].models[0]) y rompe la conexión de
  // forma silenciosa. "gemini-2.0-flash" está confirmado funcionando (mismo
  // endpoint v1beta, misma key por query param) fuera de este proyecto.
  models: [
    "gemini-2.5-flash",
    "gemini-2.0-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-lite",
  ],

  async test(model) {
    const res = await fetch(`${BASE}/${model}?key=${await key()}`);
    if (!res.ok) throw new Error(`Gemini ${res.status}: ${await res.text()}`);
    const json = await res.json();
    return json.displayName ?? model;
  },

  async listModels() {
    const res = await fetch(`${BASE}?key=${await key()}&pageSize=200`);
    if (!res.ok) throw new Error(`Gemini ${res.status}: ${await res.text()}`);
    const json = await res.json();
    return (json.models ?? [])
      .filter((m: { supportedGenerationMethods?: string[] }) =>
        (m.supportedGenerationMethods ?? []).includes("generateContent"),
      )
      .map((m: { name: string }) => m.name.replace(/^models\//, ""))
      .filter((id: string) => id.startsWith("gemini"));
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    const systemText = args.system.map((b) => b.text).join("\n\n");

    const parts: unknown[] = [{ text: args.turn.text }];
    if (args.turn.imageBase64) {
      parts.push({
        inline_data: { mime_type: "image/png", data: args.turn.imageBase64 },
      });
    }

    const contents = [
      ...args.history.map((h) => ({
        role: h.role === "assistant" ? "model" : "user",
        parts: [{ text: h.text }],
      })),
      { role: "user", parts },
    ];

    const url = `${BASE}/${args.model}:streamGenerateContent?alt=sse&key=${await key()}`;
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        contents,
        systemInstruction: { parts: [{ text: systemText }] },
        generationConfig: {
          maxOutputTokens: DEPTH_MAX_TOKENS[args.depth],
          // Los modelos 2.5 "piensan" por defecto y el pensamiento consume el
          // presupuesto de tokens, dejando el texto vacío. Lo desactivamos.
          thinkingConfig: { thinkingBudget: 0 },
        },
      }),
      signal: args.signal,
    });
    if (!res.ok || !res.body) {
      throw new Error(`Gemini ${res.status}: ${await res.text()}`);
    }

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };
    // Diagnóstico para cuando Gemini responde 200 OK pero sin texto (bloqueo
    // de seguridad, corte por longitud, prompt sin contenido…): sin esto el
    // "vacío" llega al usuario sin ninguna pista de la causa real.
    let blockReason: string | undefined;
    let finishReason: string | undefined;
    // Payloads crudos (sin parsear candidatos) por si el vacío final no deja
    // ninguna pista arriba: revela si el servidor no manda eventos en
    // absoluto o si manda una forma de respuesta que no reconocemos.
    const rawEvents: string[] = [];

    for await (const data of streamSse(res.body)) {
      rawEvents.push(data);
      let json: Record<string, unknown>;
      try {
        json = JSON.parse(data);
      } catch {
        continue; // evento no-JSON (p. ej. un keep-alive); se ignora
      }
      blockReason ??= (json.promptFeedback as { blockReason?: string } | undefined)
        ?.blockReason;
      const candidate = (
        json.candidates as
          | Array<{ finishReason?: string; content?: { parts?: { text?: string }[] } }>
          | undefined
      )?.[0];
      finishReason = candidate?.finishReason ?? finishReason;
      const delta: string | undefined = candidate?.content?.parts
        ?.map((p) => p.text ?? "")
        .join("");
      if (delta) {
        text += delta;
        args.onDelta(delta);
      }
      const um = json.usageMetadata as
        | {
            promptTokenCount?: number;
            candidatesTokenCount?: number;
            cachedContentTokenCount?: number;
          }
        | undefined;
      if (um) {
        usage = {
          inputTokens: um.promptTokenCount ?? 0,
          outputTokens: um.candidatesTokenCount ?? 0,
          cacheReadTokens: um.cachedContentTokenCount ?? 0,
        };
      }
    }

    if (text.trim() === "") {
      // Bloqueo por seguridad: es un "refusal" real, no un fallo de red.
      if (blockReason || finishReason === "SAFETY") {
        return { text: "", usage, refused: true };
      }
      const detail = finishReason ? ` (finishReason: ${finishReason})` : "";
      const raw =
        rawEvents.length > 0
          ? ` Payload: ${rawEvents.join(" | ").slice(0, 500)}`
          : " El servidor no devolvió ningún evento de datos.";
      throw new Error(`Gemini: respuesta vacía sin candidatos${detail}.${raw}`);
    }

    return { text, usage, refused: false };
  },
};
