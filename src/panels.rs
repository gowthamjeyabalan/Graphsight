//! Left control panel and right inspector.

use crate::app::*;
use crate::data::*;
use crate::layout::*;
use crate::model::*;
use crate::style::*;
use crate::dock::Tab;
use crate::theme::{self, UiExt};
use egui::{CollapsingHeader, ComboBox, RichText, Ui};

#[derive(Clone)]
pub struct ColInfo {
    pub name: String,
    pub numeric: bool,
    pub metric: bool,
}

fn combo_col(ui: &mut Ui, id: &str, current: &mut Option<String>, cols: &[ColInfo], numeric_only: bool, none_label: &str) -> bool {
    let mut changed = false;
    let text = current.clone().unwrap_or_else(|| none_label.to_string());
    ComboBox::from_id_salt(id).selected_text(text).width(ui.spacing().combo_width).show_ui(ui, |ui| {
        if ui.selectable_label(current.is_none(), none_label).clicked() {
            *current = None;
            changed = true;
        }
        for c in cols.iter().filter(|c| !numeric_only || c.numeric) {
            let label = if c.metric { format!("{}  (computed)", c.name) } else { c.name.clone() };
            if ui.selectable_label(current.as_deref() == Some(c.name.as_str()), label).clicked() {
                *current = Some(c.name.clone());
                changed = true;
            }
        }
    });
    changed
}

/// The editor for one rule (numeric range or category checklist). Returns true if anything changed.
fn rule_body(ui: &mut Ui, kind: &mut RuleKind, col: &Column, id: usize) -> bool {
    let mut changed = false;
    match kind {
        RuleKind::Range { lo, hi, keep_missing } => {
            let (cmin, cmax) = col.num_range().unwrap_or((0.0, 1.0));
            let span = (cmax - cmin).abs();
            let speed = if span > 0.0 { span / 200.0 } else { 0.01 };
            ui.horizontal(|ui| {
                ui.label("from");
                changed |= ui.add(egui::DragValue::new(lo).speed(speed).range(cmin..=cmax)).changed();
                ui.label("to");
                changed |= ui.add(egui::DragValue::new(hi).speed(speed).range(cmin..=cmax)).changed();
            });
            let mut l = *lo as f32;
            let mut h = *hi as f32;
            let (fmin, fmax) = (cmin as f32, cmax as f32);
            if fmax > fmin {
                if ui.add(egui::Slider::new(&mut l, fmin..=fmax).show_value(false)).changed() {
                    *lo = (l as f64).min(*hi);
                    changed = true;
                }
                if ui.add(egui::Slider::new(&mut h, fmin..=fmax).show_value(false)).changed() {
                    *hi = (h as f64).max(*lo);
                    changed = true;
                }
            }
            changed |= ui.switch(keep_missing, "Keep nodes without a value").changed();
        }
        RuleKind::Cats { allowed } => {
            let (codes, labels) = col.as_categories();
            if allowed.len() != labels.len() + 1 {
                *allowed = vec![true; labels.len() + 1];
                changed = true;
            }
            let mut counts = vec![0usize; labels.len() + 1];
            for &c in &codes {
                counts[if c == NULL_CODE { labels.len() } else { c as usize }] += 1;
            }
            let n_on = allowed.iter().filter(|a| **a).count();
            ui.horizontal(|ui| {
                ui.weak(format!("{} of {} selected", n_on, allowed.len()));
                if ui.small_button("All").clicked() {
                    allowed.iter_mut().for_each(|a| *a = true);
                    changed = true;
                }
                if ui.small_button("None").clicked() {
                    allowed.iter_mut().for_each(|a| *a = false);
                    changed = true;
                }
                if ui.small_button("Invert").clicked() {
                    allowed.iter_mut().for_each(|a| *a = !*a);
                    changed = true;
                }
            });
            egui::ScrollArea::vertical().id_salt(("cats", id)).max_height(140.0).show(ui, |ui| {
                for (k, a) in allowed.iter_mut().enumerate() {
                    if k == labels.len() && counts[k] == 0 {
                        continue;
                    }
                    let name = if k == labels.len() { "(empty)".to_string() } else { labels[k].clone() };
                    changed |= ui.checkbox(a, format!("{name}  ({})", counts[k])).changed();
                }
            });
        }
    }
    changed
}

/// Width of the label column in a row: roughly a third of the panel, within sensible limits.
fn label_width(ui: &Ui) -> f32 {
    (ui.available_width() * 0.34).clamp(80.0, 140.0)
}

fn label_cell(ui: &mut Ui, text: &str, width: f32) {
    ui.allocate_ui_with_layout(egui::vec2(width, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(egui::vec2(width, 22.0));
        ui.add(egui::Label::new(text).truncate());
    });
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, log: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let lw = label_width(ui);
        label_cell(ui, text, lw);
        ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
        changed = ui.add(egui::Slider::new(v, range).logarithmic(log)).changed();
    });
    changed
}

fn body(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
    f(ui)
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let lw = label_width(ui);
        label_cell(ui, label, lw);
        ui.spacing_mut().combo_width = (ui.available_width() - 12.0).max(90.0);
        add(ui);
    });
}

impl App {
    fn col_infos(&self) -> Vec<ColInfo> {
        self.s
            .g
            .node_cols
            .iter()
            .map(|c| ColInfo { name: c.name.clone(), numeric: c.is_numeric(), metric: c.is_metric })
            .collect()
    }

    /// Content of one dockable panel.
    pub fn panel_content(&mut self, tab: Tab, ui: &mut Ui) {
        let cols = self.col_infos();
        theme::panel_body(ui, |ui| match tab {
            Tab::Data => self.section_data(ui),
            Tab::Layout => self.section_layout(ui, &cols),
            Tab::Appearance => self.section_appearance(ui, &cols),
            Tab::Filter => self.section_filter(ui, &cols),
            Tab::Analysis => self.section_analysis(ui),
            Tab::Subgraph => self.section_subgraph(ui, &cols),
            Tab::Inspector => self.inspector(ui),
            Tab::Legend => self.legend(ui),
            Tab::Canvas | Tab::Table => {}
        });
    }

