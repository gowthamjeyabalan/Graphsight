//! The Window menu (panels on and off, workspace templates), the Settings dialog, and the theme follow-up.

use crate::app::{App, WsDialog};
use crate::config::Workspace;
use crate::dock::{self, Tab};
use crate::theme::{self, ThemeMode, UiExt};
use egui::{Context, Ui};

/// What the user chose for one of their saved workspaces.
enum WsAct {
    Apply,
    Update,
    Delete,
}

const MAX_NAME: usize = 40;

impl App {
    /// Write the user's settings to disk (the real application only).
    pub fn save_settings(&mut self) {
        if self.persist {
            self.settings.save();
        }
    }

    /// Switch the section box on or off. The first time it is switched on it is fitted around the visible nodes.
    pub fn set_section(&mut self, on: bool) {
        self.s.section.enabled = on;
        if on && !self.s.section.placed {
            self.s.fit_section(false);
        }
    }

    /// When the interface changes between dark and light (the user's choice, or the operating system's), the graph
    /// background follows if the settings say so.
    pub fn follow_theme(&mut self, ctx: &Context) {
        let dark = ctx.theme() == egui::Theme::Dark;
        if dark != self.applied_dark {
            self.applied_dark = dark;
            if self.settings.graph_follows_theme {
                self.s.look.dark = dark;
                self.restyle();
            }
            ctx.request_repaint();
        }
    }

    // ───────────────────────── Window menu ─────────────────────────

