//! Proxy hacia Ollama. El WebView de la app compilada tiene el origen
//! `http://tauri.localhost`, que Ollama rechaza con 403 (sus orígenes
//! permitidos por defecto son localhost, 127.0.0.1, tauri://, app://…): solo
//! funcionaba en desarrollo (`http://localhost:1420`). Desde Rust no hay CORS
//! porque reqwest no envía `Origin`, así que no hace falta tocar OLLAMA_ORIGINS.
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::watch;

use crate::state::AppState;

const DEFAULT_BASE: &str = "http://localhost:11434";
/// Solo lectura, sin efectos.
const GET_PATHS: &[&str] = &["/api/tags", "/api/ps", "/api/version"];
/// `/api/generate` se usa sin prompt para precargar el modelo.
const POST_PATHS: &[&str] = &["/api/show", "/api/generate"];

fn endpoint(base_url: Option<&str>, path: &str) -> Result<(String, String), String> {
    let base = base_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BASE)
        .trim_end_matches('/')
        .to_string();
    let url = url::Url::parse(&base).map_err(|e| format!("URL de Ollama no válida ({base}): {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("La URL de Ollama debe empezar por http:// o https://".into());
    }
    Ok((format!("{base}{path}"), base))
}

fn request_error(base: &str, e: reqwest::Error) -> String {
    if e.is_connect() {
        format!("No se pudo conectar con Ollama en {base}. ¿Está abierto?")
    } else if e.is_timeout() {
        format!("Ollama ({base}) no respondió a tiempo")
    } else {
        format!("Ollama: {e}")
    }
}

/// Ollama devuelve los errores como `{"error": "..."}`.
async fn error_body(resp: reqwest::Response) -> String {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or(text);
    format!("Ollama {status}: {msg}")
}

#[tauri::command]
pub async fn ollama_get(
    state: State<'_, Arc<AppState>>,
    base_url: Option<String>,
    path: String,
) -> Result<Value, String> {
    if !GET_PATHS.contains(&path.as_str()) {
        return Err(format!("Ruta de Ollama no permitida: {path}"));
    }
    let (url, base) = endpoint(base_url.as_deref(), &path)?;
    let resp = state
        .http
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| request_error(&base, e))?;
    if !resp.status().is_success() {
        return Err(error_body(resp).await);
    }
    resp.json().await.map_err(|e| format!("Respuesta de Ollama no válida: {e}"))
}

#[tauri::command]
pub async fn ollama_post(
    state: State<'_, Arc<AppState>>,
    base_url: Option<String>,
    path: String,
    mut body: Value,
) -> Result<Value, String> {
    if !POST_PATHS.contains(&path.as_str()) {
        return Err(format!("Ruta de Ollama no permitida: {path}"));
    }
    body["stream"] = Value::Bool(false);
    let (url, base) = endpoint(base_url.as_deref(), &path)?;
    // Cargar un modelo grande en memoria puede tardar.
    let resp = state
        .http
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| request_error(&base, e))?;
    if !resp.status().is_success() {
        return Err(error_body(resp).await);
    }
    resp.json().await.map_err(|e| format!("Respuesta de Ollama no válida: {e}"))
}

/// POST /api/chat en streaming: reenvía cada objeto NDJSON por `on_event` y
/// termina con `{"__end": true}` (el canal no garantiza llegar antes que la
/// respuesta del comando).
#[tauri::command]
pub async fn ollama_chat(
    state: State<'_, Arc<AppState>>,
    base_url: Option<String>,
    request_id: String,
    mut body: Value,
    on_event: Channel<Value>,
) -> Result<(), String> {
    body["stream"] = Value::Bool(true);
    let (url, base) = endpoint(base_url.as_deref(), "/api/chat")?;
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    state
        .ollama_cancel
        .lock()
        .unwrap()
        .insert(request_id.clone(), cancel_tx);

    let result = stream_chat(&state.http, &url, &base, &body, &mut cancel_rx, |v| {
        on_event.send(v).map_err(|e| e.to_string())
    })
    .await;

    state.ollama_cancel.lock().unwrap().remove(&request_id);
    let _ = on_event.send(json!({ "__end": true }));
    result
}

