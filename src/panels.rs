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

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) -> egui::InnerResponse<()> {
    ui.horizontal(|ui| {
        let lw = label_width(ui);
        label_cell(ui, label, lw);
        ui.spacing_mut().combo_width = (ui.available_width() - 12.0).max(90.0);
        add(ui);
    })
}

/// Pick an attribute for edges: an edge column, or a node column at the source or the target.
fn combo_edge_ref(ui: &mut Ui, id: &str, current: &mut Option<crate::edgeattr::EdgeRef>, g: &crate::graph::Graph, none_label: &str) -> bool {
    use crate::edgeattr::{End, EdgeRef};
    let mut changed = false;
    let text = current.as_ref().map_or_else(|| none_label.to_string(), |r| r.label());
    ComboBox::from_id_salt(id).selected_text(text).width(ui.spacing().combo_width).height(360.0).show_ui(ui, |ui| {
        if ui.selectable_label(current.is_none(), none_label).clicked() {
            *current = None;
            changed = true;
        }
        let groups = [
            (End::Edge, "Edge attributes", g.edge_cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>()),
            (End::Source, "Source node", g.node_cols.iter().map(|c| c.name.clone()).collect()),
            (End::Target, "Target node", g.node_cols.iter().map(|c| c.name.clone()).collect()),
        ];
        for (end, title, names) in groups {
            if names.is_empty() {
                continue;
            }
            ui.label(theme::secondary(title));
            for name in names {
                let r = EdgeRef { end, col: name };
                if ui.selectable_label(current.as_ref() == Some(&r), r.label()).clicked() {
                    *current = Some(r);
                    changed = true;
                }
            }
        }
    });
    changed
}

