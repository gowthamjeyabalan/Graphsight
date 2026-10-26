//! Presets: named sets of settings that can be put back on any graph — the appearance (node and edge colours, sizes,
//! widths, line styles, labels), the edge attributes made from both ends, the filters (node and edge rules, the
//! expression, the search) and the layout settings. They are kept in `presets.json` next to the other settings;
//! settings for columns a graph does not have are skipped.

use crate::app::App;
use crate::config::config_dir;
use crate::edgeattr::EdgeAttr;
use crate::layout::LayoutSettings;
use crate::model::{FilterState, Look};
use crate::theme;
use egui::Ui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub look: Look,
    pub edge_attrs: Vec<EdgeAttr>,
    pub filter: FilterState,
    pub layout: LayoutSettings,
}

impl Default for Preset {
    fn default() -> Self {
        Preset { name: String::new(), look: Look::default(), edge_attrs: vec![], filter: FilterState::default(), layout: LayoutSettings::default() }
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

enum Act {
    Apply,
    Update,
    Delete,
}

impl App {
    /// The settings on screen now, as a preset called `name`.
    pub fn current_preset(&self, name: &str) -> Preset {
        Preset { name: name.to_string(), look: self.s.look.clone(), edge_attrs: self.s.edge_attrs.clone(), filter: self.s.filter.clone(), layout: self.s.layout.clone() }
    }

    /// Put a preset's settings on the graph that is loaded. The layout is run again only when its settings differ.
    pub fn apply_preset(&mut self, p: &Preset) {
        let same_layout = serde_json::to_string(&self.s.layout).ok() == serde_json::to_string(&p.layout).ok();
        self.s.look = p.look.clone();
        self.s.set_edge_attrs(p.edge_attrs.clone());
        self.s.filter = p.filter.clone();
        self.s.layout = p.layout.clone();
        self.s.drop_missing_columns();
        self.refilter();
        self.restyle();
        if !same_layout && self.s.g.n() > 0 {
            self.s.apply_layout_now();
            self.auto_fit = true;
        }
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
                ui.label(theme::secondary("None yet. Name the settings you have now and save them."));
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
            let mut save_now = false;
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("Name").desired_width(140.0));
                let ready = !self.preset_name.trim().is_empty();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.add_enabled(ready, egui::Button::new("Save current")).clicked() || (ready && enter) {
                    save_now = true;
                }
            });
            ui.add_space(4.0);
            ui.label(theme::secondary(
                "A preset keeps the appearance (colours, sizes, edge widths and line styles, labels), the edge attributes made from both ends, the filters and the layout settings. It works on any graph: settings for columns that graph lacks are skipped.",
            ));

            if save_now {
                let name: String = self.preset_name.trim().chars().take(60).collect();
                let p = self.current_preset(&name);
                match self.presets.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&name)) {
                    Some(q) => *q = p,
                    None => self.presets.push(p),
                }
                self.persist_presets();
                self.preset_name.clear();
                self.toast(ui.ctx(), format!("Preset “{name}” saved"));
            }
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
}
