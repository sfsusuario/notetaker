import { useEffect, useRef } from "react";

/**
 * Auto-scroll al fondo cuando llega contenido nuevo, salvo que el usuario
 * haya subido manualmente (a más de `slackPx` del fondo).
 */
export function useAutoScroll<T extends HTMLElement>(
  dep: unknown,
  slackPx = 60,
) {
  const ref = useRef<T | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distance < slackPx + el.clientHeight * 0.2 || el.scrollTop === 0) {
      el.scrollTop = el.scrollHeight;
    }
  }, [dep, slackPx]);

  return ref;
}
