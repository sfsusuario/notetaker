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
- `server.rs`: un proceso por app; se lanza en el primer uso con `--host 127.0.0.1 --port <libre> -m <modelo> -t <hilos>`; se considera listo cuando acepta conexiones TCP; se mata en `RunEvent::Exit` (con `taskkill` de respaldo). Se reinicia si cambia modelo, hilos (limitados a los hilos del equipo) o aceleración.
- GPU (`Accel::Auto|Gpu`): si existe `whisper/gpu/ggml-vulkan.dll` con `ggml-vulkan.tag == RELEASE_TAG` (lo compila `scripts/build-whisper-vulkan.ps1`), se lanza con `GGML_BACKEND_PATH=<dll>` y `GGML_VK_DISABLE_COOPMAT=1` (coopmat da texto corrupto en Intel Arc); si no, con `-ng`. El DLL vive fuera de `bin/` porque ggml carga cualquier backend que encuentre junto al exe. Si el arranque con GPU falla, se reintenta en CPU. El hilo que drena stderr anota el dispositivo (`whisper_status.running.backend`).
- `sysprofile.rs` (`system_profile`): CPU y núcleos P/E (`GetLogicalProcessorInformationEx`), RAM (sysinfo), GPUs (DXGI), batería y la configuración recomendada (`recommend`, con pruebas).
- `client.rs`: `POST /inference` multipart (`file` WAV 16 kHz, `response_format=verbose_json`, `language`, `no_language_probabilities=true`: sin él el servidor repite el encoder en cada petición para calcular probabilidades de idioma), filtro de alucinaciones (`no_speech_prob`, lista negra) y unión de segmentos cortados a mitad de palabra (el trozo que continúa llega sin espacio inicial).
- En vivo (`live.rs`): `UtteranceCutter` (700 ms de silencio, máx. 15 s, pre-roll 200 ms) → cola sin límite (nunca descarta; aviso `degraded` con ≥ 4 pendientes) → inferencia secuencial (`whisper_infer` mutex) con `vad=true` → hablante por segmento (`speaker.rs`, solo "Otros") → `merge_runs` une trozos seguidos del mismo hablante → `stt://final`. Idioma por pista con `LangState` (se fija con dos detecciones coincidentes de frases ≥ 3 s con texto; 1 de cada 4 vuelve a detectar). Texto provisional de "Otros": cada 2 s, sin finales pendientes y con `try_lock` del servidor, se transcribe `cutter.pending()` y se emite como `stt://partial` (`{fuente}-pending`); un contador de generación descarta resultados de frases ya cerradas. Mientras hay cola sin texto provisional, "…".
- `speaker.rs`: carga `sherpa-onnx-c-api.dll` con `libloading` (`LOAD_WITH_ALTERED_SEARCH_PATH` para encontrar `onnxruntime.dll`), huella CAM++ normalizada por segmento y `OnlineSpeakers` (coseno ≥ 0.55; segmentos < 2 s solo se asignan a hablantes existentes con ≥ 0.40). Un extractor por app (`AppState.speaker_embedder`). Al terminar, `alignToCurrent` (frontend) renumera el análisis completo para que coincida con lo visto.
- Modelo VAD: `whisper/models/ggml-silero-v5.1.2.bin`, descargado al arrancar si falta; `server.rs` lanza con `-vm` y reinicia el servidor si el modelo aparece después.
- Archivo (`file.rs`): frases por silencio (`CutterConfig::FILE`) agrupadas por `pack_windows` en tramos contiguos de hasta 28 s (un hueco > 3 s abre ventana nueva). whisper codifica 30 s por petición aunque la frase dure 3, así que agrupar reduce las peticiones ~3x. Progreso por ventana con la velocidad en "x tiempo real". Con idioma `auto`, el detectado en la primera ventana ("spanish") se envía en las siguientes.

## Datos

SQLite (plugin-sql): `sessions`, `segments (session_id, source, speaker, text, start_ms, end_ms, received_at)`, `chat_messages`, `speakers` (etiquetas personalizadas por `speaker_key`). Orden de burbujas por `received_at`; dedupe por `id` determinista `"{source}-{start}-{end}"`.

Etiquetas de hablante (`app/speakers.ts`): `mic` → **Yo**; `system` con `speaker` → **Hablante N** (o el nombre que le pongas); `system` sin speaker → **Otros**; `file` → **Transcripción** o Hablante N.

`TranscriptFeed` muestra el minuto de cada burbuja y abre cabecera nueva al cambiar de hablante o tras 1 min de audio (por `startMs`, no por llegada: un archivo llega entero en segundos).