    pub fn window_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Window", |ui| {
            ui.label(theme::secondary("Panels"));
            let mut dock = self.dock.take().unwrap_or_else(dock::default_dock);
            for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::Canvas) {
                let mut open = dock::is_open(&dock, tab);
                if ui.checkbox(&mut open, tab.title()).changed() {
                    if open {
                        dock::open_or_focus(&mut dock, tab);
                    } else {
                        dock::close(&mut dock, tab);
                    }
                }
            }
            ui.horizontal(|ui| {
                if ui.small_button("Show all").clicked() {
                    let closed: Vec<Tab> = Tab::ALL.into_iter().filter(|t| *t != Tab::Canvas && !dock::is_open(&dock, *t)).collect();
                    for tab in closed {
                        dock::open_or_focus(&mut dock, tab);
                    }
                }
                if ui.small_button("Hide all").clicked() {
                    dock::close_all_panels(&mut dock);
                }
            });
            ui.separator();

            // ── workspace templates ──
            ui.label(theme::secondary("Workspaces"));
            for (name, layout) in dock::builtin_workspaces() {
                if ui.button(name).on_hover_text("A ready-made arrangement of the panels").clicked() {
                    dock = layout;
                    ui.close();
                }
            }
            let mut act: Option<(usize, WsAct)> = None;
            if !self.workspaces.is_empty() {
                ui.separator();
                ui.label(theme::secondary("Your workspaces"));
                for (i, w) in self.workspaces.iter().enumerate() {
                    ui.menu_button(w.name.clone(), |ui| {
                        if ui.button("Open").clicked() {
                            act = Some((i, WsAct::Apply));
                            ui.close();
                        }
                        if ui.button("Replace with the current arrangement").clicked() {
                            act = Some((i, WsAct::Update));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            act = Some((i, WsAct::Delete));
                            ui.close();
                        }
                    });
                }
            }
            ui.separator();
            if ui.button("Save current arrangement as a workspace…").clicked() {
                self.ws_dialog = Some(WsDialog { name: String::new() });
                ui.close();
            }
            if ui.button("Reset panel layout").clicked() {
                dock = dock::default_dock();
                ui.close();
            }

            // the chosen workspace action is carried out after the lists above are done with
            if let Some((i, a)) = act {
                match a {
                    WsAct::Apply => {
                        if let Some(d) = self.workspaces.get(i).and_then(|w| w.dock()) {
                            dock = d;
                        } else {
                            self.error = Some("This workspace could not be read. Delete it and save it again.".into());
                        }
                    }
                    WsAct::Update => {
                        if let (Some(text), Some(w)) = (crate::config::layout_to_string(&dock), self.workspaces.get_mut(i)) {
                            w.layout = text;
                            self.persist_workspaces();
                        }
                    }
                    WsAct::Delete => {
                        if i < self.workspaces.len() {
                            self.workspaces.remove(i);
                            self.persist_workspaces();
                        }
                    }
                }
            }
            self.dock = Some(dock);
        });
    }

    fn persist_workspaces(&self) {
        if self.persist {
            crate::config::save_workspaces(&self.workspaces);
        }
    }

    /// The small dialog that names and saves the current panel arrangement.
    pub fn workspace_dialog(&mut self, ctx: &Context) {
        let Some(dlg) = &mut self.ws_dialog else { return };
        let mut save = false;
        let mut cancel = false;
        let existing = self.workspaces.iter().any(|w| w.name.eq_ignore_ascii_case(dlg.name.trim()));
        egui::Window::new("Save workspace").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label("Name for this arrangement of panels:");
            let resp = ui.add(egui::TextEdit::singleline(&mut dlg.name).hint_text("e.g. Review").desired_width(f32::INFINITY));
            if dlg.name.chars().count() > MAX_NAME {
                dlg.name = dlg.name.chars().take(MAX_NAME).collect();
            }
            if !resp.has_focus() && dlg.name.is_empty() {
                resp.request_focus();
            }
            let ready = !dlg.name.trim().is_empty();
            if existing {
                ui.label(theme::secondary("A workspace with this name exists; saving replaces it."));
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(ready, egui::Button::new(if existing { "Replace" } else { "Save" })).clicked() || (ready && resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    save = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if save {
            let name = dlg.name.trim().to_string();
            if let Some(text) = self.dock.as_ref().and_then(crate::config::layout_to_string) {
                match self.workspaces.iter_mut().find(|w| w.name.eq_ignore_ascii_case(&name)) {
                    Some(w) => {
                        w.name = name.clone();
                        w.layout = text;
                    }
                    None => self.workspaces.push(Workspace { name: name.clone(), layout: text }),
                }
                self.persist_workspaces();
                self.toast(ctx, format!("Workspace “{name}” saved"));
            }
            self.ws_dialog = None;
        } else if cancel {
            self.ws_dialog = None;
        }
    }

    // ───────────────────────── Settings ─────────────────────────

    /// The Settings menu in the menu bar: the common switches, and the full dialog.
    pub fn settings_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.menu_button("Settings", |ui| {
            let mut changed = false;
            ui.label(theme::secondary("Theme"));
            for (mode, name) in [(ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light"), (ThemeMode::System, "System")] {
                if ui.radio(self.settings.theme == mode, name).clicked() {
                    self.settings.theme = mode;
                    theme::set_mode(&ctx, mode);
                    changed = true;
                }
            }
            changed |= ui.checkbox(&mut self.settings.graph_follows_theme, "Graph background follows theme").changed();
            ui.separator();
            changed |= ui.checkbox(&mut self.settings.animate_view, "Animate view changes").changed();
            changed |= ui.checkbox(&mut self.settings.show_toolbar, "Show zoom / view buttons").changed();
            changed |= ui.checkbox(&mut self.settings.show_tooltips, "Show node details on hover").changed();
            changed |= ui.checkbox(&mut self.settings.show_frame_time, "Show frame time").changed();
            ui.separator();
            if ui.add(egui::Button::new("All settings…").shortcut_text(ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma)))).clicked() {
                self.show_settings = true;
                ui.close();
            }
            if changed {
                self.save_settings();
                ctx.request_repaint();
            }
        });
    }

    pub fn settings_dialog(&mut self, ctx: &Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut reset = false;
        let mut clear_recent = false;
        let mut reset_panels = false;
        egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(false).default_width(380.0).show(ctx, |ui| {
            theme::heading(ui, "Appearance");
            ui.label("Theme");
            let mut mode = self.settings.theme;
            if ui.segmented(&mut mode, &[(ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light"), (ThemeMode::System, "System")]) {
                self.settings.theme = mode;
                theme::set_mode(ctx, mode);
                changed = true;
            }
            changed |= ui.switch(&mut self.settings.graph_follows_theme, "Graph background follows the theme").on_hover_text("Also switch the graph background between dark and light.").changed();
            ui.horizontal(|ui| {
                ui.label("Interface size");
                let mut scale = self.settings.ui_scale;
                let r = ui.add(egui::Slider::new(&mut scale, 0.8..=1.6).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.settings.ui_scale = scale;
                    ctx.set_zoom_factor(scale);
                    changed = true;
                }
            });

            theme::heading(ui, "Graph view");
            changed |= ui.switch(&mut self.settings.animate_view, "Animate view changes").changed();
            changed |= ui.switch(&mut self.settings.show_toolbar, "Show the zoom / view buttons").changed();
            changed |= ui.switch(&mut self.settings.show_tooltips, "Show details when hovering a node").changed();
            changed |= ui.switch(&mut self.settings.show_frame_time, "Show frame time in the menu bar").changed();
            ui.label(theme::secondary("Filtered-out nodes"));
            let mut mode = self.s.look.filtered;
            if ui.segmented(&mut mode, &[(crate::model::FilteredMode::Dim, "Dim"), (crate::model::FilteredMode::Hide, "Hide")]) {
                self.s.look.filtered = mode;
            }

            theme::heading(ui, "Start-up and files");
            changed |= ui.switch(&mut self.settings.restore_layout, "Reopen the last panel arrangement").changed();
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.settings.recent.is_empty(), egui::Button::new("Clear recent sessions")).clicked() {
                    clear_recent = true;
                }
                if ui.button("Reset panel layout").clicked() {
                    reset_panels = true;
                }
            });
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Restore default settings").clicked() {
                    reset = true;
                }
                if let Some(d) = crate::config::config_dir() {
                    ui.label(theme::secondary(format!("Saved in {}", d.display())));
                }
            });
        });
        if clear_recent {
            self.settings.recent.clear();
            changed = true;
        }
        if reset_panels {
            self.dock = Some(dock::default_dock());
        }
        if reset {
            let keep = std::mem::take(&mut self.settings.recent);
            self.settings = crate::config::Settings { recent: keep, ..Default::default() };
            theme::set_mode(ctx, self.settings.theme);
            ctx.set_zoom_factor(self.settings.ui_scale);
            changed = true;
        }
        if changed {
            self.save_settings();
            ctx.request_repaint();
        }
        self.show_settings = open;
    }
}
