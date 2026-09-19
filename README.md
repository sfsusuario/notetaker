# NoteTaker

Aplicación de escritorio (Windows) para **transcribir reuniones en tiempo real o desde una grabación**, guardarlas en un historial y **conversar con una IA sobre lo hablado**.

- **Dos motores de transcripción**: Deepgram (nube, nova-3, detecta hablantes) y **whisper.cpp local** (sin conexión, sin API key).
- **Tres modos**:
  - **En vivo**: graba y transcribe a la vez (micrófono, audio del sistema o ambos).
  - **Solo grabar**: guarda el audio sin transcribir, sin API key ni modelo. La sesión queda como *Sin transcribir* y la transcribes cuando quieras con el motor que prefieras.
  - **Desde grabación**: transcribe un archivo existente (wav, mp3, m4a, ogg, flac).
- **Hablantes**: con mic + sistema por separado siempre distingues *Yo* de *Otros*; Deepgram además separa *Hablante 1, 2…*. El selector de motor muestra si detecta hablantes.
- **Historial** con título automático (IA) editable, búsqueda por contenido, reproductor sincronizado con las burbujas, renombrado de hablantes, exportación (md/txt/json) y retranscripción con otro motor.
- **Chat con IA** por sesión (Gemini por defecto; también OpenAI, Anthropic, DeepSeek, Kimi u Ollama local). La lista de modelos se obtiene de la API oficial de cada proveedor.
- **Parada automática**: si te olvidas de detener, la app lo propone con una cuenta atrás cancelable cuando termina la reunión detectada, cuando lleva 10 minutos sin oír nada o al alcanzar la duración máxima (4 h). Configurable en Ajustes → *Detener automáticamente*.
- **Detección de reuniones** (Teams, Zoom, Google Meet): un popup superior con ajustes rápidos pregunta si quieres transcribir. Icono en la bandeja; cerrar la ventana la oculta.

## Desarrollo

Requisitos: Node 22+, Rust (cargo), Visual Studio 2022 Build Tools (C++), Windows 10+ con WebView2. **No** hace falta CMake ni LLVM: el motor local es un proceso auxiliar precompilado.

```powershell
.\run.ps1              # desarrollo con hot reload
.\run.ps1 -Release     # compila y lanza el binario de producción
.\run.ps1 -Clean       # build desde cero
.\start.ps1            # lanza el binario ya compilado (sin recompilar)
.\build.ps1                           # .exe final + instaladores NSIS/MSI
.\build.ps1 -NoBundle -Run            # solo el .exe (más rápido) y lo lanza
.\scripts\setup-whisper.ps1 -Model base   # instala whisper.cpp + modelo (también desde Ajustes)
.\scripts\clean.ps1 [-Data]           # limpia builds (y datos, con confirmación)
```

O directamente: `npm install`, `npm run tauri dev`, `npm run tauri build`.

## Primer uso

1. **Ajustes → Proveedor de IA**: elige proveedor, pulsa *Lista oficial* para cargar sus modelos, guarda la API key y *Probar conexión*.
2. **Ajustes → Transcripción**: guarda la API key de Deepgram **o** instala el motor local en **Whisper local** (servidor + modelo `base`).
3. **Nueva sesión**: elige el modo (*En vivo*, *Solo grabar* o *Desde grabación*), revisa el resumen y arranca. Las secciones de motor, fuentes e idioma se despliegan solo si quieres cambiarlas.

*Solo grabar* no necesita nada configurado: útil si la reunión empieza ya y prefieres resolver la transcripción después.

Las API keys se guardan en el Administrador de credenciales de Windows (servicio `notetaker`), nunca en texto plano.

## Dónde están los datos

| Qué | Dónde |
|---|---|
| Grabaciones WAV (16 kHz mono por fuente + `mix.wav`) | `%LOCALAPPDATA%\com.efsteps.notetaker\recordings\<sessionId>\` |
| whisper.cpp (`bin\whisper-server.exe` + DLLs) y modelos GGML | `%LOCALAPPDATA%\com.efsteps.notetaker\whisper\` |
| Base de datos SQLite (sesiones, segmentos, chat, hablantes) | `%APPDATA%\com.efsteps.notetaker\notetaker.db` |
| Ajustes de la UI | `localStorage` del WebView (`notetaker-settings`) |

## Motores

| | Deepgram | Whisper local |
|---|---|---|
| Latencia en vivo | texto parcial inmediato | 1–4 s por frase (corte por silencio) |
| Hablantes | Sí (`diarize`) + Yo/Otros | Solo Yo/Otros por fuente |
| Requiere | API key + internet | CPU (base ≈ tiempo real en un i7 de portátil) |
| Archivos | REST pregrabado con hablantes | Ventanas de ~28 s secuenciales |

Recomendación en CPU sin GPU: `base` para vivo, `small` para archivos.

## Detección de reuniones (Windows)

Cada 3 s (solo si no hay sesión activa) se comprueban: procesos de Teams/Zoom, títulos de ventana (código de Google Meet `xxx-xxxx-xxx`, "Zoom Meeting") y el registro de Windows que indica qué app usa el micrófono (`ConsentStore\microphone`). Al detectar una reunión aparece un popup siempre-encima con motor, fuentes e idioma; *Ignorar* la silencia hasta que termine. Nunca se graba sin confirmar.

## Arquitectura

Ver [docs/01-arquitectura.md](docs/01-arquitectura.md).