fn edge_mode_name(m: EdgeColorMode) -> &'static str {
    match m {
        EdgeColorMode::Uniform => "Single colour",
        EdgeColorMode::Source => "Source node's colour",
        EdgeColorMode::Blend => "Blend of both ends' colours",
        EdgeColorMode::SourceAttr => "Source node attribute",
        EdgeColorMode::TargetAttr => "Target node attribute",
        EdgeColorMode::Attribute => "Edge attribute",
    }
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
            Tab::Query => self.section_query(ui, &cols),
            Tab::Section => self.section_panel(ui),
            Tab::Inspector => self.inspector(ui),
            Tab::Legend => self.legend(ui),
            Tab::Canvas | Tab::Table | Tab::EdgeTable => {}
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
            crate::theme::heading(ui, "Node shape");
            row(ui, "Shape by", |ui| {
                restyle |= combo_col(ui, "shape_by", &mut look.shape_by, cols, false, "(circles)");
            })
            .response
            .on_hover_text("One shape per value: circle, square, triangle, triangle pointing down, diamond, pentagon, hexagon, star, plus, ring. In the 3D coordinates view they become sphere, cube, cone, cone pointing down, octahedron, pentagonal and hexagonal prism, star prism, 3D cross and torus.");
            ui.add_space(4.0);
            crate::theme::heading(ui, "Edges");
            row(ui, "Colour", |ui| {
                ComboBox::from_id_salt("edge_mode").selected_text(edge_mode_name(look.edge_mode)).show_ui(ui, |ui| {
                    use EdgeColorMode::*;
                    for m in [Uniform, Source, Blend, SourceAttr, TargetAttr, Attribute] {
                        if m == Attribute && self.s.g.edge_cols.is_empty() {
                            continue;
                        }
                        restyle |= ui.selectable_value(&mut look.edge_mode, m, edge_mode_name(m)).changed();
                    }
                });
            });
            let edge_col = match look.edge_mode {
                EdgeColorMode::Attribute => {
                    row(ui, "Attribute", |ui| {
                        let ecols: Vec<ColInfo> = self.s.g.edge_cols.iter().map(|c| ColInfo { name: c.name.clone(), numeric: c.is_numeric(), metric: c.is_metric }).collect();
                        restyle |= combo_col(ui, "edge_by", &mut look.edge_by, &ecols, false, "(none)");
                    });
                    look.edge_by.as_deref().and_then(|c| self.s.g.edge_col(c))
                }
                EdgeColorMode::SourceAttr | EdgeColorMode::TargetAttr => {
                    row(ui, "Node attribute", |ui| {
                        restyle |= combo_col(ui, "edge_node_by", &mut look.edge_node_by, cols, false, "(none)");
                    });
                    look.edge_node_by.as_deref().and_then(|c| self.s.g.node_col(c))
                }
                _ => None,
            };
            if let Some(col) = edge_col {
                let numeric = col.is_numeric() && !col.hint_cat;
                if numeric {
                    restyle |= ui.switch(&mut look.edge_as_cat, "Treat values as categories").on_hover_text("For the edge colours").changed();
                }
                if numeric && !look.edge_as_cat {
                    row(ui, "Palette", |ui| {
                        ComboBox::from_id_salt("edge_palette").selected_text(look.palette.name()).show_ui(ui, |ui| {
                            for p in Palette::ALL {
                                restyle |= ui.selectable_value(&mut look.palette, p, p.name()).changed();
                            }
                        });
                    })
                    .response
                    .on_hover_text("Shared with the node colours");
                }
            }
            restyle |= self.edge_attr_builder(ui, cols);
            let look = &mut self.s.look;
            row(ui, "Width by", |ui| {
                restyle |= combo_edge_ref(ui, "edge_width_by", &mut look.edge_width_by, &self.s.g, "(same width)");
            });
            if look.edge_width_by.is_some() {
                restyle |= slider(ui, &mut look.edge_wmin, 0.1..=2.0, "Thinnest ×", false);
                restyle |= slider(ui, &mut look.edge_wmax, 1.0..=10.0, "Thickest ×", false);
            }
            row(ui, "Line style by", |ui| {
                restyle |= combo_edge_ref(ui, "edge_dash_by", &mut look.edge_dash_by, &self.s.g, "(solid lines)");
            })
            .response
            .on_hover_text("Solid, dashed, dotted … one pattern per value");
            slider(ui, &mut look.edge_alpha, 0.02..=1.0, "Opacity", true);
            slider(ui, &mut look.edge_width, 0.4..=5.0, if look.edge_width_by.is_some() { "Base width" } else { "Width" }, false);
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
            if restyle {
                self.restyle();
            }
        });
    }

    // ───────────────────────── filter ─────────────────────────

    fn section_filter(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        let active = !self.s.filter.is_empty() || self.s.n_manual_hidden() > 0;
        let title = if self.s.filter.edge_rules.is_empty() {
            format!("Showing {} of {} nodes", self.s.n_vis, self.s.g.n())
        } else {
            format!("Showing {} of {} nodes, {} of {} edges", self.s.n_vis, self.s.g.n(), self.s.n_vis_edges, self.s.g.edges.len())
        };
        body(ui, |ui| {
            // always shown, so the widgets below keep stable ids (a line that appears and disappears would
            // shift auto-generated ids and make the text boxes lose focus after the first typed character)
            ui.label(crate::theme::secondary(title));
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(theme::secondary("Nodes that do not pass"));
                ui.segmented(&mut self.s.look.filtered, &[(FilteredMode::Dim, "Dim"), (FilteredMode::Hide, "Hide")]);
            });
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
                theme::inner_card(ui).show(ui, |ui| {
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
            // rules on edge attributes
            if !self.s.g.edge_cols.is_empty() {
                ui.add_space(4.0);
                theme::heading(ui, "Edges");
                let mut remove: Option<usize> = None;
                for (ri, rule) in self.s.filter.edge_rules.iter_mut().enumerate() {
                    let Some(col) = self.s.g.edge_cols.iter().find(|c| c.name == rule.col) else { continue };
                    theme::inner_card(ui).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            changed |= ui.switch(&mut rule.enabled, "").changed();
                            ui.label(crate::theme::semibold(&rule.col));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() {
                                    remove = Some(ri);
                                }
                            });
                        });
                        changed |= rule_body(ui, &mut rule.kind, col, 10_000 + ri);
                    });
                }
                if let Some(r) = remove {
                    self.s.filter.edge_rules.remove(r);
                    changed = true;
                }
                ui.menu_button("＋ Add edge rule", |ui| {
                    ui.set_min_width(180.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for c in &self.s.g.edge_cols {
                            let label = if c.is_metric { format!("{}  (from both ends)", c.name) } else { c.name.clone() };
                            if ui.button(label).clicked() {
                                self.s.filter.edge_rules.push(Rule::new_for(c));
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                })
                .response
                .on_hover_text("Edges that fail a rule are dimmed or hidden, like filtered-out nodes");
            }
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
        self.run_tool(crate::view::Tool::Selection);
    }

    // ───────────────────────── inspector ─────────────────────────

    pub fn inspector(&mut self, ui: &mut Ui) {
        crate::theme::heading(ui, "Selection");
        let sel = self.s.selected_list();
        if sel.is_empty() && self.s.sel_edges.len() == 1 {
            self.edge_details(ui, self.s.sel_edges[0] as usize, true);
        } else if sel.is_empty() && self.s.sel_edges.len() > 1 {
            ui.label(format!("{} edges selected", self.s.sel_edges.len()));
        } else if sel.is_empty() {
            ui.weak("Click a node or an edge, or Shift-drag a box on the canvas.");
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

    /// Appearance ▸ Edges: make an edge attribute from the nodes at both ends. Returns true when the colours change.
    fn edge_attr_builder(&mut self, ui: &mut Ui, cols: &[ColInfo]) -> bool {
        use crate::edgeattr::{Combine, EdgeAttr};
        let mut restyle = false;
        CollapsingHeader::new("Edge attributes from both ends").id_salt("edge_attrs").default_open(!self.s.edge_attrs.is_empty()).show(ui, |ui| {
            ui.label(theme::secondary("Make an edge attribute from the nodes at the two ends of every edge, then colour the edges by it."));
            let d = &mut self.edge_draft;
            let before = (d.col.clone(), d.how);
            row(ui, "Node attribute", |ui| {
                combo_col(ui, "edge_draft_col", &mut d.col, cols, false, "(choose)");
            });
            let numeric = d.col.as_deref().and_then(|c| self.s.g.node_col(c)).is_some_and(|c| c.is_numeric());
            if d.how.numbers_only() && !numeric {
                d.how = Combine::Pair;
            }
            row(ui, "Make", |ui| {
                ComboBox::from_id_salt("edge_draft_how").selected_text(d.how.label()).show_ui(ui, |ui| {
                    for h in Combine::ANY.into_iter().chain(Combine::NUMBERS.into_iter().filter(|_| numeric)) {
                        ui.selectable_value(&mut d.how, h, h.label());
                    }
                });
            });
            // the suggested name follows the choices until it is typed over
            let suggested = |c: &Option<String>, h: Combine| c.as_deref().map(|c| EdgeAttr::default_name(c, h)).unwrap_or_default();
            if d.name.is_empty() || d.name == suggested(&before.0, before.1) {
                d.name = suggested(&d.col, d.how);
            }
            if (d.col.clone(), d.how) != before {
                d.error = None;
            }
            row(ui, "Name", |ui| {
                ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(f32::INFINITY));
            });
            let ready = d.col.is_some();
            if ui.add_enabled(ready, egui::Button::new("Add edge attribute")).on_hover_text("Then the edges are coloured by it").clicked() {
                let a = EdgeAttr { name: d.name.trim().to_string(), col: d.col.clone().unwrap_or_default(), how: d.how };
                match self.s.add_edge_attr(a.clone()) {
                    Ok(()) => {
                        self.s.look.edge_mode = EdgeColorMode::Attribute;
                        self.s.look.edge_by = Some(a.name);
                        self.edge_draft = Default::default();
                        restyle = true;
                    }
                    Err(e) => self.edge_draft.error = Some(e),
                }
            }
            if let Some(e) = &self.edge_draft.error {
                ui.colored_label(egui::Color32::from_rgb(235, 100, 90), e);
            }
            let mut remove = None;
            for (i, a) in self.s.edge_attrs.iter().enumerate() {
                let missing = self.s.g.node_col(&a.col).is_none();
                ui.horizontal(|ui| {
                    if ui.small_button("×").on_hover_text("Remove this edge attribute").clicked() {
                        remove = Some(i);
                    }
                    ui.label(&a.name);
                    ui.label(theme::secondary(if missing { format!("{} is not in this graph", a.col) } else { format!("{}: {}", a.col, a.how.short()) }));
                });
            }
            if let Some(i) = remove {
                self.s.remove_edge_attr(i);
                restyle = true;
            }
        });
        restyle
    }

    pub fn legend(&mut self, ui: &mut Ui) {
        let st = &self.s.style;
        let colours = !matches!(st.edge_legend, Legend::None);
        let edges = colours || st.width_legend.is_some() || !st.dash_legend.is_empty();
        let shapes = !st.shape_legend.is_empty();
        let solid = self.s.is_3d();
        egui::ScrollArea::vertical().id_salt("legend_scroll").auto_shrink([false, false]).show(ui, |ui| {
            if edges || shapes {
                theme::heading(ui, "Nodes");
            }
            legend_body(ui, &st.legend, None);
            if shapes {
                ui.add_space(4.0);
                ui.label(format!("Shape: {}", st.shape_title));
                let ink = ui.visuals().text_color();
                for (name, k, count) in st.shape_legend.iter().take(40) {
                    let shape = crate::shapes::NodeShape::from_index(*k);
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                        let mut mesh = egui::epaint::Mesh::default();
                        crate::shapes::fill_flat(&mut mesh, shape, r.center(), 5.5, ink);
                        ui.painter().add(egui::Shape::mesh(mesh));
                        ui.label(format!("{name}  ({count})")).on_hover_text(if solid { shape.solid_name() } else { shape.name() });
                    });
                }
                if st.shape_legend.len() > 40 {
                    ui.weak(format!("… {} more (shapes repeat after ten)", st.shape_legend.len() - 40));
                }
            }
            if !edges {
                return;
            }
            ui.add_space(6.0);
            theme::heading(ui, "Edges");
            let ink = ui.visuals().text_color();
            if colours {
                legend_body(ui, &st.edge_legend, Some(&st.edge_legend_title));
                ui.add_space(4.0);
            }
            if let Some((title, lo, hi)) = &st.width_legend {
                ui.label(format!("Width: {title}"));
                ui.horizontal(|ui| {
                    for (w, text) in [(1.0f32, lo), (5.0, hi)] {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(26.0, 12.0), egui::Sense::hover());
                        ui.painter().line_segment([r.left_center(), r.right_center()], egui::Stroke::new(w, ink));
                        ui.label(text.as_str());
                    }
                });
                ui.add_space(4.0);
            }
            if !st.dash_legend.is_empty() {
                ui.label(format!("Line style: {}", st.dash_title));
                for (name, dash, count) in st.dash_legend.iter().take(40) {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(34.0, 12.0), egui::Sense::hover());
                        let (d, g, _) = DASHES[*dash as usize % DASHES.len()];
                        let stroke = egui::Stroke::new(2.0, ink);
                        if *dash == 0 {
                            ui.painter().line_segment([r.left_center(), r.right_center()], stroke);
                        } else {
                            ui.painter().extend(egui::Shape::dashed_line(&[r.left_center(), r.right_center()], stroke, d * 2.0, g * 2.0));
                        }
                        ui.label(format!("{name}  ({count})"));
                    });
                }
                if st.dash_legend.len() > 40 {
                    ui.weak(format!("… {} more", st.dash_legend.len() - 40));
                }
            }
        });
    }

    /// One edge: its two ends, its direction and every edge attribute. With `links`, the ends can be clicked.
    pub fn edge_details(&mut self, ui: &mut Ui, e: usize, links: bool) {
        let Some(&(a, b)) = self.s.g.edges.get(e) else { return };
        let (a, b) = (a as usize, b as usize);
        let arrow = if self.s.g.edge_directed(e) { "→" } else { "—" };
        ui.label(RichText::new(format!("{} {arrow} {}", self.s.label_of(a), self.s.label_of(b))).strong());
        let mut go = None;
        egui::Grid::new(("edge_attrs", links)).num_columns(2).striped(links).show(ui, |ui| {
            for (name, i) in [("source", a), ("target", b)] {
                ui.weak(name);
                if links {
                    if ui.link(&self.s.g.ids[i]).clicked() {
                        go = Some(i);
                    }
                } else {
                    ui.label(&self.s.g.ids[i]);
                }
                ui.end_row();
            }
            for c in &self.s.g.edge_cols {
                ui.weak(&c.name);
                ui.label(c.label_at(e).unwrap_or_else(|| "—".into()));
                ui.end_row();
            }
        });
        if !self.s.edge_passes(e) {
            ui.weak("Taken out by an edge rule (Filter).");
        }
        if let Some(i) = go {
            self.select_and_focus(i);
        }
    }
}

