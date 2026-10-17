//! The session: loaded data, graph, layout state, appearance, filters, selection.
//! Free of any GUI toolkit so it can also drive the headless exporter and the tests.

use crate::changes::{self, ChangeInfo, ChangeSettings};
use crate::coords::{self, Cam3, CoordSettings, Orbit};
use crate::data::*;
use crate::expr;
use crate::graph::{Graph, build_graph};
use crate::layout::*;
use crate::style::*;
use crate::subgraph::{self, SubInfo, SubSettings};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use egui::Vec2 as V2;
use std::collections::HashMap;
use std::path::Path;

// ───────────────────────────── appearance ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EdgeColorMode {
    Uniform,
    Source,
    Blend,
    Attribute,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LabelMode {
    Auto,
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FilteredMode {
    Hide,
    Dim,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub color_by: Option<String>,
    /// Treat a numeric column as categories (colour per distinct value).
    pub color_as_cat: bool,
    pub palette: Palette,
    pub invert: bool,
    pub log_scale: bool,
    /// Clip the colour scale to the 2nd–98th percentile.
    pub robust: bool,
    pub node_color: Rgb,
    pub size_by: Option<String>,
    pub node_size: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub edge_mode: EdgeColorMode,
    pub edge_by: Option<String>,
    pub edge_alpha: f32,
    pub edge_width: f32,
    pub arrows: bool,
    pub labels: LabelMode,
    pub label_by: Option<String>,
    pub highlight_hood: bool,
    pub filtered: FilteredMode,
    pub dark: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            color_by: Some("_community".into()),
            color_as_cat: false,
            palette: Palette::Viridis,
            invert: false,
            log_scale: false,
            robust: false,
            node_color: [86, 156, 214],
            size_by: None,
            node_size: 4.0,
            size_min: 0.6,
            size_max: 3.5,
            edge_mode: EdgeColorMode::Uniform,
            edge_by: None,
            edge_alpha: 0.35,
            edge_width: 1.0,
            arrows: true,
            labels: LabelMode::Auto,
            label_by: None,
            highlight_hood: true,
            filtered: FilteredMode::Hide,
            dark: true,
        }
    }
}

impl Look {
    pub fn background(&self) -> Rgb {
        if self.dark { [20, 22, 28] } else { [250, 250, 252] }
    }
    pub fn edge_base(&self) -> Rgb {
        if self.dark { [170, 180, 195] } else { [90, 96, 108] }
    }
    pub fn text_color(&self) -> Rgb {
        if self.dark { [225, 228, 235] } else { [30, 32, 40] }
    }
}

#[derive(Clone, Debug)]
pub enum Legend {
    None,
    Cat(Vec<(String, Rgb, usize)>),
    Continuous { name: String, lo: f64, hi: f64, palette: Palette, log: bool, invert: bool },
}

pub struct StyleCache {
    pub node_rgb: Vec<Rgb>,
    pub node_r: Vec<f32>,
    pub edge_rgb: Option<Vec<Rgb>>,
    pub legend: Legend,
}

impl Default for StyleCache {
    fn default() -> Self {
        StyleCache { node_rgb: vec![], node_r: vec![], edge_rgb: None, legend: Legend::None }
    }
}

fn scale_range(col: &Column, robust: bool) -> Option<(f64, f64)> {
    if robust {
        let (lo, hi) = (col.percentile(0.02)?, col.percentile(0.98)?);
        if hi > lo {
            return Some((lo, hi));
        }
    }
    col.num_range()
}

/// Map one column to colours (works for node and edge columns alike).
fn color_column(col: &Column, as_cat: bool, look: &Look) -> (Vec<Rgb>, Legend) {
    let n = col.len();
    if !col.is_numeric() || as_cat || col.hint_cat {
        let (codes, labels) = col.as_categories();
        let mut counts = vec![0usize; labels.len()];
        let colors: Vec<Rgb> = codes
            .iter()
            .map(|&c| {
                if c == NULL_CODE {
                    MISSING
                } else {
                    counts[c as usize] += 1;
                    cat_color(c as usize)
                }
            })
            .collect();
        let legend = Legend::Cat(labels.iter().enumerate().map(|(i, l)| (l.clone(), cat_color(i), counts[i])).collect());
        (colors, legend)
    } else {
        let Some((lo, hi)) = scale_range(col, look.robust) else {
            return (vec![MISSING; n], Legend::None);
        };
        let colors = (0..n)
            .map(|i| match col.num(i) {
                None => MISSING,
                Some(v) => {
                    let mut t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.5 };
                    if look.log_scale {
                        t = log_curve(t);
                    }
                    if look.invert {
                        t = 1.0 - t;
                    }
                    look.palette.color(t)
                }
            })
            .collect();
        (colors, Legend::Continuous { name: col.name.clone(), lo, hi, palette: look.palette, log: look.log_scale, invert: look.invert })
    }
}

