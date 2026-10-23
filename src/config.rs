//! Per-user settings: where they live, the recent-sessions list and the saved panel layout.

use crate::dock::Tab;
use crate::theme::{Backdrop, ThemeMode};
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Most recent first.
    pub recent: Vec<String>,
    /// Camera moves (zoom to selection, fit, presets …) glide instead of jumping.
    pub animate_view: bool,
    /// Dark, light, or follow the operating system.
    pub theme: ThemeMode,
    /// The graph background changes with the interface theme.
    pub graph_follows_theme: bool,
    /// Interface scale (1.0 = normal).
    pub ui_scale: f32,
    /// Open the panel arrangement that was open when Grapht was last closed.
    pub restore_layout: bool,
    /// The floating zoom / view buttons at the bottom of the graph.
    pub show_toolbar: bool,
    /// Details of the node under the pointer.
    pub show_tooltips: bool,
    /// The “ms / frame” read-out in the menu bar.
    pub show_frame_time: bool,
    /// The material Windows draws behind the window.
    pub backdrop: Backdrop,
    /// How solid the menu bar and the panels are (1 = opaque).
    pub panel_opacity: f32,
    /// How solid the graph view is.
    pub board_opacity: f32,
    /// How solid menus, popups and floating windows are.
    pub menu_opacity: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            recent: vec![],
            animate_view: true,
            theme: ThemeMode::Dark,
            graph_follows_theme: true,
            ui_scale: 1.0,
            restore_layout: true,
            show_toolbar: true,
            show_tooltips: true,
            show_frame_time: true,
            backdrop: Backdrop::Acrylic,
            panel_opacity: 0.4,
            board_opacity: 1.0,
            menu_opacity: 0.9,
        }
    }
}

impl Settings {
    /// The translucency these settings ask for, with `backdrop_on` saying whether Windows is drawing the material.
    /// Without it there is nothing behind the panels and the graph, so they stay solid; menus and floating windows still
    /// let the graph and panels show through.
    pub fn translucency(&self, backdrop_on: bool) -> crate::theme::Translucency {
        let see = |v: f32| if backdrop_on && self.backdrop != Backdrop::None { v.clamp(0.2, 1.0) } else { 1.0 };
        crate::theme::Translucency { panels: see(self.panel_opacity), board: see(self.board_opacity), menus: self.menu_opacity.clamp(0.3, 1.0) }
    }

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


// ───────────────────────────── workspace templates ─────────────────────────────

/// A named panel arrangement the user saved from the Window menu.
#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// The arrangement, in the same text form as `layout.json`.
    pub layout: String,
}

impl Workspace {
    pub fn dock(&self) -> Option<DockState<Tab>> {
        layout_from_str(&self.layout)
    }
}

pub fn load_workspaces() -> Vec<Workspace> {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("workspaces.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_workspaces(list: &[Workspace]) {
    if let (Some(d), Ok(t)) = (config_dir(), serde_json::to_string_pretty(list)) {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("workspaces.json"), t);
    }
}
