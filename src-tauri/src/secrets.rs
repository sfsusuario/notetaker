//! API keys en el llavero del sistema (Windows Credential Manager / macOS
//! Keychain). Nunca se guardan en localStorage ni en la base de datos.
use keyring::Entry;

const SERVICE: &str = "notetaker";

fn entry(name: &str) -> Result<Entry, String> {
    if !matches!(
        name,
        "deepgram" | "anthropic" | "openai" | "gemini" | "deepseek" | "kimi"
    ) {
        return Err(format!("Secreto desconocido: {name}"));
    }
    Entry::new(SERVICE, name).map_err(|e| format!("Error de keyring: {e}"))
}

pub fn get_secret_internal(name: &str) -> Result<String, String> {
    entry(name)?
        .get_password()
        .map_err(|e| format!("No se pudo leer el secreto '{name}': {e}"))
}

/// Lee la API key de Deepgram (nunca viaja al WebView). `trim()`: una clave
/// pegada con espacios o salto de línea rompe la cabecera Authorization.
pub fn read_deepgram_key() -> Result<String, String> {
    let api_key = get_secret_internal("deepgram")
        .map_err(|_| "Falta la API key de Deepgram. Configúrala en Ajustes.".to_string())?
        .trim()
        .to_string();
    if api_key.is_empty() {
        return Err("La API key de Deepgram está vacía. Configúrala en Ajustes.".to_string());
    }
    Ok(api_key)
}

pub fn has_secret(name: &str) -> bool {
    matches!(get_secret_internal(name), Ok(v) if !v.trim().is_empty())
}

#[tauri::command]
pub fn secret_set(name: String, value: String) -> Result<(), String> {
    entry(&name)?
        .set_password(value.trim())
        .map_err(|e| format!("No se pudo guardar el secreto: {e}"))
}

#[tauri::command]
pub fn secret_get(name: String) -> Result<Option<String>, String> {
    match entry(&name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("No se pudo leer el secreto: {e}")),
    }
}

#[tauri::command]
pub fn secret_delete(name: String) -> Result<(), String> {
    match entry(&name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("No se pudo borrar el secreto: {e}")),
    }
}