pub fn compute_style(g: &Graph, look: &Look) -> StyleCache {
    let n = g.n();
    let (node_rgb, legend) = match look.color_by.as_deref().and_then(|c| g.node_col(c)) {
        Some(col) => color_column(col, look.color_as_cat, look),
        None => (vec![look.node_color; n], Legend::None),
    };
    let node_r = match look.size_by.as_deref().and_then(|c| g.node_col(c)).filter(|c| c.is_numeric() || true) {
        Some(col) => {
            let (codes, nl) = if col.is_numeric() { (vec![], 0) } else { let (c, l) = col.as_categories(); (c, l.len()) };
            let range = if col.is_numeric() { scale_range(col, true) } else { None };
            (0..n)
                .map(|i| {
                    let t = if col.is_numeric() {
                        match (col.num(i), range) {
                            (Some(v), Some((lo, hi))) if hi > lo => (((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32).sqrt(),
                            (Some(_), _) => 0.5,
                            _ => 0.0,
                        }
                    } else if codes[i] == NULL_CODE || nl <= 1 {
                        0.0
                    } else {
                        codes[i] as f32 / (nl - 1) as f32
                    };
                    look.node_size * (look.size_min + (look.size_max - look.size_min) * t)
                })
                .collect()
        }
        None => vec![look.node_size; n],
    };
    let edge_rgb = match look.edge_mode {
        EdgeColorMode::Uniform => None,
        EdgeColorMode::Source => Some(g.edges.iter().map(|&(s, _)| node_rgb[s as usize]).collect()),
        EdgeColorMode::Blend => Some(
            g.edges
                .iter()
                .map(|&(s, t)| {
                    let (a, b) = (node_rgb[s as usize], node_rgb[t as usize]);
                    [((a[0] as u16 + b[0] as u16) / 2) as u8, ((a[1] as u16 + b[1] as u16) / 2) as u8, ((a[2] as u16 + b[2] as u16) / 2) as u8]
                })
                .collect(),
        ),
        EdgeColorMode::Attribute => look.edge_by.as_deref().and_then(|c| g.edge_col(c)).map(|c| color_column(c, false, look).0),
    };
    StyleCache { node_rgb, node_r, edge_rgb, legend }
}

// ───────────────────────────── filters ─────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuleKind {
    Range { lo: f64, hi: f64, keep_missing: bool },
    /// One flag per category label plus a final flag for "(empty)".
    Cats { allowed: Vec<bool> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub enabled: bool,
    pub col: String,
    pub kind: RuleKind,
}

impl Rule {
    pub fn new_for(col: &Column) -> Rule {
        let kind = if col.is_numeric() && !col.hint_cat {
            let (lo, hi) = col.num_range().unwrap_or((0.0, 1.0));
            RuleKind::Range { lo, hi, keep_missing: true }
        } else {
            RuleKind::Cats { allowed: vec![true; col.as_categories().1.len() + 1] }
        };
        Rule { enabled: true, col: col.name.clone(), kind }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterState {
    pub rules: Vec<Rule>,
    pub expr_text: String,
    pub search: String,
}

impl FilterState {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.expr_text.trim().is_empty() && self.search.trim().is_empty()
    }
}

fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    hay.to_lowercase().contains(needle_lower)
}

// ───────────────────────────── camera ─────────────────────────────

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ViewCam {
    /// World point shown at the centre of the view.
    pub center: V2,
    pub zoom: f32,
}

impl Default for ViewCam {
    fn default() -> Self {
        ViewCam { center: V2::ZERO, zoom: 1.0 }
    }
}

impl ViewCam {
    #[inline]
    pub fn to_screen(&self, p: V2, size: V2) -> V2 {
        (p - self.center) * self.zoom + size * 0.5
    }
    #[inline]
    pub fn to_world(&self, s: V2, size: V2) -> V2 {
        (s - size * 0.5) / self.zoom + self.center
    }
}

/// Node radius in screen pixels. Nodes grow slower than the zoom so they stay readable.
#[inline]
pub fn node_radius_px(r_world: f32, zoom: f32) -> f32 {
    (r_world * zoom.powf(0.75)).clamp(1.0, 120.0)
}

// ───────────────────────────── session ─────────────────────────────

pub struct TableSrc<M> {
    pub name: String,
    /// File the table was read from (None for the built-in sample).
    pub path: Option<std::path::PathBuf>,
    pub table: RawTable,
    pub mapping: M,
}

struct Anim {
    from: Vec<V2>,
    to: Vec<V2>,
    t: f32,
}

pub struct Session {
    pub edge_src: Option<TableSrc<EdgeMapping>>,
    pub node_src: Option<TableSrc<NodeMapping>>,
    pub preset: Vec<(String, f32, f32)>,
    pub g: Graph,
    pub pos: Vec<V2>,
    pub layout: LayoutSettings,
    pub sim: ForceSim,
    pub look: Look,
    pub filter: FilterState,
    pub change: ChangeSettings,
    pub change_info: Option<ChangeInfo>,
    pub sub: SubSettings,
    pub sub_info: Option<SubInfo>,
    pub coords: CoordSettings,
    /// 3D positions (x right, y into the screen, z up); filled while node coordinates are in use.
    pub pos3: Vec<[f32; 3]>,
    pub cam3: Cam3,
    /// Nodes that have no value in a coordinate column.
    pub coords_missing: usize,
    coords_saved_pos: Option<Vec<V2>>,
    /// Node positions from before the sub-graph was shown, put back when it is switched off.
    sub_saved_pos: Option<Vec<V2>>,
    pub expr_error: Option<String>,
    pub vis: Vec<bool>,
    pub manual_hidden: Vec<bool>,
    pub n_vis: usize,
    pub n_vis_edges: usize,
    pub sel: Vec<bool>,
    pub sel_count: usize,
    pub sel_version: u64,
    pub style: StyleCache,
    pub fit_requested: bool,
    anim: Option<Anim>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            edge_src: None,
            node_src: None,
            preset: vec![],
            g: Graph::default(),
            pos: vec![],
            layout: LayoutSettings::default(),
            sim: ForceSim::new(),
            look: Look::default(),
            filter: FilterState::default(),
            change: ChangeSettings::default(),
            change_info: None,
            sub: SubSettings::default(),
            sub_info: None,
            coords: CoordSettings::default(),
            pos3: vec![],
            cam3: Cam3::default(),
            coords_missing: 0,
            coords_saved_pos: None,
            sub_saved_pos: None,
            expr_error: None,
            vis: vec![],
            manual_hidden: vec![],
            n_vis: 0,
            n_vis_edges: 0,
            sel: vec![],
            sel_count: 0,
            sel_version: 0,
            style: StyleCache::default(),
            fit_requested: false,
            anim: None,
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    // ── loading ──

    pub fn set_edges(&mut self, name: String, table: RawTable, mapping: EdgeMapping) {
        self.edge_src = Some(TableSrc { name, path: None, table, mapping });
    }

    pub fn set_nodes(&mut self, name: String, table: RawTable, mapping: NodeMapping) {
        self.node_src = Some(TableSrc { name, path: None, table, mapping });
    }

    pub fn load_json(&mut self, path: &Path) -> Result<()> {
        let j = read_json(path)?;
        let fname = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut em = auto_edge_mapping(&j.edges);
        em.directed = j.directed.unwrap_or(false);
        self.set_edges(fname.clone(), j.edges, em);
        if let Some(e) = &mut self.edge_src {
            e.path = Some(path.to_path_buf());
        }
        self.node_src = j.nodes.map(|t| {
            let m = auto_node_mapping(&t);
            TableSrc { name: fname.clone(), path: Some(path.to_path_buf()), table: t, mapping: m }
        });
        self.preset = j.positions;
        Ok(())
    }

    pub fn clear_all(&mut self) {
        self.edge_src = None;
        self.node_src = None;
        self.preset.clear();
        self.rebuild();
    }

    /// Rebuild the graph from the stored tables. Existing node positions are kept by id.
    pub fn rebuild(&mut self) {
        let old: HashMap<String, V2> =
            self.g.ids.iter().cloned().zip(self.pos.iter().copied()).collect();
        let g = build_graph(
            self.edge_src.as_ref().map(|s| (&s.table, &s.mapping)),
            self.node_src.as_ref().map(|s| (&s.table, &s.mapping)),
        );
        let n = g.n();
        let mut pos = initial_spiral(n);
        let mut known = 0usize;
        for (i, id) in g.ids.iter().enumerate() {
            if let Some(p) = old.get(id) {
                pos[i] = *p;
                known += 1;
            }
        }
        // positions shipped in the file (Cytoscape) win for a fresh graph
        let mut from_file = 0usize;
        if known * 2 < n {
            for (id, x, y) in &self.preset {
                if let Some(&i) = g.index.get(id) {
                    pos[i as usize] = V2::new(*x, *y);
                    from_file += 1;
                }
            }
        }
        let first_time = known * 2 < n;
        self.g = g;
        self.pos = pos;
        self.manual_hidden = vec![false; n];
        self.sel = vec![false; n];
        self.sel_count = 0;
        self.sel_version += 1;
        self.sim = ForceSim::new();
        self.anim = None;
        if let Some(e) = self.g.edges.len().checked_sub(0).filter(|&e| e > 0 && first_time) {
            self.look.edge_alpha = (1200.0 / e as f32).clamp(0.05, 0.45);
        }
        // drop settings that point at columns that no longer exist
        let has = |g: &Graph, c: &Option<String>| c.as_ref().map(|c| g.node_col(c).is_some()).unwrap_or(true);
        if !has(&self.g, &self.look.color_by) {
            self.look.color_by = None;
        }
        if first_time && self.look.color_by.is_none() {
            self.look.color_by = self.g.node_col("_community").map(|_| "_community".to_string());
        }
        if !has(&self.g, &self.look.size_by) {
            self.look.size_by = None;
        }
        if !has(&self.g, &self.look.label_by) {
            self.look.label_by = None;
        }
        if let Some(e) = &self.look.edge_by {
            if self.g.edge_col(e).is_none() {
                self.look.edge_by = None;
            }
        }
        for c in [&mut self.layout.order_by, &mut self.layout.ring_by, &mut self.layout.group_by, &mut self.layout.x_by, &mut self.layout.y_by] {
            if c.as_ref().is_some_and(|name| self.g.node_col(name).is_none()) {
                *c = None;
            }
        }
        self.filter.rules.retain(|r| self.g.node_col(&r.col).is_some());
        if self.change.col.as_ref().is_some_and(|c| self.g.node_col(c).is_none()) {
            self.change.col = None;
            self.change.pick = None;
        }
        if self.sub.rule.as_ref().is_some_and(|r| self.g.node_col(&r.col).is_none()) {
            self.sub.rule = None;
        }
        for c in [&mut self.coords.x, &mut self.coords.y, &mut self.coords.z] {
            if c.as_ref().is_some_and(|name| self.g.node_col(name).is_none()) {
                *c = None;
            }
        }
        if self.coords.x.is_none() || self.coords.y.is_none() {
            self.coords.enabled = false;
        }
        self.sub_saved_pos = None;
        self.coords_saved_pos = None;
        self.pos3.clear();
        self.recompute_subgraph();
        self.recompute_visibility();
        self.recompute_style();
        if self.coords.enabled {
            self.apply_coords();
            if self.coords_active() {
                return;
            }
        }
        if first_time {
            if from_file * 2 >= n && n > 0 {
                self.fit_requested = true; // keep the file's own positions
            } else {
                self.apply_layout();
            }
        } else {
            self.refresh_sim_if_force();
        }
    }

    // ── visibility ──

    pub fn recompute_visibility(&mut self) {
        let g = &self.g;
        let n = g.n();
        let mut vis: Vec<bool> = self.manual_hidden.iter().map(|h| !h).collect();
        vis.resize(n, true);
        if let Some(si) = &self.sub_info {
            if si.member.len() == n {
                for i in 0..n {
                    vis[i] &= si.member[i];
                }
            }
        }
        for r in self.filter.rules.iter().filter(|r| r.enabled) {
            subgraph::rule_pass(g, r, &mut vis);
        }
        match expr::compile(&self.filter.expr_text, g) {
            Ok(Some(c)) => {
                self.expr_error = None;
                for i in 0..n {
                    if vis[i] {
                        vis[i] = c.matches(g, i);
                    }
                }
            }
            Ok(None) => self.expr_error = None,
            Err(e) => self.expr_error = Some(e),
        }
        let q = self.filter.search.trim().to_lowercase();
        if !q.is_empty() {
            for i in 0..n {
                if !vis[i] {
                    continue;
                }
                let mut hit = contains_ci(&g.ids[i], &q);
                if !hit {
                    hit = g.node_cols.iter().any(|c| !c.is_metric && c.cat_str(i).is_some_and(|s| contains_ci(s, &q)));
                }
                vis[i] = hit;
            }
        }
        self.n_vis = vis.iter().filter(|v| **v).count();
        self.n_vis_edges = g.edges.iter().filter(|&&(s, t)| vis[s as usize] && vis[t as usize]).count();
        self.vis = vis;
        self.recompute_changes();
    }

    /// Re-find the connections where the chosen attribute changes (also after any filter change,
    /// because only visible connections are compared).
    pub fn recompute_changes(&mut self) {
        let info = if self.change.enabled {
            self.change
                .col
                .as_deref()
                .and_then(|c| self.g.node_col(c))
                .map(|col| changes::compute(&self.g, col, self.change.min_diff, &self.vis, &self.change.pick, self.change.only && self.change.connect))
        } else {
            None
        };
        self.change_info = info;
    }

    /// Work out which nodes belong to the sub-graph (from the settings; needs the whole graph, so it is
    /// cached and not redone when only the filter changes).
    pub fn recompute_subgraph(&mut self) {
        self.sub_info = match (&self.sub.rule, self.sub.enabled) {
            (Some(rule), true) if self.g.node_col(&rule.col).is_some() => {
                let info = subgraph::compute(&self.g, rule, self.sub.hops, self.sub.connect);
                // a rule nothing matches would leave an empty canvas: show the whole graph instead
                if info.n_match == 0 { Some(SubInfo { member: vec![true; self.g.n()], connector: vec![false; self.g.n()], ..info }) } else { Some(info) }
            }
            _ => None,
        };
    }

    /// True while a sub-graph is being shown (a rule that matches nothing does not count).
    pub fn sub_active(&self) -> bool {
        self.sub_info.as_ref().is_some_and(|i| i.n_match > 0)
    }

    /// Apply the sub-graph settings: recompute, lay the visible part out afresh, or bring the full layout back.
    pub fn apply_subgraph(&mut self) {
        if self.sub.enabled && self.sub_saved_pos.is_none() && self.sub.rule.is_some() {
            self.sub_saved_pos = Some(self.pos.clone());
        }
        self.recompute_subgraph();
        self.recompute_visibility();
        if self.sub_active() {
            self.apply_layout();
        } else if let Some(p) = self.sub_saved_pos.take() {
            if p.len() == self.pos.len() {
                self.pos = p;
            }
            self.anim = None;
            self.fit_requested = true;
            self.refresh_sim_if_force();
        }
    }

    // ── node coordinates ──

    /// True while nodes are placed by their coordinate columns (layouts and dragging are off).
    pub fn coords_active(&self) -> bool {
        self.coords.enabled && !self.g.ids.is_empty() && self.pos3.len() == self.g.n()
    }

    /// True while the 3D orbit view is shown.
    pub fn is_3d(&self) -> bool {
        self.coords_active() && self.coords.three_d && self.coords.z.is_some()
    }

    /// Place the nodes by the chosen columns, or hand the layout back when coordinates are switched off.
    pub fn apply_coords(&mut self) {
        if self.coords.enabled {
            if let Some(p) = coords::place(&self.g, &self.coords) {
                if self.coords_saved_pos.is_none() {
                    self.coords_saved_pos = Some(self.pos.clone());
                }
                self.pos = p.pos2;
                self.pos3 = p.pos3;
                self.coords_missing = p.missing;
                self.sim.running = false;
                self.anim = None;
                self.fit_requested = true;
                if self.is_3d() {
                    self.fit_cam3();
                }
                return;
            }
        }
        // off, or the columns are not usable: go back to a normal layout
        self.pos3.clear();
        self.coords_missing = 0;
        if let Some(old) = self.coords_saved_pos.take() {
            if old.len() == self.pos.len() {
                self.pos = old;
            }
            self.fit_requested = true;
            self.refresh_sim_if_force();
        } else if self.g.n() > 0 {
            self.apply_layout();
        }
    }

    /// Point the 3D camera at the visible nodes (the viewing angle stays as it is).
    pub fn fit_cam3(&mut self) {
        let pts: Vec<[f32; 3]> = (0..self.g.n()).filter(|&i| self.vis[i]).map(|i| self.pos3[i]).collect();
        let pts = if pts.is_empty() { self.pos3.clone() } else { pts };
        self.cam3.fit(&pts);
    }

    /// How points are turned into screen positions right now (2D camera, or the 3D orbit camera).
    pub fn projector(&self, cam: &ViewCam, size: V2) -> Projector {
        if self.is_3d() { Projector::Orbit(self.cam3.orbit(size)) } else { Projector::Flat { cam: *cam, size } }
    }

    pub fn recompute_style(&mut self) {
        self.style = compute_style(&self.g, &self.look);
    }

    // ── selection ──

    pub fn clear_selection(&mut self) {
        if self.sel_count > 0 {
            self.sel.iter_mut().for_each(|s| *s = false);
            self.sel_count = 0;
            self.sel_version += 1;
        }
    }

    pub fn set_selected(&mut self, i: usize, on: bool) {
        if self.sel[i] != on {
            self.sel[i] = on;
            if on { self.sel_count += 1 } else { self.sel_count -= 1 }
            self.sel_version += 1;
        }
    }

    pub fn select_only(&mut self, i: usize) {
        self.clear_selection();
        self.set_selected(i, true);
    }

    pub fn toggle_selected(&mut self, i: usize) {
        let on = !self.sel[i];
        self.set_selected(i, on);
    }

    pub fn selected_list(&self) -> Vec<u32> {
        (0..self.g.n() as u32).filter(|&i| self.sel[i as usize]).collect()
    }

    pub fn select_neighbors(&mut self) {
        let cur = self.selected_list();
        for i in cur {
            for &v in self.g.neighbors(i as usize).to_vec().iter() {
                self.set_selected(v as usize, true);
            }
        }
    }

    pub fn invert_selection(&mut self) {
        for i in 0..self.g.n() {
            if self.vis[i] {
                self.toggle_selected(i);
            }
        }
    }

    pub fn hide_selected(&mut self) {
        for i in 0..self.g.n() {
            if self.sel[i] {
                self.manual_hidden[i] = true;
            }
        }
        self.clear_selection();
        self.recompute_visibility();
    }

    pub fn isolate_selected(&mut self) {
        if self.sel_count == 0 {
            return;
        }
        for i in 0..self.g.n() {
            self.manual_hidden[i] = !self.sel[i];
        }
        self.recompute_visibility();
    }

    pub fn show_all_hidden(&mut self) {
        self.manual_hidden.iter_mut().for_each(|h| *h = false);
        self.recompute_visibility();
    }

    pub fn n_manual_hidden(&self) -> usize {
        self.manual_hidden.iter().filter(|h| **h).count()
    }

    // ── layout ──

    pub fn active_nodes(&self) -> Vec<u32> {
        if self.layout.visible_only || self.sub_active() {
            (0..self.g.n() as u32).filter(|&i| self.vis[i as usize]).collect()
        } else {
            (0..self.g.n() as u32).collect()
        }
    }

    fn anchors(&self, active: &[u32]) -> Option<Vec<V2>> {
        let name = self.layout.group_by.as_deref()?;
        if self.layout.force.group_pull > 0.0 { group_anchors(&self.g, name, active) } else { None }
    }

    pub fn setup_force(&mut self) {
        if self.coords_active() {
            return;
        }
        let active = self.active_nodes();
        let anchors = self.anchors(&active);
        self.sim.setup(&self.g, &active, &self.layout.force, anchors);
    }

    /// Re-run the force problem setup after parameters changed, and warm the simulation up.
    pub fn refresh_sim_if_force(&mut self) {
        if self.layout.kind == LayoutKind::Force && !self.coords_active() {
            self.setup_force();
            self.sim.reheat(0.4);
        }
    }

    /// Apply the chosen layout. Static layouts glide into place; the force layout starts running.
    pub fn apply_layout(&mut self) {
        self.apply_layout_inner(true);
    }

    /// Same, but jumps directly (used by the headless exporter).
    pub fn apply_layout_now(&mut self) {
        self.apply_layout_inner(false);
    }

    fn apply_layout_inner(&mut self, animate: bool) {
        self.fit_requested = true;
        if self.coords_active() {
            self.apply_coords(); // the nodes sit where their coordinates say
            return;
        }
        let active = self.active_nodes();
        if self.layout.kind == LayoutKind::Force {
            self.anim = None;
            self.setup_force();
            self.sim.reheat(1.0);
            return;
        }
        self.sim.running = false;
        let mut target = self.pos.clone();
        apply_static(&self.g, &active, &self.layout, &mut target);
        if animate && self.g.n() > 0 {
            self.anim = Some(Anim { from: self.pos.clone(), to: target, t: 0.0 });
        } else {
            self.pos = target;
            self.anim = None;
        }
    }

    /// Put the session back into a saved state: positions, pins, selection and hand-hidden nodes by id.
    pub fn restore_state(&mut self, positions: &[(String, f32, f32)], pinned: &[String], selected: &[String], hidden: &[String]) {
        let n = self.g.n();
        for (id, x, y) in positions {
            if let Some(&i) = self.g.index.get(id) {
                self.pos[i as usize] = V2::new(*x, *y);
            }
        }
        self.anim = None;
        self.sim.running = false;
        self.fit_requested = false;
        self.sim.pinned = vec![false; n];
        for id in pinned {
            if let Some(&i) = self.g.index.get(id) {
                self.sim.pinned[i as usize] = true;
            }
        }
        for id in hidden {
            if let Some(&i) = self.g.index.get(id) {
                self.manual_hidden[i as usize] = true;
            }
        }
        for id in selected {
            if let Some(&i) = self.g.index.get(id) {
                self.set_selected(i as usize, true);
            }
        }
    }

    /// Forget settings that refer to columns the (possibly changed) files no longer have.
    pub fn drop_missing_columns(&mut self) {
        let g = &self.g;
        let miss = |c: &Option<String>| c.as_ref().is_some_and(|c| g.node_col(c).is_none());
        if miss(&self.look.color_by) {
            self.look.color_by = None;
        }
        if miss(&self.look.size_by) {
            self.look.size_by = None;
        }
        if miss(&self.look.label_by) {
            self.look.label_by = None;
        }
        if self.look.edge_by.as_ref().is_some_and(|e| g.edge_col(e).is_none()) {
            self.look.edge_by = None;
        }
        for c in [&mut self.layout.order_by, &mut self.layout.ring_by, &mut self.layout.group_by, &mut self.layout.x_by, &mut self.layout.y_by] {
            if miss(c) {
                *c = None;
            }
        }
        self.filter.rules.retain(|r| g.node_col(&r.col).is_some());
        if self.change.col.as_ref().is_some_and(|c| g.node_col(c).is_none()) {
            self.change.col = None;
            self.change.pick = None;
        }
        if self.sub.rule.as_ref().is_some_and(|r| g.node_col(&r.col).is_none()) {
            self.sub.rule = None;
        }
    }

    pub fn is_animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Advance position glide. Returns true while still animating.
    pub fn tick_anim(&mut self, dt: f32) -> bool {
        let Some(a) = &mut self.anim else { return false };
        a.t = (a.t + dt / 0.55).min(1.0);
        let e = 1.0 - (1.0 - a.t).powi(3);
        for i in 0..self.pos.len() {
            self.pos[i] = a.from[i] + (a.to[i] - a.from[i]) * e;
        }
        if a.t >= 1.0 {
            self.anim = None;
            return false;
        }
        true
    }

    /// Run the force layout to completion (headless).
    pub fn settle(&mut self, max_ticks: usize) {
        let p = self.layout.force.clone();
        let mut left = max_ticks;
        while self.sim.running && left > 0 {
            let chunk = left.min(25);
            self.sim.step(&mut self.pos, &p, chunk);
            left -= chunk;
        }
    }

    pub fn bounds(&self, visible_only: bool) -> Option<(V2, V2)> {
        let (mut lo, mut hi) = (V2::splat(f32::INFINITY), V2::splat(f32::NEG_INFINITY));
        let mut any = false;
        for i in 0..self.g.n() {
            if visible_only && !self.vis[i] {
                continue;
            }
            lo = lo.min(self.pos[i]);
            hi = hi.max(self.pos[i]);
            any = true;
        }
        any.then_some((lo, hi))
    }

    pub fn fit_camera(&self, size: V2) -> ViewCam {
        let Some((mut lo, mut hi)) = self.bounds(true).or_else(|| self.bounds(false)) else { return ViewCam::default() };
        // the force layout can fling a few loose nodes far away: frame the dense core instead
        if self.layout.kind == LayoutKind::Force && self.n_vis >= 40 {
            let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = (0..self.g.n()).filter(|&i| self.vis[i]).map(|i| (self.pos[i].x, self.pos[i].y)).unzip();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = xs.len();
            let (a, b) = ((n as f32 * 0.015) as usize, ((n as f32 * 0.985) as usize).min(n - 1));
            let (core_lo, core_hi) = (V2::new(xs[a], ys[a]), V2::new(xs[b], ys[b]));
            let pad = (core_hi - core_lo) * 0.12;
            lo = lo.max(core_lo - pad);
            hi = hi.min(core_hi + pad);
        }
        let ext = (hi - lo).max(V2::splat(1.0));
        let margin = 0.9;
        let zoom = (size.x * margin / ext.x).min(size.y * margin / ext.y).clamp(0.001, 50.0);
        ViewCam { center: (lo + hi) * 0.5, zoom }
    }

    pub fn label_of(&self, i: usize) -> String {
        match self.look.label_by.as_deref().and_then(|c| self.g.node_col(c)).and_then(|c| c.label_at(i)) {
            Some(l) => l,
            None => self.g.ids[i].clone(),
        }
    }
}

// ───────────────────────────── projection ─────────────────────────────

pub enum Projector {
    Flat { cam: ViewCam, size: V2 },
    Orbit(Orbit),
}

impl Projector {
    /// Screen position, pixels per world unit, and depth (0 in 2D) of node `i`; None if it cannot be seen.
    #[inline]
    pub fn at(&self, s: &Session, i: usize) -> Option<(V2, f32, f32)> {
        match self {
            Projector::Flat { cam, size } => Some((cam.to_screen(s.pos[i], *size), cam.zoom, 0.0)),
            Projector::Orbit(o) => o.project(s.pos3[i]),
        }
    }
}

// ───────────────────────────── draw list ─────────────────────────────

/// Colour of highlighted attribute-change connections (Apple's system orange: stands out on both backgrounds).
pub const CHANGE_COLOR: Rgb = [255, 149, 10];

pub struct DrawNode {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub rgba: [u8; 4],
    pub selected: bool,
    /// Sits on a highlighted attribute change: drawn with an accent ring.
    pub ring: bool,
    /// Distance from the camera (3D only; 0 in 2D). Far nodes are drawn first.
    pub depth: f32,
    pub idx: u32,
}

pub struct DrawEdge {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub rgba: [u8; 4],
    pub target_r: f32,
    /// Gets an arrow head (if arrows are on).
    pub directed: bool,
}

#[derive(Default)]
pub struct DrawList {
    pub nodes: Vec<DrawNode>,
    /// Faded edges, drawn first.
    pub edges_back: Vec<DrawEdge>,
    pub edges: Vec<DrawEdge>,
    /// Highlighted attribute-change edges, thicker and drawn last.
    pub edges_hi: Vec<DrawEdge>,
}

/// Focus/neighbourhood masks used to fade everything but a node and its neighbours.
pub struct Highlight<'a> {
    pub focus: &'a [bool],
    pub hood: &'a [bool],
    pub active: bool,
}

pub fn build_draw_list(s: &Session, cam: &ViewCam, size: V2, hi: &Highlight, out: &mut DrawList) {
    out.nodes.clear();
    out.edges.clear();
    out.edges_back.clear();
    out.edges_hi.clear();
    let g = &s.g;
    let n = g.n();
    if n == 0 {
        return;
    }
    let dim_hidden = s.look.filtered == FilteredMode::Dim;
    let proj = s.projector(cam, size);
    let pr: Vec<Option<(V2, f32, f32)>> = (0..n).map(|i| proj.at(s, i)).collect();
    let radius: Vec<f32> = (0..n).map(|i| pr[i].map(|p| node_radius_px(s.style.node_r[i], p.1)).unwrap_or(0.0)).collect();
    // in 3D, things further away fade a little
    let threed = s.is_3d();
    let (mut dmin, mut dmax) = (f32::INFINITY, f32::NEG_INFINITY);
    if threed {
        for i in 0..n {
            if let (true, Some(p)) = (s.vis[i], pr[i]) {
                dmin = dmin.min(p.2);
                dmax = dmax.max(p.2);
            }
        }
    }
    let depth_k = |d: f32, strength: f32| if threed && dmax > dmin { 1.0 - strength * ((d - dmin) / (dmax - dmin)).clamp(0.0, 1.0) } else { 1.0 };

    // edges
    let base = s.look.edge_base();
    let ea = s.look.edge_alpha;
    let uniform = s.style.edge_rgb.is_none();
    for (e, &(a, b)) in g.edges.iter().enumerate() {
        let (ai, bi) = (a as usize, b as usize);
        if ai == bi {
            continue;
        }
        let both = s.vis[ai] && s.vis[bi];
        if !both && !dim_hidden {
            continue;
        }
        let (Some((pa, _, da)), Some((pb, _, db))) = (pr[ai], pr[bi]) else { continue };
        if (pa.x < 0.0 && pb.x < 0.0) || (pa.y < 0.0 && pb.y < 0.0) || (pa.x > size.x && pb.x > size.x) || (pa.y > size.y && pb.y > size.y) {
            continue;
        }
        let mut alpha = ea;
        let mut back = false;
        let mut top = false;
        if let (true, Some(ci)) = (both, &s.change_info) {
            if ci.edge_hi[e] {
                top = true;
            } else if s.change.only {
                if !ci.edge_path[e] {
                    continue;
                }
                alpha *= 0.8; // a joining path: ordinary edge colour, a little softer
            } else {
                alpha *= 0.1;
                back = true;
            }
        } else if !both {
            alpha *= 0.12;
            back = true;
        } else if hi.active && !(hi.focus[ai] || hi.focus[bi]) {
            alpha *= 0.12;
            back = true;
        } else if hi.active {
            alpha = (alpha * 2.5).clamp(0.5, 1.0);
        }
        if let Some(si) = &s.sub_info {
            if si.connector[ai] || si.connector[bi] {
                alpha *= 0.7;
            }
        }
        let c = if top { CHANGE_COLOR } else if uniform { base } else { s.style.edge_rgb.as_ref().unwrap()[e] };
        alpha *= depth_k((da + db) * 0.5, 0.5);
        if top {
            alpha = 0.95 * depth_k((da + db) * 0.5, 0.25);
        }
        let de = DrawEdge { a: [pa.x, pa.y], b: [pb.x, pb.y], rgba: [c[0], c[1], c[2], (alpha * 255.0) as u8], target_r: radius[bi], directed: g.edge_directed(e) };
        if top { out.edges_hi.push(de) } else if back { out.edges_back.push(de) } else { out.edges.push(de) }
    }

    // nodes (faded first, then normal)
    let mut faded: Vec<DrawNode> = Vec::new();
    for i in 0..n {
        let visible = s.vis[i];
        if !visible && !dim_hidden {
            continue;
        }
        let Some((p, _, depth)) = pr[i] else { continue };
        let r = radius[i];
        if p.x < -r || p.y < -r || p.x > size.x + r || p.y > size.y + r {
            continue;
        }
        let c = s.style.node_rgb[i];
        let mut alpha: f32 = 1.0;
        let mut ring = false;
        if let (true, Some(ci)) = (visible, &s.change_info) {
            if ci.node_hi[i] {
                ring = true;
            } else if s.change.only {
                if !ci.node_path[i] {
                    continue;
                }
                alpha = 0.6; // joins pieces together
            } else {
                alpha = 0.3;
            }
        } else if !visible {
            alpha = 0.12;
        } else if hi.active && !hi.hood[i] {
            alpha = 0.15;
        }
        if s.sub_info.as_ref().is_some_and(|si| si.connector[i]) {
            alpha = alpha.min(0.6);
        }
        alpha *= depth_k(depth, 0.35);
        let dn = DrawNode { x: p.x, y: p.y, r, rgba: [c[0], c[1], c[2], (alpha * 255.0) as u8], selected: s.sel[i], ring, depth, idx: i as u32 };
        if alpha < 0.995 { faded.push(dn) } else { out.nodes.push(dn) }
    }
    if threed {
        // painter's order: far to near
        let far_first = |a: &DrawNode, b: &DrawNode| b.depth.total_cmp(&a.depth);
        faded.sort_by(far_first);
        out.nodes.sort_by(far_first);
    }
    faded.append(&mut out.nodes);
    out.nodes = faded;
}

/// Index of the visible node under `p` (screen space). In 2D the top-most (last drawn) wins, in 3D the nearest.
pub fn pick_node(s: &Session, cam: &ViewCam, size: V2, p: V2) -> Option<usize> {
    let proj = s.projector(cam, size);
    let mut best: Option<(usize, f32, f32)> = None; // index, pixel distance, depth
    for i in 0..s.g.n() {
        if !s.vis[i] {
            continue;
        }
        let Some((sp, scale, depth)) = proj.at(s, i) else { continue };
        let r = node_radius_px(s.style.node_r[i], scale).max(4.0) + 2.0;
        let d = (sp - p).length();
        if d > r {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bd, bdepth)) => {
                if matches!(proj, Projector::Orbit(_)) { depth < bdepth - 1e-3 || (depth - bdepth).abs() <= 1e-3 && d <= bd } else { d <= bd }
            }
        };
        if better {
            best = Some((i, d, depth));
        }
    }
    best.map(|(i, _, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let e = parse_csv_text("source,target,weight\na,b,1\nb,c,2\nc,a,1\nc,d,5\nx,y,1\n").unwrap();
        let n = parse_csv_text("id,group,score\na,g1,1.5\nb,g1,2\nc,g2,\nd,g2,4\n").unwrap();
        let mut s = Session::new();
        s.set_edges("e".into(), e.clone(), auto_edge_mapping(&e));
        s.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
        s.layout.kind = LayoutKind::Circular;
        s.rebuild();
        s
    }

    #[test]
    fn rebuild_sets_everything_up() {
        let mut s = session();
        assert_eq!(s.g.n(), 6);
        assert_eq!(s.pos.len(), 6);
        assert_eq!(s.vis.len(), 6);
        assert_eq!(s.n_vis, 6);
        assert_eq!(s.style.node_rgb.len(), 6);
        // auto-picks the community metric for colour
        assert_eq!(s.look.color_by.as_deref(), Some("_community"));
        s.apply_layout_now();
        assert!(s.pos.iter().all(|p| p.x.is_finite()));
    }

    #[test]
    fn filters_combine_and_count() {
        let mut s = session();
        s.filter.rules.push(Rule { enabled: true, col: "group".into(), kind: RuleKind::Cats { allowed: vec![true, false, false] } });
        s.recompute_visibility();
        assert_eq!(s.n_vis, 2); // only a, b are g1
        assert_eq!(s.n_vis_edges, 1);
        s.filter.rules.clear();
        s.filter.expr_text = "score >= 2 and group == \"g2\"".into();
        s.recompute_visibility();
        assert_eq!(s.n_vis, 1); // d
        s.filter.expr_text = "score >=".into();
        s.recompute_visibility();
        assert!(s.expr_error.is_some());
        s.filter.expr_text.clear();
        s.filter.search = "G2".into();
        s.recompute_visibility();
        assert_eq!(s.n_vis, 2); // c, d
    }

    #[test]
    fn range_rule_keeps_or_drops_missing() {
        let mut s = session();
        s.filter.rules.push(Rule { enabled: true, col: "score".into(), kind: RuleKind::Range { lo: 1.0, hi: 3.0, keep_missing: false } });
        s.recompute_visibility();
        assert_eq!(s.n_vis, 2); // a, b
        if let RuleKind::Range { keep_missing, .. } = &mut s.filter.rules[0].kind {
            *keep_missing = true;
        }
        s.recompute_visibility();
        assert_eq!(s.n_vis, 2 + 3); // + c (missing), x, y (not in node file)
    }

    #[test]
    fn manual_hide_and_isolate() {
        let mut s = session();
        s.select_only(0);
        s.hide_selected();
        assert_eq!(s.n_vis, 5);
        s.show_all_hidden();
        s.select_only(1);
        s.set_selected(2, true);
        s.isolate_selected();
        assert_eq!(s.n_vis, 2);
    }

    #[test]
    fn reload_keeps_positions_by_id() {
        let mut s = session();
        s.apply_layout_now();
        let before = s.pos[s.g.index["a"] as usize];
        // add a node attribute file later: node set unchanged → positions retained
        let n = parse_csv_text("id,extra\na,1\nb,2\nc,3\nd,4\nx,1\ny,2\n").unwrap();
        s.set_nodes("n2".into(), n.clone(), auto_node_mapping(&n));
        s.rebuild();
        assert_eq!(s.pos[s.g.index["a"] as usize], before);
        assert!(s.g.node_col("extra").is_some());
    }

    #[test]
    fn draw_list_respects_filters_and_culling() {
        let mut s = session();
        s.apply_layout_now();
        let size = V2::new(800.0, 600.0);
        let cam = s.fit_camera(size);
        let mut dl = DrawList::default();
        let none = vec![false; s.g.n()];
        let hi = Highlight { focus: &none, hood: &none, active: false };
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.nodes.len(), 6);
        assert_eq!(dl.edges.len() + dl.edges_back.len(), 5);
        s.filter.search = "a".into();
        s.recompute_visibility();
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.nodes.len(), s.n_vis);
        // zoom far away → everything culled
        let far = ViewCam { center: V2::new(1e6, 1e6), zoom: 1.0 };
        build_draw_list(&s, &far, size, &hi, &mut dl);
        assert!(dl.nodes.is_empty() && dl.edges.is_empty());
    }

    #[test]
    fn style_modes() {
        let mut s = session();
        s.look.color_by = Some("score".into());
        s.look.size_by = Some("_degree".into());
        s.look.edge_mode = EdgeColorMode::Blend;
        s.recompute_style();
        assert!(matches!(s.style.legend, Legend::Continuous { .. }));
        assert!(s.style.edge_rgb.is_some());
        let max_r = s.style.node_r.iter().cloned().fold(0.0, f32::max);
        let min_r = s.style.node_r.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(max_r > min_r);
        s.look.color_by = Some("group".into());
        s.recompute_style();
        match &s.style.legend {
            Legend::Cat(v) => assert_eq!(v.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picking_finds_nearest_node() {
        let mut s = session();
        s.apply_layout_now();
        let size = V2::new(800.0, 600.0);
        let cam = s.fit_camera(size);
        let p = cam.to_screen(s.pos[2], size);
        assert_eq!(pick_node(&s, &cam, size, p + V2::new(1.0, 1.0)), Some(2));
        assert_eq!(pick_node(&s, &cam, size, V2::new(-500.0, -500.0)), None);
    }
}
