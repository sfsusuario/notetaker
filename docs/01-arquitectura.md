# Arquitectura

Tauri 2 (Rust) + React 19 / TypeScript / Tailwind 3 / Zustand. Todo lo que toca el sistema (audio, procesos, registro, archivos, llavero, whisper-server) vive en Rust; la lógica de red hacia los LLM vive en el WebView (fetch + SSE); las API keys en el llavero del SO.

## Procesos y ventanas

```
┌─ notetaker.exe (Tauri) ───────────────────────────────────────────────┐
│  Rust                                                                  │
│   ├─ audio/capture (cpal, hilo por fuente; loopback WASAPI = input     │
│   │    stream sobre un dispositivo de salida)                          │
│   ├─ session/pipeline: frames → resample 16 kHz → WAV → chunks 100 ms  │
│   ├─ stt/deepgram_live (WS)  |  stt/whisper/live (HTTP → sidecar)      │
│   ├─ meeting/ (hilo cada 3 s: procesos + títulos + registro micrófono) │
│   ├─ tray (bandeja)  ·  secrets (keyring)  ·  files                    │
│   └─ plugin-sql (migraciones)                                           │
│  WebView "main"  (index.html)   WebView "popup" (popup.html, 440×236)  │
└───────────────┬────────────────────────────────────────────────────────┘
                │ HTTP 127.0.0.1:<puerto libre>
        whisper-server.exe (whisper.cpp, proceso hijo, CREATE_NO_WINDOW)
```

## Modos de sesión

| Modo | `sessions.mode` | Motor | Al terminar |
|---|---|---|---|
| En vivo | `live` | Deepgram o Whisper | `status = done` |
| Solo grabar | `record` | ninguno (`engine = "none"`) | `status = recorded`; se transcribe después desde el detalle |
| Desde grabación | `file` | Deepgram o Whisper | `status = done` |

En "solo grabar" el pipeline es idéntico salvo que no se instancia ningún motor: los chunks se descartan y solo se escribe el WAV. `transcribeSession()` reutiliza `transcribe_file` sobre `mic.wav` / `system.wav`, así que la transcripción posterior conserva la separación Yo/Otros.

## Flujo en vivo

1. `session_start_live(cfg)` construye el `Engine` (lee la key de Deepgram del keyring o asegura `whisper-server` con el modelo pedido) y lanza un `spawn_source_stream` por fuente (mic / system).
2. Cada fuente: cpal → `Resampler` → chunks de 1600 muestras (100 ms) → `WavSink` (`recordings/<id>/<source>.wav`) → `audio://metrics` (10 Hz) → `AudioChunk { samples, position_ms }` al motor.
3. **La línea de tiempo es el audio escrito**: `position_ms` solo avanza con audio grabado (la pausa descarta frames). Deepgram reinicia sus timestamps por conexión, así que se suma `conn_offset_ms`; whisper devuelve tiempos relativos a cada frase (`utterance.start_ms + seg.start`). Resultado: `startMs` de cada burbuja = posición exacta en el WAV para el reproductor.
4. Eventos hacia el WebView: `stt://partial`, `stt://final`, `stt://status`, `audio://metrics`, `audio://error`, `session://stopped`. Solo la ventana `main` escribe en SQLite (`wireEvents.ts`).
5. `session_stop`: para la captura → el pipeline cierra el WAV → el motor drena (Deepgram `CloseStream`, whisper vacía su cola) → `mix.wav` para reproducción → `session://stopped`.

## Whisper local

