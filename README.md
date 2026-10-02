# NoteTaker

Aplicación de escritorio (Windows) para **transcribir reuniones en tiempo real o desde una grabación**, guardarlas en un historial y **conversar con una IA sobre lo hablado**.

- **Dos motores de transcripción**: Deepgram (nube, nova-3, detecta hablantes) y **whisper.cpp local** (sin conexión, sin API key).
- **Tres modos**:
  - **En vivo**: graba y transcribe a la vez (micrófono, audio del sistema o ambos).
  - **Solo grabar**: guarda el audio sin transcribir, sin API key ni modelo. La sesión queda como *Sin transcribir* y la transcribes cuando quieras con el motor que prefieras.
  - **Desde grabación**: transcribe un archivo existente (wav, mp3, m4a, ogg, flac).
- **Hablantes**: con mic + sistema por separado siempre distingues *Yo* de *Otros*; Deepgram además separa *Hablante 1, 2…* en vivo y, con whisper, la identificación local de hablantes lo hace al terminar (ver abajo). El selector de motor muestra si detecta hablantes.
- **Historial** con título automático (IA) editable, búsqueda por contenido, reproductor sincronizado con las burbujas (cada una con su minuto), renombrado de hablantes, exportación (md/txt/json) y retranscripción con otro motor.
- **Chat con IA** por sesión (Ollama local por defecto, que elige solo el mejor modelo instalado; también Gemini, OpenAI, Anthropic, DeepSeek o Kimi). La lista de modelos se obtiene de la API oficial de cada proveedor.
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
.\scripts\build-whisper-vulkan.ps1        # compila e instala la aceleración GPU (Vulkan) de whisper
.\scripts\clean.ps1 [-Data]           # limpia builds (y datos, con confirmación)
```

O directamente: `npm install`, `npm run tauri dev`, `npm run tauri build`.

## Primer uso

1. **Ajustes → Proveedor de IA**: con [Ollama](https://ollama.com) instalado no hace falta nada: el modelo *Automático* usa el mejor que tengas. Pulsa *Probar conexión* para precargarlo y ver si corre en GPU. Para un proveedor en la nube, elígelo, carga su *Lista oficial* y guarda la API key.
2. **Ajustes → Transcripción**: guarda la API key de Deepgram **o** instala el motor local en **Whisper local** (servidor + el modelo que indique *Recomendado* para tu equipo).
3. **Nueva sesión**: elige el modo (*En vivo*, *Solo grabar* o *Desde grabación*), revisa el resumen y arranca. Las secciones de motor, fuentes e idioma se despliegan solo si quieres cambiarlas.

*Solo grabar* no necesita nada configurado: útil si la reunión empieza ya y prefieres resolver la transcripción después.

Las API keys se guardan en el Administrador de credenciales de Windows (servicio `notetaker`), nunca en texto plano.

## Dónde están los datos

| Qué | Dónde |
|---|---|
| Grabaciones WAV (16 kHz mono por fuente + `mix.wav`) | `%LOCALAPPDATA%\com.efsteps.notetaker\recordings\<sessionId>\` |
| whisper.cpp (`bin\whisper-server.exe` + DLLs), modelos GGML y backend GPU opcional (`gpu\ggml-vulkan.dll`) | `%LOCALAPPDATA%\com.efsteps.notetaker\whisper\` |
| Identificación de hablantes (sherpa-onnx + modelos pyannote/CAM++) | `%LOCALAPPDATA%\com.efsteps.notetaker\diarize\` |
| Base de datos SQLite (sesiones, segmentos, chat, hablantes) | `%APPDATA%\com.efsteps.notetaker\notetaker.db` |
| Ajustes de la UI | `localStorage` del WebView (`notetaker-settings`) |

## Motores

| | Deepgram | Whisper local |
|---|---|---|
| Latencia en vivo | texto parcial inmediato | texto provisional de *Otros* en ~4 s; definitivo 1–3 s tras cada frase |
| Hablantes | Sí (`diarize`) + Yo/Otros | Yo/Otros en vivo; *Hablante N* al terminar (identificación local) |
| Requiere | API key + internet | CPU o GPU (Vulkan) |
| Archivos | REST pregrabado con hablantes | Frases agrupadas en ventanas de hasta 28 s |

## Rendimiento del motor local

**Ajustes → Whisper local** muestra el hardware detectado (CPU con núcleos P/E, RAM, GPU, batería) y la configuración *Recomendada* para ese equipo, aplicable con un clic.

Medido en un Core Ultra 7 258V + Arc 140V (en batería), 120 s de voz en español:

| Configuración | Velocidad | Texto |
|---|---|---|
| GPU + `large-v3-turbo-q8_0` | 4.3x tiempo real | igual que turbo completo |
| GPU + `small-q5_1` | 6.9x | |
| CPU (8 hilos) + `small-q5_1` | 4.2x | |
| CPU (8 hilos) + `large-v3-turbo` | 1.0x | referencia |

- **GPU**: whisper.cpp no publica binarios Vulkan para Windows; `scripts\build-whisper-vulkan.ps1` compila solo el backend (`ggml-vulkan.dll`) del mismo tag que el servidor oficial, sin Vulkan SDK ni administrador. Con *Aceleración = Automático* se usa si está instalado y, si la GPU falla al arrancar, se vuelve a la CPU.
- **Drivers Intel Arc**: las matrices cooperativas (*coopmat*) del driver dan texto corrupto (WER 47–91 %), así que la app arranca whisper con `GGML_VK_DISABLE_COOPMAT=1`.
- **Hilos**: nunca más que los hilos del equipo; con 20 hilos en una CPU de 8, turbo bajó de 1.0x a 0.6x tiempo real. Con GPU bastan 4.
- **Archivos**: las frases se agrupan en ventanas de hasta 28 s, porque whisper procesa 30 s por petición aunque la frase dure 3: en 5 min de conversación son 13 peticiones en vez de 42 (2.7x más rápido). Con idioma *Automático* se detecta en la primera ventana y se reutiliza.
- **Peticiones**: se pide `no_language_probabilities`; si no, whisper-server repite el encoder en cada petición solo para calcular probabilidades de idioma que la app no usa (4–5 s por ventana).
- **En la app**, un capítulo de 19 min con GPU + `large-v3-turbo-q8_0` + idioma automático: 114 s (9.6x tiempo real), con cargador y sin modelos de Ollama ocupando la GPU. Bajo carga sostenida el portátil limita la potencia por temperatura y en batería la velocidad cae a menos de la mitad.
- Los segmentos que whisper corta a mitad de palabra ("caball" + "ero") se unen antes de mostrarse.

### En vivo

Medido reproduciendo una llamada real de 8 min (micrófono + sistema, con altavoces) contra el servidor en GPU:

| | Antes | Ahora |
|---|---|---|
| Primer texto de una frase de *Otros* (desde que empieza a hablar) | mediana 13 s, máx. 24 s | **mediana 4.2 s** (texto provisional) |
| Trabajo del servidor | 276 s | 82 s (+ texto provisional, opcional) |
| Burbujas basura en *Yo* (ruido, eco) | ~30 ("Thank you.", "Спасибо.", "*sad music*") | ~4 |

- **VAD Silero** dentro de whisper-server (`ggml-silero-v5.1.2.bin`, 0.9 MB, se descarga solo): el ruido de teclas, golpes o música no llega al modelo, que sobre ruido inventaba frases en idiomas al azar.
- **Idioma por pista**: con *Automático*, detectarlo en cada frase cuesta una pasada extra; se fija cuando dos frases con voz real coinciden (y 1 de cada 4 lo vuelve a comprobar). Es por pista porque la reunión puede ir en un idioma y tu voz en otro: forzar uno sobre el otro inventa traducciones.
- **Texto provisional** (Ajustes → Whisper local, activado por defecto): cada ~2 s, si el servidor está libre, se transcribe la frase en curso de *Otros* y se muestra mientras sigue hablando. Usa más GPU y batería.
- **Sin descartes**: antes, con 4 frases en cola se tiraban las siguientes.
- **Eco**: con altavoces el micrófono capta a los demás; las burbujas de *Yo* que repiten lo dicho por *Otros* se eliminan. Con auriculares no hay eco.
- Los trozos cortos seguidos del mismo hablante se unen en una burbuja.

## Identificación de hablantes (local)

**Ajustes → Identificar hablantes → Instalar** descarga [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (binario oficial CPU) con la segmentación pyannote 3.0 y la huella de voz CAM++ multilingüe (~52 MB). Al terminar una transcripción con whisper (en vivo, archivo o retranscripción) analiza la pista *Otros* (todos los participantes remotos llegan mezclados por el audio del sistema) o el archivo, y reparte las burbujas en *Hablante 1, 2…*; tu micrófono sigue siendo *Yo*. También se lanza a mano desde el menú de la sesión. Con una sola voz se mantiene *Otros*.

**En vivo** (activado por defecto): cada intervención de *Otros* recibe un hablante al momento con la huella de voz de cada segmento (librería en C de sherpa-onnx, ~25 ms por trozo en la CPU) y agrupación al vuelo; calibrado con los 3 lectores, acierta el 91–92.5 % (el techo con esa granularidad es 92–93.5 %). Al terminar, el análisis completo corrige las etiquetas **manteniendo la numeración** que ya se veía, así que los nombres que pongas durante la reunión se conservan.

Medido con 3 lectores reales mezclados en turnos de 1.5–20 s (Core Ultra 7 258V, CPU, 4 hilos):

| | |
|---|---|
| Velocidad | ~27x tiempo real (30 min en 67 s; ~2–3 min por hora de reunión) |
| Memoria | ~350 MB mientras analiza |
| Número de hablantes | acertado sin indicarlo |
| Burbujas con el hablante correcto | 82 de 84 (97.6 %) en 6 min |

- Corre en la CPU con prioridad baja mientras whisper usa la GPU, así que no retrasa la transcripción en vivo; en archivos, se suma a lo que tarde whisper (6 min de audio: 37 s de whisper + 12 s de hablantes).
- Más de 4 hilos va más lento (núcleos E). Los modelos de huella entrenados solo en inglés (wespeaker, eres2net) confundían voces en español.
- En reuniones reales espera más errores: voces que se solapan, intervenciones de una palabra, audio comprimido de Teams o voces parecidas. Haz clic en el nombre de un hablante para renombrarlo; repetir el análisis borra esos nombres en la pista *Otros*.

## Ollama (IA local)

- La app habla con Ollama desde Rust: el WebView de la app compilada tiene el origen `http://tauri.localhost`, que Ollama rechaza (403) si se llama con `fetch`.
- El modelo *Automático* elige el mejor modelo de chat instalado (familia, tamaño hasta ~14B y que quepa en la mitad de la RAM; descarta embeddings y penaliza los de código y los que razonan siempre). En los modelos de razonamiento se desactiva el pensamiento (gpt-oss solo admite nivel bajo).
- Medido con una reunión de 19 min y 4 preguntas de detalle (Arc 140V):

  | Modelo | Aciertos | 1.ª respuesta | Siguientes | Memoria |
  |---|---|---|---|---|
  | qwen3:8b (elegido) | 3.5/4 | ~40 s | 3–7 s | 5.8 GB |
  | gpt-oss:20b | 3.5/4 | ~40 s | 6–17 s | 10.8 GB |
  | llama3.1:8b | 3.5/4 | ~37 s | 2–6 s | 5.5 GB |
  | gemma3:4b | 2.5/4 | ~20 s | 3–4 s | 2.8 GB |

  La primera respuesta es lenta porque el modelo lee toda la transcripción; al abrir el chat se precarga el modelo y las siguientes preguntas reutilizan la caché.
