/**
 * Itera los payloads `data:` de un stream Server-Sent Events (fetch body).
 * Común a OpenAI, Gemini y Ollama; Anthropic usa su propio SDK.
 */
function* linesToData(chunk: string): Generator<string> {
  for (const line of chunk.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("data:")) {
      yield trimmed.slice(5).trim();
    }
  }
}

export async function* streamSse(
  body: ReadableStream<Uint8Array>,
): AsyncGenerator<string> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      // Normaliza CRLF → LF: algunos servidores (Gemini) separan eventos con
      // "\r\n\r\n", que NO contiene la subcadena "\n\n" que buscamos como
      // límite — sin esto, indexOf("\n\n") nunca encuentra nada y no se
      // emite NINGÚN evento, aunque la respuesta llegue completa y sea 200 OK.
      buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, "\n");

      let idx: number;
      // Los eventos SSE se separan por línea en blanco (\n\n)
      while ((idx = buffer.indexOf("\n\n")) !== -1) {
        const chunk = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);
        yield* linesToData(chunk);
      }
    }
    // El último evento puede llegar sin la línea en blanco final si el
    // servidor cierra la conexión justo después de escribirlo.
    if (buffer.trim()) yield* linesToData(buffer);
  } finally {
    reader.releaseLock();
  }
}

/** Itera objetos JSON delimitados por línea (NDJSON) — lo usa Ollama. */
export async function* streamNdjson(
  body: ReadableStream<Uint8Array>,
): AsyncGenerator<unknown> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let idx: number;
      while ((idx = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (line) yield JSON.parse(line);
      }
    }
  } finally {
    reader.releaseLock();
  }
}