/// Hace el POST y entrega cada objeto NDJSON a `on_obj` según llega.
async fn stream_chat(
    http: &reqwest::Client,
    url: &str,
    base: &str,
    body: &Value,
    cancel_rx: &mut watch::Receiver<bool>,
    mut on_obj: impl FnMut(Value) -> Result<(), String>,
) -> Result<(), String> {
    let mut resp = tokio::select! {
        _ = cancel_rx.changed() => return Err("Cancelado".to_string()),
        r = http.post(url).json(body).send() => r.map_err(|e| request_error(base, e))?,
    };
    if !resp.status().is_success() {
        return Err(error_body(resp).await);
    }
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel_rx.changed() => return Err("Cancelado".to_string()),
            c = resp.chunk() => c.map_err(|e| request_error(base, e))?,
        };
        let Some(bytes) = chunk else { break };
        buf.extend_from_slice(&bytes);
        // Un objeto puede llegar partido entre dos fragmentos TCP.
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if let Some(v) = parse_line(&line)? {
                on_obj(v)?;
            }
        }
    }
    match parse_line(&buf)? {
        Some(v) => on_obj(v),
        None => Ok(()),
    }
}

fn parse_line(line: &[u8]) -> Result<Option<Value>, String> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(format!("Ollama: {err}"));
    }
    Ok(Some(v))
}

#[tauri::command]
pub fn ollama_cancel(state: State<'_, Arc<AppState>>, request_id: String) {
    if let Some(tx) = state.ollama_cancel.lock().unwrap().get(&request_id) {
        let _ = tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_and_validates() {
        assert_eq!(endpoint(None, "/api/tags").unwrap().0, "http://localhost:11434/api/tags");
        assert_eq!(
            endpoint(Some(" http://127.0.0.1:11434/ "), "/api/chat").unwrap().0,
            "http://127.0.0.1:11434/api/chat"
        );
        assert!(endpoint(Some("file:///etc/passwd"), "/api/tags").is_err());
        assert!(endpoint(Some("no es una url"), "/api/tags").is_err());
    }

    #[test]
    fn parse_line_surfaces_ollama_errors() {
        assert_eq!(parse_line(b"  \r\n").unwrap(), None);
        assert_eq!(parse_line(b"{\"done\":true}\n").unwrap(), Some(json!({"done": true})));
        assert!(parse_line(b"{\"error\":\"model not found\"}").unwrap_err().contains("model not found"));
    }

    /// Contra el Ollama local: `cargo test --lib ollama -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn streams_from_local_ollama() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::new();
        let model = std::env::var("OLLAMA_TEST_MODEL").unwrap_or_else(|_| "gemma3:4b".into());
        let (url, base) = endpoint(None, "/api/chat").unwrap();
        let body = json!({
            "model": model,
            "stream": true,
            "messages": [{"role": "user", "content": "Responde solo: hola"}],
            "options": {"num_predict": 20, "num_ctx": 8192},
        });
        let (_tx, mut rx) = watch::channel(false);
        let (mut deltas, mut done) = (0, false);
        stream_chat(&http, &url, &base, &body, &mut rx, |v| {
            if v["message"]["content"].as_str().is_some_and(|s| !s.is_empty()) {
                deltas += 1;
            }
            done |= v["done"].as_bool() == Some(true);
            Ok(())
        })
        .await
        .unwrap();
        assert!(deltas > 0 && done, "deltas={deltas} done={done}");

        // Modelo inexistente: el error de Ollama llega legible.
        let bad = json!({"model": "no-existe:1b", "stream": true, "messages": []});
        let err = stream_chat(&http, &url, &base, &bad, &mut rx, |_| Ok(())).await.unwrap_err();
        assert!(err.contains("404") || err.contains("not found"), "{err}");
    }
}
