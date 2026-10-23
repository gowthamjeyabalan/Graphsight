//! The Window menu (panels on and off, workspace templates), the Settings dialog, and the theme follow-up.

use crate::app::App;
use crate::config::Workspace;
use crate::dock::{self, Tab};
use crate::theme::{self, Backdrop, ThemeMode, UiExt};
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
            ui.set_min_width(240.0);
            section_label(ui, "Panels");
            let mut dock = self.dock.take().unwrap_or_else(dock::default_dock);
            for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::Canvas) {
                let open = dock::is_open(&dock, tab);
                if check_row(ui, tab.title(), open).clicked() {
                    if open {
                        dock::close(&mut dock, tab);
                    } else {
                        dock::open_or_focus(&mut dock, tab);
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
            ui.add_space(4.0);
            ui.separator();

            // ── workspace templates ──
            section_label(ui, "Workspaces");
            for (name, layout) in dock::builtin_workspaces() {
                if ui.button(name).on_hover_text("A ready-made arrangement of the panels").clicked() {
                    dock = layout;
                    ui.close();
                }
            }
            let mut act: Option<(usize, WsAct)> = None;
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
            // name it and save the arrangement that is on screen
            let mut save = false;
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.ws_name).hint_text("Name").desired_width(120.0));
                if self.ws_name.chars().count() > MAX_NAME {
                    self.ws_name = self.ws_name.chars().take(MAX_NAME).collect();
                }
                let ready = !self.ws_name.trim().is_empty();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.add_enabled(ready, egui::Button::new("Save current")).clicked() || (ready && enter) {
                    save = true;
                }
            });
            ui.add_space(4.0);
            ui.separator();
            if ui.button("Default layout").clicked() {
                dock = dock::default_dock();
                ui.close();
            }

            // the chosen workspace action is carried out after the lists above are done with
            if save {
                let name = self.ws_name.trim().to_string();
                if let Some(text) = crate::config::layout_to_string(&dock) {
                    match self.workspaces.iter_mut().find(|w| w.name.eq_ignore_ascii_case(&name)) {
                        Some(w) => {
                            w.name = name.clone();
                            w.layout = text;
                        }
                        None => self.workspaces.push(Workspace { name: name.clone(), layout: text }),
                    }
                    self.persist_workspaces();
                    self.ws_name.clear();
                    self.toast(ui.ctx(), format!("Workspace “{name}” saved"));
                }
            }
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

    /// “Settings” in the menu bar is a button, not a drop-down: it opens the settings window and nothing else.
    pub fn settings_button(&mut self, ui: &mut Ui) {
        if ui.button("Settings").on_hover_text("Appearance, translucency and the graph view  (Ctrl+,)").clicked() {
            self.show_settings = true;
        }
    }

    pub fn settings_dialog(&mut self, ctx: &Context) {
        if !self.show_settings {
            return;
        }
        let p = theme::pal_ctx(ctx);
        let screen = ctx.content_rect();
        let size = egui::vec2(840.0f32.min(screen.width() - 40.0), 600.0f32.min(screen.height() - 60.0));
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0)).corner_radius(12);
        let mut changed = false;
        let modal = egui::Modal::new(egui::Id::unique("settings")).frame(frame).show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                // ── sections, on the left ──
                egui::Frame::new().fill(p.bg).corner_radius(egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(176.0);
                        ui.set_min_height(size.y - 24.0);
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
                        ui.label(theme::semibold("Settings").size(16.0).color(p.text));
                        ui.add_space(12.0);
                        for (k, name) in SETTINGS_SECTIONS.iter().enumerate() {
                            let sel = self.settings_section == k;
                            let text = egui::RichText::new(*name).size(13.0).color(if sel { p.text } else { p.text_secondary });
                            if ui.add(egui::Button::selectable(sel, text).min_size(egui::vec2(176.0, 30.0))).clicked() {
                                self.settings_section = k;
                            }
                        }
                    });
                });
                // ── the section, on the right ──
                ui.vertical(|ui| {
                    ui.set_width(size.x - 200.0);
                    ui.set_min_height(size.y);
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label(theme::semibold(SETTINGS_SECTIONS[self.settings_section]).size(18.0).color(p.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(14.0);
                            if ui.add(egui::Button::new(egui::RichText::new("×").size(17.0)).frame(false)).on_hover_text("Close (Esc)").clicked() {
                                self.show_settings = false;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt(("settings-body", self.settings_section)).auto_shrink([false, false]).show(ui, |ui| {
                        egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 22, top: 4, bottom: 16 }).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            changed |= match self.settings_section {
                                0 => self.settings_appearance(ui),
                                1 => self.settings_graph_view(ui),
                                2 => self.settings_files(ui),
                                3 => settings_shortcuts(ui),
                                _ => settings_about(ui),
                            };
                        });
                    });
                });
            });
        });
        // Esc or a click outside closes it
        if modal.should_close() {
            self.show_settings = false;
        }
        if changed {
            if self.persist {
                theme::set_translucency(ctx, self.translucency());
            }
            self.save_settings();
            ctx.request_repaint();
        }
    }

    fn settings_appearance(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        settings_group(ui, "LOOK");
        setting_row(ui, "Mode", "Light, dark, or the same as Windows.", |ui| {
            ui.set_width(230.0);
            let mut mode = self.settings.theme;
            if ui.segmented(&mut mode, &[(ThemeMode::System, "System"), (ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light")]) {
                self.settings.theme = mode;
                theme::set_mode(&ctx, mode);
                changed = true;
            }
        });
        setting_row(ui, "Interface size", "Makes text, panels and controls larger or smaller. The graph's own zoom is separate.", |ui| {
            let mut pct = self.settings.ui_scale * 100.0;
            let mut moved = false;
            let mut done = false;
            ui.spacing_mut().slider_width = 170.0;
            // right to left: the slider, then its value in a box before it
            let r = ui.add(egui::Slider::new(&mut pct, 80.0..=160.0).show_value(false).step_by(5.0));
            moved |= r.changed();
            done |= r.drag_stopped() || (r.changed() && !r.dragged());
            let r = ui.add(egui::DragValue::new(&mut pct).range(80.0..=160.0).suffix("%").fixed_decimals(1).speed(0.5));
            moved |= r.changed();
            done |= r.drag_stopped() || (r.changed() && !r.dragged());
            if moved && done {
                self.settings.ui_scale = pct / 100.0;
                ctx.set_zoom_factor(self.settings.ui_scale);
                changed = true;
            }
        });
        setting_row(ui, "Graph background", "Switch the graph background between dark and light together with the interface.", |ui| {
            changed |= ui.switch(&mut self.settings.graph_follows_theme, "").changed();
        });

        settings_group(ui, "TRANSLUCENCY");
        setting_row(ui, "Window backdrop", "The material Windows 11 draws behind Grapht. Mica takes a soft tint from the desktop wallpaper, Mica Alt a stronger one, and Acrylic blurs whatever is behind the window.", |ui| {
            ui.set_width(300.0);
            let mut b = self.settings.backdrop;
            if ui.segmented(&mut b, &[(Backdrop::None, "None"), (Backdrop::Mica, "Mica"), (Backdrop::MicaAlt, "Mica Alt"), (Backdrop::Acrylic, "Acrylic")]) {
                self.settings.backdrop = b;
                changed = true;
            }
        });
        let on = self.settings.backdrop != Backdrop::None;
        if on && self.persist && !self.backdrop_on() {
            ui.label(theme::secondary("This version of Windows cannot draw a backdrop (it needs Windows 11, version 22H2 or later), so panels stay solid."));
            ui.add_space(8.0);
        }
        let solid = "Needs a window backdrop.";
        changed |= setting_row_percent(ui, "Panels", if on { "How solid the menu bar, the panels and the space around them are; lower lets more of the backdrop through." } else { solid }, &mut self.settings.panel_opacity, 0.2, on);
        changed |= setting_row_percent(ui, "Graph", if on { "How solid the graph view is. At 100% it is a plain surface; lower lets the backdrop through behind your work." } else { solid }, &mut self.settings.board_opacity, 0.2, on);
        changed |= setting_row_percent(ui, "Menus and floating windows", "Menus, popups and windows such as this one, over the graph and panels.", &mut self.settings.menu_opacity, 0.3, true);
        let d = crate::config::Settings::default();
        let custom = (self.settings.backdrop, self.settings.panel_opacity, self.settings.board_opacity, self.settings.menu_opacity) != (d.backdrop, d.panel_opacity, d.board_opacity, d.menu_opacity);
        if ui.add_enabled(custom, egui::Button::new("Reset translucency")).clicked() {
            self.settings.backdrop = d.backdrop;
            self.settings.panel_opacity = d.panel_opacity;
            self.settings.board_opacity = d.board_opacity;
            self.settings.menu_opacity = d.menu_opacity;
            changed = true;
        }
        changed
    }

    fn settings_graph_view(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        settings_group(ui, "VIEW");
        setting_row(ui, "Animate view changes", "Camera moves glide for about 0.3 s. Scrolling or dragging stops them.", |ui| {
            changed |= ui.switch(&mut self.settings.animate_view, "").changed();
        });
        setting_row(ui, "View buttons", "The zoom and view buttons floating over the graph.", |ui| {
            changed |= ui.switch(&mut self.settings.show_toolbar, "").changed();
        });
        setting_row(ui, "Node details on hover", "Show a node's attributes when the pointer rests on it.", |ui| {
            changed |= ui.switch(&mut self.settings.show_tooltips, "").changed();
        });
        setting_row(ui, "Frame time", "The ms / frame read-out at the right of the menu bar.", |ui| {
            changed |= ui.switch(&mut self.settings.show_frame_time, "").changed();
        });

        settings_group(ui, "FILTER");
        setting_row(ui, "Filtered-out nodes", "What happens to nodes that do not pass the filter: dimmed, or hidden altogether.", |ui| {
            ui.set_width(160.0);
            let mut mode = self.s.look.filtered;
            if ui.segmented(&mut mode, &[(crate::model::FilteredMode::Dim, "Dim"), (crate::model::FilteredMode::Hide, "Hide")]) {
                self.s.look.filtered = mode;
            }
        });
        changed
    }

    fn settings_files(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        settings_group(ui, "STARTING");
        setting_row(ui, "Reopen the panel arrangement", "When Grapht opens, put the panels back the way they were. When off, it starts with the default layout.", |ui| {
            changed |= ui.switch(&mut self.settings.restore_layout, "").changed();
        });
        setting_row(ui, "Panel layout", "Put every panel back in its usual place now.", |ui| {
            if ui.button("Reset panel layout").clicked() {
                self.dock = Some(dock::default_dock());
            }
        });

        settings_group(ui, "RECENT SESSIONS");
        let n = self.settings.recent.len();
        let about = match n {
            0 => "No sessions in File ▸ Open recent.".to_owned(),
            1 => "1 session in File ▸ Open recent.".to_owned(),
            n => format!("{n} sessions in File ▸ Open recent."),
        };
        setting_row(ui, "Recent sessions", &about, |ui| {
            if ui.add_enabled(n > 0, egui::Button::new("Clear")).clicked() {
                self.settings.recent.clear();
                changed = true;
            }
        });

        settings_group(ui, "SETTINGS");
        let place = crate::config::config_dir().map_or("Settings are kept for this session only.".to_owned(), |d| format!("Saved in {}", d.display()));
        setting_row(ui, "Restore defaults", &place, |ui| {
            if ui.button("Restore default settings").clicked() {
                let keep = std::mem::take(&mut self.settings.recent);
                self.settings = crate::config::Settings { recent: keep, ..Default::default() };
                theme::set_mode(&ctx, self.settings.theme);
                ctx.set_zoom_factor(self.settings.ui_scale);
                changed = true;
            }
        });
        changed
    }
}

const SETTINGS_SECTIONS: [&str; 5] = ["Appearance", "Graph view", "Start-up and files", "Keyboard shortcuts", "About"];

fn settings_shortcuts(ui: &mut Ui) -> bool {
    let p = theme::pal_ctx(ui.ctx());
    let groups: [(&str, &[(&str, &str)]); 3] = [
        ("FILES", &[("Ctrl+O", "Open session"), ("Ctrl+S", "Save session"), ("Ctrl+Shift+S", "Save session as"), ("Ctrl+,", "Settings")]),
        ("VIEW", &[("F", "Zoom to fit"), ("S", "Zoom to the selection"), ("R", "Zoom to a region (drag a rectangle)"), ("Home", "Home view"), ("Alt+Left / Alt+Right", "Previous / next view"), ("F11", "Maximise the graph"), ("Space + drag", "Pan")]),
        ("SELECTION", &[("Ctrl+A", "Select every visible node"), ("Ctrl / Shift + click", "Add to the selection"), ("Delete", "Hide the selected nodes"), ("Esc", "Clear the selection, then leave full screen"), ("Space", "Pause or resume the layout")]),
    ];
    for (title, keys) in groups {
        settings_group(ui, title);
        egui::Grid::new(("shortcuts", title)).num_columns(2).spacing(egui::vec2(24.0, 8.0)).min_col_width(170.0).show(ui, |ui| {
            for (combo, what) in keys {
                ui.label(egui::RichText::new(*combo).family(theme::medium_family()).size(13.0).color(p.text));
                ui.label(egui::RichText::new(*what).size(13.0).color(p.text_secondary));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
    }
    false
}

fn settings_about(ui: &mut Ui) -> bool {
    let p = theme::pal_ctx(ui.ctx());
    ui.add_space(8.0);
    ui.label(theme::semibold(format!("Grapht {}", env!("CARGO_PKG_VERSION"))).size(15.0).color(p.text));
    ui.label(egui::RichText::new("A fast native desktop app for exploring graphs stored in CSV and JSON files.").size(13.0).color(p.text_secondary));
    ui.add_space(10.0);
    ui.label(egui::RichText::new("Written in Rust with egui. Released under the MIT licence.").size(12.5).color(p.text_secondary));
    ui.add_space(6.0);
    ui.hyperlink_to("github.com/gowthamjeyabalan/Grapht", "https://github.com/gowthamjeyabalan/Grapht");
    false
}

/// A titled group of settings, with air above it.
fn settings_group(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    theme::heading(ui, title);
    ui.add_space(8.0);
}

// ───────────────────────── small widgets for the menus and the settings window ─────────────────────────

/// A small quiet title above a group of menu items or settings.
fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).family(theme::semibold_family()).size(11.5).weak());
}

/// A menu row with a tick in front when it is on. Clicking does not close the menu, so several can be switched in a row.
fn check_row(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let size = egui::vec2(ui.available_width().max(200.0), 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, v.widgets.hovered.bg_fill);
        }
        let ink = if resp.hovered() { v.widgets.hovered.fg_stroke.color } else { v.widgets.inactive.fg_stroke.color };
        if on {
            let c = egui::pos2(rect.left() + 14.0, rect.center().y);
            let st = egui::Stroke::new(1.8, ink);
            ui.painter().line_segment([c + egui::vec2(-4.5, 0.5), c + egui::vec2(-1.2, 3.8)], st);
            ui.painter().line_segment([c + egui::vec2(-1.2, 3.8), c + egui::vec2(5.0, -4.0)], st);
        }
        ui.painter().text(egui::pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::TextStyle::Button.resolve(ui.style()), ink);
    }
    resp
}