    // ───────────────────────── data ─────────────────────────

    fn section_data(&mut self, ui: &mut Ui) {
        body(ui, |ui| {
            let mut remove_edges = false;
            let mut remove_nodes = false;
            let mut remap_edges = false;
            let mut remap_nodes = false;
            let mut directed_changed = false;
            if let Some(src) = &mut self.s.edge_src {
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::theme::semibold("Edges"));
                    ui.label(format!("{} · {} rows", src.name, src.table.rows.len()));
                });
                ui.horizontal(|ui| {
                    let m = &src.mapping;
                    ui.weak(format!(
                        "{} → {}{}",
                        src.table.headers[m.src],
                        src.table.headers[m.dst],
                        m.weight.map(|w| format!(" · weight: {}", src.table.headers[w])).unwrap_or_default()
                    ));
                });
                ui.horizontal(|ui| {
                    if let Some(dc) = src.mapping.dir_col {
                        let nd = self.s.g.edge_dir.iter().filter(|d| **d).count();
                        ui.weak(format!(
                            "Direction: {} · {} directed, {} undirected",
                            src.table.headers.get(dc).map(String::as_str).unwrap_or("?"),
                            nd,
                            self.s.g.edge_dir.len() - nd
                        ));
                    } else if ui.switch(&mut src.mapping.directed, "Directed").changed() {
                        directed_changed = true;
                    }
                    if ui.small_button("Remap…").clicked() {
                        remap_edges = true;
                    }
                    if ui.small_button("Remove").clicked() {
                        remove_edges = true;
                    }
                });
            } else {
                ui.weak("No edges loaded");
            }
            ui.add_space(2.0);
            if let Some(src) = &self.s.node_src {
                ui.horizontal_wrapped(|ui| {
                    ui.label(crate::theme::semibold("Nodes"));
                    ui.label(format!("{} · {} rows", src.name, src.table.rows.len()));
                });
                ui.horizontal(|ui| {
                    ui.weak(format!("id: {}", src.table.headers[src.mapping.id]));
                    if ui.small_button("Remap…").clicked() {
                        remap_nodes = true;
                    }
                    if ui.small_button("Remove").clicked() {
                        remove_nodes = true;
                    }
                });
            } else {
                ui.weak("No node attributes loaded (optional)");
            }
            if directed_changed {
                self.rebuild();
            }
            if remap_edges {
                if let Some(src) = &self.s.edge_src {
                    self.pending = Some(Pending { name: src.name.clone(), path: src.path.clone(), table: src.table.clone(), kind: PendKind::Edges(src.mapping.clone()) });
                }
            }
            if remap_nodes {
                if let Some(src) = &self.s.node_src {
                    self.pending = Some(Pending { name: src.name.clone(), path: src.path.clone(), table: src.table.clone(), kind: PendKind::Nodes(src.mapping.clone()) });
                }
            }
            if remove_edges {
                self.s.edge_src = None;
                self.rebuild();
            }
            if remove_nodes {
                self.s.node_src = None;
                self.rebuild();
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Load sample").clicked() {
                    self.load_sample();
                }
            });
            ui.weak("Tip: drop .csv / .json files onto the window.");
            let st = &self.s.g.stats;
            let mut notes = Vec::new();
            if st.skipped_edge_rows > 0 {
                notes.push(format!("{} edge rows skipped (empty endpoint)", st.skipped_edge_rows));
            }
            if st.self_loops > 0 {
                notes.push(format!("{} self-loops (not drawn)", st.self_loops));
            }
            if st.parallel_edges > 0 {
                notes.push(format!("{} parallel edges", st.parallel_edges));
            }
            if st.duplicate_node_rows > 0 {
                notes.push(format!("{} duplicate node ids ignored", st.duplicate_node_rows));
            }
            if st.unknown_direction > 0 {
                notes.push(format!("{} edges have an unrecognised direction value (default used)", st.unknown_direction));
            }
            if st.nodes_from_edges_only > 0 {
                notes.push(format!("{} nodes appear only in the edges file", st.nodes_from_edges_only));
            }
            for n in notes {
                ui.weak(format!("• {n}"));
            }
        });
    }

    // ───────────────────────── layout ─────────────────────────

    fn section_layout(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        body(ui, |ui| {
            if self.coords_section(ui, cols) {
                return; // the nodes sit at their coordinates: layout controls do not apply
            }
            ui.add_space(6.0);
            crate::theme::heading(ui, "Layout");
            let mut apply = false;
            let mut kind = self.s.layout.kind;
            ComboBox::from_id_salt("layout_kind").selected_text(kind.name()).width(ui.available_width() - 12.0).show_ui(ui, |ui| {
                for k in LayoutKind::ALL {
                    ui.selectable_value(&mut kind, k, k.name());
                }
            });
            if kind != self.s.layout.kind {
                self.s.layout.kind = kind;
                apply = true;
            }
            ui.add_space(2.0);
            let has_weights = self.s.g.weight_col.is_some();
            let l = &mut self.s.layout;
            match l.kind {
                LayoutKind::Force => {
                    let mut changed = false;
                    changed |= slider(ui, &mut l.force.charge, 1.0..=400.0, "Repulsion", true);
                    changed |= slider(ui, &mut l.force.link_dist, 4.0..=300.0, "Link distance", true);
                    changed |= slider(ui, &mut l.force.link_strength, 0.05..=3.0, "Link strength", false);
                    changed |= slider(ui, &mut l.force.gravity, 0.0..=0.4, "Gravity", false);
                    changed |= slider(ui, &mut l.force.friction, 0.05..=0.9, "Friction", false);
                    if has_weights {
                        changed |= ui.switch(&mut l.force.use_weights, "Heavier edges pull closer").changed();
                    }
                    row(ui, "Group by", |ui| {
                        changed |= combo_col(ui, "force_group", &mut l.group_by, cols, false, "(none)");
                    });
                    if l.group_by.is_some() {
                        changed |= slider(ui, &mut l.force.group_pull, 0.0..=0.5, "Group pull", false);
                    }
                    if changed {
                        self.s.refresh_sim_if_force();
                        self.sim_paused = false;
                    }
                    ui.horizontal(|ui| {
                        let label = if self.s.sim.running { "⏸ Pause" } else { "▶ Run" };
                        if ui.button(label).clicked() {
                            if self.s.sim.running {
                                self.s.sim.running = false;
                                self.sim_paused = true;
                            } else {
                                self.s.setup_force();
                                self.s.sim.reheat(0.6);
                                self.sim_paused = false;
                            }
                        }
                        if ui.button("Reheat").clicked() {
                            self.s.sim.reheat(1.0);
                            self.sim_paused = false;
                        }
                        if ui.button("Unpin all").clicked() {
                            self.s.sim.unpin_all();
                        }
                    });
                    ui.switch(&mut self.pin_after_drag, "Keep dragged nodes pinned");
                    let status = if self.s.sim.running { format!("running · energy {:.3}", self.s.sim.alpha) } else { "settled".to_string() };
                    ui.weak(status);
                }
                LayoutKind::Circular | LayoutKind::Grid => {
                    row(ui, "Order by", |ui| {
                        apply |= combo_col(ui, "order_by", &mut l.order_by, cols, false, "(file order)");
                    });
                    apply |= ui.switch(&mut l.order_desc, "Descending").changed();
                }
                LayoutKind::Concentric => {
                    row(ui, "Rings by", |ui| {
                        apply |= combo_col(ui, "ring_by", &mut l.ring_by, cols, false, "(single ring)");
                    });
                    let numeric_ring = l.ring_by.as_ref().and_then(|n| cols.iter().find(|c| &c.name == n)).map(|c| c.numeric).unwrap_or(false);
                    if numeric_ring {
                        let mut bins = l.ring_bins as f32;
                        if slider(ui, &mut bins, 2.0..=20.0, "Rings", false) {
                            l.ring_bins = bins.round() as usize;
                            apply = true;
                        }
                        apply |= ui.switch(&mut l.center_high, "Highest values in the centre").changed();
                    }
                    row(ui, "Order within ring", |ui| {
                        apply |= combo_col(ui, "order_by2", &mut l.order_by, cols, false, "(by degree)");
                    });
                }
                LayoutKind::Hierarchical => {
                    apply |= ui.segmented(&mut l.orientation, &[(Orientation::TopDown, "Top-down"), (Orientation::LeftRight, "Left-right"), (Orientation::Radial, "Radial")]);
                    ui.weak("Roots: nodes without incoming edges (directed) or the biggest hub per component.");
                }
                LayoutKind::GroupBy => {
                    row(ui, "Group by", |ui| {
                        apply |= combo_col(ui, "group_by", &mut l.group_by, cols, false, "(single group)");
                    });
                    ui.weak("Groups are packed side by side; hubs sit in the middle of each group.");
                }
                LayoutKind::Scatter => {
                    row(ui, "X axis", |ui| {
                        apply |= combo_col(ui, "x_by", &mut l.x_by, cols, false, "(file order)");
                        apply |= ui.switch(&mut l.x_log, "log").on_hover_text("Spread out skewed numeric values").changed();
                    });
                    row(ui, "Y axis", |ui| {
                        apply |= combo_col(ui, "y_by", &mut l.y_by, cols, false, "(none)");
                        apply |= ui.switch(&mut l.y_log, "log").on_hover_text("Spread out skewed numeric values").changed();
                    });
                }
                LayoutKind::Random => {}
            }
            ui.switch(&mut self.s.layout.visible_only, "Lay out visible (filtered) nodes only");
            if self.s.layout.kind != LayoutKind::Force && ui.button("Apply layout").clicked() {
                apply = true;
            }
            if self.s.layout.kind == LayoutKind::Force && ui.button("Restart layout from scratch").clicked() {
                apply = true;
            }
            if apply {
                self.s.apply_layout();
                self.sim_paused = false;
                self.auto_fit = true;
            }
        });
    }

    // ───────────────────────── appearance ─────────────────────────

    fn section_appearance(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        body(ui, |ui| {
            let mut restyle = false;
            let look = &mut self.s.look;
            crate::theme::heading(ui, "Node colour");
            row(ui, "Colour by", |ui| {
                restyle |= combo_col(ui, "color_by", &mut look.color_by, cols, false, "(single colour)");
            });
            let cur = look.color_by.as_ref().and_then(|n| self.s.g.node_col(n));
            match cur {
                None => {
                    row(ui, "Colour", |ui| {
                        restyle |= ui.color_edit_button_srgb(&mut look.node_color).changed();
                    });
                }
                Some(col) => {
                    let numeric = col.is_numeric() && !col.hint_cat;
                    if numeric {
                        restyle |= ui.switch(&mut look.color_as_cat, "Treat values as categories").changed();
                    }
                    if numeric && !look.color_as_cat {
                        row(ui, "Palette", |ui| {
                            ComboBox::from_id_salt("palette").selected_text(look.palette.name()).show_ui(ui, |ui| {
                                for p in Palette::ALL {
                                    restyle |= ui.selectable_value(&mut look.palette, p, p.name()).changed();
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            restyle |= ui.switch(&mut look.invert, "Invert").changed();
                            restyle |= ui.switch(&mut look.log_scale, "Log scale").on_hover_text("Spread out skewed values (degree, PageRank …)").changed();
                            restyle |= ui.switch(&mut look.robust, "Clip 2–98%").on_hover_text("Ignore extreme outliers when scaling").changed();
                        });
                    }
                }
            }
            ui.add_space(4.0);
            crate::theme::heading(ui, "Node size");
            row(ui, "Size by", |ui| {
                restyle |= combo_col(ui, "size_by", &mut look.size_by, cols, true, "(constant)");
            });
            restyle |= slider(ui, &mut look.node_size, 1.0..=20.0, "Base size", false);
            if look.size_by.is_some() {
                restyle |= slider(ui, &mut look.size_min, 0.1..=1.0, "Smallest ×", false);
                restyle |= slider(ui, &mut look.size_max, 1.0..=8.0, "Largest ×", false);
            }
            ui.add_space(4.0);
            crate::theme::heading(ui, "Edges");
            row(ui, "Colour", |ui| {
                ComboBox::from_id_salt("edge_mode")
                    .selected_text(match look.edge_mode {
                        EdgeColorMode::Uniform => "Single colour",
                        EdgeColorMode::Source => "Source node colour",
                        EdgeColorMode::Blend => "Blend of both ends",
                        EdgeColorMode::Attribute => "By edge attribute",
                    })
                    .show_ui(ui, |ui| {
                        restyle |= ui.selectable_value(&mut look.edge_mode, EdgeColorMode::Uniform, "Single colour").changed();
                        restyle |= ui.selectable_value(&mut look.edge_mode, EdgeColorMode::Source, "Source node colour").changed();
                        restyle |= ui.selectable_value(&mut look.edge_mode, EdgeColorMode::Blend, "Blend of both ends").changed();
                        if !self.s.g.edge_cols.is_empty() {
                            restyle |= ui.selectable_value(&mut look.edge_mode, EdgeColorMode::Attribute, "By edge attribute").changed();
                        }
                    });
            });
            if look.edge_mode == EdgeColorMode::Attribute {
                row(ui, "Attribute", |ui| {
                    let ecols: Vec<ColInfo> = self.s.g.edge_cols.iter().map(|c| ColInfo { name: c.name.clone(), numeric: c.is_numeric(), metric: false }).collect();
                    restyle |= combo_col(ui, "edge_by", &mut look.edge_by, &ecols, false, "(none)");
                });
            }
            slider(ui, &mut look.edge_alpha, 0.02..=1.0, "Opacity", true);
            slider(ui, &mut look.edge_width, 0.4..=5.0, "Width", false);
            if self.s.g.directed {
                ui.switch(&mut look.arrows, "Arrow heads");
            }
            ui.add_space(4.0);
            crate::theme::heading(ui, "Labels and highlighting");
            ui.segmented(&mut look.labels, &[(LabelMode::Auto, "Labels when zoomed in"), (LabelMode::Never, "No labels")]);
            row(ui, "Label text", |ui| {
                combo_col(ui, "label_by", &mut look.label_by, cols, false, "(node id)");
            });
            ui.switch(&mut look.highlight_hood, "Fade all but the selection's neighbours");
            ui.add_space(2.0);
            ui.label(theme::secondary("Filtered-out nodes"));
            ui.segmented(&mut look.filtered, &[(FilteredMode::Hide, "Hide"), (FilteredMode::Dim, "Dim")]);
            if restyle {
                self.restyle();
            }
        });
    }

    // ───────────────────────── filter ─────────────────────────

    fn section_filter(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        let active = !self.s.filter.is_empty() || self.s.n_manual_hidden() > 0;
        let title = format!("Showing {} of {} nodes", self.s.n_vis, self.s.g.n());
        body(ui, |ui| {
            // always shown, so the widgets below keep stable ids (a line that appears and disappears would
            // shift auto-generated ids and make the text boxes lose focus after the first typed character)
            ui.label(crate::theme::secondary(title));
            let mut changed = false;
            changed |= ui.add(egui::TextEdit::singleline(&mut self.s.filter.search).id_salt("filter_search").hint_text("Search id or text…").desired_width(f32::INFINITY)).changed();
            let resp = ui.add(egui::TextEdit::singleline(&mut self.s.filter.expr_text).id_salt("filter_expr").hint_text("Expression: score > 3 and group == \"A\"").desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
            changed |= resp.changed();
            resp.on_hover_text("Operators: == != < <= > >=  contains  startswith  in [a, b]  and  or  not  ( )\nUse `backticks` for column names with spaces. See Help for more.");
            if let Some(e) = &self.s.expr_error {
                ui.colored_label(egui::Color32::from_rgb(235, 100, 90), e);
            }
            ui.add_space(2.0);

            // rules
            let mut remove: Option<usize> = None;
            for (ri, rule) in self.s.filter.rules.iter_mut().enumerate() {
                let Some(col) = self.s.g.node_cols.iter().find(|c| c.name == rule.col) else { continue };
                theme::inner_card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.switch(&mut rule.enabled, "").changed();
                        ui.label(crate::theme::semibold(&rule.col));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                remove = Some(ri);
                            }
                        });
                    });
                    changed |= rule_body(ui, &mut rule.kind, col, ri);
                });
            }
            if let Some(r) = remove {
                self.s.filter.rules.remove(r);
                changed = true;
            }

            ui.horizontal(|ui| {
                ui.menu_button("＋ Add rule", |ui| {
                    ui.set_min_width(180.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for c in cols {
                            let label = if c.metric { format!("{}  (computed)", c.name) } else { c.name.clone() };
                            if ui.button(label).clicked() {
                                if let Some(col) = self.s.g.node_col(&c.name) {
                                    self.s.filter.rules.push(Rule::new_for(col));
                                    changed = true;
                                }
                                ui.close();
                            }
                        }
                    });
                });
                if active && ui.button("Clear all").clicked() {
                    self.s.filter = FilterState::default();
                    self.s.show_all_hidden();
                    changed = true;
                }
            });
            if self.s.n_manual_hidden() > 0 {
                ui.horizontal(|ui| {
                    ui.weak(format!("{} nodes hidden by hand", self.s.n_manual_hidden()));
                    if ui.small_button("Show them").clicked() {
                        self.s.show_all_hidden();
                        changed = true;
                    }
                });
            }
            if changed {
                self.refilter();
                if self.s.layout.visible_only {
                    self.s.refresh_sim_if_force();
                }
            }
            if active {
                ui.weak(format!("Showing {} nodes and {} edges", self.s.n_vis, self.s.n_vis_edges));
            }
        });
    }

    // ───────────────────────── analysis ─────────────────────────

    fn section_analysis(&mut self, ui: &mut Ui) {
        body(ui, |ui| {
            let g = &self.s.g;
            let n = g.n();
            if n == 0 {
                ui.weak("Load a graph first.");
                return;
            }
            let e = g.edges.len();
            let n_comp = g.node_col("_component").and_then(|c| c.num_range()).map(|(_, hi)| hi as usize + 1).unwrap_or(0);
            let n_comm = g.node_col("_community").and_then(|c| c.num_range()).map(|(_, hi)| hi as usize + 1).unwrap_or(0);
            let max_deg = g.node_col("_degree").and_then(|c| c.num_range()).map(|(_, hi)| hi).unwrap_or(0.0);
            let density = if n > 1 { 2.0 * e as f64 / (n as f64 * (n as f64 - 1.0)) } else { 0.0 };
            egui::Grid::new("stats").num_columns(2).show(ui, |ui| {
                for (k, v) in [
                    ("Nodes", n.to_string()),
                    ("Edges", e.to_string()),
                    ("Type", match g.edge_dir.iter().filter(|d| **d).count() {
                        0 => "undirected".into(),
                        k if k == g.edge_dir.len() => "directed".into(),
                        k => format!("mixed ({k} directed)"),
                    }),
                    ("Average degree", format!("{:.2}", 2.0 * e as f64 / n as f64)),
                    ("Max degree", format!("{max_deg:.0}")),
                    ("Density", format!("{density:.5}")),
                    ("Components", n_comp.to_string()),
                    ("Communities", n_comm.to_string()),
                ] {
                    ui.label(k);
                    ui.label(v);
                    ui.end_row();
                }
            });
            ui.weak("Computed columns (_degree, _pagerank, _kcore, _component, _community …) can be used for colour, size, filters and sorting.");
            ui.add_space(4.0);
            crate::theme::heading(ui, "Top nodes");
            let cols = self.col_infos();
            combo_col(ui, "top_metric", &mut self.top_metric, &cols, true, "(choose a numeric column)");
            let mut pick: Option<usize> = None;
            if let Some(col) = self.top_metric.as_ref().and_then(|m| self.s.g.node_col(m)) {
                let mut idx: Vec<usize> = (0..n).filter(|&i| self.s.vis[i] && col.num(i).is_some()).collect();
                idx.sort_by(|&a, &b| col.num(b).partial_cmp(&col.num(a)).unwrap_or(std::cmp::Ordering::Equal));
                egui::Grid::new("top").striped(true).show(ui, |ui| {
                    for &i in idx.iter().take(10) {
                        if ui.link(self.s.label_of(i)).clicked() {
                            pick = Some(i);
                        }
                        ui.label(col.label_at(i).unwrap_or_default());
                        ui.end_row();
                    }
                });
            }
            if let Some(i) = pick {
                self.select_and_focus(i);
            }
        });
    }

    pub fn select_and_focus(&mut self, i: usize) {
        self.s.select_only(i);
        self.auto_fit = false;
        if self.s.is_3d() {
            self.s.cam3.target = self.s.pos3[i];
            return;
        }
        self.cam.center = self.s.pos[i];
        self.cam.zoom = self.cam.zoom.max(0.8);
    }

    // ───────────────────────── inspector ─────────────────────────

    pub fn inspector(&mut self, ui: &mut Ui) {
        crate::theme::heading(ui, "Selection");
        let sel = self.s.selected_list();
        if sel.is_empty() {
            ui.weak("Click a node, or Shift-drag a box on the canvas.");
        } else if sel.len() == 1 {
            let i = sel[0] as usize;
            ui.label(RichText::new(self.s.label_of(i)).heading());
            egui::Grid::new("attrs").num_columns(2).striped(true).show(ui, |ui| {
                ui.weak("id");
                ui.label(&self.s.g.ids[i]);
                ui.end_row();
                for c in &self.s.g.node_cols {
                    ui.weak(&c.name);
                    ui.label(c.label_at(i).unwrap_or_else(|| "—".into()));
                    ui.end_row();
                }
            });
            let nb: Vec<u32> = self.s.g.neighbors(i).to_vec();
            ui.add_space(6.0);
            CollapsingHeader::new(format!("Neighbours ({})", nb.len())).default_open(true).show(ui, |ui| {
                let mut go = None;
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for &v in nb.iter().take(300) {
                        if ui.link(self.s.label_of(v as usize)).clicked() {
                            go = Some(v as usize);
                        }
                    }
                    if nb.len() > 300 {
                        ui.weak(format!("… and {} more", nb.len() - 300));
                    }
                });
                if let Some(v) = go {
                    self.select_and_focus(v);
                }
            });
        } else {
            ui.label(format!("{} nodes selected", sel.len()));
            // quick numeric summary
            egui::Grid::new("sel_stats").striped(true).num_columns(2).show(ui, |ui| {
                for c in self.s.g.node_cols.iter().filter(|c| c.is_numeric() && !c.hint_cat).take(8) {
                    let vals: Vec<f64> = sel.iter().filter_map(|&i| c.num(i as usize)).collect();
                    if vals.is_empty() {
                        continue;
                    }
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    ui.weak(format!("mean {}", c.name));
                    ui.label(fmt_num(mean));
                    ui.end_row();
                }
            });
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let some = self.s.sel_count > 0;
            if ui.add_enabled(some, egui::Button::new("＋ Neighbours")).clicked() {
                self.s.select_neighbors();
            }
            if ui.add_enabled(some, egui::Button::new("Invert")).clicked() {
                self.s.invert_selection();
            }
            if ui.add_enabled(some, egui::Button::new("Clear")).clicked() {
                self.s.clear_selection();
            }
            if ui.button("Select all visible").clicked() {
                for i in 0..self.s.g.n() {
                    if self.s.vis[i] {
                        self.s.set_selected(i, true);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let some = self.s.sel_count > 0;
            if ui.add_enabled(some, egui::Button::new("Hide")).clicked() {
                self.s.hide_selected();
                self.table.dirty = true;
            }
            if ui.add_enabled(some, egui::Button::new("Isolate")).on_hover_text("Hide everything except the selection").clicked() {
                self.s.isolate_selected();
                self.table.dirty = true;
            }
            if self.s.n_manual_hidden() > 0 && ui.button("Show all hidden").clicked() {
                self.s.show_all_hidden();
                self.table.dirty = true;
            }
        });
    }

    pub fn legend(&mut self, ui: &mut Ui) {
        match &self.s.style.legend {
            Legend::None => {
                ui.weak("Single colour");
            }
            Legend::Cat(items) => {
                egui::ScrollArea::vertical().id_salt("legend_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    for (name, c, count) in items.iter().take(80) {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 5.5, egui::Color32::from_rgb(c[0], c[1], c[2]));
                            ui.label(format!("{name}  ({count})"));
                        });
                    }
                    if items.len() > 80 {
                        ui.weak(format!("… {} more categories", items.len() - 80));
                    }
                });
            }
            Legend::Continuous { name, lo, hi, palette, log, invert } => {
                ui.label(name);
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(220.0), 14.0), egui::Sense::hover());
                let steps = 48;
                for k in 0..steps {
                    let t0 = k as f32 / steps as f32;
                    let mut t = t0;
                    if *log {
                        t = log_curve(t);
                    }
                    if *invert {
                        t = 1.0 - t;
                    }
                    let c = palette.color(t);
                    let x0 = r.left() + r.width() * t0;
                    let x1 = r.left() + r.width() * (k + 1) as f32 / steps as f32;
                    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x0, r.top()), egui::pos2(x1 + 0.5, r.bottom())), 0.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
                }
                ui.horizontal(|ui| {
                    ui.weak(fmt_num(*lo));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(fmt_num(*hi)));
                });
            }
        }
    }
}