## Identificación de hablantes

`src-tauri/src/diarize.rs`: `sherpa-onnx-offline-speaker-diarization.exe` (release oficial CPU, `v1.13.8`) como proceso auxiliar en `diarize/bin`, modelos en `diarize/models/{segmentation,embedding}.onnx` (pyannote 3.0 y CAM++ zh_en advanced). `diarize_install` descarga y extrae con el `tar` de Windows; `diarize_audio(path)` analiza un WAV 16 kHz (4 hilos, ventana 0.25, umbral 0.9, prioridad baja, job object), reasigna los grupos espurios (todos sus tramos < 3 s y < 3 % del tiempo) al hablante vecino y numera por orden de aparición. Un análisis a la vez (`AppState.diarize`).

`app/diarize.ts`: `maybeDiarize` se llama al terminar con whisper (`finalizeLiveSession`, `startFromFile`, `transcribeSession`) si `diarizeAuto` y está instalado; `diarizeSession` analiza `system.wav` y `file.wav`, asigna a cada segmento el hablante con más solapamiento (`assignSpeakers`) y lo guarda con `db.setSegmentSpeakers`. Con una sola voz deja `speaker = null`.

## Parada automática

El mismo hilo del sondeo (3 s) hace de vigilante (`meeting::autostop_tick`), también con la detección de reuniones apagada. Tres disparadores, evaluados en `meeting::decide` (función pura, con pruebas en `meeting::tests`):

1. **Fin de reunión**: la sesión guarda `meeting_key`/`meeting_app` (al arrancar o adjuntada después si la llamada empieza más tarde). Se exige un margen de 60 s desde que se adjunta y un debounce de **20 sondeos en Teams** (su detección depende del micrófono: silenciarse la hace desaparecer) y 5 en Zoom/Meet. Mismo app con pid distinto = continuación, no final.
2. **Silencio**: el pipeline marca `last_voice_at` cuando el rms supera `AUTOSTOP_VOICE_RMS` (0.010, ≈ −40 dBFS) durante 300 ms seguidos. Se congela en pausa y **no cuenta dentro de una reunión en curso** (escuchar sin hablar es legítimo).
3. **Duración máxima**: tiempo de pared, pausas incluidas.

Al disparar se guarda un `PendingStop` con `deadline_ms` absoluto, se emite `autostop://proposed` y se muestra el popup; al vencer, `stop_live(.., "auto-…")`. «Seguir grabando» (`autostop_cancel`) desvincula la reunión, reinicia el contador de silencio o prorroga el tope según el motivo, y suprime nuevas propuestas 60 s. Si entre dos sondeos pasan más de 30 s se asume suspensión del equipo: se reinicia todo en vez de cortar al despertar.

## Detección de reuniones

`meeting/detector.rs`: Teams = proceso `ms-teams.exe` **y** micrófono en uso por Teams; Zoom = `zoom.exe` **y** (ventana "Zoom Meeting" o micrófono en uso); Meet = título de ventana con código `xxx-xxxx-xxx`. El registro `HKCU\...\CapabilityAccessManager\ConsentStore\microphone\{NonPackaged\*|*}` con `LastUsedTimeStop == 0` indica captura activa. Transiciones → `meeting://detected` / `meeting://ended`; el popup envía `popup_action("start", config)` y `main` recibe `meeting://start-request`.

## LLM

`services/llm/providers/*` (misma capa que custom-iv): `generate` en streaming, `test`, `listModels` (lista oficial de la API).

Ollama (proveedor por defecto) va por `src-tauri/src/ollama.rs`: `ollama_get` / `ollama_post` (rutas en lista blanca) y `ollama_chat`, que reenvía el NDJSON por un `Channel` y termina con `{"__end": true}`; `ollama_cancel` corta la petición. Hace falta porque Ollama responde 403 al origen `http://tauri.localhost` del WebView compilado. En `providers/ollama.ts`, `model = "auto"` resuelve con `pickBestModel` el mejor modelo instalado, `contextFor` fija `num_ctx` según el prompt (el de serie, 4096, recorta la transcripción en silencio), `think` se apaga en modelos de razonamiento y `keep_alive` es 30 min; `warmup` precarga el modelo al abrir el chat. `chatClient.ts` construye el system prompt con la transcripción formateada `[mm:ss] Hablante: texto` (recorte a los últimos ~60k caracteres) y persiste el chat por sesión. `generateTitle` produce el título automático (≤ 8 palabras) a los ~90 s en vivo o al terminar un archivo; nunca pisa un título editado (`title_auto = 0`).
