//! The session: loaded data, graph, layout state, appearance, filters, selection.
//! Free of any GUI toolkit so it can also drive the headless exporter and the tests.

use crate::boxes::{self, BoxInfo, BoxItem, BoxSettings};
use crate::changes::{self, ChangeInfo, ChangeSettings};
use crate::coords::{self, Cam3, CoordSettings, Orbit, Xform};
use crate::data::*;
use crate::expr;
use crate::graph::{Graph, build_graph};
use crate::layout::*;
use crate::query::{self, QueryInfo, QueryMode, QuerySettings, XTab};
use crate::section::{Outside, SectionSettings};
use crate::style::*;
use crate::subgraph::{self, ContractInfo, ContractSettings, SubInfo, SubSettings};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use egui::Vec2 as V2;
use std::collections::HashMap;
use std::path::Path;
use crate::edgeattr::EdgeRef;

// ───────────────────────────── appearance ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum EdgeColorMode {
    Uniform,
    /// The source node's colour.
    Source,
    Blend,
    /// An edge column (from the file, or made from both ends).
    Attribute,
    /// Any node column, taken from the source node.
    SourceAttr,
    /// Any node column, taken from the target node.
    TargetAttr,
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
    /// One shape per category of this column (circles without it); solids in the 3D coordinates view.
    pub shape_by: Option<String>,
    pub node_size: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub edge_mode: EdgeColorMode,
    pub edge_by: Option<String>,
    /// The node column for colouring edges by their source or target node.
    pub edge_node_by: Option<String>,
    /// Colour edges by a numeric column as categories.
    pub edge_as_cat: bool,
    /// Edge width from an attribute: thinnest and thickest, as multiples of the base width.
    pub edge_width_by: Option<EdgeRef>,
    pub edge_wmin: f32,
    pub edge_wmax: f32,
    /// Line pattern (solid, dashed, dotted …) per category of an attribute.
    pub edge_dash_by: Option<EdgeRef>,
    pub edge_alpha: f32,
    pub edge_width: f32,
    pub arrows: bool,
    pub labels: LabelMode,
    pub label_by: Option<String>,
    pub highlight_hood: bool,
    pub filtered: FilteredMode,
    pub dark: bool,
    /// Your own colours instead of the theme's (None: the theme's): edges in "single colour", the graph background,
    /// labels and other text on the graph.
    pub edge_color: Option<Rgb>,
    pub background_color: Option<Rgb>,
    pub label_color: Option<Rgb>,
    /// Your own colour for a value of a column: (column, value, colour). Used wherever that column colours nodes or
    /// edges by category.
    pub cat_colors: Vec<(String, String, Rgb)>,
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
            shape_by: None,
            node_size: 4.0,
            size_min: 0.6,
            size_max: 3.5,
            edge_mode: EdgeColorMode::Uniform,
            edge_by: None,
            edge_node_by: None,
            edge_as_cat: false,
            edge_width_by: None,
            edge_wmin: 0.5,
            edge_wmax: 4.0,
            edge_dash_by: None,
            edge_alpha: 0.35,
            edge_width: 1.0,
            arrows: true,
            labels: LabelMode::Auto,
            label_by: None,
            highlight_hood: true,
            filtered: FilteredMode::Dim,
            dark: true,
            edge_color: None,
            background_color: None,
            label_color: None,
            cat_colors: vec![],
        }
    }
}

impl Look {
    pub fn background(&self) -> Rgb {
        self.background_color.unwrap_or(self.default_background())
    }
    pub fn edge_base(&self) -> Rgb {
        self.edge_color.unwrap_or(self.default_edge())
    }
    pub fn text_color(&self) -> Rgb {
        self.label_color.unwrap_or(self.default_text())
    }
    pub fn default_background(&self) -> Rgb {
        if self.dark { [20, 22, 28] } else { [250, 250, 252] }
    }
    pub fn default_edge(&self) -> Rgb {
        if self.dark { [170, 180, 195] } else { [90, 96, 108] }
    }
    pub fn default_text(&self) -> Rgb {
        if self.dark { [225, 228, 235] } else { [30, 32, 40] }
    }

    /// Your own colour for a value of a column, if you chose one.
    pub fn cat_color_of(&self, col: &str, value: &str) -> Option<Rgb> {
        self.cat_colors.iter().find(|(c, v, _)| c == col && v == value).map(|x| x.2)
    }