- Se pide un contexto (`num_ctx`) acorde a la transcripción y nunca menor que el ya cargado: con el de serie (4096 tokens) Ollama recorta en silencio el principio y el modelo inventa las respuestas; cambiarlo a la baja obliga a recargar el modelo.
- La transcripción va al modelo en líneas de ~45 s como máximo con su minuto, para que las citas `[mm:ss]` sean reales.
- **GPU integrada**: Ollama ignora las iGPU por defecto. Para usarla, define las variables de usuario `OLLAMA_IGPU_ENABLE=1` y, con drivers Intel antiguos, `GGML_VK_DISABLE_COOPMAT=1`, y reinicia Ollama. *Probar conexión* indica si el modelo corre en GPU o CPU.

## Detección de reuniones (Windows)

Cada 3 s (solo si no hay sesión activa) se comprueban: procesos de Teams/Zoom, títulos de ventana (código de Google Meet `xxx-xxxx-xxx`, "Zoom Meeting") y el registro de Windows que indica qué app usa el micrófono (`ConsentStore\microphone`). Al detectar una reunión aparece un popup siempre-encima con motor, fuentes e idioma; *Ignorar* la silencia hasta que termine. Nunca se graba sin confirmar.

## Arquitectura

Ver [docs/01-arquitectura.md](docs/01-arquitectura.md).
