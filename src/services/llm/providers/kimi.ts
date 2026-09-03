import { secretGet } from "../../ipc/native";
import { streamSse } from "./sse";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS } from "./types";

// Kimi (Moonshot AI) expone una API compatible con OpenAI.
// .ai = endpoint internacional; .cn = China. Se puede sobrescribir con baseUrl.
const BASE = "https://api.moonshot.ai/v1";

async function key(): Promise<string> {
  const k = await secretGet("kimi");
  if (!k) throw new Error("Falta la API key de Kimi (Moonshot).");
  return k;
}

export const kimiProvider: LlmProvider = {
  id: "kimi",
  secretName: "kimi",
  // OJO: .ai (internacional) y .cn (China) son catálogos DISTINTOS sobre la
  // misma familia de nombres — un id válido en .cn (p. ej. "kimi-k2-0905-preview")
  // no existe en .ai, y el server .ai responde 401 (no 404) ante un modelo
  // desconocido, así que el fallo se ve como "API key inválida" sin serlo.
  // "kimi-k2.6" es el id internacional confirmado (ver CV-Generator, mismo
  // endpoint). "kimi-k2.7-code-highspeed" es una variante más rápida/óptima
  // orientada a código (útil para el trigger "code"). "kimi-latest" es el
  // alias estable de Moonshot al modelo K2 vigente. El botón ↻ trae la lista
  // real de la cuenta.
  models: [
    "kimi-k2.6",
    "kimi-k2.7-code-highspeed",
    "kimi-latest",
    "moonshot-v1-128k",
    "moonshot-v1-32k",
    "moonshot-v1-8k",
  ],

  async test(model, baseUrl) {
    const res = await fetch(`${baseUrl || BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model,
        max_tokens: 1,
        // Los modelos de razonamiento de Kimi (k2.x) rechazan cualquier
        // temperature != 1 con 400; enviarlo siempre es inofensivo para el
        // resto (moonshot-v1-*) y evita el fallo silencioso por omisión.
        temperature: 1,
        messages: [{ role: "user", content: "ping" }],
      }),
    });
    if (!res.ok) throw new Error(`Kimi ${res.status}: ${await res.text()}`);
    return model;
  },

  async listModels(baseUrl) {
    const res = await fetch(`${baseUrl || BASE}/models`, {
      headers: { Authorization: `Bearer ${await key()}` },
    });
    if (!res.ok) throw new Error(`Kimi ${res.status}: ${await res.text()}`);
    const json = await res.json();
    return (json.data ?? []).map((m: { id: string }) => m.id).sort();
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    // Kimi chat es solo texto: la captura de pantalla se ignora.
    const systemText = args.system.map((b) => b.text).join("\n\n");
    const messages = [
      { role: "system", content: systemText },
      ...args.history.map((h) => ({ role: h.role, content: h.text })),
      { role: "user", content: args.turn.text },
    ];

    const res = await fetch(`${args.baseUrl || BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model: args.model,
        max_tokens: DEPTH_MAX_TOKENS[args.depth],
        // Ver nota en test(): los modelos k2.x de Kimi solo aceptan
        // temperature: 1 (400 "invalid temperature" con cualquier otro valor).
        temperature: 1,
        stream: true,
        messages,
      }),
      signal: args.signal,
    });
    if (!res.ok || !res.body) {
      throw new Error(`Kimi ${res.status}: ${await res.text()}`);
    }

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };

    for await (const data of streamSse(res.body)) {
      if (data === "[DONE]") break;
      const json = JSON.parse(data);
      const delta: string | undefined = json.choices?.[0]?.delta?.content;
      if (delta) {
        text += delta;
        args.onDelta(delta);
      }
      if (json.usage) {
        usage = {
          inputTokens: json.usage.prompt_tokens ?? 0,
          outputTokens: json.usage.completion_tokens ?? 0,
          cacheReadTokens: json.usage.cached_tokens ?? 0,
        };
      }
    }

    return { text, usage, refused: false };
  },
};