    /// Choose (Some) or forget (None) your own colour for a value of a column.
    pub fn set_cat_color(&mut self, col: &str, value: &str, rgb: Option<Rgb>) {
        self.cat_colors.retain(|(c, v, _)| !(c == col && v == value));
        if let Some(rgb) = rgb {
            self.cat_colors.push((col.to_string(), value.to_string(), rgb));
        }
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
    /// What the edge colours mean, when they come from an attribute (counts are edges), and its title.
    pub edge_legend: Legend,
    pub edge_legend_title: String,
    /// The columns the node and edge colour legends show.
    pub legend_col: Option<String>,
    pub edge_legend_col: Option<String>,
    /// Width multiplier per edge, when width follows an attribute; the attribute and its range for the legend.
    pub edge_w: Option<Vec<f32>>,
    pub width_legend: Option<(String, String, String)>,
    /// Line pattern per edge (an index into `DASHES`), when it follows an attribute; the legend: category, pattern, count.
    pub edge_dash: Option<Vec<u8>>,
    pub dash_legend: Vec<(String, u8, usize)>,
    pub dash_title: String,
    /// Shape per node (an index into `NodeShape::ALL`), when shapes follow a column; the legend and its title.
    pub node_shape: Option<Vec<u8>>,
    pub shape_legend: Vec<(String, u8, usize)>,
    pub shape_title: String,
}

impl Default for StyleCache {
    fn default() -> Self {
        StyleCache {
            node_rgb: vec![],
            node_r: vec![],
            edge_rgb: None,
            legend: Legend::None,
            edge_legend: Legend::None,
            edge_legend_title: String::new(),
            legend_col: None,
            edge_legend_col: None,
            edge_w: None,
            width_legend: None,
            edge_dash: None,
            dash_legend: vec![],
            dash_title: String::new(),
            node_shape: None,
            shape_legend: vec![],
            shape_title: String::new(),
        }
    }
}

/// Line patterns as (dash, gap) lengths in multiples of the line width; (0, 0) is a solid line.
pub const DASHES: [(f32, f32, &str); 6] =
    [(0.0, 0.0, "solid"), (6.0, 4.0, "dashed"), (1.2, 3.0, "dotted"), (12.0, 5.0, "long dashes"), (6.0, 3.0, "dash-dot"), (2.5, 2.5, "fine dashes")];

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
        // the default colour of each value, or the one you chose for it
        let by_label: Vec<Rgb> = labels.iter().enumerate().map(|(i, l)| look.cat_color_of(&col.name, l).unwrap_or(cat_color(i))).collect();
        let colors: Vec<Rgb> = codes
            .iter()
            .map(|&c| {
                if c == NULL_CODE {
                    MISSING
                } else {
                    counts[c as usize] += 1;
                    by_label[c as usize]
                }
            })
            .collect();
        let legend = Legend::Cat(labels.iter().enumerate().map(|(i, l)| (l.clone(), by_label[i], counts[i])).collect());
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
        _ => None,
    };
    let (edge_rgb, edge_legend, edge_legend_title) = match look.edge_mode {
        EdgeColorMode::Attribute => match look.edge_by.as_deref().and_then(|c| g.edge_col(c)) {
            Some(col) => {
                let (rgb, l) = color_column(col, look.edge_as_cat, look);
                (Some(rgb), l, col.name.clone())
            }
            None => (None, Legend::None, String::new()),
        },
        EdgeColorMode::SourceAttr | EdgeColorMode::TargetAttr => match look.edge_node_by.as_deref().and_then(|c| g.node_col(c)) {
            Some(col) => {
                // the same colours the nodes would get from this column, taken from one end of each edge
                let src = look.edge_mode == EdgeColorMode::SourceAttr;
                let end = |&(s, t): &(u32, u32)| if src { s as usize } else { t as usize };
                let (rgb, mut l) = color_column(col, look.edge_as_cat, look);
                if let Legend::Cat(items) = &mut l {
                    let codes = col.as_categories().0;
                    items.iter_mut().for_each(|it| it.2 = 0);
                    for e in &g.edges {
                        if let Some(it) = items.get_mut(codes[end(e)] as usize) {
                            it.2 += 1;
                        }
                    }
                }
                let title = format!("{} node: {}", if src { "Source" } else { "Target" }, col.name);
                (Some(g.edges.iter().map(|e| rgb[end(e)]).collect()), l, title)
            }
            None => (None, Legend::None, String::new()),
        },
        _ => (edge_rgb, Legend::None, String::new()),
    };
    // width from an attribute: numbers scale between the thinnest and thickest, categories step through that range
    let (edge_w, width_legend) = match look.edge_width_by.as_ref().and_then(|r| r.values(g).map(|c| (r, c))) {
        Some((r, col)) => {
            let (lo_w, hi_w) = (look.edge_wmin, look.edge_wmax);
            if col.is_numeric() && !col.hint_cat {
                let range = scale_range(&col, true);
                let w = (0..col.len())
                    .map(|i| match (col.num(i), range) {
                        (Some(v), Some((lo, hi))) if hi > lo => lo_w + (hi_w - lo_w) * ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32,
                        (Some(_), _) => (lo_w + hi_w) * 0.5,
                        _ => lo_w,
                    })
                    .collect();
                let (lo, hi) = range.unwrap_or((0.0, 0.0));
                (Some(w), Some((r.label(), crate::data::fmt_num(lo), crate::data::fmt_num(hi))))
            } else {
                let (codes, labels) = col.as_categories();
                let k = labels.len().max(2) - 1;
                let w = codes.iter().map(|&c| if c == NULL_CODE { lo_w } else { lo_w + (hi_w - lo_w) * c as f32 / k as f32 }).collect();
                (Some(w), Some((r.label(), labels.first().cloned().unwrap_or_default(), labels.last().cloned().unwrap_or_default())))
            }
        }
        None => (None, None),
    };
    // a line pattern per category (numbers count as categories); the patterns repeat after six
    let (edge_dash, dash_legend, dash_title) = match look.edge_dash_by.as_ref().and_then(|r| r.values(g).map(|c| (r, c))) {
        Some((r, col)) => {
            let (codes, labels) = col.as_categories();
            let mut counts = vec![0usize; labels.len()];
            let d = codes
                .iter()
                .map(|&c| {
                    if c == NULL_CODE {
                        0
                    } else {
                        counts[c as usize] += 1;
                        (c as usize % DASHES.len()) as u8
                    }
                })
                .collect();
            let legend = labels.into_iter().enumerate().map(|(i, l)| (l, (i % DASHES.len()) as u8, counts[i])).collect();
            (Some(d), legend, r.label())
        }
        None => (None, vec![], String::new()),
    };
    // a shape per category (numbers count as categories); after ten they repeat, missing values stay circles
    let (node_shape, shape_legend, shape_title) = match look.shape_by.as_deref().and_then(|c| g.node_col(c)) {
        Some(col) => {
            let k = crate::shapes::NodeShape::ALL.len();
            let (codes, labels) = col.as_categories();
            let mut counts = vec![0usize; labels.len()];
            let v = codes
                .iter()
                .map(|&c| {
                    if c == NULL_CODE {
                        0
                    } else {
                        counts[c as usize] += 1;
                        (c as usize % k) as u8
                    }
                })
                .collect();
            let legend = labels.into_iter().enumerate().map(|(i, l)| (l, (i % k) as u8, counts[i])).collect();
            (Some(v), legend, col.name.clone())
        }
        None => (None, vec![], String::new()),
    };
    let legend_col = look.color_by.clone().filter(|c| g.node_col(c).is_some());
    let edge_legend_col = match look.edge_mode {
        EdgeColorMode::Attribute => look.edge_by.clone(),
        EdgeColorMode::SourceAttr | EdgeColorMode::TargetAttr => look.edge_node_by.clone(),
        _ => None,
    };
    StyleCache {
        node_rgb,
        node_r,
        edge_rgb,
        legend,
        edge_legend,
        edge_legend_title,
        legend_col,
        edge_legend_col,
        edge_w,
        width_legend,
        edge_dash,
        dash_legend,
        dash_title,
        node_shape,
        shape_legend,
        shape_title,
    }
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
    /// Rules on edge attributes: an edge that fails one is dimmed or hidden (as filtered-out nodes are).
    pub edge_rules: Vec<Rule>,
}

