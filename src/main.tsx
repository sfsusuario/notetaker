import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

/**
 * Panel de error global. Sin esto, un fallo al montar React o una promesa
 * rechazada en el arranque dejan la ventana en negro sin ninguna pista.
 */
function showFatal(title: string, detail: unknown) {
  const text = detail instanceof Error ? (detail.stack ?? detail.message) : String(detail);
  console.error(title, detail);
  const root = document.getElementById("root");
  if (!root) return;
  if (root.querySelector("[data-fatal]")) return;
  const box = document.createElement("div");
  box.setAttribute("data-fatal", "");
  box.style.cssText =
    "position:fixed;inset:auto 12px 12px 12px;z-index:9999;max-height:50vh;overflow:auto;" +
    "background:#3f1d2b;color:#fecdd3;border:1px solid #f43f5e55;border-radius:12px;" +
    "padding:12px 14px;font:12px/1.5 ui-monospace,Consolas,monospace;white-space:pre-wrap";
  box.textContent = `${title}\n\n${text}`;
  root.appendChild(box);
}

window.addEventListener("error", (e) => showFatal("Error en la interfaz", e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) =>
  showFatal("Error no controlado durante el arranque", e.reason),
);

try {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
} catch (e) {
  showFatal("No se pudo iniciar la aplicación", e);
}
