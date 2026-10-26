//! Work sessions: everything needed to come back to exactly the same view later.
//!
//! A session file (`.graphsight`, JSON; `.grapht` from before the rename still opens) stores which files were loaded and how their columns were mapped,
//! the appearance / layout / filter settings, node positions and pins, the selection, hand-hidden nodes,
//! the table sort and the camera. The data itself is re-read from the original files.

use crate::app::{App, PendKindHint, SAMPLE_EDGES, SAMPLE_NODES};
use crate::data::*;
use crate::layout::LayoutSettings;
use crate::model::*;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "graphsight";
/// Sessions saved while the app was called Grapht; they still open.
pub const OLD_EXTENSION: &str = "grapht";

/// Is this a session file (new or old extension)?
pub fn is_session(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION) || e.eq_ignore_ascii_case(OLD_EXTENSION))
}
const VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct SrcRef<M> {
    pub name: String,
    /// Absolute path at save time (None for the built-in sample).
    pub path: Option<String>,
    /// Path relative to the session file, tried first so a project folder can be moved or shared.
    pub rel: Option<String>,
    pub sample: bool,
    /// Both tables came from one JSON file.
    pub json: bool,
    pub mapping: M,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectFile {
    pub app: String,
    pub version: u32,
    pub edges: Option<SrcRef<EdgeMapping>>,
    pub nodes: Option<SrcRef<NodeMapping>>,
    pub look: Look,
    /// Edge attributes made from both ends (recipes).
    pub edge_attrs: Vec<crate::edgeattr::EdgeAttr>,
    pub layout: LayoutSettings,
    pub filter: FilterState,
    pub change: crate::changes::ChangeSettings,
    pub sub: crate::subgraph::SubSettings,
    pub contract: crate::subgraph::ContractSettings,
    pub query: crate::query::QuerySettings,
    pub coords: crate::coords::CoordSettings,
    pub boxes: crate::boxes::BoxSettings,
    pub section: crate::section::SectionSettings,
    pub cam3: crate::coords::Cam3,
    pub cam: ViewCam,
    pub auto_fit: bool,
    pub positions: Vec<(String, f32, f32)>,
    pub pinned: Vec<String>,
    pub selected: Vec<String>,
    pub hidden: Vec<String>,
    pub table_sort: Option<(String, bool)>,
    pub top_metric: Option<String>,
    pub pin_after_drag: bool,
}

impl Default for ProjectFile {
    fn default() -> Self {
        ProjectFile {
            app: "Graphsight".into(),
            version: VERSION,
            edges: None,
            nodes: None,
            look: Look::default(),
            edge_attrs: vec![],
            layout: LayoutSettings::default(),
            filter: FilterState::default(),
            change: Default::default(),
            sub: Default::default(),
            contract: Default::default(),
            query: Default::default(),
            coords: Default::default(),
            boxes: Default::default(),
            section: Default::default(),
            cam3: Default::default(),
            cam: ViewCam::default(),
            auto_fit: true,
            positions: vec![],
            pinned: vec![],
            selected: vec![],
            hidden: vec![],
            table_sort: None,
            top_metric: None,
            pin_after_drag: false,
        }
    }
}

fn relative_to(path: &Path, base: &Path) -> Option<String> {
    path.strip_prefix(base).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn make_ref<M: Clone>(src: &TableSrc<M>, base: Option<&Path>, json: bool) -> SrcRef<M> {
    let sample = src.path.is_none() && src.name.starts_with("sample ");
    SrcRef {
        name: src.name.clone(),
        path: src.path.as_ref().map(|p| p.to_string_lossy().to_string()),
        rel: match (&src.path, base) {
            (Some(p), Some(b)) => relative_to(p, b),
            _ => None,
        },
        sample,
        json,
        mapping: src.mapping.clone(),
    }
}

fn resolve(r_path: &Option<String>, rel: &Option<String>, base: &Path) -> Option<PathBuf> {
    if let Some(rel) = rel {
        let p = base.join(rel);
        if p.exists() {
            return Some(p);
        }
    }
    r_path.as_ref().map(PathBuf::from).filter(|p| p.exists())
}

fn is_json(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("geojson"))
}

impl App {
    pub fn project_title(&self) -> String {
        match &self.project {
            Some(p) => format!("{} — Graphsight", p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()),
            None => "Graphsight".to_string(),
        }
    }

    pub fn build_project(&self, base: Option<&Path>) -> ProjectFile {
        let s = &self.s;
        let n = s.g.n();
        let same_json = match (&s.edge_src, &s.node_src) {
            (Some(e), Some(nn)) => e.path.is_some() && e.path == nn.path && e.path.as_deref().is_some_and(is_json),
            _ => false,
        };
        let edges_json = s.edge_src.as_ref().and_then(|e| e.path.as_deref()).is_some_and(is_json);
        let ids = &s.g.ids;
        let listed = |flags: &[bool]| -> Vec<String> { (0..n.min(flags.len())).filter(|&i| flags[i]).map(|i| ids[i].clone()).collect() };
        ProjectFile {
            edges: s.edge_src.as_ref().map(|e| make_ref(e, base, edges_json)),
            nodes: s.node_src.as_ref().map(|nn| make_ref(nn, base, same_json)),
            look: s.look.clone(),
            edge_attrs: s.edge_attrs.clone(),
            layout: s.layout.clone(),
            filter: s.filter.clone(),
            change: s.change.clone(),
            sub: s.sub.clone(),
            contract: s.contract.clone(),
            query: s.query.clone(),
            coords: s.coords.clone(),
            boxes: s.boxes.clone(),
            section: s.section.clone(),
            cam3: s.cam3,
            cam: self.cam,
            auto_fit: self.auto_fit,
            positions: (0..n).map(|i| (ids[i].clone(), s.pos[i].x, s.pos[i].y)).collect(),
            pinned: listed(&s.sim.pinned),
            selected: listed(&s.sel),
            hidden: listed(&s.manual_hidden),
            table_sort: self.table.sort.clone(),
            top_metric: self.top_metric.clone(),
            pin_after_drag: self.pin_after_drag,
            ..Default::default()
        }
    }

    pub fn save_project_to(&mut self, path: &Path) -> Result<()> {
        let mut path = path.to_path_buf();
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
        }
        let base = path.parent().map(|p| p.to_path_buf());
        let pf = self.build_project(base.as_deref());
        let text = serde_json::to_string_pretty(&pf)?;
        let tmp = path.with_extension("graphsight.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("saving {}", path.display()))?;
        self.project = Some(path.clone());
        self.settings.push_recent(&path);
        Ok(())
    }

    pub fn save_project(&mut self) -> Result<bool> {
        match self.project.clone() {
            Some(p) => self.save_project_to(&p).map(|_| true),
            None => self.save_project_as(),
        }
    }

    /// Returns false if the user cancelled the dialog.
    pub fn save_project_as(&mut self) -> Result<bool> {
        let mut d = rfd::FileDialog::new().add_filter("Graphsight session", &[EXTENSION]).set_file_name("session.graphsight");
        if let Some(dir) = self.project.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        match d.save_file() {
            Some(p) => self.save_project_to(&p).map(|_| true),
            None => Ok(false),
        }
    }

    pub fn open_project_dialog(&mut self) {
        let mut d = rfd::FileDialog::new().add_filter("Graphsight session", &[EXTENSION, OLD_EXTENSION]);
        if let Some(dir) = self.project.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_file() {
            self.open_project(&p);
        }
    }

    pub fn new_project(&mut self) {
        self.s.edge_attrs.clear();
        self.s.clear_all();
        self.s.look = Look::default();
        self.s.layout = LayoutSettings::default();
        self.s.filter = FilterState::default();
        self.s.change = Default::default();
        self.s.sub = Default::default();
        self.s.contract = Default::default();
        self.s.query = Default::default();
        self.s.coords = Default::default();
        self.s.boxes = Default::default();
        self.s.section = Default::default();
        self.table.sort = None;
        self.project = None;
        self.after_graph_change();
    }

    /// Open a session file, reporting problems through the error dialog.
    pub fn open_project(&mut self, path: &Path) {
        match self.try_open_project(path) {
            Ok(warnings) => {
                if !warnings.is_empty() {
                    self.error = Some(warnings.join("\n"));
                }
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn try_open_project(&mut self, path: &Path) -> Result<Vec<String>> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let pf: ProjectFile = serde_json::from_str(&text).with_context(|| format!("{} is not a valid Graphsight session", path.display()))?;
        if pf.version > VERSION {
            bail!("this session was saved by a newer version of Graphsight (format {}).", pf.version);
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut warnings = Vec::new();

        self.s.clear_all();
        // ── data ──
        let mut json_done = false;
        if let Some(e) = &pf.edges {
            if e.sample {
                if let Ok(t) = parse_csv_text(SAMPLE_EDGES) {
                    let m = valid_edge_mapping(&e.mapping, &t);
                    self.s.set_edges(e.name.clone(), t, m);
                }
            } else if let Some(p) = resolve(&e.path, &e.rel, base) {
                if is_json(&p) {
                    match self.s.load_json(&p) {
                        Ok(()) => {
                            json_done = true;
                            if let Some(src) = &mut self.s.edge_src {
                                src.mapping.directed = e.mapping.directed;
                            }
                        }
                        Err(err) => warnings.push(format!("{}: {err:#}", p.display())),
                    }
                } else {
                    match read_csv(&p) {
                        Ok(t) => {
                            let m = valid_edge_mapping(&e.mapping, &t);
                            self.s.set_edges(e.name.clone(), t, m);
                            if let Some(src) = &mut self.s.edge_src {
                                src.path = Some(p);
                            }
                        }
                        Err(err) => warnings.push(format!("{}: {err:#}", p.display())),
                    }
                }
            } else {
                warnings.push(format!("Edges file not found: {}", e.path.as_deref().unwrap_or(&e.name)));
            }
        }
        if let Some(nn) = &pf.nodes {
            if json_done && nn.json {
                // already loaded together with the edges
            } else if nn.sample {
                if let Ok(t) = parse_csv_text(SAMPLE_NODES) {
                    let m = valid_node_mapping(&nn.mapping, &t);
                    self.s.set_nodes(nn.name.clone(), t, m);
                }
            } else if let Some(p) = resolve(&nn.path, &nn.rel, base) {
                match read_csv(&p) {
                    Ok(t) => {
                        let m = valid_node_mapping(&nn.mapping, &t);
                        self.s.set_nodes(nn.name.clone(), t, m);
                        if let Some(src) = &mut self.s.node_src {
                            src.path = Some(p);
                        }
                    }
                    Err(err) => warnings.push(format!("{}: {err:#}", p.display())),
                }
            } else {
                warnings.push(format!("Nodes file not found: {}", nn.path.as_deref().unwrap_or(&nn.name)));
            }
        }

        // ── settings, then the graph ──
        self.s.look = pf.look.clone();
        self.s.edge_attrs = pf.edge_attrs.clone();
        self.s.layout = pf.layout.clone();
        self.s.filter = pf.filter.clone();
        self.s.change = pf.change.clone();
        self.s.sub = pf.sub.clone();
        self.s.contract = pf.contract.clone();
        self.s.query = pf.query.clone();
        self.s.coords = pf.coords.clone();
        self.s.boxes = pf.boxes.clone();
        self.s.section = pf.section.clone();
        self.rebuild();
        self.s.look = pf.look.clone();
        self.s.restore_state(&pf.positions, &pf.pinned, &pf.selected, &pf.hidden);
        self.s.drop_missing_columns();
        self.s.recompute_boxes();
        self.s.recompute_visibility();
        self.s.recompute_style();
        if pf.coords.enabled && self.s.coords_active() {
            self.s.cam3 = pf.cam3;
        }
        self.cam = pf.cam;
        self.auto_fit = pf.auto_fit;
        self.table.sort = pf.table_sort.clone();
        self.table.dirty = true;
        self.top_metric = pf.top_metric.clone();
        self.pin_after_drag = pf.pin_after_drag;
        self.sim_paused = false;
        self.project = Some(path.to_path_buf());
        self.settings.push_recent(path);
        let _ = PendKindHint::Edges; // keep the import alive for readers of this module
        Ok(warnings)
    }
}

fn valid_edge_mapping(m: &EdgeMapping, t: &RawTable) -> EdgeMapping {
    let n = t.headers.len();
    if m.src < n && m.dst < n && m.weight.is_none_or(|w| w < n) && m.dir_col.is_none_or(|d| d < n) { m.clone() } else { auto_edge_mapping(t) }
}

fn valid_node_mapping(m: &NodeMapping, t: &RawTable) -> NodeMapping {
    if m.id < t.headers.len() { m.clone() } else { auto_node_mapping(t) }
}