impl FilterState {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.expr_text.trim().is_empty() && self.search.trim().is_empty() && self.edge_rules.is_empty()
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
    /// Nodes taken out by attribute, with their neighbours linked directly (graph contraction).
    pub contract: ContractSettings,
    pub contract_info: Option<ContractInfo>,
    /// The section box / section lines that cut the viewport.
    pub section: SectionSettings,
    /// Nodes that only the filter takes out. With "Dim" they stay on screen, faded; they cannot be clicked.
    pub dimmed: Vec<bool>,
    /// The connection query (settings, and the result of the last run; at most one of the two results is set).
    pub query: QuerySettings,
    pub query_info: Option<QueryInfo>,
    pub xtab: Option<XTab>,
    pub query_error: Option<String>,
    pub coords: CoordSettings,
    /// 3D positions (x right, y into the screen, z up); filled while node coordinates are in use.
    pub pos3: Vec<[f32; 3]>,
    pub cam3: Cam3,
    /// Nodes that have no value in a coordinate column.
    pub coords_missing: usize,
    /// The transform that turned coordinate values into `pos3` (bounding boxes use it too).
    pub coords_xform: Option<Xform>,
    pub boxes: BoxSettings,
    /// The boxes to draw (3D view only); empty while boxes are off.
    pub box_items: Vec<BoxItem>,
    pub box_info: BoxInfo,
    coords_saved_pos: Option<Vec<V2>>,
    /// Node positions from before the sub-graph was shown, put back when it is switched off.
    sub_saved_pos: Option<Vec<V2>>,
    pub expr_error: Option<String>,
    pub vis: Vec<bool>,
    pub manual_hidden: Vec<bool>,
    pub n_vis: usize,
    pub n_vis_edges: usize,
    /// Edges that pass the edge rules (all true without any).
    pub edge_vis: Vec<bool>,
    pub sel: Vec<bool>,
    pub sel_count: usize,
    pub sel_version: u64,
    /// Selected edges (indices into `g.edges`). Edges can be selected on their own or together with nodes.
    pub sel_edges: Vec<u32>,
    pub style: StyleCache,
    pub fit_requested: bool,
    /// Edge attributes made from both ends (made again with every rebuild).
    pub edge_attrs: Vec<crate::edgeattr::EdgeAttr>,
    /// Solids from an OBJ file, the settings for them, which solid each node has, and how many matched.
    pub geo: Option<crate::geometry::SharedLib>,
    pub geo_set: crate::geometry::GeoSettings,
    pub geo_node: Vec<Option<u32>>,
    pub geo_info: crate::geometry::GeoInfo,
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
            contract: ContractSettings::default(),
            contract_info: None,
            section: SectionSettings::default(),
            dimmed: vec![],
            query: QuerySettings::default(),
            query_info: None,
            xtab: None,
            query_error: None,
            coords: CoordSettings::default(),
            pos3: vec![],
            cam3: Cam3::default(),
            coords_missing: 0,
            coords_xform: None,
            boxes: BoxSettings::default(),
            box_items: vec![],
            box_info: BoxInfo::default(),
            coords_saved_pos: None,
            sub_saved_pos: None,
            expr_error: None,
            vis: vec![],
            manual_hidden: vec![],
            n_vis: 0,
            edge_vis: vec![],
            n_vis_edges: 0,
            sel: vec![],
            sel_count: 0,
            sel_version: 0,
            sel_edges: vec![],
            style: StyleCache::default(),
            fit_requested: false,
            geo: None,
            geo_set: Default::default(),
            geo_node: vec![],
            geo_info: Default::default(),
            edge_attrs: vec![],
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
        // positions of the nodes we already know (borrowed names: no copy of every id)
        let old: HashMap<&str, V2> = self.g.ids.iter().map(|s| s.as_str()).zip(self.pos.iter().copied()).collect();
        let g = build_graph(
            self.edge_src.as_ref().map(|s| (&s.table, &s.mapping)),
            self.node_src.as_ref().map(|s| (&s.table, &s.mapping)),
        );
        let n = g.n();
        let mut pos = initial_spiral(n);
        let mut known = 0usize;
        for (i, id) in g.ids.iter().enumerate() {
            if let Some(p) = old.get(id.as_str()) {
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
        crate::edgeattr::apply(&mut self.g, &self.edge_attrs);
        self.rematch_geometry();
        self.pos = pos;
        self.manual_hidden = vec![false; n];
        self.sel = vec![false; n];
        self.sel_count = 0;
        self.sel_edges.clear();
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
        if !has(&self.g, &self.look.shape_by) {
            self.look.shape_by = None;
        }
        if !has(&self.g, &self.look.label_by) {
            self.look.label_by = None;
        }
        if let Some(e) = &self.look.edge_by {
            if self.g.edge_col(e).is_none() {
                self.look.edge_by = None;
            }
        }
        if !has(&self.g, &self.look.edge_node_by) {
            self.look.edge_node_by = None;
        }
        self.drop_missing_edge_refs();
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
        if self.contract.rule.as_ref().is_some_and(|r| self.g.node_col(&r.col).is_none()) {
            self.contract.rule = None;
        }
        self.drop_missing_query_columns();
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
        self.coords_xform = None;
        self.recompute_boxes();
        self.recompute_subgraph();
        self.recompute_contract();
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
        if let Some(ci) = &self.contract_info {
            if ci.removed.len() == n {
                for i in 0..n {
                    vis[i] &= !ci.removed[i];
                }
            }
        }
        // what the filter below takes out is only dimmed; hand-hidden nodes and nodes outside the subgraph are gone
        let before_filter = vis.clone();
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
        // several values separated by commas: a node passes if it matches any of them. A value that is exactly a node's
        // id finds that node only (so "n1" does not also find n10, n11 …); others find ids and texts containing them.
        let terms: Vec<String> = self.filter.search.split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect();
        if !terms.is_empty() {
            let ids_lower: std::collections::HashSet<String> = g.ids.iter().map(|s| s.to_lowercase()).collect();
            let (exact, partial): (Vec<&String>, Vec<&String>) = terms.iter().partition(|t| ids_lower.contains(t.as_str()));
            for i in 0..n {
                if !vis[i] {
                    continue;
                }
                let id = g.ids[i].to_lowercase();
                let mut hit = exact.iter().any(|t| **t == id) || partial.iter().any(|t| id.contains(t.as_str()));
                if !hit && !partial.is_empty() {
                    hit = g.node_cols.iter().any(|c| !c.is_metric && c.cat_str(i).is_some_and(|s| partial.iter().any(|t| contains_ci(s, t))));
                }
                vis[i] = hit;
            }
        }
        self.n_vis = vis.iter().filter(|v| **v).count();
        let mut edge_vis = vec![true; g.edges.len()];
        for r in self.filter.edge_rules.iter().filter(|r| r.enabled) {
            if let Some(col) = g.edge_col(&r.col) {
                subgraph::col_pass(col, &r.kind, &mut edge_vis);
            }
        }
        let mut n_edges = g.edges.iter().zip(&edge_vis).filter(|&(&(s, t), &ok)| ok && vis[s as usize] && vis[t as usize]).count();
        for list in self.bridge_lists() {
            n_edges += list.iter().filter(|&&(s, t, _)| vis[s as usize] && vis[t as usize]).count();
        }
        self.n_vis_edges = n_edges;
        self.dimmed = (0..n).map(|i| before_filter[i] && !vis[i]).collect();
        self.vis = vis;
        self.edge_vis = edge_vis;
        self.recompute_changes();
        self.recompute_query();
    }

    /// Work the connection query out again (after its settings, the filter or the data changed).
    pub fn recompute_query(&mut self) {
        self.query_info = None;
        self.xtab = None;
        self.query_error = None;
        if !self.query.is_set() || self.g.n() == 0 {
            return;
        }
        match self.query.mode {
            QueryMode::Count => match query::run(&self.g, &self.query, &self.vis) {
                Ok(info) => self.query_info = Some(info),
                Err(e) => self.query_error = Some(e),
            },
            QueryMode::CrossTab => {
                let (Some(r), Some(c)) = (self.query.rows.as_deref(), self.query.cols.as_deref()) else { return };
                let vis = self.query.visible_only.then_some(self.vis.as_slice());
                match query::crosstab(&self.g, r, c, self.query.dir, vis) {
                    Ok(x) => self.xtab = Some(x),
                    Err(e) => self.query_error = Some(e),
                }
            }
        }
    }

    /// Forget query settings that name a column the data no longer has.
    pub fn drop_missing_query_columns(&mut self) {
        let g = &self.g;
        let gone = |c: &Option<String>| c.as_deref().is_some_and(|n| n != query::ID_COLUMN && g.node_col(n).is_none());
        for cond in [&mut self.query.a, &mut self.query.b] {
            if gone(&cond.col) {
                cond.col = None;
                cond.value.clear();
            }
        }
        if gone(&self.query.rows) {
            self.query.rows = None;
        }
        if gone(&self.query.cols) {
            self.query.cols = None;
        }
    }

    /// Make exactly these nodes the selection.
    pub fn select_nodes(&mut self, nodes: impl IntoIterator<Item = usize>) {
        self.clear_selection();
        for i in nodes {
            if i < self.sel.len() {
                self.set_selected(i, true);
            }
        }
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
                let info = subgraph::compute(&self.g, rule, self.sub.hops, self.sub.connect, self.sub.contract);
                // a rule nothing matches would leave an empty canvas: show the whole graph instead
                if info.n_match == 0 {
                    Some(SubInfo { member: vec![true; self.g.n()], connector: vec![false; self.g.n()], bridges: vec![], ..info })
                } else {
                    Some(info)
                }
            }
            _ => None,
        };
    }

    /// Work out which nodes are contracted away (needs the whole graph, so it is cached like the subgraph).
    pub fn recompute_contract(&mut self) {
        self.contract_info = match (&self.contract.rule, self.contract.enabled) {
            (Some(rule), true) if self.g.node_col(&rule.col).is_some() => Some(subgraph::contract_nodes(&self.g, rule)),
            _ => None,
        };
    }

    /// True while nodes are being contracted away (a rule that removes nothing, or everything, does not count).
    pub fn contract_active(&self) -> bool {
        self.contract_info.as_ref().is_some_and(|c| c.n_removed > 0 && c.n_removed < self.g.n())
    }

    /// The links that stand in for contracted or joined-over nodes: from the subgraph and from contraction.
    pub fn bridge_lists(&self) -> [&[subgraph::Bridge]; 2] {
        let sub: &[subgraph::Bridge] = self.sub_info.as_ref().filter(|i| i.n_match > 0).map(|i| i.bridges.as_slice()).unwrap_or(&[]);
        let con: &[subgraph::Bridge] = if self.contract_active() { self.contract_info.as_ref().map(|c| c.bridges.as_slice()).unwrap_or(&[]) } else { &[] };
        [sub, con]
    }

    /// True while a sub-graph is being shown (a rule that matches nothing does not count).
    pub fn sub_active(&self) -> bool {
        self.sub_info.as_ref().is_some_and(|i| i.n_match > 0)
    }

    /// Apply the sub-graph settings: recompute, lay the visible part out afresh, or bring the full layout back.
    pub fn apply_subgraph(&mut self) {
        let wants = (self.sub.enabled && self.sub.rule.is_some()) || (self.contract.enabled && self.contract.rule.is_some());
        if wants && self.sub_saved_pos.is_none() {
            self.sub_saved_pos = Some(self.pos.clone());
        }
        self.recompute_subgraph();
        self.recompute_contract();
        self.recompute_visibility();
        if self.sub_active() || self.contract_active() {
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
                self.coords_xform = Some(p.xform);
                self.recompute_boxes();
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
        self.coords_xform = None;
        self.recompute_boxes();
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

    /// Point the 3D camera at the visible nodes and their boxes (the viewing angle stays as it is). Boxes that lie
    /// far from the nodes (placeholder values such as -9999, a site outline) are left out of the framing, or one
    /// of them would push the camera away until everything else shrinks to nothing.
    pub fn fit_cam3(&mut self) {
        self.cam3 = self.cam3_fit_target();
    }

    /// The camera `fit_cam3` would set, without changing anything.
    pub fn cam3_fit_target(&self) -> Cam3 {
        let boxes_shown = self.boxes_shown();
        let mut pts: Vec<[f32; 3]> = if boxes_shown && !self.boxes.show_nodes {
            vec![] // boxes alone: frame just those
        } else {
            (0..self.g.n()).filter(|&i| self.vis[i] && !self.section_hides(i)).map(|i| self.pos3[i]).collect()
        };
        if boxes_shown {
            let near = self.boxes_near_nodes();
            let visible: Vec<usize> = (0..self.box_items.len()).filter(|&k| { let n = self.box_items[k].node as usize; self.vis[n] && !self.section_hides(n) }).collect();
            // when most boxes sit around their nodes the odd ones out are ignored; when most do not, the boxes
            // are somewhere else altogether (another origin or unit): frame the boxes, since that is what was asked for
            let ignore_far = visible.iter().filter(|&&k| near[k]).count() * 2 >= visible.len();
            if !ignore_far {
                pts.clear();
            }
            for &k in &visible {
                if !ignore_far || near[k] {
                    pts.push(self.box_items[k].lo);
                    pts.push(self.box_items[k].hi);
                }
            }
        }
        let pts = if pts.is_empty() { self.pos3.clone() } else { pts };
        let mut cam = self.cam3;
        cam.fit(&pts);
        cam
    }

    /// For each box: does it lie within (a generous margin around) the cloud of all nodes?
    fn boxes_near_nodes(&self) -> Vec<bool> {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in self.pos3.iter().filter(|p| p.iter().all(|v| v.is_finite())) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if lo[0] > hi[0] {
            return vec![true; self.box_items.len()];
        }
        let ext = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
        let margin = ext * 0.35 + 1.0;
        self.box_items
            .iter()
            .map(|b| (0..3).all(|k| b.lo[k] >= lo[k] - margin && b.hi[k] <= hi[k] + margin))
            .collect()
    }

    /// Work out the bounding boxes again (after a setting, the coordinates or the data changed).
    pub fn recompute_boxes(&mut self) {
        let (items, info) = match (&self.coords_xform, self.boxes.enabled && self.boxes.has_columns() && self.coords_active()) {
            (Some(xf), true) => boxes::compute(&self.g, xf, &self.boxes),
            _ => (vec![], BoxInfo::default()),
        };
        self.box_items = items;
        self.box_info = info;
        self.box_info.n_far = self.boxes_near_nodes().iter().filter(|n| !**n).count();
    }

    /// True while bounding boxes are on screen (3D view, boxes on, at least one box).
    pub fn boxes_shown(&self) -> bool {
        self.is_3d() && self.boxes.enabled && !self.box_items.is_empty()
    }

    /// True while the nodes and edges are hidden so that only the boxes show.
    pub fn graph_hidden(&self) -> bool {
        self.boxes_shown() && !self.boxes.show_nodes
    }

    /// How points are turned into screen positions right now (2D camera, or the 3D orbit camera).
    pub fn projector(&self, cam: &ViewCam, size: V2) -> Projector {
        if self.is_3d() { Projector::Orbit(self.cam3.orbit(size)) } else { Projector::Flat { cam: *cam, size } }
    }

    pub fn recompute_style(&mut self) {
        self.style = compute_style(&self.g, &self.look);
    }

    /// Load solids from an OBJ file and match them to the nodes (by the chosen column, or the one that matches best).
    pub fn load_geometry(&mut self, path: &Path) -> Result<()> {
        let lib = crate::geometry::read_obj(path)?;
        // keep the chosen column unless it matches nothing in a loaded graph (a session sets it before its graph is built)
        let keep = match self.geo_set.match_by.as_deref() {
            None => false,
            Some(_) if self.g.n() == 0 => true,
            Some(c) => (c == "id" || self.g.node_col(c).is_some()) && crate::geometry::match_nodes(&self.g, &lib, c).1.nodes > 0,
        };
        if !keep {
            self.geo_set.match_by = crate::geometry::best_column(&self.g, &lib);
        }
        self.geo_set.path = Some(path.to_path_buf());
        self.geo = Some(std::sync::Arc::new(lib));
        self.rematch_geometry();
        Ok(())
    }

    /// Work out again which solid each node has (after the data or the matching column changed).
    pub fn rematch_geometry(&mut self) {
        match (&self.geo, &self.geo_set.match_by) {
            (Some(lib), Some(col)) => {
                let (m, info) = crate::geometry::match_nodes(&self.g, lib, col);
                self.geo_node = m;
                self.geo_info = info;
            }
            _ => {
                self.geo_node.clear();
                self.geo_info = Default::default();
            }
        }
    }

    pub fn clear_geometry(&mut self) {
        self.geo = None;
        self.geo_set = Default::default();
        self.rematch_geometry();
    }

    /// Forget edge settings whose attribute is gone.
    pub fn drop_missing_edge_refs(&mut self) {
        let g = &self.g;
        for r in [&mut self.look.edge_width_by, &mut self.look.edge_dash_by] {
            if r.as_ref().is_some_and(|r| !r.exists(g)) {
                *r = None;
            }
        }
        self.filter.edge_rules.retain(|r| g.edge_col(&r.col).is_some());
    }

    /// Does edge `e` pass the edge rules?
    pub fn edge_passes(&self, e: usize) -> bool {
        self.edge_vis.get(e).copied().unwrap_or(true)
    }

    /// Make an edge attribute from both ends and keep its recipe.
    pub fn add_edge_attr(&mut self, a: crate::edgeattr::EdgeAttr) -> Result<(), String> {
        if a.name.trim().is_empty() {
            return Err("Give the attribute a name.".into());
        }
        if self.g.edge_col(&a.name).is_some() {
            return Err(format!("There is already an edge attribute called “{}”.", a.name));
        }
        let col = a.compute(&self.g).ok_or_else(|| format!("“{}” cannot be made from {}.", a.how.label(), a.col))?;
        self.g.edge_cols.push(col);
        self.edge_attrs.push(a);
        Ok(())
    }

    /// Use these edge attributes from both ends instead of the ones there are (a preset brings its own).
    pub fn set_edge_attrs(&mut self, list: Vec<crate::edgeattr::EdgeAttr>) {
        self.g.edge_cols.retain(|c| !c.is_metric);
        self.edge_attrs = list;
        crate::edgeattr::apply(&mut self.g, &self.edge_attrs);
    }

    /// Forget an edge attribute made from both ends (and stop colouring by it).
    pub fn remove_edge_attr(&mut self, i: usize) {
        if i >= self.edge_attrs.len() {
            return;
        }
        let a = self.edge_attrs.remove(i);
        self.g.edge_cols.retain(|c| !(c.is_metric && c.name == a.name));
        if self.look.edge_by.as_deref() == Some(a.name.as_str()) {
            self.look.edge_by = None;
        }
        self.drop_missing_edge_refs();
        self.recompute_visibility();
        self.recompute_style();
    }

    // ── section box / section lines ──

    /// Number of axes the section works on: x and y in the flat view, x, y and z in 3D.
    pub fn section_dims(&self) -> usize {
        if self.is_3d() { 3 } else { 2 }
    }

    /// Where node `i` sits for the section: its layout position, or its 3D position.
    pub fn section_point(&self, i: usize) -> [f32; 3] {
        if self.is_3d() { self.pos3[i] } else { [self.pos[i].x, self.pos[i].y, 0.0] }
    }

    /// True while a section cuts the viewport.
    pub fn section_active(&self) -> bool {
        self.section.enabled && self.section.placed && self.section.any_line(self.section_dims())
    }

    /// Per node: is it outside the section? None while no section is on.
    pub fn section_outside(&self) -> Option<Vec<bool>> {
        if !self.section_active() {
            return None;
        }
        let dims = self.section_dims();
        Some((0..self.g.n()).map(|i| !self.section.keeps(self.section_point(i), dims)).collect())
    }

    /// Is node `i` outside an active section (dimmed or hidden)?
    pub fn section_outside_node(&self, i: usize) -> bool {
        self.section_active() && !self.section.keeps(self.section_point(i), self.section_dims())
    }

    /// Is node `i` taken out of sight (not just dimmed) by the section?
    pub fn section_hides(&self, i: usize) -> bool {
        self.section_active() && self.section.outside == Outside::Hide && !self.section.keeps(self.section_point(i), self.section_dims())
    }

    /// Fit the section box to the visible nodes (or to the selection).
    pub fn fit_section(&mut self, selection_only: bool) -> bool {
        let pts: Vec<[f32; 3]> = (0..self.g.n()).filter(|&i| self.vis[i] && (!selection_only || self.sel[i])).map(|i| self.section_point(i)).collect();
        let all: Vec<[f32; 3]> = (0..self.g.n()).filter(|&i| self.vis[i]).map(|i| self.section_point(i)).collect();
        self.section.set_range(all.into_iter());
        self.section.fit_to(pts.into_iter())
    }

    // ── selection ──

    pub fn clear_selection(&mut self) {
        if self.sel_count > 0 {
            self.sel.iter_mut().for_each(|s| *s = false);
            self.sel_count = 0;
            self.sel_version += 1;
        }
        if !self.sel_edges.is_empty() {
            self.sel_edges.clear();
            self.sel_version += 1;
        }
    }

    pub fn edge_selected(&self, e: usize) -> bool {
        self.sel_edges.contains(&(e as u32))
    }

    pub fn set_edge_selected(&mut self, e: usize, on: bool) {
        if e >= self.g.edges.len() {
            return;
        }
        let at = self.sel_edges.iter().position(|&x| x == e as u32);
        match (at, on) {
            (None, true) => self.sel_edges.push(e as u32),
            (Some(k), false) => {
                self.sel_edges.remove(k);
            }
            _ => return,
        }
        self.sel_version += 1;
    }

    pub fn toggle_edge_selected(&mut self, e: usize) {
        let on = !self.edge_selected(e);
        self.set_edge_selected(e, on);
    }

    /// Make this edge the whole selection.
    pub fn select_only_edge(&mut self, e: usize) {
        self.clear_selection();
        self.set_edge_selected(e, true);
    }

    /// Is anything at all selected (nodes or edges)?
    pub fn has_selection(&self) -> bool {
        self.sel_count > 0 || !self.sel_edges.is_empty()
    }

    /// The nodes the selection stands for: the selected nodes and the two ends of every selected edge (visible ones only).
    pub fn selection_nodes(&self) -> Vec<usize> {
        let mut mark = self.sel.clone();
        for &e in &self.sel_edges {
            if let Some(&(a, b)) = self.g.edges.get(e as usize) {
                mark[a as usize] = true;
                mark[b as usize] = true;
            }
        }
        (0..self.g.n()).filter(|&i| mark[i] && self.vis[i]).collect()
    }

    /// The point the 3D camera turns around: the middle of the box around the selected nodes and selected edges
    /// (an edge counts with both of its ends). None when nothing visible is selected.
    pub fn selection_pivot3(&self) -> Option<[f32; 3]> {
        let nodes = self.selection_nodes();
        if nodes.is_empty() || self.pos3.len() != self.g.n() {
            return None;
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for i in nodes {
            for k in 0..3 {
                lo[k] = lo[k].min(self.pos3[i][k]);
                hi[k] = hi[k].max(self.pos3[i][k]);
            }
        }
        Some([(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5])
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
        if self.layout.visible_only || self.sub_active() || self.contract_active() {
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
        let bridges: Vec<(u32, u32)> = self.bridge_lists().into_iter().flatten().map(|&(a, b, _)| (a, b)).collect();
        self.sim.setup(&self.g, &active, &self.layout.force, anchors, &bridges);
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
    /// Put the nodes back where a named view had them, and hide the nodes it had hidden.
    pub fn restore_view(&mut self, positions: &[(String, f32, f32)], hidden: &[String]) {
        for (id, x, y) in positions {
            if let Some(&i) = self.g.index.get(id) {
                self.pos[i as usize] = V2::new(*x, *y);
            }
        }
        self.anim = None;
        self.sim.running = false;
        self.fit_requested = false;
        self.manual_hidden = vec![false; self.g.n()];
        for id in hidden {
            if let Some(&i) = self.g.index.get(id) {
                self.manual_hidden[i as usize] = true;
            }
        }
    }

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
        self.drop_missing_edge_refs();
        let g = &self.g;
        let miss = |c: &Option<String>| c.as_ref().is_some_and(|c| g.node_col(c).is_none());
        if miss(&self.look.color_by) {
            self.look.color_by = None;
        }
        if miss(&self.look.size_by) {
            self.look.size_by = None;
        }
        if miss(&self.look.shape_by) {
            self.look.shape_by = None;
        }
        if miss(&self.look.label_by) {
            self.look.label_by = None;
        }
        if self.look.edge_by.as_ref().is_some_and(|e| g.edge_col(e).is_none()) {
            self.look.edge_by = None;
        }
        if miss(&self.look.edge_node_by) {
            self.look.edge_node_by = None;
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
        for c in self.boxes.min.iter_mut().chain(self.boxes.max.iter_mut()) {
            if c.as_ref().is_some_and(|name| g.node_col(name).is_none()) {
                *c = None;
            }
        }
        if self.boxes.rule.as_ref().is_some_and(|r| g.node_col(&r.col).is_none()) {
            self.boxes.rule = None;
        }
        if self.sub.rule.as_ref().is_some_and(|r| g.node_col(&r.col).is_none()) {
            self.sub.rule = None;
        }
        if self.contract.rule.as_ref().is_some_and(|r| g.node_col(&r.col).is_none()) {
            self.contract.rule = None;
        }
        self.drop_missing_query_columns();
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
            if visible_only && (!self.vis[i] || self.section_hides(i)) {
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
            let (mut xs, mut ys): (Vec<f32>, Vec<f32>) = (0..self.g.n()).filter(|&i| self.vis[i] && !self.section_hides(i)).map(|i| (self.pos[i].x, self.pos[i].y)).unzip();
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
    /// Index into `NodeShape::ALL` (0 = circle).
    pub shape: u8,
    /// In 3D, a node big enough on screen (or filling its bounding box) is a shaded solid: its visible triangles,
    /// far ones first. Empty: drawn flat.
    pub solid: Vec<([[f32; 2]; 3], [u8; 4])>,
}

pub struct DrawEdge {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub rgba: [u8; 4],
    pub target_r: f32,
    /// Gets an arrow head (if arrows are on).
    pub directed: bool,
    /// Width as a multiple of the base edge width.
    pub w: f32,
    /// Line pattern: an index into `DASHES` (0 = solid).
    pub dash: u8,
}

/// A bounding box projected to the screen (3D only). Parts behind the camera are already cut away.
#[derive(Clone)]
pub struct DrawBox {
    /// The six faces (order of `boxes::FACES`) as screen polygons: the points and how many there are (0 = not seen).
    pub faces: [([[f32; 2]; 8], usize); 6],
    /// The twelve edges (order of `boxes::EDGES`); None if behind the camera.
    pub edges: [Option<([f32; 2], [f32; 2])>; 12],
    /// Which faces look towards the camera.
    pub front: [bool; 6],
    pub rgb: [u8; 3],
    /// Opacity of the faces and of the edges, 0 to 255.
    pub fill: u8,
    pub line: u8,
    pub width: f32,
    pub depth: f32,
}

impl DrawBox {
    /// The visible face polygons.
    pub fn polygons(&self) -> impl Iterator<Item = &[[f32; 2]]> {
        self.faces.iter().filter(|f| f.1 >= 3).map(|f| &f.0[..f.1])
    }

    /// The visible edges with their opacity: edges along the far side of the box are fainter.
    pub fn lines(&self) -> impl Iterator<Item = ([f32; 2], [f32; 2], u8)> + '_ {
        boxes::EDGES.iter().zip(self.edges.iter()).filter_map(|(&(_, _, f1, f2), e)| {
            let (a, b) = (*e)?;
            let alpha = if self.front[f1] || self.front[f2] { self.line } else { (self.line as f32 * 0.4) as u8 };
            Some((a, b, alpha))
        })
    }
}

#[derive(Default)]
pub struct DrawList {
    /// Bounding boxes, far to near; drawn before everything else.
    pub boxes: Vec<DrawBox>,
    pub nodes: Vec<DrawNode>,
    /// Faded edges, drawn first.
    pub edges_back: Vec<DrawEdge>,
    pub edges: Vec<DrawEdge>,
    /// Highlighted attribute-change edges, thicker and drawn last.
    pub edges_hi: Vec<DrawEdge>,
    /// Draw only every k-th edge (1 = all of them). Set by the caller while the view is being dragged or zoomed.
    pub edge_stride: u32,
}

/// Focus/neighbourhood masks used to fade everything but a node and its neighbours.
pub struct Highlight<'a> {
    pub focus: &'a [bool],
    pub hood: &'a [bool],
    pub active: bool,
    /// The edges that stand out (walking the graph); None: those touching a focused node.
    pub edges: Option<&'a [bool]>,
    /// Hide what does not stand out, instead of fading it.
    pub hide: bool,
}

pub fn build_draw_list(s: &Session, cam: &ViewCam, size: V2, hi: &Highlight, out: &mut DrawList) {
    out.nodes.clear();
    out.edges.clear();
    out.edges_back.clear();
    out.edges_hi.clear();
    out.boxes.clear();
    let g = &s.g;
    let n = g.n();
    if n == 0 {
        return;
    }
    let dim_hidden = s.look.filtered == FilteredMode::Dim;
    // a node is on screen if it is visible, or if the filter alone took it out and filtered-out nodes are only dimmed
    let shown = |i: usize| s.vis[i] || (dim_hidden && s.dimmed.get(i).copied().unwrap_or(false));
    // section box: nodes outside it are dimmed or dropped
    let outside = s.section_outside();
    let section_hide = s.section.outside == Outside::Hide;
    let out_of = |i: usize| outside.as_ref().is_some_and(|o| o[i]);
    let proj = s.projector(cam, size);
    if let Projector::Orbit(o) = &proj {
        if s.boxes_shown() {
            build_boxes(s, o, size, hi, out);
        }
        if s.graph_hidden() {
            return;
        }
    }
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

    // edges: split into chunks that rayon works on side by side; the chunks are joined in order, so the drawing
    // order (and therefore the picture) does not depend on how many cores there are
    let base = s.look.edge_base();
    let ea = s.look.edge_alpha;
    let uniform = s.style.edge_rgb.is_none();
    // while the user drags or zooms a very large graph only every k-th edge is drawn, a little stronger
    let stride = out.edge_stride.max(1) as usize;
    let boost = (stride as f32).min(3.0);
    let edge_one = |e: usize, a: u32, b: u32| -> Option<(DrawEdge, u8)> {
        let (ai, bi) = (a as usize, b as usize);
        if ai == bi {
            return None;
        }
        let both = s.vis[ai] && s.vis[bi];
        if !(shown(ai) && shown(bi)) {
            return None;
        }
        let passes = s.edge_passes(e);
        if !passes && !dim_hidden {
            return None;
        }
        let both = both && passes;
        let cut = out_of(ai) || out_of(bi);
        if cut && section_hide {
            return None;
        }
        let (Some((pa, _, da)), Some((pb, _, db))) = (pr[ai], pr[bi]) else { return None };
        if (pa.x < 0.0 && pb.x < 0.0) || (pa.y < 0.0 && pb.y < 0.0) || (pa.x > size.x && pb.x > size.x) || (pa.y > size.y && pb.y > size.y) {
            return None;
        }
        let mut alpha = ea;
        let mut kind = 0u8; // 0 normal, 1 faded (drawn first), 2 highlighted (drawn last)
        if let (true, Some(ci)) = (both, &s.change_info) {
            if ci.edge_hi[e] {
                kind = 2;
            } else if s.change.only {
                if ci.edge_path[e] {
                    alpha *= 0.8; // a joining path: ordinary edge colour, a little softer
                } else {
                    alpha *= 0.06; // everything else is only dimmed
                    kind = 1;
                }
            } else {
                alpha *= 0.1;
                kind = 1;
            }
        } else if !both {
            alpha *= 0.12;
            kind = 1;
        } else if hi.active && !hi.edges.map_or(hi.focus[ai] || hi.focus[bi], |es| es.get(e).copied().unwrap_or(false)) {
            if hi.hide {
                return None;
            }
            alpha *= 0.12;
            kind = 1;
        } else if hi.active {
            alpha = (alpha * 2.5).clamp(0.5, 1.0);
        }
        if cut {
            alpha *= 0.1;
            kind = if kind == 2 { 1 } else { kind.max(1) };
        }
        if stride > 1 && kind != 2 {
            if e % stride != 0 {
                return None;
            }
            alpha = (alpha * boost).min(1.0);
        }
        if let Some(si) = &s.sub_info {
            if si.connector[ai] || si.connector[bi] {
                alpha *= 0.7;
            }
        }
        let c = if kind == 2 { CHANGE_COLOR } else if uniform { base } else { s.style.edge_rgb.as_ref().unwrap()[e] };
        alpha *= depth_k((da + db) * 0.5, 0.5);
        if kind == 2 {
            alpha = 0.95 * depth_k((da + db) * 0.5, 0.25);
        }
        let w = s.style.edge_w.as_ref().map_or(1.0, |w| w[e]);
        let dash = s.style.edge_dash.as_ref().map_or(0, |d| d[e]);
        let de = DrawEdge { a: [pa.x, pa.y], b: [pb.x, pb.y], rgba: [c[0], c[1], c[2], (alpha * 255.0) as u8], target_r: radius[bi], directed: g.edge_directed(e), w, dash };
        Some((de, kind))
    };
    const CHUNK: usize = 8192;
    if g.edges.len() < 2 * CHUNK {
        for (e, &(a, b)) in g.edges.iter().enumerate() {
            if let Some((de, kind)) = edge_one(e, a, b) {
                match kind {
                    2 => out.edges_hi.push(de),
                    1 => out.edges_back.push(de),
                    _ => out.edges.push(de),
                }
            }
        }
    } else {
        use rayon::prelude::*;
        let parts: Vec<[Vec<DrawEdge>; 3]> = g
            .edges
            .par_chunks(CHUNK)
            .enumerate()
            .map(|(ci, chunk)| {
                let mut v: [Vec<DrawEdge>; 3] = [Vec::with_capacity(chunk.len()), Vec::new(), Vec::new()];
                for (k, &(a, b)) in chunk.iter().enumerate() {
                    if let Some((de, kind)) = edge_one(ci * CHUNK + k, a, b) {
                        v[kind as usize].push(de);
                    }
                }
                v
            })
            .collect();
        out.edges.reserve(parts.iter().map(|p| p[0].len()).sum());
        for [normal, faded, hot] in parts {
            out.edges.extend(normal);
            out.edges_back.extend(faded);
            out.edges_hi.extend(hot);
        }
    }

    // links standing in for contracted nodes: dashed, so they read as "a path used to be here"
    {
        let c = base;
        for (list_no, list) in s.bridge_lists().into_iter().enumerate() {
          if list_no == 1 && !s.contract.show_links {
              continue;
          }
          for &(a, b, directed) in list {
            let (ai, bi) = (a as usize, b as usize);
            if !(s.vis[ai] && s.vis[bi]) || (section_hide && (out_of(ai) || out_of(bi))) {
                continue;
            }
            let (Some((pa, _, da)), Some((pb, _, db))) = (pr[ai], pr[bi]) else { continue };
            let mut alpha = (ea * 2.2).clamp(0.25, 0.85) * depth_k((da + db) * 0.5, 0.5);
            if out_of(ai) || out_of(bi) {
                alpha *= 0.1;
            }
            let rgba = [c[0], c[1], c[2], (alpha * 255.0) as u8];
            let d = pb - pa;
            let len = d.length();
            if !len.is_finite() || len < 1.0 {
                continue;
            }
            let dir = d / len;
            let (dash, gap) = (7.0f32, 5.0f32);
            let mut t = 0.0f32;
            let mut guard = 0;
            while t < len && guard < 4000 {
                let e = (t + dash).min(len);
                let (p0, p1) = (pa + dir * t, pa + dir * e);
                out.edges.push(DrawEdge { a: [p0.x, p0.y], b: [p1.x, p1.y], rgba, target_r: 0.0, directed: false, w: 1.0, dash: 0 });
                t += dash + gap;
                guard += 1;
            }
            if directed {
                // a short solid stretch at the target carries the arrow head
                let tail = pa + dir * (len - dash.min(len));
                out.edges.push(DrawEdge { a: [tail.x, tail.y], b: [pb.x, pb.y], rgba, target_r: radius[bi], directed: true, w: 1.0, dash: 0 });
            }
          }
        }
    }

    // nodes (faded first, then normal)
    let mut faded: Vec<DrawNode> = Vec::new();
    // in 3D, solids: lit from the upper left, a little in front of the camera
    let orbit = if let Projector::Orbit(o) = &proj { Some(*o) } else { None };
    let light = orbit.map(|o| {
        let l = [-o.fwd[0] * 0.55 + o.up[0] * 0.7 - o.right[0] * 0.45, -o.fwd[1] * 0.55 + o.up[1] * 0.7 - o.right[1] * 0.45, -o.fwd[2] * 0.55 + o.up[2] * 0.7 - o.right[2] * 0.45];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1e-6);
        [l[0] / n, l[1] / n, l[2] / n]
    });
    // each node's box, when the shapes are to fill them
    let geo_fill = s.geo.is_some() && s.geo_set.show && s.geo_set.place == crate::geometry::Place::FillBox;
    // triangles drawn from OBJ solids this frame: past this, nodes are drawn as plain nodes again
    let mut geo_budget: usize = 1_500_000;
    let fit: Option<Vec<Option<u32>>> = (orbit.is_some() && (s.boxes.fit_shapes || geo_fill) && s.boxes_shown()).then(|| {
        let mut m = vec![None; n];
        for (k, b) in s.box_items.iter().enumerate() {
            if let Some(slot) = m.get_mut(b.node as usize) {
                *slot = Some(k as u32);
            }
        }
        m
    });
    for i in 0..n {
        let visible = s.vis[i];
        if !shown(i) || (section_hide && out_of(i)) {
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
                // joins pieces together, or is only dimmed
                alpha = if ci.node_path[i] { 0.6 } else { 0.12 };
            } else {
                alpha = 0.3;
            }
        } else if !visible {
            alpha = 0.12;
        } else if hi.active && !hi.hood[i] {
            if hi.hide {
                continue;
            }
            alpha = 0.15;
        }
        if s.sub_info.as_ref().is_some_and(|si| si.connector[i]) {
            alpha = alpha.min(0.6);
        }
        if out_of(i) {
            alpha = alpha.min(0.1);
        }
        alpha *= depth_k(depth, 0.35);
        let shape = s.style.node_shape.as_ref().map_or(0, |v| v[i]);
        let rgba = [c[0], c[1], c[2], (alpha * 255.0) as u8];
        let mut solid = Vec::new();
        // selected and highlighted solids take a tint, as they cannot carry a halo
        let tint = |c: [u8; 4], t: Rgb, k: f32| [0, 1, 2].map(|j| (c[j] as f32 * (1.0 - k) + t[j] as f32 * k) as u8);
        let solid_col = if s.sel[i] {
            let t = tint(rgba, [10, 132, 255], 0.45);
            [t[0], t[1], t[2], rgba[3]]
        } else if ring {
            let t = tint(rgba, CHANGE_COLOR, 0.5);
            [t[0], t[1], t[2], rgba[3]]
        } else {
            rgba
        };
        // a solid from the OBJ file
        if let (Some(o), Some(light), Some(lib), Some(xf), true) = (orbit, light, &s.geo, s.coords_xform, s.geo_set.show) {
            if let Some(mesh) = s.geo_node.get(i).copied().flatten().and_then(|k| lib.meshes.get(k as usize)) {
                use crate::geometry::Place;
                // every placement is a scale and a shift per axis
                let scale = [0, 1, 2].map(|a| xf.scale[a] as f32);
                let model = ([0, 1, 2].map(|a| scale[a]), [0, 1, 2].map(|a| (-xf.mid[a] * xf.scale[a]) as f32));
                // at the node: the middle of the solid goes on the node, wherever the file put the solid
                let at_node = |k: f32| {
                    let mul = [0, 1, 2].map(|a| scale[a] * k);
                    (mul, [0, 1, 2].map(|a| s.pos3[i][a] - (mesh.lo[a] + mesh.hi[a]) * 0.5 * mul[a]))
                };
                let boxed = fit.as_ref().and_then(|m| m[i]).map(|k| &s.box_items[k as usize]);
                let (mul, add) = match (s.geo_set.place, boxed) {
                    (Place::Model, _) => model,
                    (Place::FillBox, Some(b)) => {
                        let mul = [0, 1, 2].map(|a| (b.hi[a] - b.lo[a]) / (mesh.hi[a] - mesh.lo[a]).max(1e-9));
                        (mul, [0, 1, 2].map(|a| b.lo[a] - mesh.lo[a] * mul[a]))
                    }
                    _ => at_node(s.geo_set.size),
                };
                let place = |v: [f32; 3]| [0, 1, 2].map(|a| v[a] * mul[a] + add[a]);
                // too small on screen to be worth its triangles: drawn as a plain node
                let (lo, hi) = (place(mesh.lo), place(mesh.hi));
                let centre = [0, 1, 2].map(|a| (lo[a] + hi[a]) * 0.5);
                let reach = (0..3).map(|a| (hi[a] - lo[a]).abs()).fold(0.0f32, f32::max);
                let px = o.project(centre).map_or(0.0, |(_, f, _)| reach * f);
                if px >= 2.0 && mesh.tris.len() <= geo_budget {
                    geo_budget -= mesh.tris.len();
                    let proj3 = |q: [f32; 3]| o.project(q).map(|(v, _, d)| ([v.x, v.y], d));
                    solid = crate::geometry::mesh_on_screen(mesh, place, proj3, o.eye, o.fwd, o.ortho, light, solid_col);
                }
            }
        }
        if let (Some(o), Some(light), true) = (orbit, light, solid.is_empty()) {
            let ns = crate::shapes::NodeShape::from_index(shape);
            let boxed = fit.as_ref().and_then(|m| m[i]).map(|k| &s.box_items[k as usize]).filter(|_| s.boxes.fit_shapes);
            // a node shown at least a few pixels across is drawn solid; one that fills its box always is
            let placed = match boxed {
                Some(b) => Some(crate::shapes::Placed {
                    centre: [(b.lo[0] + b.hi[0]) * 0.5, (b.lo[1] + b.hi[1]) * 0.5, (b.lo[2] + b.hi[2]) * 0.5],
                    half: [((b.hi[0] - b.lo[0]) * 0.5).max(1e-6), ((b.hi[1] - b.lo[1]) * 0.5).max(1e-6), ((b.hi[2] - b.lo[2]) * 0.5).max(1e-6)],
                }),
                // without "Shape by", nodes stay flat circles as before
                None if r >= 4.5 && s.style.node_shape.is_some() => {
                    let w = r / pr[i].map_or(1.0, |q| q.1.max(1e-9));
                    Some(crate::shapes::Placed { centre: s.pos3[i], half: [w, w * ns.depth(), w] })
                }
                None => None,
            };
            if let Some(at) = placed {
                let col = solid_col;
                let proj3 = |q: [f32; 3]| o.project(q).map(|(v, _, d)| ([v.x, v.y], d));
                solid = crate::shapes::solid_on_screen(ns, &at, proj3, o.eye, o.fwd, o.ortho, light, col).into_iter().map(|(t, c, _)| (t, c)).collect();
            }
        }
        let dn = DrawNode { x: p.x, y: p.y, r, rgba, selected: s.sel[i], ring, depth, idx: i as u32, shape, solid };
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

/// Project the bounding boxes of the visible nodes, far ones first.
fn build_boxes(s: &Session, o: &Orbit, size: V2, hi: &Highlight, out: &mut DrawList) {
    let st = &s.boxes;
    let ink = |a: f32, k: f32| (a.clamp(0.0, 1.0) * k.clamp(0.0, 1.0) * 255.0).round() as u8;
    use rayon::prelude::*;
    // every box is projected on its own, so thousands of them are spread over the cores (order is kept); a few
    // hundred are quicker on one thread than the hand-over costs
    let project = |b: &BoxItem| -> Option<DrawBox> {
        let i = b.node as usize;
        if !s.vis[i] || s.section_hides(i) {
            return None;
        }
        // the eight corners; whatever is behind the camera is cut off at the near plane
        let corners: [[f32; 3]; 8] = std::array::from_fn(|k| boxes::corner(b, k));
        let mut faces = [([[0.0f32; 2]; 8], 0usize); 6];
        let mut edges: [Option<([f32; 2], [f32; 2])>; 12] = [None; 12];
        let (mut x0, mut x1, mut y0, mut y1) = (f32::INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::NEG_INFINITY);
        let mut seen = |p: V2| {
            x0 = x0.min(p.x);
            x1 = x1.max(p.x);
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        };
        for (f, face) in boxes::FACES.iter().enumerate() {
            let quad = [corners[face[0]], corners[face[1]], corners[face[2]], corners[face[3]]];
            let (pts, n) = o.clip_polygon(&quad);
            for k in 0..n {
                faces[f].0[k] = [pts[k].x, pts[k].y];
                seen(pts[k]);
            }
            faces[f].1 = n;
        }
        for (e, &(a, c, _, _)) in boxes::EDGES.iter().enumerate() {
            if let Some((p, q)) = o.clip_segment(corners[a], corners[c]) {
                edges[e] = Some(([p.x, p.y], [q.x, q.y]));
                seen(p);
                seen(q);
            }
        }
        if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
            return None; // all of it is behind the camera
        }
        if x0.abs().max(x1.abs()).max(y0.abs()).max(y1.abs()) > 1.0e7 {
            return None; // numbers this large are not safe to draw
        }
        if x1 < 0.0 || y1 < 0.0 || x0 > size.x || y0 > size.y {
            return None;
        }
        let centre = [0, 1, 2].map(|k| (b.lo[k] + b.hi[k]) * 0.5);
        let depth = ((centre[0] - o.eye[0]).powi(2) + (centre[1] - o.eye[1]).powi(2) + (centre[2] - o.eye[2]).powi(2)).sqrt();
        // a box much bigger than the view (a site outline) would tint everything: draw only its outline
        let giant = (x1 - x0) > size.x * 4.0 || (y1 - y0) > size.y * 4.0;
        let selected = s.sel[i];
        let fade = if hi.active && !hi.hood[i] { 0.25 } else { 1.0 } * if s.section_outside_node(i) { 0.1 } else { 1.0 };
        let rgb = match st.color_mode {
            boxes::BoxColor::Single => st.color,
            boxes::BoxColor::Node => s.style.node_rgb[i],
        };
        Some(DrawBox {
            faces,
            edges,
            front: boxes::front_faces(b, o.eye),
            rgb,
            fill: if giant { 0 } else { ink(if selected { (st.fill * 1.8).max(0.12) } else { st.fill }, fade) },
            line: ink(if selected { 1.0 } else { st.outline }, fade),
            width: st.line_width * if selected { 1.8 } else { 1.0 },
            depth,
        })
    };
    if s.box_items.len() >= 1500 {
        // thousands of boxes: projected side by side. Only the (depth, index) pairs are sorted, because the boxes
        // are large; they are copied once, in order
        let made: Vec<DrawBox> = s.box_items.par_iter().filter_map(project).collect();
        let mut order: Vec<(f32, u32)> = made.iter().enumerate().map(|(k, b)| (b.depth, k as u32)).collect();
        order.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        out.boxes.extend(order.iter().map(|&(_, k)| made[k as usize].clone()));
    } else {
        // a few hundred are quicker on one thread, and without the extra copy
        out.boxes.extend(s.box_items.iter().filter_map(project));
        out.boxes.sort_by(|a, b| b.depth.total_cmp(&a.depth)); // far to near
    }
}

/// Index of the visible node under `p` (screen space). In 2D the top-most (last drawn) wins, in 3D the nearest.
pub fn pick_node(s: &Session, cam: &ViewCam, size: V2, p: V2) -> Option<usize> {
    if s.graph_hidden() {
        return None; // nothing is drawn to click on
    }
    let proj = s.projector(cam, size);
    let mut best: Option<(usize, f32, f32)> = None; // index, pixel distance, depth
    for i in 0..s.g.n() {
        if !s.vis[i] || s.section_hides(i) {
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

/// Index of the visible edge closest to `p` (screen space) if it passes within a few pixels of it.
pub fn pick_edge(s: &Session, cam: &ViewCam, size: V2, p: V2) -> Option<usize> {
    if s.graph_hidden() {
        return None;
    }
    let proj = s.projector(cam, size);
    let reach = 5.0f32.max(s.look.edge_width * 0.5 + 3.0);
    let mut best: Option<(usize, f32, f32)> = None; // index, pixel distance, depth
    for (e, &(a, b)) in s.g.edges.iter().enumerate() {
        let (a, b) = (a as usize, b as usize);
        if !s.vis[a] || !s.vis[b] || !s.edge_passes(e) || s.section_hides(a) || s.section_hides(b) {
            continue;
        }
        let (Some((pa, _, da)), Some((pb, _, db))) = (proj.at(s, a), proj.at(s, b)) else { continue };
        let d = pb - pa;
        let len2 = d.length_sq();
        let t = if len2 < 1e-6 { 0.0 } else { (((p - pa).x * d.x + (p - pa).y * d.y) / len2).clamp(0.0, 1.0) };
        let dist = (pa + d * t - p).length();
        if dist > reach {
            continue;
        }
        let depth = da + (db - da) * t;
        let better = match best {
            None => true,
            Some((_, bd, bdepth)) => {
                if matches!(proj, Projector::Orbit(_)) { depth < bdepth - 1e-3 || ((depth - bdepth).abs() <= 1e-3 && dist < bd) } else { dist < bd }
            }
        };
        if better {
            best = Some((e, dist, depth));
        }
    }
    best.map(|(e, _, _)| e)
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
        let hi = Highlight { focus: &none, hood: &none, active: false, edges: None, hide: false };
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.nodes.len(), 6);
        assert_eq!(dl.edges.len() + dl.edges_back.len(), 5);
        s.filter.search = "a".into();
        s.recompute_visibility();
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.nodes.len(), 6, "filtered-out nodes are dimmed, not removed (the default)");
        s.look.filtered = FilteredMode::Hide;
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
