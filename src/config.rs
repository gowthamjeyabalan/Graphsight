//! Per-user settings: where they live, the recent-sessions list and the saved panel layout.

use crate::dock::Tab;
use egui_dock::DockState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// `%APPDATA%\Grapht` on Windows, `~/Library/Application Support/Grapht` on macOS, `~/.config/grapht` elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("GRAPHT_CONFIG_DIR") {
        return Some(d.into());
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Grapht"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join("Library/Application Support/Grapht"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("grapht"))
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Most recent first.
    pub recent: Vec<String>,
}

impl Settings {
    pub fn load() -> Settings {
        config_dir()
            .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let (Some(d), Ok(t)) = (config_dir(), serde_json::to_string_pretty(self)) {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("settings.json"), t);
        }
    }

    pub fn push_recent(&mut self, path: &std::path::Path) {
        let p = path.to_string_lossy().to_string();
        self.recent.retain(|r| *r != p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
        self.save();
    }
}

pub fn load_layout() -> Option<DockState<Tab>> {
    let t = std::fs::read_to_string(config_dir()?.join("layout.json")).ok()?;
    layout_from_str(&t)
}

/// Parse a saved layout. Panel rectangles that were never measured serialise as `null` (infinite floats);
/// they are recomputed on the first frame, so any such number is read back as 0.
pub fn layout_from_str(text: &str) -> Option<DockState<Tab>> {
    fn fix(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("x").is_some_and(|x| x.is_null()) && m.get("y").is_some_and(|y| y.is_null()) {
                    m.insert("x".into(), 0.0.into());
                    m.insert("y".into(), 0.0.into());
                }
                m.values_mut().for_each(fix);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(fix),
            _ => {}
        }
    }
    let mut v: serde_json::Value = serde_json::from_str(text).ok()?;
    fix(&mut v);
    let dock: DockState<Tab> = serde_json::from_value(v).ok()?;
    crate::dock::is_sane(&dock).then_some(dock)
}

pub fn layout_to_string(dock: &DockState<Tab>) -> Option<String> {
    serde_json::to_string(dock).ok()
}

pub fn save_layout_text(text: &str) {
    if let Some(d) = config_dir() {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("layout.json"), text);
    }
}