// ───────────────────────── subgraph ─────────────────────────

impl App {
    fn section_subgraph(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        if self.s.g.n() == 0 {
            ui.weak("Load a graph first.");
            return;
        }
        self.sub_section(ui, cols);
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        self.change_section(ui, cols);
    }

    fn sub_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        crate::theme::heading(ui, "Show a subgraph");
        ui.weak("Keep only the nodes that match, lay them out again and leave the rest out. Switch it off to get the full graph and its layout back.");
        ui.add_space(4.0);
        let mut dirty = false;
        dirty |= ui.switch(&mut self.s.sub.enabled, "Show subgraph").changed();
        let mut cur = self.s.sub.rule.as_ref().map(|r| r.col.clone());
        row(ui, "Attribute", |ui| {
            if combo_col(ui, "sub_col", &mut cur, cols, false, "(choose an attribute)") {
                self.s.sub.rule = cur.as_ref().and_then(|c| self.s.g.node_col(c)).map(|col| {
                    let mut r = Rule::new_for(col);
                    if let RuleKind::Cats { allowed } = &mut r.kind {
                        allowed.iter_mut().for_each(|a| *a = false); // tick the values you want
                    }
                    r
                });
                if self.s.sub.rule.is_some() {
                    self.s.sub.enabled = true;
                }
                dirty = true;
            }
        });
        if let Some(rule) = &mut self.s.sub.rule {
            if let Some(col) = self.s.g.node_cols.iter().find(|c| c.name == rule.col) {
                theme::inner_card().show(ui, |ui| {
                    dirty |= rule_body(ui, &mut rule.kind, col, 1000);
                });
            }
        }
        ui.add_space(2.0);
        ui.label("Include neighbours");
        dirty |= ui.segmented(&mut self.s.sub.hops, &[(0u8, "None"), (1, "1 step"), (2, "2 steps"), (3, "3 steps")]);
        dirty |= ui
            .switch(&mut self.s.sub.connect, "Join separate pieces into one graph")
            .on_hover_text("Where the matching nodes fall apart into pieces, walk the full graph along the shortest paths and add the nodes needed to connect them. Added nodes are drawn dimmer.")
            .changed();
        if dirty {
            self.s.apply_subgraph();
            self.table.dirty = true;
        }
        if let Some(si) = self.s.sub_info.clone() {
            ui.add_space(4.0);
            if si.n_match == 0 {
                ui.weak("Nothing matches yet. Tick the values you want above.");
            } else {
                let shown = si.n_match + si.n_neighbours + si.n_connectors;
                ui.label(format!("{shown} nodes shown"));
                let mut parts = vec![format!("{} matching", si.n_match)];
                if si.n_neighbours > 0 {
                    parts.push(format!("{} neighbours", si.n_neighbours));
                }
                if si.n_connectors > 0 {
                    parts.push(format!("{} joining nodes", si.n_connectors));
                }
                ui.label(crate::theme::secondary(parts.join(" + ")));
                if self.s.sub.connect {
                    if si.unreachable > 0 {
                        ui.label(crate::theme::secondary(format!("{} pieces stay apart: the full graph does not connect them", si.unreachable)));
                    } else if si.pieces_before > 1 {
                        ui.label(crate::theme::secondary(format!("{} pieces joined into one graph", si.pieces_before)));
                    }
                } else if si.pieces_before > 1 {
                    ui.label(crate::theme::secondary(format!("{} separate pieces", si.pieces_before)));
                }
            }
        }
    }

    fn change_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        crate::theme::heading(ui, "Highlight where a value changes");
        ui.weak("Marks the connections whose two ends have different values of an attribute, for example where Level goes from 1 to 2.");
        ui.add_space(4.0);
        let mut dirty = false;
        dirty |= ui.switch(&mut self.s.change.enabled, "Highlight changes").changed();
        row(ui, "Attribute", |ui| {
            if combo_col(ui, "change_col", &mut self.s.change.col, cols, false, "(choose an attribute)") {
                self.s.change.pick = None;
                if self.s.change.col.is_some() {
                    self.s.change.enabled = true; // choosing an attribute is a clear request to see it
                }
                dirty = true;
            }
        });
        let numeric = self.s.change.col.as_deref().and_then(|c| self.s.g.node_col(c)).is_some_and(|c| c.is_numeric() && !c.hint_cat);
        if numeric {
            row(ui, "Ignore up to", |ui| {
                dirty |= ui.add(egui::DragValue::new(&mut self.s.change.min_diff).speed(0.05).range(0.0..=f64::MAX)).on_hover_text("Differences up to this size do not count as a change. 0 counts every difference.").changed();
            });
        }
        dirty |= ui.switch(&mut self.s.change.only, "Show only these connections").changed();
        if self.s.change.only {
            dirty |= ui
                .switch(&mut self.s.change.connect, "Join separate pieces into one graph")
                .on_hover_text("The changes usually form separate pieces. This adds the connections and nodes needed, along the shortest paths in the graph, to join them into one. They are drawn dimmer.")
                .changed();
        }
        if dirty {
            self.s.recompute_changes();
        }

        let Some(info) = self.s.change_info.clone() else {
            if self.s.change.enabled && self.s.change.col.is_none() {
                ui.add_space(6.0);
                ui.weak("Choose an attribute above.");
            }
            return;
        };
        ui.add_space(6.0);
        let pct = if info.n_compared > 0 { 100.0 * info.n_change as f64 / info.n_compared as f64 } else { 0.0 };
        ui.label(format!("{} of {} connections change ({pct:.1}%)", info.n_change, info.n_compared));
        ui.label(crate::theme::secondary(format!("{} nodes sit on a change", info.n_boundary_nodes)));
        if self.s.change.only && self.s.change.connect && info.pieces_before > 1 {
            let extra = if info.n_connectors > 0 { format!(" with {} joining nodes", info.n_connectors) } else { String::new() };
            if info.pieces_after > 1 {
                ui.label(crate::theme::secondary(format!("{} pieces, {} could not be joined{extra}", info.pieces_before, info.pieces_after - 1)));
            } else {
                ui.label(crate::theme::secondary(format!("{} pieces joined into one graph{extra}", info.pieces_before)));
            }
        }
        if info.n_unknown > 0 {
            ui.label(crate::theme::secondary(format!("{} connections skipped: a value is missing", info.n_unknown)));
        }
        if let Some(m) = info.mean_abs_diff {
            ui.label(crate::theme::secondary(format!("Average size of a change: {}", fmt_num(m))));
        }

        if !info.transitions.is_empty() {
            ui.add_space(6.0);
            ui.label(theme::semibold("Changes by value"));
            ui.weak("Click one to highlight only that kind of change.");
            let directed = self.s.g.directed;
            let mut toggle: Option<(String, String)> = None;
            egui::ScrollArea::vertical().id_salt("change_list").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                for t in &info.transitions {
                    let picked = self.s.change.pick.as_ref().is_some_and(|(a, b)| *a == t.from && *b == t.to);
                    let hint = match (directed, t.dir) {
                        (true, 1) => "  up",
                        (true, -1) => "  down",
                        _ => "",
                    };
                    let text = format!("{}  →  {}{}", t.from, t.to, hint);
                    ui.horizontal(|ui| {
                        let w = ui.available_width();
                        let r = ui.add_sized([(w - 64.0).max(80.0), 22.0], egui::Button::selectable(picked, text).truncate());
                        ui.label(RichText::new(t.count.to_string()).monospace());
                        if r.clicked() {
                            toggle = Some((t.from.clone(), t.to.clone()));
                        }
                    });
                }
            });
            if let Some(k) = toggle {
                self.s.change.pick = if self.s.change.pick.as_ref() == Some(&k) { None } else { Some(k) };
                self.s.recompute_changes();
            }
        } else if info.numeric && info.n_change > 0 {
            ui.add_space(4.0);
            ui.weak("This attribute has too many different values for a per-value list.");
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Select nodes at changes").clicked() {
                self.s.clear_selection();
                for (i, hit) in info.node_hi.iter().enumerate() {
                    if *hit {
                        self.s.set_selected(i, true);
                    }
                }
            }
            if self.s.change.pick.is_some() && ui.button("Show all changes").clicked() {
                self.s.change.pick = None;
                self.s.recompute_changes();
            }
        });
    }
}