/// One legend: a swatch per category (a dot for nodes, a short line for edges), or the colour scale.
fn legend_body(ui: &mut Ui, legend: &Legend, edge_title: Option<&str>) {
    match legend {
        Legend::None => {
            ui.weak("Single colour");
        }
        Legend::Cat(items) => {
            if let Some(t) = edge_title {
                ui.label(t);
            }
            for (name, c, count) in items.iter().take(80) {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    let col = egui::Color32::from_rgb(c[0], c[1], c[2]);
                    if edge_title.is_some() {
                        ui.painter().line_segment([r.left_center(), r.right_center()], egui::Stroke::new(3.0, col));
                    } else {
                        ui.painter().circle_filled(r.center(), 5.5, col);
                    }
                    ui.label(format!("{name}  ({count})"));
                });
            }
            if items.len() > 80 {
                ui.weak(format!("… {} more categories", items.len() - 80));
            }
        }
        Legend::Continuous { name, lo, hi, palette, log, invert } => {
            ui.label(edge_title.unwrap_or(name));
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

// ───────────────────────── subgraph ─────────────────────────

impl App {
    fn section_subgraph(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        if self.s.g.n() == 0 {
            ui.weak("Load a graph first.");
            return;
        }
        // three parts, always in this order: what to show, what to contract away, what to highlight
        egui::CollapsingHeader::new(theme::semibold("Show graph")).id_salt("sg_show").default_open(true).show(ui, |ui| self.sub_section(ui, cols));
        egui::CollapsingHeader::new(theme::semibold("Contract nodes")).id_salt("sg_contract").default_open(true).show(ui, |ui| self.contract_section(ui, cols));
        egui::CollapsingHeader::new(theme::semibold("Highlight changes")).id_salt("sg_highlight").default_open(true).show(ui, |ui| self.change_section(ui, cols));
    }

    fn sub_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        ui.weak("Keep only the nodes that match, lay them out again and leave the rest out. Switch it off to get the full graph and its layout back.");
        ui.add_space(4.0);
        let mut dirty = false;
        dirty |= ui.switch(&mut self.s.sub.enabled, "Show only matching nodes").changed();
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
                theme::inner_card(ui).show(ui, |ui| {
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

    fn contract_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        ui.weak("Take out the nodes with the attribute values you pick and rebuild the graph around them, as in graph contraction: nodes that were linked through a removed node get a direct dashed link, so what was connected stays connected.");
        ui.add_space(4.0);
        let mut dirty = false;
        dirty |= ui.switch(&mut self.s.contract.enabled, "Contract nodes").changed();
        ui.switch(&mut self.s.contract.show_links, "Show the added dashed links").on_hover_text("Hide them to see only the nodes that remain and the connections they already had.");
        let mut cur = self.s.contract.rule.as_ref().map(|r| r.col.clone());
        row(ui, "Remove by", |ui| {
            if combo_col(ui, "contract_col", &mut cur, cols, false, "(choose an attribute)") {
                self.s.contract.rule = cur.as_ref().and_then(|c| self.s.g.node_col(c)).map(|col| {
                    let mut r = Rule::new_for(col);
                    if let RuleKind::Cats { allowed } = &mut r.kind {
                        allowed.iter_mut().for_each(|a| *a = false); // tick the values to remove
                    }
                    r
                });
                if self.s.contract.rule.is_some() {
                    self.s.contract.enabled = true;
                }
                dirty = true;
            }
        });
        if let Some(rule) = &mut self.s.contract.rule {
            if let Some(col) = self.s.g.node_cols.iter().find(|c| c.name == rule.col) {
                ui.label(theme::secondary(if matches!(rule.kind, RuleKind::Cats { .. }) { "Tick the values to remove" } else { "Remove nodes with a value in this range" }));
                theme::inner_card(ui).show(ui, |ui| {
                    dirty |= rule_body(ui, &mut rule.kind, col, 2000);
                });
            }
        }
        if dirty {
            self.s.apply_subgraph();
            self.table.dirty = true;
        }
        if let Some(ci) = self.s.contract_info.clone() {
            ui.add_space(4.0);
            let n = self.s.g.n();
            if ci.n_removed == 0 {
                ui.weak("Nothing is removed yet. Tick the values to remove above.");
            } else if ci.n_removed >= n {
                ui.weak("That would remove every node, so nothing is contracted. Narrow the choice.");
            } else {
                ui.label(format!("{} nodes removed, {} remain", ci.n_removed, n - ci.n_removed));
                ui.label(crate::theme::secondary(format!(
                    "{} direct {} added where removed nodes used to connect",
                    ci.bridges.len(),
                    if ci.bridges.len() == 1 { "link" } else { "links" }
                )));
            }
        }
    }

    fn change_section(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
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

/// A number for a short label: no needless decimals.
fn fmt_num(v: f64) -> String {
    if v.abs() >= 1000.0 { format!("{v:.0}") } else if v.abs() >= 10.0 { format!("{v:.1}") } else { format!("{v:.2}") }
}

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
                    theme::inner_card(ui).show(ui, |ui| {
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
            ui.add_enabled_ui(self.s.boxes.show_nodes, |ui| {
                ui.switch(&mut self.s.boxes.fit_shapes, "Shapes fill their boxes").on_hover_text(
                    "Each node with a box is drawn as its 3D shape (Appearance ▸ Node shape; a sphere without one) stretched to fill the box. Lower the fill opacity above to see the shapes clearly.",
                );
            });

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
                if info.n_far > 0 {
                    if info.n_far * 2 >= info.n_boxes {
                        ui.label(crate::theme::secondary("Most boxes lie away from the nodes. Check that the box columns use the same units and origin as the X, Y, Z columns:"));
                        let g = &self.s.g;
                        let range = |names: [&Option<String>; 2]| -> Option<(f64, f64)> {
                            let lo = g.node_col(names[0].as_deref()?)?.num_range()?.0;
                            let hi = g.node_col(names[1].as_deref()?)?.num_range()?.1;
                            Some((lo, hi))
                        };
                        let node_range = |c: &Option<String>| c.as_deref().and_then(|c| g.node_col(c)).and_then(|c| c.num_range());
                        for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                            let coords = [&self.s.coords.x, &self.s.coords.y, &self.s.coords.z][k];
                            if let (Some(n), Some(b)) = (node_range(coords), range([&self.s.boxes.min[k], &self.s.boxes.max[k]])) {
                                ui.label(crate::theme::secondary(format!("{axis}: nodes {} to {}, boxes {} to {}", fmt_num(n.0), fmt_num(n.1), fmt_num(b.0), fmt_num(b.1))));
                            }
                        }
                    } else {
                        let some = plural(info.n_far, "box lies", "boxes lie");
                        ui.label(crate::theme::secondary(format!("{some} far outside the nodes (check their values); the camera ignores them when framing")));
                    }
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
                    for (label, v) in [("Top", View3::Top), ("Front", View3::Front), ("Back", View3::Back), ("Left", View3::Left), ("Right", View3::Right), ("Iso", View3::Iso)] {
                        if ui.small_button(label).clicked() {
                            self.run_tool(crate::view::Tool::Preset(v));
                        }
                    }
                });
                ui.weak("Drag to orbit, middle- or right-drag (or Space + drag) to pan, scroll to zoom, double-click empty space or press F to fit. The buttons at the bottom of the graph zoom to the selection or a region.");
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

// ───────────────────────── connection query ─────────────────────────

impl App {
    fn section_query(&mut self, ui: &mut Ui, cols: &[ColInfo]) {
        use crate::query::{Dir, QueryMode};
        if self.s.g.n() == 0 {
            ui.weak("Load a graph first.");
            return;
        }
        let ctx = ui.ctx().clone();
        crate::theme::heading(ui, "Connection query");
        ui.weak("How many nodes with one attribute connect to nodes with another? Build the two conditions; click a result to select those nodes on the canvas.");
        ui.add_space(4.0);
        let mut dirty = ui.segmented(&mut self.s.query.mode, &[(QueryMode::Count, "Count"), (QueryMode::CrossTab, "Cross-tab")]);
        ui.add_space(4.0);

        match self.s.query.mode {
            QueryMode::Count => {
                ui.label("Nodes where");
                dirty |= cond_editor(ui, "qa", &mut self.s.query.a, &self.s.g, cols, "(any node)");
                ui.add_space(2.0);
                ui.label("connect to nodes where");
                dirty |= cond_editor(ui, "qb", &mut self.s.query.b, &self.s.g, cols, "(any other node)");
                ui.add_space(2.0);
                ui.label("Direction");
                dirty |= ui.segmented(&mut self.s.query.dir, &[(Dir::Either, "Either way"), (Dir::Out, "First → second"), (Dir::In, "Second → first")]);
            }
            QueryMode::CrossTab => {
                row(ui, "Rows", |ui| dirty |= combo_col(ui, "xt_rows", &mut self.s.query.rows, cols, false, "(choose an attribute)"));
                row(ui, "Columns", |ui| dirty |= combo_col(ui, "xt_cols", &mut self.s.query.cols, cols, false, "(choose an attribute)"));
                ui.label("Direction");
                dirty |= ui.segmented(&mut self.s.query.dir, &[(Dir::Either, "Either way"), (Dir::Out, "Row → column"), (Dir::In, "Column → row")]);
            }
        }
        if self.s.g.directed {
            ui.weak("A directed connection is outgoing for its source and incoming for its target; an undirected one counts both ways.");
        }
        dirty |= ui.switch(&mut self.s.query.visible_only, "Only visible nodes").on_hover_text("Leave out nodes hidden by the filter, a subgraph or Hide.").changed();
        if dirty {
            self.s.recompute_query();
        }
        ui.add_space(6.0);

        if let Some(e) = self.s.query_error.clone() {
            ui.colored_label(egui::Color32::from_rgb(235, 100, 90), e);
            return;
        }
        if !self.s.query.is_set() {
            ui.weak(match self.s.query.mode {
                QueryMode::Count => "Choose an attribute for at least one of the conditions.",
                QueryMode::CrossTab => "Choose the two attributes to compare.",
            });
            return;
        }
        let mut select: Option<(Vec<usize>, String)> = None;
        if let Some(info) = self.s.query_info.clone() {
            let (first, second) = crate::query::sentences(&self.s.g, &self.s.query, &info);
            theme::inner_card(ui).show(ui, |ui| {
                let result_link = |ui: &mut Ui, text: String, n: usize| -> bool {
                    let r = ui
                        .add(egui::Label::new(RichText::new(text).color(ui.visuals().hyperlink_color)).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(format!("Click to select these {n} nodes"));
                    r.clicked() && n > 0
                };
                if result_link(ui, first, info.n_a_conn) {
                    select = Some((info.connected(), "connected nodes".into()));
                }
                if let Some(second) = second {
                    if result_link(ui, second, info.n_a_none()) {
                        select = Some((info.unconnected(), "nodes without a connection".into()));
                    }
                }
            });
            if info.n_a > 0 && info.n_b > 0 {
                ui.add_space(4.0);
                let pct = 100.0 * info.n_a_conn as f64 / info.n_a as f64;
                egui::Grid::new("query_counts").num_columns(2).show(ui, |ui| {
                    for (k, v) in [
                        ("First group", info.n_a.to_string()),
                        ("  with a connection", format!("{} ({pct:.0}%)", info.n_a_conn)),
                        ("  without", info.n_a_none().to_string()),
                        ("Second group", info.n_b.to_string()),
                        ("  reached", info.n_b_reached.to_string()),
                        ("Connections between", info.n_edges.to_string()),
                    ] {
                        ui.label(k);
                        ui.label(RichText::new(v).monospace());
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(info.n_a_conn > 0, egui::Button::new("Select connected")).clicked() {
                        select = Some((info.connected(), "connected nodes".into()));
                    }
                    if ui.add_enabled(info.n_a_none() > 0, egui::Button::new("Select without")).clicked() {
                        select = Some((info.unconnected(), "nodes without a connection".into()));
                    }
                    if ui.add_enabled(info.n_b_reached > 0, egui::Button::new("Select reached")).on_hover_text("The nodes of the second group that are connected to the first").clicked() {
                        select = Some((info.reached(), "nodes reached".into()));
                    }
                });
            }
        } else if let Some(x) = self.s.xtab.clone() {
            ui.label(RichText::new(format!("{} connections counted", x.n_edges)).strong());
            ui.weak("Each cell is the number of connections between a row value and a column value. Click a cell to select the nodes at both ends.");
            let max = x.max_count().max(1) as f32;
            let base = ui.visuals().selection.bg_fill;
            let mut clicked: Option<(usize, usize)> = None;
            egui::ScrollArea::both().id_salt("xtab_scroll").max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("xtab_grid").spacing([4.0, 4.0]).show(ui, |ui| {
                    ui.label(theme::secondary(format!("{} ↓  {} →", x.rows_name, x.cols_name)));
                    for c in &x.cols {
                        ui.label(RichText::new(short(c, 12)).strong()).on_hover_text(c);
                    }
                    ui.end_row();
                    for (r, rl) in x.rows.iter().enumerate() {
                        ui.label(RichText::new(short(rl, 16)).strong()).on_hover_text(rl);
                        for c in 0..x.cols.len() {
                            let n = x.count(r, c);
                            if n == 0 {
                                ui.weak("·");
                                continue;
                            }
                            let k = n as f32 / max;
                            let fill = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (40.0 + 190.0 * k) as u8);
                            let b = ui.add(egui::Button::new(RichText::new(n.to_string()).monospace()).fill(fill).min_size(egui::vec2(40.0, 22.0)));
                            if b.on_hover_text(format!("{} = {}  ×  {} = {}: {n} connections", x.rows_name, rl, x.cols_name, x.cols[c])).clicked() {
                                clicked = Some((r, c));
                            }
                        }
                        ui.end_row();
                    }
                });
            });
            if let Some((r, c)) = clicked {
                select = Some((x.cell_nodes(&self.s.g, r, c), format!("nodes in {} × {}", x.rows[r], x.cols[c])));
            }
        }
        if let Some((nodes, what)) = select {
            let n = nodes.len();
            self.s.select_nodes(nodes);
            self.toast(&ctx, format!("Selected {n} {what}"));
        }
    }
}

/// One condition of the query: attribute, operator and value.
fn cond_editor(ui: &mut Ui, id: &str, cond: &mut crate::query::Cond, g: &crate::graph::Graph, cols: &[ColInfo], any_label: &str) -> bool {
    use crate::query::{ID_COLUMN, Op};
    let mut changed = false;
    // attribute (the node id is offered as a pseudo-attribute)
    ui.horizontal(|ui| {
        ui.spacing_mut().combo_width = (ui.available_width() - 12.0).max(90.0);
        let shown = match cond.col.as_deref() {
            None => any_label.to_string(),
            Some(c) if c == ID_COLUMN && g.node_col(c).is_none() => "(node id)".into(),
            Some(c) => c.to_string(),
        };
        ComboBox::from_id_salt((id, "col")).selected_text(shown).width(ui.spacing().combo_width).show_ui(ui, |ui| {
            if ui.selectable_label(cond.col.is_none(), any_label).clicked() {
                cond.col = None;
                changed = true;
            }
            if g.node_col(ID_COLUMN).is_none() && ui.selectable_label(cond.col.as_deref() == Some(ID_COLUMN), "(node id)").clicked() {
                cond.col = Some(ID_COLUMN.into());
                cond.value.clear();
                changed = true;
            }
            for c in cols {
                let label = if c.metric { format!("{}  (computed)", c.name) } else { c.name.clone() };
                if ui.selectable_label(cond.col.as_deref() == Some(c.name.as_str()), label).clicked() {
                    cond.col = Some(c.name.clone());
                    cond.value.clear();
                    changed = true;
                }
            }
        });
    });
    let Some(col_name) = cond.col.clone() else { return changed };
    ui.horizontal(|ui| {
        ComboBox::from_id_salt((id, "op")).selected_text(cond.op.label()).width(96.0).show_ui(ui, |ui| {
            for op in Op::ALL {
                changed |= ui.selectable_value(&mut cond.op, op, op.label()).changed();
            }
        });
        if !cond.op.needs_value() {
            return;
        }
        // a list of the existing values for text attributes with "is" / "is not"; a text box otherwise
        let col = g.node_col(&col_name);
        let categorical = col.is_some_and(|c| !c.is_numeric() || c.hint_cat);
        if categorical && matches!(cond.op, Op::Is | Op::IsNot) {
            let labels = col.map(|c| c.as_categories().1).unwrap_or_default();
            if labels.len() <= 400 {
                let text = if cond.value.is_empty() { "(choose a value)".to_string() } else { cond.value.clone() };
                ComboBox::from_id_salt((id, "val")).selected_text(text).width((ui.available_width() - 8.0).max(80.0)).show_ui(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for l in &labels {
                            if ui.selectable_label(cond.value == *l, l).clicked() {
                                cond.value = l.clone();
                                changed = true;
                            }
                        }
                    });
                });
                return;
            }
        }
        let hint = if col.is_some_and(|c| c.is_numeric()) { "number" } else { "text" };
        changed |= ui.add(egui::TextEdit::singleline(&mut cond.value).id_salt((id, "txt")).hint_text(hint).desired_width(f32::INFINITY)).changed();
    });
    changed
}

