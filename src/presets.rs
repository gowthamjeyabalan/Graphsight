//! Presets and named views.
//!
//! A **preset** is a named set of settings that can be put on any graph: the appearance (colours, sizes, edge widths
//! and line styles, labels, your own colours), the edge attributes made from both ends, the filters, and the Layout
//! tab: the layout, node coordinates and the 3D view, bounding boxes and how geometry is placed. Presets are kept in
//! `presets.json` next to the other settings; settings for columns a graph does not have are skipped.
//!
//! A **named view** is the view as it is now, kept in the session: all of the above plus the camera (2D and 3D), the
//! section box, where every node is and which nodes are hidden. Opening it brings all of that back.

use crate::app::App;
use crate::boxes::BoxSettings;
use crate::config::config_dir;
use crate::coords::{Cam3, CoordSettings};
use crate::edgeattr::EdgeAttr;
use crate::geometry::GeoSettings;
use crate::layout::LayoutSettings;
use crate::model::{FilterState, Look, ViewCam};
use crate::section::SectionSettings;
use crate::theme;
use egui::{Context, Ui};
use serde::{Deserialize, Serialize};

/// The settings a preset or a view keeps.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub look: Look,
    pub edge_attrs: Vec<EdgeAttr>,
    pub filter: FilterState,
    pub layout: LayoutSettings,
    pub coords: CoordSettings,
    pub boxes: BoxSettings,
    /// How solids are matched and placed (the geometry file itself stays as it is).
    pub geometry: GeoSettings,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    #[serde(flatten)]
    pub settings: Settings,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NamedView {
    pub name: String,
    #[serde(flatten)]
    pub settings: Settings,
    pub section: SectionSettings,
    pub cam: ViewCam,
    pub cam3: Cam3,
    /// Where every node is (by id), and the nodes hidden by hand.
    pub positions: Vec<(String, f32, f32)>,
    pub hidden: Vec<String>,
}

impl Default for NamedView {
    fn default() -> Self {
        NamedView {
            name: String::new(),
            settings: Settings::default(),
            section: SectionSettings::default(),
            cam: ViewCam::default(),
            cam3: Cam3::default(),
            positions: vec![],
            hidden: vec![],
        }
    }
}

pub fn load() -> Vec<Preset> {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("presets.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(list: &[Preset]) {
    if let (Some(d), Ok(t)) = (config_dir(), serde_json::to_string_pretty(list)) {
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join("presets.json"), t);
    }
}

/// What the name window is naming.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Naming {
    Preset,
    View,
}

/// The small window that asks for a name (typing in a menu closes it at the first click).
pub struct NameDialog {
    pub what: Naming,
    pub name: String,
}

enum Act {
    Apply,
    Update,
    Delete,
}

fn same<T: Serialize>(a: &T, b: &T) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

impl App {
    /// The settings on screen now.
    pub fn current_settings(&self) -> Settings {
        let mut geometry = self.s.geo_set.clone();
        geometry.path = None;
        Settings {
            look: self.s.look.clone(),
            edge_attrs: self.s.edge_attrs.clone(),
            filter: self.s.filter.clone(),
            layout: self.s.layout.clone(),
            coords: self.s.coords.clone(),
            boxes: self.s.boxes.clone(),
            geometry,
        }
    }

    /// The settings on screen now, as a preset called `name`.
    pub fn current_preset(&self, name: &str) -> Preset {
        Preset { name: name.to_string(), settings: self.current_settings() }
    }

    /// The view as it is now, called `name`.
    pub fn current_view(&self, name: &str) -> NamedView {
        let s = &self.s;
        let n = s.g.n();
        NamedView {
            name: name.to_string(),
            settings: self.current_settings(),
            section: s.section.clone(),
            cam: self.cam,
            cam3: s.cam3,
            positions: (0..n.min(s.pos.len())).map(|i| (s.g.ids[i].clone(), s.pos[i].x, s.pos[i].y)).collect(),
            hidden: (0..n.min(s.manual_hidden.len())).filter(|&i| s.manual_hidden[i]).map(|i| s.g.ids[i].clone()).collect(),
        }
    }

