//! Señales de Windows para detectar reuniones: títulos de ventanas visibles y
//! el registro de "micrófono en uso" (CapabilityAccessManager).

#[cfg(windows)]
pub fn list_window_titles() -> Vec<(u32, String)> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(u32, String)>);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return BOOL(1);
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        if n <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        out.push((pid, title));
        BOOL(1)
    }

    let mut out: Vec<(u32, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Nombres (minúsculas) de los procesos/paquetes que están usando el micrófono
/// ahora mismo: `LastUsedTimeStop == 0` en ConsentStore\microphone.
#[cfg(windows)]
pub fn mic_in_use_by() -> Vec<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn in_use(parent: &RegKey, sub: &str) -> bool {
        parent
            .open_subkey(sub)
            .ok()
            .and_then(|k| k.get_value::<u64, _>("LastUsedTimeStop").ok())
            .map(|v| v == 0)
            .unwrap_or(false)
    }

    let mut out = Vec::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let Ok(root) = hkcu.open_subkey(base) else {
        return out;
    };
    for name in root.enum_keys().flatten() {
        if name.eq_ignore_ascii_case("NonPackaged") {
            if let Ok(np) = root.open_subkey(&name) {
                for n in np.enum_keys().flatten() {
                    if in_use(&np, &n) {
                        // "C:#Program Files#Zoom#bin#Zoom.exe" → "zoom.exe"
                        let exe = n.rsplit('#').next().unwrap_or(&n).to_lowercase();
                        out.push(exe);
                    }
                }
            }
        } else if in_use(&root, &name) {
            out.push(name.to_lowercase());
        }
    }
    out
}

#[cfg(not(windows))]
pub fn list_window_titles() -> Vec<(u32, String)> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn mic_in_use_by() -> Vec<String> {
    Vec::new()
}
