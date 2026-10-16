//! Left control panel and right inspector.

use crate::app::*;
use crate::data::*;
use crate::layout::*;
use crate::model::*;
use crate::style::*;
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
    ComboBox::from_id_salt(id).selected_text(text).width(180.0).show_ui(ui, |ui| {
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

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, log: bool) -> bool {
    ui.add(egui::Slider::new(v, range).text(text).logarithmic(log)).changed()
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
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

    pub fn left_panel(&mut self, ui: &mut Ui) {
        let cols = self.col_infos();
        ui.add_space(4.0);
        self.section_data(ui);
        self.section_layout(ui, &cols);
        self.section_appearance(ui, &cols);
        self.section_filter(ui, &cols);
        self.section_analysis(ui);
        ui.add_space(20.0);
    }

    // ───────────────────────── data ─────────────────────────

    fn section_data(&mut self, ui: &mut Ui) {
        CollapsingHeader::new(RichText::new("Data").strong()).default_open(true).show(ui, |ui| {
            let mut remove_edges = false;
            let mut remove_nodes = false;
            let mut remap_edges = false;
            let mut remap_nodes = false;
            let mut directed_changed = false;
            if let Some(src) = &mut self.s.edge_src {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Edges").strong());
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
                    if ui.checkbox(&mut src.mapping.directed, "Directed").changed() {
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
                    ui.label(RichText::new("Nodes").strong());
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
                    self.pending = Some(Pending { name: src.name.clone(), table: src.table.clone(), kind: PendKind::Edges(src.mapping.clone()) });
                }
            }
            if remap_nodes {
                if let Some(src) = &self.s.node_src {
                    self.pending = Some(Pending { name: src.name.clone(), table: src.table.clone(), kind: PendKind::Nodes(src.mapping.clone()) });
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
        CollapsingHeader::new(RichText::new("Layout").strong()).default_open(true).show(ui, |ui| {
            let mut apply = false;
            let mut kind = self.s.layout.kind;
            ComboBox::from_id_salt("layout_kind").selected_text(kind.name()).width(230.0).show_ui(ui, |ui| {
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
                        changed |= ui.checkbox(&mut l.force.use_weights, "Heavier edges pull closer").changed();
                    }
                    row(ui, "Pull toward group", |ui| {
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
                    ui.checkbox(&mut self.pin_after_drag, "Keep dragged nodes pinned");
                    let status = if self.s.sim.running { format!("running · energy {:.3}", self.s.sim.alpha) } else { "settled".to_string() };
                    ui.weak(status);
                }
                LayoutKind::Circular | LayoutKind::Grid => {
                    row(ui, "Order by", |ui| {
                        apply |= combo_col(ui, "order_by", &mut l.order_by, cols, false, "(file order)");
                    });
                    apply |= ui.checkbox(&mut l.order_desc, "Descending").changed();
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
                        apply |= ui.checkbox(&mut l.center_high, "Highest values in the centre").changed();
                    }
                    row(ui, "Order within ring", |ui| {
                        apply |= combo_col(ui, "order_by2", &mut l.order_by, cols, false, "(by degree)");
                    });
                }
                LayoutKind::Hierarchical => {
                    ui.horizontal(|ui| {
                        apply |= ui.selectable_value(&mut l.orientation, Orientation::TopDown, "Top-down").changed();
                        apply |= ui.selectable_value(&mut l.orientation, Orientation::LeftRight, "Left-right").changed();
                        apply |= ui.selectable_value(&mut l.orientation, Orientation::Radial, "Radial").changed();
                    });
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
                        apply |= ui.checkbox(&mut l.x_log, "log").on_hover_text("Spread out skewed numeric values").changed();
                    });
                    row(ui, "Y axis", |ui| {
                        apply |= combo_col(ui, "y_by", &mut l.y_by, cols, false, "(none)");
                        apply |= ui.checkbox(&mut l.y_log, "log").on_hover_text("Spread out skewed numeric values").changed();
                    });
                }
                LayoutKind::Random => {}
            }
            ui.checkbox(&mut self.s.layout.visible_only, "Lay out visible (filtered) nodes only");
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
        CollapsingHeader::new(RichText::new("Appearance").strong()).default_open(true).show(ui, |ui| {
            let mut restyle = false;
            let look = &mut self.s.look;
            ui.label(RichText::new("Node colour").strong());
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
                        restyle |= ui.checkbox(&mut look.color_as_cat, "Treat values as categories").changed();
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
                            restyle |= ui.checkbox(&mut look.invert, "Invert").changed();
                            restyle |= ui.checkbox(&mut look.log_scale, "Log scale").on_hover_text("Spread out skewed values (degree, PageRank …)").changed();
                            restyle |= ui.checkbox(&mut look.robust, "Clip 2–98%").on_hover_text("Ignore extreme outliers when scaling").changed();
                        });
                    }
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Node size").strong());
            row(ui, "Size by", |ui| {
                restyle |= combo_col(ui, "size_by", &mut look.size_by, cols, true, "(constant)");
            });
            restyle |= slider(ui, &mut look.node_size, 1.0..=20.0, "Base size", false);
            if look.size_by.is_some() {
                restyle |= slider(ui, &mut look.size_min, 0.1..=1.0, "Smallest ×", false);
                restyle |= slider(ui, &mut look.size_max, 1.0..=8.0, "Largest ×", false);
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Edges").strong());
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
                ui.checkbox(&mut look.arrows, "Arrow heads");
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Labels and highlighting").strong());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut look.labels, LabelMode::Auto, "Labels when zoomed in");
                ui.selectable_value(&mut look.labels, LabelMode::Never, "Never");
            });
            row(ui, "Label text", |ui| {
                combo_col(ui, "label_by", &mut look.label_by, cols, false, "(node id)");
            });
            ui.checkbox(&mut look.highlight_hood, "Fade everything but the selection's neighbourhood");
            ui.horizontal(|ui| {
                ui.label("Filtered-out nodes:");
                ui.selectable_value(&mut look.filtered, FilteredMode::Hide, "Hide");
                ui.selectable_value(&mut look.filtered, FilteredMode::Dim, "Dim");
            });
            if restyle {
                self.restyle();
            }
        });
    }

    // ───────────────────────── filter ─────────────────────────

    fn section_filter(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        let active = !self.s.filter.is_empty() || self.s.n_manual_hidden() > 0;
        let title = if active { format!("Filter  ({} of {} nodes)", self.s.n_vis, self.s.g.n()) } else { "Filter".to_string() };
        CollapsingHeader::new(RichText::new(title).strong()).id_salt("filter").default_open(true).show(ui, |ui| {
            let mut changed = false;
            changed |= ui.add(egui::TextEdit::singleline(&mut self.s.filter.search).hint_text("Search id or text…").desired_width(f32::INFINITY)).changed();
            let resp = ui.add(egui::TextEdit::singleline(&mut self.s.filter.expr_text).hint_text("Expression: score > 3 and group == \"A\"").desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
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
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut rule.enabled, "").changed();
                        ui.label(RichText::new(&rule.col).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                remove = Some(ri);
                            }
                        });
                    });
                    match &mut rule.kind {
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
                            changed |= ui.checkbox(keep_missing, "Keep nodes without a value").changed();
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
                            egui::ScrollArea::vertical().id_salt(("cats", ri)).max_height(140.0).show(ui, |ui| {
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
        CollapsingHeader::new(RichText::new("Analysis").strong()).default_open(false).show(ui, |ui| {
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
                    ("Type", if g.directed { "directed".into() } else { "undirected".into() }),
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
            ui.label(RichText::new("Top nodes").strong());
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
        self.cam.center = self.s.pos[i];
        self.cam.zoom = self.cam.zoom.max(0.8);
    }

    // ───────────────────────── inspector ─────────────────────────

    pub fn inspector(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.label(RichText::new("Selection").strong());
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

        // legend
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Legend").strong());
        match &self.s.style.legend {
            Legend::None => {
                ui.weak("Single colour");
            }
            Legend::Cat(items) => {
                egui::ScrollArea::vertical().id_salt("legend_scroll").max_height(260.0).show(ui, |ui| {
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