/// A setting: its name and a line about it on the left, the control on the right (laid out right to left).
fn setting_row(ui: &mut Ui, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    let p = theme::pal_ctx(ui.ctx());
    ui.horizontal(|ui| {
        let w = ui.available_width();
        ui.vertical(|ui| {
            ui.set_width((w * 0.5).max(180.0));
            ui.label(egui::RichText::new(title).size(13.5).color(p.text));
            if !desc.is_empty() {
                ui.label(egui::RichText::new(desc).size(12.0).color(p.text_secondary));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| control(ui));
    });
    ui.add_space(12.0);
}

/// A 0–100% setting: its value in a box next to a slider. Returns true when it changed.
fn setting_row_percent(ui: &mut Ui, title: &str, desc: &str, value: &mut f32, min: f32, enabled: bool) -> bool {
    let mut changed = false;
    setting_row(ui, title, desc, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let mut pct = *value * 100.0;
            ui.spacing_mut().slider_width = 170.0;
            changed |= ui.add(egui::Slider::new(&mut pct, min * 100.0..=100.0).show_value(false)).changed();
            changed |= ui.add(egui::DragValue::new(&mut pct).range(min * 100.0..=100.0).suffix("%").fixed_decimals(1).speed(0.5)).changed();
            *value = (pct / 100.0).clamp(min, 1.0);
        });
    });
    changed
}