    /// Put these settings on the loaded graph. With `run_layout`, the layout is run again when its settings differ.
    pub fn apply_settings(&mut self, p: &Settings, run_layout: bool) {
        let same_layout = same(&self.s.layout, &p.layout);
        let same_coords = same(&self.s.coords, &p.coords);
        self.s.look = p.look.clone();
        self.s.set_edge_attrs(p.edge_attrs.clone());
        self.s.filter = p.filter.clone();
        self.s.layout = p.layout.clone();
        self.s.boxes = p.boxes.clone();
        // the geometry file stays; only how it is matched and placed changes
        let file = self.s.geo_set.path.clone();
        self.s.geo_set = p.geometry.clone();
        self.s.geo_set.path = file;
        self.s.rematch_geometry();
        if !same_coords {
            self.s.coords = p.coords.clone();
            self.s.apply_coords();
        }
        self.s.recompute_boxes();
        self.s.drop_missing_columns();
        self.refilter();
        self.restyle();
        if run_layout && !same_layout && self.s.g.n() > 0 && !self.s.coords_active() {
            self.s.apply_layout_now();
            self.auto_fit = true;
        }
    }

    /// Put a preset's settings on the graph that is loaded.
    pub fn apply_preset(&mut self, p: &Preset) {
        self.apply_settings(&p.settings, true);
    }

    /// Open a named view: its settings, section box, node places, hidden nodes and camera.
    pub fn open_view(&mut self, v: &NamedView) {
        self.apply_settings(&v.settings, false);
        self.s.section = v.section.clone();
        self.s.restore_view(&v.positions, &v.hidden);
        self.cam = v.cam;
        self.s.cam3 = v.cam3;
        self.auto_fit = false;
        self.refilter();
    }

    fn persist_presets(&self) {
        if self.persist {
            save(&self.presets);
        }
    }