/// A label cut to `n` characters, with an ellipsis if it was longer.
fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

// ───────────────────────── section box ─────────────────────────

impl App {
    pub fn section_panel(&mut self, ui: &mut Ui) {
        if self.s.g.n() == 0 {
            ui.weak("Load a graph first.");
            return;
        }
        let dims = self.s.section_dims();
        crate::theme::heading(ui, "Section box");
        ui.weak("Cut the view with a box. Nodes outside it are dimmed or hidden. Every side is a section line (a plane in 3D) that can be switched on or off by itself: one line cuts the graph in two, two make a band, all of them a box.");
        ui.add_space(4.0);
        let mut on = self.s.section.enabled;
        if ui.switch(&mut on, "Section box").changed() {
            self.set_section(on);
        }
        let enabled = self.s.section.enabled;
        ui.add_enabled_ui(enabled, |ui| {
            ui.label(theme::secondary("Outside the section"));
            ui.segmented(&mut self.s.section.outside, &[(crate::section::Outside::Dim, "Dim"), (crate::section::Outside::Hide, "Hide")]);
            ui.switch(&mut self.s.section.show_frame, "Show the box and lines in the view");
            ui.switch(&mut self.s.section.invert, "Keep what is outside instead");
            ui.add_space(2.0);
            let names = ["X", "Y", "Z"];
            for k in 0..dims {
                let (lo, hi) = (self.s.section.range_lo[k], self.s.section.range_hi[k]);
                let (lo, hi) = if hi > lo { (lo.min(self.s.section.min[k]), hi.max(self.s.section.max[k])) } else { (self.s.section.min[k] - 1.0, self.s.section.max[k] + 1.0) };
                theme::inner_card(ui).show(ui, |ui| {
                    ui.label(theme::semibold(format!("{} lines", names[k])));
                    for (side, label) in [(0usize, "from"), (1, "to")] {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.s.section.side_on[k][side], "").on_hover_text("Switch this section line on or off");
                            ui.label(label);
                            let on_line = self.s.section.side_on[k][side];
                            let sec = &mut self.s.section;
                            let value = if side == 0 { &mut sec.min[k] } else { &mut sec.max[k] };
                            ui.spacing_mut().slider_width = (ui.available_width() - 80.0).max(60.0);
                            let changed = ui.add_enabled(on_line, egui::Slider::new(value, lo..=hi).clamping(egui::SliderClamping::Never)).changed();
                            if changed {
                                sec.placed = true;
                                sec.normalise();
                            }
                        });
                    }
                });
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Fit to visible nodes").clicked() {
                    self.s.fit_section(false);
                }
                if ui.add_enabled(self.s.sel_count > 0, egui::Button::new("From selection")).clicked() {
                    self.s.fit_section(true);
                }
                if ui.button("All lines on").clicked() {
                    self.s.section.side_on = [[true; 2]; 3];
                }
            });
            if self.s.section_active() {
                let (inside, total) = match self.s.section_outside() {
                    Some(o) => (o.iter().zip(&self.s.vis).filter(|(out, vis)| !**out && **vis).count(), self.s.n_vis),
                    None => (self.s.n_vis, self.s.n_vis),
                };
                ui.label(theme::secondary(format!("{inside} of {total} visible nodes are inside the section")));
            } else if enabled && !self.s.section.any_line(dims) {
                ui.label(theme::secondary("All section lines are off, so nothing is cut."));
            }
            if self.s.is_3d() {
                ui.weak("Drag the green arrows on the sides of the box to push them into the graph or pull them out.");
            } else {
                ui.weak("Drag a dashed line or its arrow in the view to move it.");
            }
        });
    }
}
