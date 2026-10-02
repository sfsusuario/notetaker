import { useEffect, useState } from "react";
import { AUTO_MODEL, resolveOllamaModel } from "../services/llm/providers/ollama";
import { useSettingsStore } from "../stores/useSettingsStore";

/** Modelo de IA para mostrar; en Ollama "auto" se resuelve al modelo real. */
export function useLlmModelLabel(): string {
  const llm = useSettingsStore((s) => s.llm);
  const [resolved, setResolved] = useState<string | null>(null);
  const isAuto = llm.provider === "ollama" && (!llm.model || llm.model === AUTO_MODEL);

  useEffect(() => {
    setResolved(null);
    if (!isAuto) return;
    let alive = true;
    resolveOllamaModel(llm.model, llm.baseUrl)
      .then((name) => alive && setResolved(name))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [isAuto, llm.model, llm.baseUrl]);

  if (!isAuto) return llm.model;
  return resolved ? `auto · ${resolved}` : "auto";
}