    pub fn presets_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Presets", |ui| {
            ui.set_min_width(260.0);
            ui.set_max_width(300.0);
            ui.label(egui::RichText::new("Presets").family(theme::semibold_family()).size(11.5).weak());
            if self.presets.is_empty() {
                ui.label(theme::secondary("None yet."));
            }
            let mut act: Option<(usize, Act)> = None;
            for (i, p) in self.presets.iter().enumerate() {
                ui.menu_button(p.name.clone(), |ui| {
                    if ui.button("Apply").on_hover_text("Use these settings on this graph").clicked() {
                        act = Some((i, Act::Apply));
                        ui.close();
                    }
                    if ui.button("Replace with the current settings").clicked() {
                        act = Some((i, Act::Update));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        act = Some((i, Act::Delete));
                        ui.close();
                    }
                });
            }
            ui.add_space(2.0);
            if ui.button("Save the current settings as a preset…").clicked() {
                self.name_dialog = Some(NameDialog { what: Naming::Preset, name: String::new() });
                ui.close();
            }
            ui.add_space(4.0);
            ui.label(theme::secondary(
                "A preset keeps the appearance and your colours, the edge attributes made from both ends, the filters, and the Layout tab: layout, node coordinates and 3D, bounding boxes and geometry placement. It works on any graph; settings for columns a graph lacks are skipped.",
            ));
            if let Some((i, a)) = act {
                match a {
                    Act::Apply => {
                        let p = self.presets[i].clone();
                        self.apply_preset(&p);
                        self.toast(ui.ctx(), format!("Preset “{}” applied", p.name));
                    }
                    Act::Update => {
                        let name = self.presets[i].name.clone();
                        self.presets[i] = self.current_preset(&name);
                        self.persist_presets();
                        self.toast(ui.ctx(), format!("Preset “{name}” updated"));
                    }
                    Act::Delete => {
                        self.presets.remove(i);
                        self.persist_presets();
                    }
                }
            }
        });
    }

    pub fn views_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Views", |ui| {
            ui.set_min_width(260.0);
            ui.set_max_width(300.0);
            ui.label(egui::RichText::new("Named views").family(theme::semibold_family()).size(11.5).weak());
            if self.views.is_empty() {
                ui.label(theme::secondary("None yet."));
            }
            let mut act: Option<(usize, Act)> = None;
            for (i, v) in self.views.iter().enumerate() {
                ui.menu_button(v.name.clone(), |ui| {
                    if ui.button("Open").on_hover_text("Back to this view: camera, section box, layout and appearance").clicked() {
                        act = Some((i, Act::Apply));
                        ui.close();
                    }
                    if ui.button("Replace with the view as it is now").clicked() {
                        act = Some((i, Act::Update));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        act = Some((i, Act::Delete));
                        ui.close();
                    }
                });
            }
            ui.add_space(2.0);
            if ui.add_enabled(self.s.g.n() > 0, egui::Button::new("Save the current view…")).clicked() {
                self.name_dialog = Some(NameDialog { what: Naming::View, name: String::new() });
                ui.close();
            }
            ui.add_space(4.0);
            ui.label(theme::secondary(
                "A view keeps everything a preset does plus the camera (2D or 3D), the section box, where the nodes are and which are hidden. Views are saved with the session.",
            ));
            if let Some((i, a)) = act {
                match a {
                    Act::Apply => {
                        let v = self.views[i].clone();
                        self.open_view(&v);
                        self.toast(ui.ctx(), format!("View “{}”", v.name));
                    }
                    Act::Update => {
                        let name = self.views[i].name.clone();
                        self.views[i] = self.current_view(&name);
                        self.toast(ui.ctx(), format!("View “{name}” updated"));
                    }
                    Act::Delete => {
                        self.views.remove(i);
                    }
                }
            }
        });
    }

    /// The window that names a new preset or view.
    pub fn name_dialog(&mut self, ctx: &Context) {
        let Some(dlg) = &mut self.name_dialog else { return };
        let what = dlg.what;
        let existing = match what {
            Naming::Preset => self.presets.iter().any(|p| p.name.eq_ignore_ascii_case(dlg.name.trim())),
            Naming::View => self.views.iter().any(|v| v.name.eq_ignore_ascii_case(dlg.name.trim())),
        };
        let (title, ask, hint) = match what {
            Naming::Preset => ("Save preset", "Name for these settings:", "e.g. Systems by colour"),
            Naming::View => ("Save view", "Name for this view:", "e.g. Plant room, level 2"),
        };
        let mut save_now = false;
        let mut cancel = false;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(ask);
            let resp = ui.add(egui::TextEdit::singleline(&mut dlg.name).hint_text(hint).desired_width(f32::INFINITY));
            if dlg.name.chars().count() > 60 {
                dlg.name = dlg.name.chars().take(60).collect();
            }
            if !resp.has_focus() && dlg.name.is_empty() {
                resp.request_focus();
            }
            let ready = !dlg.name.trim().is_empty();
            if existing {
                ui.label(theme::secondary("This name is taken; saving replaces it."));
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(ready, egui::Button::new(if existing { "Replace" } else { "Save" })).clicked() || (ready && resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    save_now = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if save_now {
            let name = dlg.name.trim().to_string();
            match what {
                Naming::Preset => {
                    let p = self.current_preset(&name);
                    match self.presets.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&name)) {
                        Some(q) => *q = p,
                        None => self.presets.push(p),
                    }
                    self.persist_presets();
                    self.toast(ctx, format!("Preset “{name}” saved"));
                }
                Naming::View => {
                    let v = self.current_view(&name);
                    match self.views.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&name)) {
                        Some(q) => *q = v,
                        None => self.views.push(v),
                    }
                    self.toast(ctx, format!("View “{name}” saved; it is kept when you save the session"));
                }
            }
            self.name_dialog = None;
        } else if cancel {
            self.name_dialog = None;
        }
    }
}