// ───────────────────────── bounding boxes ─────────────────────────

impl App {
    /// "Bounding boxes" block in the Layout tab, shown while the 3D view is on.
    fn boxes_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        use crate::boxes::BoxColor;
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);
        crate::theme::heading(ui, "Bounding boxes");
        ui.weak("Draw a 3D box for each node from six attributes: the minimum and maximum of X, Y and Z, in the same units as the coordinates.");
        ui.add_space(4.0);
        let numeric: Vec<ColInfo> = cols.iter().filter(|c| c.numeric && !c.metric).cloned().collect();
        let mut recompute = false; // boxes change
        let mut refit = false; // and the camera should look at them
        let r = ui.switch(&mut self.s.boxes.enabled, "Show bounding boxes");
        if r.changed() {
            if self.s.boxes.enabled && !self.s.boxes.has_columns() {
                let (min, max) = crate::boxes::guess_columns(&self.s.g);
                for k in 0..3 {
                    if self.s.boxes.min[k].is_none() {
                        self.s.boxes.min[k] = min[k].clone();
                    }
                    if self.s.boxes.max[k].is_none() {
                        self.s.boxes.max[k] = max[k].clone();
                    }
                }
            }
            recompute = true;
            refit = true;
        }
        if self.s.boxes.enabled {
            for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                row(ui, &format!("Min {axis}"), |ui| {
                    if combo_col(ui, &format!("bx_min{k}"), &mut self.s.boxes.min[k], &numeric, true, "(choose a column)") {
                        recompute = true;
                        refit = true;
                    }
                });
            }
            for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                row(ui, &format!("Max {axis}"), |ui| {
                    if combo_col(ui, &format!("bx_max{k}"), &mut self.s.boxes.max[k], &numeric, true, "(choose a column)") {
                        recompute = true;
                        refit = true;
                    }
                });
            }
            if !self.s.boxes.has_columns() {
                ui.weak("Choose all six columns.");
            }

            ui.add_space(4.0);
            ui.label("Boxes for these nodes");
            let mut cur = self.s.boxes.rule.as_ref().map(|r| r.col.clone());
            row(ui, "Attribute", |ui| {
                if combo_col(ui, "bx_rule", &mut cur, cols, false, "(all nodes)") {
                    self.s.boxes.rule = cur.as_ref().and_then(|c| self.s.g.node_col(c)).map(|col| {
                        let mut r = Rule::new_for(col);
                        if let RuleKind::Cats { allowed } = &mut r.kind {
                            allowed.iter_mut().for_each(|a| *a = false); // tick the values you want
                        }
                        r
                    });
                    recompute = true;
                    refit = true;
                }
            });
            if let Some(rule) = &mut self.s.boxes.rule {
                if let Some(col) = self.s.g.node_cols.iter().find(|c| c.name == rule.col) {
                    theme::inner_card().show(ui, |ui| {
                        recompute |= rule_body(ui, &mut rule.kind, col, 2000);
                    });
                }
            }

            ui.add_space(4.0);
            ui.label("Colour");
            ui.segmented(&mut self.s.boxes.color_mode, &[(BoxColor::Single, "One colour"), (BoxColor::Node, "Node colour")]);
            if self.s.boxes.color_mode == BoxColor::Single {
                row(ui, "Box colour", |ui| {
                    ui.color_edit_button_srgb(&mut self.s.boxes.color);
                });
            } else {
                ui.weak("Each box takes the colour of its node. Choose \"Colour by\" in the Appearance tab to colour them by an attribute.");
            }
            slider(ui, &mut self.s.boxes.fill, 0.0..=1.0, "Fill opacity", false);
            slider(ui, &mut self.s.boxes.outline, 0.0..=1.0, "Outline opacity", false);
            slider(ui, &mut self.s.boxes.line_width, 0.5..=6.0, "Outline width", false);
            if ui.switch(&mut self.s.boxes.show_nodes, "Show nodes and edges").on_hover_text("Off shows the boxes alone. Nodes cannot be clicked then; use the table to select.").changed() {
                refit = true;
            }

            let info = self.s.box_info;
            if self.s.boxes.has_columns() {
                ui.add_space(2.0);
                let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") };
                if info.n_match == 0 {
                    ui.weak("Nothing matches yet. Tick the values you want above.");
                } else if info.n_boxes == 0 {
                    ui.weak("No boxes to draw: no matching node has all six values.");
                } else {
                    ui.label(crate::theme::secondary(plural(info.n_boxes, "box drawn", "boxes drawn")));
                }
                if info.n_incomplete > 0 {
                    let who = plural(info.n_incomplete, "matching node has", "matching nodes have");
                    let gets = if info.n_incomplete == 1 { "gets" } else { "get" };
                    ui.label(crate::theme::secondary(format!("{who} a missing value and {gets} no box")));
                }
            }
        }
        if recompute {
            self.s.recompute_boxes();
        }
        if refit {
            self.auto_fit = true;
        }
    }
}

