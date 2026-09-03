//! Heurísticas de detección de reunión en curso (Teams, Zoom, Google Meet).
use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

use super::win;
use crate::state::MeetingApps;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingInfo {
    /// Clave estable por reunión (para ignorarla mientras dure).
    pub key: String,
    /// teams | zoom | meet
    pub app: String,
    pub app_label: String,
    pub title: String,
}

/// Código de Google Meet: xxx-xxxx-xxx en minúsculas.
fn find_meet_code(title: &str) -> Option<String> {
    let b = title.as_bytes();
    let is_l = |c: u8| c.is_ascii_lowercase();
    let n = b.len();
    let mut i = 0;
    while i + 12 <= n {
        let w = &b[i..i + 12];
        let ok = (0..3).all(|k| is_l(w[k]))
            && w[3] == b'-'
            && (4..8).all(|k| is_l(w[k]))
            && w[8] == b'-'
            && (9..12).all(|k| is_l(w[k]))
            && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
            && (i + 12 == n || !b[i + 12].is_ascii_alphanumeric());
        if ok {
            return Some(String::from_utf8_lossy(w).to_string());
        }
        i += 1;
    }
    None
}

pub fn detect(sys: &mut System, apps: &MeetingApps) -> Option<MeetingInfo> {
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let procs: Vec<(u32, String)> = sys
        .processes()
        .iter()
        .map(|(pid, p)| (pid.as_u32(), p.name().to_string_lossy().to_lowercase()))
        .collect();
    let titles = win::list_window_titles();
    let mic = win::mic_in_use_by();
    let mic_has = |needle: &str| mic.iter().any(|m| m.contains(needle));

    // Teams: el proceso suele estar abierto siempre; solo cuenta si usa el micro.
    if apps.teams {
        if let Some((pid, _)) = procs
            .iter()
            .find(|(_, n)| n == "ms-teams.exe" || n == "teams.exe" || n == "msteams.exe")
        {
            if mic_has("teams") {
                let title = titles
                    .iter()
                    .find(|(p, t)| p == pid && !t.is_empty())
                    .map(|(_, t)| t.clone())
                    .unwrap_or_else(|| "Microsoft Teams".into());
                return Some(MeetingInfo {
                    key: format!("teams:{pid}"),
                    app: "teams".into(),
                    app_label: "Microsoft Teams".into(),
                    title,
                });
            }
        }
    }

    // Zoom: proceso + (ventana de reunión o micrófono en uso)
    if apps.zoom {
        let zoom_pids: Vec<u32> = procs
            .iter()
            .filter(|(_, n)| n == "zoom.exe")
            .map(|(p, _)| *p)
            .collect();
        if !zoom_pids.is_empty() {
            let meeting_title = titles.iter().find(|(p, t)| {
                zoom_pids.contains(p)
                    && (t.contains("Zoom Meeting")
                        || t.contains("Reunión de Zoom")
                        || t.contains("Zoom Webinar")
                        || t.starts_with("Zoom - "))
            });
            if meeting_title.is_some() || mic_has("zoom.exe") {
                let pid = meeting_title.map(|(p, _)| *p).unwrap_or(zoom_pids[0]);
                return Some(MeetingInfo {
                    key: format!("zoom:{pid}"),
                    app: "zoom".into(),
                    app_label: "Zoom".into(),
                    title: meeting_title
                        .map(|(_, t)| t.clone())
                        .unwrap_or_else(|| "Reunión de Zoom".into()),
                });
            }
        }
    }

    // Google Meet: pestaña con código de reunión en cualquier navegador
    if apps.meet {
        for (_, t) in &titles {
            if !t.contains("Meet") {
                continue;
            }
            if let Some(code) = find_meet_code(t) {
                return Some(MeetingInfo {
                    key: format!("meet:{code}"),
                    app: "meet".into(),
                    app_label: "Google Meet".into(),
                    title: t.clone(),
                });
            }
        }
    }

    None
}