- Instalación (`stt/whisper/install.rs` o `scripts/setup-whisper.ps1`): `whisper-bin-x64.zip` (tag fijado en `RELEASE_TAG`) aplanado a `whisper/bin/`, modelos `ggml-*.bin` en `whisper/models/`.
- `server.rs`: un proceso por app; se lanza en el primer uso con `--host 127.0.0.1 --port <libre> -m <modelo> -t <hilos>`; se considera listo cuando acepta conexiones TCP; se mata en `RunEvent::Exit` (con `taskkill` de respaldo).
- `client.rs`: `POST /inference` multipart (`file` WAV 16 kHz, `response_format=verbose_json`, `language`), filtro de alucinaciones (`no_speech_prob`, lista negra).
- En vivo (`live.rs`): `UtteranceCutter` (700 ms de silencio, máx. 15 s, pre-roll 200 ms) → cola de 4 frases → inferencia secuencial (`whisper_infer` mutex) → `stt://final`; mientras hay cola, `stt://partial` con "…" (burbuja escribiendo).
- Archivo (`file.rs`): ventanas de hasta 28 s por silencio, progreso por ventana.

## Datos

SQLite (plugin-sql): `sessions`, `segments (session_id, source, speaker, text, start_ms, end_ms, received_at)`, `chat_messages`, `speakers` (etiquetas personalizadas por `speaker_key`). Orden de burbujas por `received_at`; dedupe por `id` determinista `"{source}-{start}-{end}"`.

Etiquetas de hablante (`app/speakers.ts`): `mic` → **Yo**; `system` con `speaker` → **Hablante N** (o el nombre que le pongas); `system` sin speaker → **Otros**; `file` → **Transcripción** o Hablante N.

## Parada automática

El mismo hilo del sondeo (3 s) hace de vigilante (`meeting::autostop_tick`), también con la detección de reuniones apagada. Tres disparadores, evaluados en `meeting::decide` (función pura, con pruebas en `meeting::tests`):

1. **Fin de reunión**: la sesión guarda `meeting_key`/`meeting_app` (al arrancar o adjuntada después si la llamada empieza más tarde). Se exige un margen de 60 s desde que se adjunta y un debounce de **20 sondeos en Teams** (su detección depende del micrófono: silenciarse la hace desaparecer) y 5 en Zoom/Meet. Mismo app con pid distinto = continuación, no final.
2. **Silencio**: el pipeline marca `last_voice_at` cuando el rms supera `AUTOSTOP_VOICE_RMS` (0.010, ≈ −40 dBFS) durante 300 ms seguidos. Se congela en pausa y **no cuenta dentro de una reunión en curso** (escuchar sin hablar es legítimo).
3. **Duración máxima**: tiempo de pared, pausas incluidas.

Al disparar se guarda un `PendingStop` con `deadline_ms` absoluto, se emite `autostop://proposed` y se muestra el popup; al vencer, `stop_live(.., "auto-…")`. «Seguir grabando» (`autostop_cancel`) desvincula la reunión, reinicia el contador de silencio o prorroga el tope según el motivo, y suprime nuevas propuestas 60 s. Si entre dos sondeos pasan más de 30 s se asume suspensión del equipo: se reinicia todo en vez de cortar al despertar.

## Detección de reuniones

`meeting/detector.rs`: Teams = proceso `ms-teams.exe` **y** micrófono en uso por Teams; Zoom = `zoom.exe` **y** (ventana "Zoom Meeting" o micrófono en uso); Meet = título de ventana con código `xxx-xxxx-xxx`. El registro `HKCU\...\CapabilityAccessManager\ConsentStore\microphone\{NonPackaged\*|*}` con `LastUsedTimeStop == 0` indica captura activa. Transiciones → `meeting://detected` / `meeting://ended`; el popup envía `popup_action("start", config)` y `main` recibe `meeting://start-request`.

## LLM

`services/llm/providers/*` (misma capa que custom-iv): `generate` en streaming, `test`, `listModels` (lista oficial de la API). `chatClient.ts` construye el system prompt con la transcripción formateada `[mm:ss] Hablante: texto` (recorte a los últimos ~60k caracteres) y persiste el chat por sesión. `generateTitle` produce el título automático (≤ 8 palabras) a los ~90 s en vivo o al terminar un archivo; nunca pisa un título editado (`title_auto = 0`).
