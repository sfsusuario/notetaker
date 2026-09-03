import Anthropic from "@anthropic-ai/sdk";
import { secretGet } from "../../ipc/native";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { EFFORT } from "./types";

let client: Anthropic | null = null;
let clientKey: string | null = null;

async function getClient(): Promise<Anthropic> {
  const key = await secretGet("anthropic");
  if (!key) throw new Error("Falta la API key de Anthropic.");
  if (!client || clientKey !== key) {
    // dangerouslyAllowBrowser: key del usuario, en el keyring del SO, nunca en el bundle
    client = new Anthropic({ apiKey: key, dangerouslyAllowBrowser: true });
    clientKey = key;
  }
  return client;
}

export const anthropicProvider: LlmProvider = {
  id: "anthropic",
  secretName: "anthropic",
  models: ["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5"],

  async test(model) {
    const c = await getClient();
    const m = await c.models.retrieve(model);
    return m.display_name;
  },

  async listModels() {
    const c = await getClient();
    const page = await c.models.list();
    return page.data.map((m) => m.id);
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    const c = await getClient();

    const system = args.system.map((b) =>
      b.cache
        ? {
            type: "text" as const,
            text: b.text,
            cache_control: { type: "ephemeral" as const },
          }
        : { type: "text" as const, text: b.text },
    );

    const content: Anthropic.ContentBlockParam[] = [];
    if (args.turn.imageBase64) {
      content.push({
        type: "image",
        source: {
          type: "base64",
          media_type: "image/png",
          data: args.turn.imageBase64,
        },
      });
    }
    content.push({ type: "text", text: args.turn.text });

    const stream = c.messages.stream(
      {
        model: args.model,
        max_tokens: 64000,
        thinking: { type: "adaptive" },
        output_config: { effort: EFFORT[args.depth] },
        system,
        messages: [
          ...args.history.map((h) => ({ role: h.role, content: h.text })),
          { role: "user" as const, content },
        ],
      },
      { signal: args.signal },
    );

    stream.on("text", (delta) => args.onDelta(delta));
    const final = await stream.finalMessage();

    const text = final.content
      .filter((b): b is Anthropic.TextBlock => b.type === "text")
      .map((b) => b.text)
      .join("");

    return {
      text,
      usage: {
        inputTokens: final.usage.input_tokens,
        outputTokens: final.usage.output_tokens,
        cacheReadTokens: final.usage.cache_read_input_tokens ?? 0,
      },
      refused: final.stop_reason === "refusal",
    };
  },
};