// ───────────────────────── node coordinates ─────────────────────────

impl App {
    /// The "Node coordinates" block at the top of the Layout tab. Returns true while coordinates are in use.
    fn coords_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) -> bool {
        use crate::coords::View3;
        crate::theme::heading(ui, "Node coordinates");
        let numeric: Vec<ColInfo> = cols.iter().filter(|c| c.numeric && !c.metric).cloned().collect();
        let mut dirty = false;
        let r = ui
            .switch(&mut self.s.coords.enabled, "Place nodes by coordinates")
            .on_hover_text("Put every node at the X / Y (and Z) values stored in its attributes instead of running a layout.");
        if r.changed() {
            if self.s.coords.enabled && self.s.coords.x.is_none() && self.s.coords.y.is_none() {
                let (x, y, z) = crate::coords::guess_columns(&self.s.g);
                self.s.coords.three_d = z.is_some() && x.is_some() && y.is_some();
                self.s.coords.x = x;
                self.s.coords.y = y;
                self.s.coords.z = z;
            }
            dirty = true;
        }
        if self.s.coords.enabled {
            if numeric.len() < 2 {
                ui.weak("Needs at least two numeric node attributes (for X and Y).");
            }
            row(ui, "X", |ui| dirty |= combo_col(ui, "co_x", &mut self.s.coords.x, &numeric, true, "(choose a column)"));
            row(ui, "Y", |ui| dirty |= combo_col(ui, "co_y", &mut self.s.coords.y, &numeric, true, "(choose a column)"));
            row(ui, "Z (optional)", |ui| dirty |= combo_col(ui, "co_z", &mut self.s.coords.z, &numeric, true, "(none: flat)"));
            dirty |= ui.switch(&mut self.s.coords.keep_aspect, "Keep true proportions").on_hover_text("On: one scale for all axes. Off: every axis is stretched to the same size.").changed();
            // the 3D choice only exists once there is a Z column to look at
            if self.s.coords.z.is_some() && self.s.coords.x.is_some() && self.s.coords.y.is_some() {
                ui.label("View");
                if ui.segmented(&mut self.s.coords.three_d, &[(false, "2D (top down)"), (true, "3D orbit")]) {
                    self.auto_fit = true;
                }
            } else if self.s.coords.three_d {
                self.s.coords.three_d = false;
            }
            if self.s.is_3d() {
                ui.horizontal_wrapped(|ui| {
                    for (label, v) in [("Top", View3::Top), ("Front", View3::Front), ("Side", View3::Side), ("Perspective", View3::Iso)] {
                        if ui.small_button(label).clicked() {
                            self.s.cam3.preset(v);
                            self.auto_fit = true;
                        }
                    }
                });
                ui.weak("Drag to orbit, right-drag to pan, scroll to zoom, double-click empty space or press F to fit.");
                self.boxes_section(ui, cols);
            }
            if self.s.coords.x.is_none() || self.s.coords.y.is_none() {
                ui.weak("Choose the X and Y columns.");
            } else if self.s.coords_active() {
                if self.s.coords_missing > 0 {
                    ui.label(crate::theme::secondary(format!("{} nodes have no coordinate and sit in the middle", self.s.coords_missing)));
                }
                ui.weak("Layouts and dragging nodes are off while coordinates are used.");
            } else {
                ui.weak("These columns hold text in some rows, so they cannot be used as coordinates.");
            }
        }
        if dirty {
            self.s.apply_coords();
            self.auto_fit = true;
        }
        self.s.coords_active()
    }
}
