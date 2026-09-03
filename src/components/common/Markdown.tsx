import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { CodeBlock } from "./CodeBlock";

/** Bloque de código ligero para el streaming: mismo aspecto que CodeBlock pero
 *  sin Shiki (no re-resalta en cada token). El resaltado llega al terminar. */
function LiteCode({ code, lang }: { code: string; lang?: string }) {
  return (
    <div className="my-2 overflow-hidden rounded-lg border border-zinc-700 bg-zinc-950">
      <div className="border-b border-zinc-800 px-3 py-1 text-xs text-zinc-400">
        {lang ?? "code"}
      </div>
      <pre className="overflow-x-auto p-3 text-[13px] leading-snug text-zinc-200">
        {code}
      </pre>
    </div>
  );
}

/**
 * Renderiza Markdown. En `streaming` usa un bloque de código ligero (sin Shiki)
 * para que el texto se lea ya formateado token a token sin el coste del
 * resaltado; al finalizar se vuelve a montar con el resaltado completo.
 */
export function Markdown({
  children,
  streaming = false,
}: {
  children: string;
  streaming?: boolean;
}) {
  return (
    <div className="text-sm leading-relaxed text-fg [&_h1]:text-base [&_h1]:font-semibold [&_h2]:text-sm [&_h2]:font-semibold [&_h3]:text-sm [&_h3]:font-medium [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5 [&_p]:my-1.5 [&_li]:my-0.5 [&_table]:text-xs [&_blockquote]:border-l-2 [&_blockquote]:border-line/30 [&_blockquote]:pl-3 [&_blockquote]:text-fg-muted">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { children, className, ...rest } = props;
            const match = /language-(\w+)/.exec(className ?? "");
            const text = String(children).replace(/\n$/, "");
            if (match || text.includes("\n")) {
              return streaming ? (
                <LiteCode code={text} lang={match?.[1]} />
              ) : (
                <CodeBlock code={text} lang={match?.[1]} />
              );
            }
            return (
              <code
                className="rounded bg-line/10 px-1 py-0.5 font-mono text-[12px] text-indigo-400"
                {...rest}
              >
                {children}
              </code>
            );
          },
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
