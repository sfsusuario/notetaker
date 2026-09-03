//! Motor local: whisper.cpp (`whisper-server.exe`) como proceso auxiliar.
//! - `install`: descarga del binario oficial y de los modelos GGML.
//! - `server`: ciclo de vida del proceso (un server por app, se reutiliza).
//! - `client`: POST /inference (WAV en memoria → segmentos con timestamps).
//! - `live` / `file`: integración con el pipeline en vivo y con archivos.
pub mod client;
pub mod file;
pub mod install;
pub mod live;
pub mod server;
