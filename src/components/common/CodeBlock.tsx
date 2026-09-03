import { useEffect, useState } from "react";
import type { HighlighterGeneric, BundledLanguage, BundledTheme } from "shiki";

const LANGS = [
  "typescript",
  "javascript",
  "tsx",
  "jsx",
  "python",
  "rust",
  "bash",
  "json",
  "yaml",
  "sql",
  "html",
  "css",
  "java",
  "go",
  "csharp",
  "markdown",
] as const;

type Highlighter = HighlighterGeneric<BundledLanguage, BundledTheme>;
let highlighterPromise: Promise<Highlighter> | null = null;

async function getHighlighter(): Promise<Highlighter> {
  if (!highlighterPromise) {
    highlighterPromise = (async () => {
      const { createHighlighter } = await import("shiki");
      const { createJavaScriptRegexEngine } = await import(
        "shiki/engine/javascript"
      );
      // Motor JS (sin WASM) para no necesitar 'wasm-unsafe-eval' en la CSP
      return createHighlighter({
        themes: ["github-dark"],
        langs: [...LANGS],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      });
    })();
  }
  return highlighterPromise;
}

export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const [html, setHtml] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const h = await getHighlighter();
        const safeLang = (LANGS as readonly string[]).includes(lang ?? "")
          ? (lang as BundledLanguage)
          : "markdown";
        const out = h.codeToHtml(code, { lang: safeLang, theme: "github-dark" });
        if (!cancelled) setHtml(out);
      } catch {
        if (!cancelled) setHtml(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [code, lang]);

  const copy = () => {
    void navigator.clipboard.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className="group relative my-2 overflow-hidden rounded-lg border border-zinc-700 bg-zinc-950">
      <div className="flex items-center justify-between border-b border-zinc-800 px-3 py-1 text-xs text-zinc-400">
        <span>{lang ?? "code"}</span>
        <button
          onClick={copy}
          className="rounded px-1.5 py-0.5 hover:bg-zinc-800"
        >
          {copied ? "✓ Copiado" : "Copiar"}
        </button>
      </div>
      {html ? (
        <div
          className="overflow-x-auto p-3 text-[13px] leading-snug [&_pre]:!bg-transparent [&_pre]:!p-0 [&_pre]:!m-0"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <pre className="overflow-x-auto p-3 text-[13px] leading-snug text-zinc-200">
          {code}
        </pre>
      )}
    </div>
  );
}
