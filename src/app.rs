//! Application shell: menus, file loading, dialogs, export. Panels, canvas and table are in sibling modules.

use crate::data::*;
use crate::model::*;
use crate::render;
use egui::{Context, Pos2, Vec2};
use std::path::PathBuf;

pub const SAMPLE_NODES: &str = include_str!("../sample/nodes.csv");
pub const SAMPLE_EDGES: &str = include_str!("../sample/edges.csv");

pub enum PendKind {
    Edges(EdgeMapping),
    Nodes(NodeMapping),
}

pub struct Pending {
    pub name: String,
    pub path: Option<PathBuf>,
    pub table: RawTable,
    pub kind: PendKind,
}

pub struct Textures {
    pub small: egui::TextureHandle,
    pub mid: egui::TextureHandle,
    pub big: egui::TextureHandle,
}

#[derive(Default)]
pub struct TableState {
    pub sort: Option<(String, bool)>, // column name ("id" for the id column), ascending?
    pub order: Vec<u32>,
    pub dirty: bool,
}

pub struct App {
    pub s: Session,
    pub cam: ViewCam,
    pub auto_fit: bool,
    pub canvas_size: Vec2,
    pub canvas_rect: egui::Rect,
    pub view: crate::view::ViewCtl,
    pub draw: DrawList,
    pub tex: Textures,
    pub focus: Vec<bool>,
    pub hood: Vec<bool>,
    pub hi_key: (Option<usize>, u64, bool),
    pub hi_active: bool,
    pub hover: Option<usize>,
    pub drag_node: Option<usize>,
    pub box_start: Option<Pos2>,
    pub pin_after_drag: bool,
    pub pending: Option<Pending>,
    pub error: Option<String>,
    pub toast: Option<(String, f64)>,
    pub dock: Option<egui_dock::DockState<crate::dock::Tab>>,
    pub project: Option<PathBuf>,
    pub settings: crate::config::Settings,
    /// Only the real application persists its layout; tests and headless runs must not touch the user's files.
    pub persist: bool,
    pub layout_text: String,
    pub layout_check: f64,
    pub last_title: String,
    pub show_help: bool,
    pub table: TableState,
    pub top_metric: Option<String>,
    pub sim_paused: bool,
    pub frame_ms: f32,
    pub prof: crate::prof::Prof,
}

fn make_disc(size: usize) -> egui::ColorImage {
    let rho = size as f32 / 2.0 - 1.5;
    let mut px = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - size as f32 / 2.0, y as f32 + 0.5 - size as f32 / 2.0);
            let d = (dx * dx + dy * dy).sqrt();
            let a = (rho - d + 0.5).clamp(0.0, 1.0);
            px.push(egui::Color32::from_white_alpha((a * 255.0) as u8));
        }
    }
    egui::ColorImage::new([size, size], px)
}

pub struct Args {
    pub project: Option<PathBuf>,
    pub edges: Option<PathBuf>,
    pub nodes: Option<PathBuf>,
    pub json: Option<PathBuf>,
    pub directed: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let mut app = Self::new_with_ctx(&cc.egui_ctx, args);
        app.persist = true;
        app.settings = crate::config::Settings::load();
        if let Some(d) = crate::config::load_layout() {
            app.dock = Some(d);
        }
        app.layout_text = app.dock.as_ref().and_then(crate::config::layout_to_string).unwrap_or_default();
        app
    }

    pub fn new_with_ctx(ctx: &Context, args: Args) -> Self {
        crate::theme::apply(ctx);
        let opts = egui::TextureOptions::LINEAR;
        let tex = Textures {
            small: ctx.load_texture("disc16", make_disc(16), opts),
            mid: ctx.load_texture("disc64", make_disc(64), opts),
            big: ctx.load_texture("disc256", make_disc(256), opts),
        };
        let mut app = App {
            s: Session::new(),
            cam: ViewCam::default(),
            auto_fit: true,
            canvas_size: Vec2::new(1000.0, 700.0),
            canvas_rect: egui::Rect::NOTHING,
            view: Default::default(),
            draw: DrawList::default(),
            tex,
            focus: vec![],
            hood: vec![],
            hi_key: (None, u64::MAX, false),
            hi_active: false,
            hover: None,
            drag_node: None,
            box_start: None,
            pin_after_drag: false,
            pending: None,
            error: None,
            toast: None,
            dock: Some(crate::dock::default_dock()),
            project: None,
            settings: crate::config::Settings::default(),
            persist: false,
            layout_text: String::new(),
            layout_check: 0.0,
            last_title: String::new(),
            show_help: false,
            table: TableState { dirty: true, ..Default::default() },
            top_metric: Some("_pagerank".into()),
            sim_paused: false,
            frame_ms: 0.0,
            prof: Default::default(),
        };
        if let Some(p) = args.project {
            app.open_project(&p);
        }
        if let Some(j) = args.json {
            app.load_path(j, None);
        }
        if let Some(n) = args.nodes {
            app.load_path(n, Some(PendKindHint::Nodes));
        }
        if let Some(e) = args.edges {
            app.load_path(e, Some(PendKindHint::Edges));
        }
        if args.directed {
            if let Some(src) = &mut app.s.edge_src {
                src.mapping.directed = true;
                app.rebuild();
            }
        }
        app
    }

    // ── state changes that must refresh caches ──

    pub fn rebuild(&mut self) {
        self.s.rebuild();
        self.after_graph_change();
    }

    pub fn after_graph_change(&mut self) {
        self.table.dirty = true;
        self.hi_key = (None, u64::MAX, false);
        self.hover = None;
        self.drag_node = None;
        self.auto_fit = true;
        self.view.anim = None;
        self.view.hist.clear();
        self.view.cancel_region();
    }

    pub fn refilter(&mut self) {
        self.s.recompute_visibility();
        self.table.dirty = true;
    }

    pub fn restyle(&mut self) {
        self.s.recompute_style();
    }

    pub fn toast(&mut self, ctx: &Context, msg: impl Into<String>) {
        let t = ctx.input(|i| i.time) + 4.5;
        self.toast = Some((msg.into(), t));
    }

    // ── loading ──

    pub fn load_path(&mut self, path: PathBuf, hint: Option<PendKindHint>) {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext == "json" || ext == "geojson" {
            match self.s.load_json(&path) {
                Ok(()) => self.rebuild(),
                Err(e) => self.error = Some(format!("{e:#}")),
            }
            return;
        }
        let table = match read_csv(&path) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return;
            }
        };
        let kind = hint.unwrap_or(if looks_like_edges(&table) { PendKindHint::Edges } else { PendKindHint::Nodes });
        let confident = match kind {
            PendKindHint::Edges => looks_like_edges(&table),
            PendKindHint::Nodes => table.headers.iter().any(|h| ["id", "node", "name", "key", "node_id", "nodeid", "label", "vertex"].contains(&h.to_lowercase().as_str())),
        };
        let pend = Pending {
            name,
            path: Some(path.clone()),
            kind: match kind {
                PendKindHint::Edges => PendKind::Edges(auto_edge_mapping(&table)),
                PendKindHint::Nodes => PendKind::Nodes(auto_node_mapping(&table)),
            },
            table,
        };
        if confident {
            self.commit_import(pend);
        } else {
            self.pending = Some(pend);
        }
    }

    pub fn commit_import(&mut self, p: Pending) {
        match p.kind {
            PendKind::Edges(mut m) => {
                let path = p.path.clone();
                // keep the user's directed choice when re-importing
                if let Some(old) = &self.s.edge_src {
                    m.directed = old.mapping.directed;
                }
                self.s.set_edges(p.name, p.table, m);
                if let Some(src) = &mut self.s.edge_src {
                    src.path = path;
                }
            }
            PendKind::Nodes(m) => {
                let path = p.path.clone();
                self.s.set_nodes(p.name, p.table, m);
                if let Some(src) = &mut self.s.node_src {
                    src.path = path;
                }
            }
        }
        self.rebuild();
    }

    pub fn load_sample(&mut self) {
        let (Ok(e), Ok(n)) = (parse_csv_text(SAMPLE_EDGES), parse_csv_text(SAMPLE_NODES)) else { return };
        self.s.clear_all();
        let em = auto_edge_mapping(&e);
        self.s.set_edges("sample edges.csv".into(), e, em);
        let nm = auto_node_mapping(&n);
        self.s.set_nodes("sample nodes.csv".into(), n, nm);
        self.s.look.size_by = Some("_degree".into());
        self.rebuild();
    }

    pub fn pick_and_load(&mut self, hint: Option<PendKindHint>) {
        let mut d = rfd::FileDialog::new().add_filter("Tables and graphs", &["csv", "tsv", "txt", "json"]);
        if matches!(hint, Some(PendKindHint::Edges)) {
            d = d.set_title("Open edges file (source, target, …)");
        } else if matches!(hint, Some(PendKindHint::Nodes)) {
            d = d.set_title("Open nodes file (id, attributes…)");
        }
        if let Some(p) = d.pick_file() {
            self.load_path(p, hint);
        }
    }

    pub fn handle_drops(&mut self, ctx: &Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            self.load_path(f.path().to_path_buf(), None);
        }
    }

    // ── export ──

    pub fn export_png(&mut self, ctx: &Context, whole: bool) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("graph.png").save_file() else { return };
        let (cam, w, h, scale) = if whole {
            let size = Vec2::new(1920.0, 1080.0);
            (self.s.fit_camera(size), 3840, 2160, 2.0)
        } else {
            let sz = self.canvas_size;
            (self.cam, (sz.x * 2.0) as u32, (sz.y * 2.0) as u32, 2.0)
        };
        match render::render_png(&self.s, &cam, w, h, scale) {
            Some(pm) => match pm.save_png(&path) {
                Ok(()) => self.toast(ctx, format!("Saved {}", path.display())),
                Err(e) => self.error = Some(format!("Could not save PNG: {e}")),
            },
            None => self.error = Some("Could not allocate the image".into()),
        }
    }

    pub fn export_svg(&mut self, ctx: &Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("SVG image", &["svg"]).set_file_name("graph.svg").save_file() else { return };
        let size = Vec2::new(1600.0, 1000.0);
        let svg = render::render_svg(&self.s, &self.s.fit_camera(size), size.x, size.y);
        match std::fs::write(&path, svg) {
            Ok(()) => self.toast(ctx, format!("Saved {}", path.display())),
            Err(e) => self.error = Some(format!("Could not save SVG: {e}")),
        }
    }

    pub fn export_visible_csv(&mut self, ctx: &Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("nodes_visible.csv").save_file() else { return };
        let res = (|| -> anyhow::Result<()> {
            let mut w = csv::Writer::from_path(&path)?;
            let mut head = vec!["id".to_string()];
            head.extend(self.s.g.node_cols.iter().map(|c| c.name.clone()));
            w.write_record(&head)?;
            for &i in &self.table.order {
                let i = i as usize;
                let mut row = vec![self.s.g.ids[i].clone()];
                row.extend(self.s.g.node_cols.iter().map(|c| c.label_at(i).unwrap_or_default()));
                w.write_record(&row)?;
            }
            w.flush()?;
            Ok(())
        })();
        match res {
            Ok(()) => self.toast(ctx, format!("Saved {}", path.display())),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    // ── layout persistence, shortcuts, title ──

    pub fn save_layout_now(&mut self) {
        if !self.persist {
            return;
        }
        if let Some(text) = self.dock.as_ref().and_then(crate::config::layout_to_string) {
            if text != self.layout_text {
                crate::config::save_layout_text(&text);
                self.layout_text = text;
            }
        }
    }

    /// Writes the panel arrangement to disk shortly after it changes.
    fn persist_layout(&mut self, ctx: &Context) {
        if !self.persist {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now - self.layout_check > 1.0 {
            self.layout_check = now;
            self.save_layout_now();
        }
    }

    fn update_window_title(&mut self, ctx: &Context) {
        let title = self.project_title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let save_as = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let (do_save_as, do_save, do_open) = ctx.input_mut(|i| (i.consume_shortcut(&save_as), i.consume_shortcut(&save), i.consume_shortcut(&open)));
        if do_save_as {
            self.report(ctx, |a| a.save_project_as(), "Session saved");
        } else if do_save {
            self.report(ctx, |a| a.save_project(), "Session saved");
        } else if do_open {
            self.open_project_dialog();
        }
    }

    fn report(&mut self, ctx: &Context, f: impl FnOnce(&mut App) -> anyhow::Result<bool>, done: &str) {
        match f(self) {
            Ok(true) => self.toast(ctx, done),
            Ok(false) => {}
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    // ── menus and dialogs ──

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New session")).clicked() {
                    ui.close();
                    self.new_project();
                }
                if ui.add(egui::Button::new("Open session…").shortcut_text(ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O)))).clicked() {
                    ui.close();
                    self.open_project_dialog();
                }
                ui.add_enabled_ui(!self.settings.recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        let recent = self.settings.recent.clone();
                        for r in recent {
                            let p = std::path::PathBuf::from(&r);
                            let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.clone());
                            if ui.button(label).on_hover_text(&r).clicked() {
                                ui.close();
                                self.open_project(&p);
                            }
                        }
                    });
                });
                if ui.add(egui::Button::new("Save session").shortcut_text(ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S)))).clicked() {
                    ui.close();
                    self.report(&ctx, |a| a.save_project(), "Session saved");
                }
                if ui.add(egui::Button::new("Save session as…").shortcut_text(ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S)))).clicked() {
                    ui.close();
                    self.report(&ctx, |a| a.save_project_as(), "Session saved");
                }
                ui.separator();
                if ui.button("Open edges file…").clicked() {
                    ui.close();
                    self.pick_and_load(Some(PendKindHint::Edges));
                }
                if ui.button("Open nodes file…").clicked() {
                    ui.close();
                    self.pick_and_load(Some(PendKindHint::Nodes));
                }
                if ui.button("Open JSON graph…").clicked() {
                    ui.close();
                    if let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
                        self.load_path(p, None);
                    }
                }
                ui.separator();
                if ui.button("Load sample dataset").clicked() {
                    ui.close();
                    self.load_sample();
                }
                if ui.button("Clear everything").clicked() {
                    ui.close();
                    self.s.clear_all();
                    self.after_graph_change();
                }
                ui.separator();
                if ui.button("Export PNG — current view…").clicked() {
                    ui.close();
                    self.export_png(&ctx, false);
                }
                if ui.button("Export PNG — whole graph (4K)…").clicked() {
                    ui.close();
                    self.export_png(&ctx, true);
                }
                if ui.button("Export SVG…").clicked() {
                    ui.close();
                    self.export_svg(&ctx);
                }
                if ui.button("Export visible nodes as CSV…").clicked() {
                    ui.close();
                    self.export_visible_csv(&ctx);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("View", |ui| {
                ui.label(crate::theme::secondary("Panels"));
                let mut dock = self.dock.take().unwrap_or_else(crate::dock::default_dock);
                for tab in crate::dock::Tab::ALL.into_iter().filter(|t| *t != crate::dock::Tab::Canvas) {
                    let mut open = crate::dock::is_open(&dock, tab);
                    if ui.checkbox(&mut open, tab.title()).changed() {
                        if open {
                            crate::dock::open_or_focus(&mut dock, tab);
                        } else {
                            crate::dock::close(&mut dock, tab);
                        }
                    }
                }
                if ui.button("Reset panel layout").clicked() {
                    ui.close();
                    dock = crate::dock::default_dock();
                }
                self.dock = Some(dock);
                ui.separator();
                if ui.checkbox(&mut self.s.look.dark, "Dark graph background").changed() {
                    self.restyle();
                }
                ui.separator();
                ui.checkbox(&mut self.settings.animate_view, "Animate view changes");
                if ui.button("Fit graph to window   (F)").clicked() {
                    ui.close();
                    self.run_tool(crate::view::Tool::Fit);
                }
                if ui.add_enabled(self.tool_enabled(crate::view::Tool::Selection), egui::Button::new("Zoom to selection   (S)")).clicked() {
                    ui.close();
                    self.run_tool(crate::view::Tool::Selection);
                }
                if ui.button("Zoom to region   (R)").clicked() {
                    ui.close();
                    self.run_tool(crate::view::Tool::Region);
                }
                if ui.button("Reset view   (Home)").clicked() {
                    ui.close();
                    self.run_tool(crate::view::Tool::Home);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Controls and filter syntax").clicked() {
                    ui.close();
                    self.show_help = true;
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format!("{:.1} ms / frame (CPU)", self.frame_ms));
            });
        });
    }

    fn dialogs(&mut self, ctx: &Context) {
        // import / remap dialog
        let mut commit = false;
        let mut cancel = false;
        if let Some(p) = &mut self.pending {
            let title = match p.kind {
                PendKind::Edges(_) => format!("Import edges — {}", p.name),
                PendKind::Nodes(_) => format!("Import nodes — {}", p.name),
            };
            egui::Window::new(title).collapsible(false).resizable(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("{} rows, {} columns", p.table.rows.len(), p.table.headers.len()));
                egui::ScrollArea::horizontal().max_height(150.0).show(ui, |ui| {
                    egui::Grid::new("preview").striped(true).show(ui, |ui| {
                        for h in &p.table.headers {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for r in p.table.rows.iter().take(6) {
                            for c in r {
                                ui.label(c);
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                let headers = p.table.headers.clone();
                let pick = |ui: &mut egui::Ui, id: &str, label: &str, v: &mut usize| {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(id).selected_text(headers[*v].clone()).show_ui(ui, |ui| {
                            for (i, h) in headers.iter().enumerate() {
                                ui.selectable_value(v, i, h);
                            }
                        });
                    });
                };
                match &mut p.kind {
                    PendKind::Edges(m) => {
                        pick(ui, "m_src", "Source column", &mut m.src);
                        pick(ui, "m_dst", "Target column", &mut m.dst);
                        ui.horizontal(|ui| {
                            ui.label("Weight column");
                            let cur = m.weight.map(|w| headers[w].clone()).unwrap_or_else(|| "(none)".into());
                            egui::ComboBox::from_id_salt("m_w").selected_text(cur).show_ui(ui, |ui| {
                                ui.selectable_value(&mut m.weight, None, "(none)");
                                for (i, h) in headers.iter().enumerate() {
                                    ui.selectable_value(&mut m.weight, Some(i), h);
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Direction column");
                            let cur = m.dir_col.map(|w| headers[w].clone()).unwrap_or_else(|| "(none)".into());
                            egui::ComboBox::from_id_salt("m_dir").selected_text(cur).show_ui(ui, |ui| {
                                ui.selectable_value(&mut m.dir_col, None, "(none)");
                                for (i, h) in headers.iter().enumerate() {
                                    ui.selectable_value(&mut m.dir_col, Some(i), h);
                                }
                            });
                        });
                        if m.dir_col.is_some() {
                            ui.weak("Each edge's own value (directed / undirected) decides. The switch below is used for rows without a recognised value.");
                        }
                        ui.checkbox(&mut m.directed, if m.dir_col.is_some() { "Directed when the column is empty or unknown" } else { "Directed edges (source → target)" });
                    }
                    PendKind::Nodes(m) => pick(ui, "m_id", "ID column", &mut m.id),
                }
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        }
        if commit {
            if let Some(p) = self.pending.take() {
                // a remap keeps the directed flag chosen in the dialog
                let directed = if let PendKind::Edges(m) = &p.kind { Some(m.directed) } else { None };
                self.commit_import(p);
                if let (Some(d), Some(src)) = (directed, &mut self.s.edge_src) {
                    if src.mapping.directed != d {
                        src.mapping.directed = d;
                        self.rebuild();
                    }
                }
            }
        }
        if cancel {
            self.pending = None;
        }

        // errors
        let mut close_err = false;
        if let Some(e) = &self.error {
            egui::Window::new("Problem").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(e);
                if ui.button("OK").clicked() {
                    close_err = true;
                }
            });
        }
        if close_err {
            self.error = None;
        }

        // help
        let mut open = self.show_help;
        egui::Window::new("Controls and filter syntax").open(&mut open).resizable(true).default_width(430.0).show(ctx, |ui| {
            ui.strong("Canvas");
            ui.label("• Drag empty space: pan     • Mouse wheel / pinch: zoom to cursor");
            ui.label("• Click a node: select (Ctrl/Shift+click adds)     • Double-click empty space: fit view");
            ui.label("• Drag a node to move it (all selected nodes move together)");
            ui.label("• Shift + drag on empty space: box-select (Ctrl keeps the current selection)");
            ui.label("• Double-click a node: select it and its neighbours");
            ui.label("• Middle- or right-drag, or hold Space and drag: pan (3D: also orbit / pan / zoom drag modes in the toolbar)");
            ui.label("• Esc clear selection · Del hide selected · Ctrl+A select visible · Space (tap) pause/resume layout");
            ui.add_space(6.0);
            ui.strong("View toolbar (bottom of the graph)");
            ui.label("• F fit all visible nodes · S zoom to selection · R zoom to region (drag a rectangle; Esc cancels)");
            ui.label("• Zoom in / out · Home reset view · Alt+← / Alt+→ previous / next view");
            ui.label("• 3D: Top / Front / Side / Iso views, perspective or orthographic projection");
            ui.label("• View ▸ Animate view changes turns the camera glide on or off");
            ui.add_space(6.0);
            ui.strong("Filter expressions");
            ui.monospace("age > 30 and (group == \"A\" or group in [\"B\", \"C\"])");
            ui.monospace("not name contains \"test\"     score >= 2.5     remote == true");
            ui.monospace("`first name` startswith \"An\"     degree > 10     score == null");
            ui.label("Operators: == != < <= > >=, contains / startswith (case-insensitive), in [a, b], and / or / not, parentheses. Use `backticks` around column names with spaces. `id` is the node id; computed metrics can be written without the leading underscore (degree, pagerank, community …).");
        });
        self.show_help = open;

        // toast
        if let Some((msg, until)) = &self.toast {
            if ctx.input(|i| i.time) > *until {
                self.toast = None;
            } else {
                let msg = msg.clone();
                egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -36.0]).order(egui::Order::Tooltip).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PendKindHint {
    Edges,
    Nodes,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_inner(ui);
    }

    fn on_exit(&mut self) {
        self.save_layout_now();
    }
}

impl App {
    pub fn ui_inner(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let t0 = std::time::Instant::now();
        self.prof.frame_start();
        self.handle_drops(&ctx);
        self.handle_shortcuts(&ctx);
        self.update_window_title(&ctx);

        egui::Panel::top("menu").frame(egui::Frame::new().fill(crate::theme::BG).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| self.menu_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::theme::BG)).show(ui, |ui| self.show_dock(ui));

        self.persist_layout(&ctx);
        self.dialogs(&ctx);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.frame_ms = self.frame_ms * 0.9 + ms * 0.1;
        self.prof.add(4, t0);
        self.prof.frame_end(self.s.g.n(), self.s.g.edges.len(), self.draw.edges.len() + self.draw.edges_back.len() + self.draw.edges_hi.len());
    }
}

// ───────────────────────────── headless UI tests ─────────────────────────────
// These drive the real egui code paths (panels, canvas input, table) with synthetic events and
// validate the meshes that would be sent to the GPU.

#[cfg(test)]
mod ui_tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, RawInput};

    struct Harness {
        ctx: Context,
        app: App,
        t: f64,
        mods: Modifiers,
        applied: Modifiers,
        last_verts: usize,
        last_ui_ms: f64,
        last_tess_ms: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = Context::default();
            let app = App::new_with_ctx(&ctx, Args { edges: None, nodes: None, json: None, directed: false, project: None });
            Harness { ctx, app, t: 0.0, mods: Modifiers::NONE, applied: Modifiers::NONE, last_verts: 0, last_ui_ms: 0.0, last_tess_ms: 0.0 }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.t += 1.0 / 60.0;
            let mut input = RawInput::default();
            input.screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 950.0)));
            input.time = Some(self.t);
            input.events = events;
            if self.mods != self.applied {
                input.events.insert(0, Event::ModifiersChanged(self.mods));
                self.applied = self.mods;
            }
            let app = &mut self.app;
            let t_ui = std::time::Instant::now();
            let mut out = self.ctx.run_ui(input, |ui| app.ui_inner(ui));
            self.last_ui_ms = t_ui.elapsed().as_secs_f64() * 1000.0;
            out.textures_delta.clear(); // not uploading anywhere; egui panics on dropping unapplied deltas
            let t_tess = std::time::Instant::now();
            let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
            self.last_tess_ms = t_tess.elapsed().as_secs_f64() * 1000.0;
            let mut verts = 0;
            for p in &prims {
                if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                    assert!(m.is_valid(), "invalid mesh produced");
                    verts += m.vertices.len();
                }
            }
            assert!(verts > 0, "nothing was drawn");
            self.last_verts = verts;
        }

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(vec![]);
            }
        }

        fn node_screen_pos(&self, i: usize) -> Pos2 {
            let r = self.app.canvas_rect;
            r.min + self.app.cam.to_screen(self.app.s.pos[i], r.size())
        }

        fn click(&mut self, p: Pos2, mods: Modifiers) {
            self.mods = mods;
            self.frame(vec![Event::PointerMoved(p)]);
            self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: mods }]);
            self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: mods }]);
            self.frames(2);
            self.mods = Modifiers::NONE;
        }
    }

    #[test]
    fn empty_state_renders() {
        let mut h = Harness::new();
        h.frames(3);
        assert_eq!(h.app.s.g.n(), 0);
    }

    #[test]
    fn sample_loads_layouts_and_draws() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.frames(40);
        assert_eq!(h.app.s.g.n(), 1200);
        assert!(h.app.s.sim.running || h.app.s.sim.alpha < 0.5, "force layout should be progressing");
        assert!(h.app.draw.nodes.len() > 1000);
        assert!(h.app.draw.edges.len() + h.app.draw.edges_back.len() > 3000);
        // the camera has been fitted to the graph
        assert!(h.app.cam.zoom != 1.0);
    }

    #[test]
    fn default_dock_layout_is_sane_and_roundtrips() {
        let d = crate::dock::default_dock();
        assert!(crate::dock::is_sane(&d));
        for t in crate::dock::Tab::ALL {
            assert!(crate::dock::is_open(&d, t), "{t:?} missing from the default layout");
        }
        let text = crate::config::layout_to_string(&d).unwrap();
        let back = crate::config::layout_from_str(&text).expect("a freshly built dock must load back");
        assert!(crate::dock::is_sane(&back));
        // a live layout (some panel rectangles may still be unmeasured) also loads back and is then stable
        let mut h = Harness::new();
        h.frames(3);
        let live = crate::config::layout_to_string(h.app.dock.as_ref().unwrap()).unwrap();
        let back2 = crate::config::layout_from_str(&live).expect("live layout loads");
        let t2 = crate::config::layout_to_string(&back2).unwrap();
        let back3 = crate::config::layout_from_str(&t2).unwrap();
        assert_eq!(t2, crate::config::layout_to_string(&back3).unwrap());
        // closing and reopening a panel
        let mut d = d;
        crate::dock::close(&mut d, crate::dock::Tab::Filter);
        assert!(!crate::dock::is_open(&d, crate::dock::Tab::Filter));
        crate::dock::open_or_focus(&mut d, crate::dock::Tab::Filter);
        assert!(crate::dock::is_open(&d, crate::dock::Tab::Filter));
        crate::dock::close(&mut d, crate::dock::Tab::Canvas);
        assert!(crate::dock::is_sane(&d), "the graph canvas can never be closed");
    }

    #[test]
    fn session_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("grapht-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("e.csv"), SAMPLE_EDGES).unwrap();
        std::fs::write(dir.join("n.csv"), SAMPLE_NODES).unwrap();
        let mut h = Harness::new();
        h.app.load_path(dir.join("e.csv"), Some(PendKindHint::Edges));
        h.app.load_path(dir.join("n.csv"), Some(PendKindHint::Nodes));
        h.frames(3);
        let n = h.app.s.g.n();
        assert!(n > 1000);
        h.app.s.layout.kind = crate::layout::LayoutKind::Circular;
        h.app.s.apply_layout_now();
        h.app.s.look.node_size = 7.5;
        h.app.s.look.edge_alpha = 0.123;
        h.app.s.filter.expr_text = "_degree > 2".into();
        h.app.s.select_only(5);
        h.app.s.sim.pinned = vec![false; n];
        h.app.s.sim.pinned[7] = true;
        h.app.s.manual_hidden[9] = true;
        h.app.table.sort = Some(("_degree".into(), false));
        h.app.cam = ViewCam { center: Vec2::new(12.0, -34.0), zoom: 2.5 };
        h.app.auto_fit = false;
        let want_pos = h.app.s.pos.clone();
        let want_id5 = h.app.s.g.ids[5].clone();
        let file = dir.join("work.grapht");
        h.app.save_project_to(&file).unwrap();
        assert!(h.app.project.as_deref() == Some(file.as_path()));

        let mut h2 = Harness::new();
        h2.app.open_project(&file);
        assert!(h2.app.error.is_none(), "{:?}", h2.app.error);
        assert_eq!(h2.app.s.g.n(), n);
        assert_eq!(h2.app.s.layout.kind, crate::layout::LayoutKind::Circular);
        assert_eq!(h2.app.s.look.node_size, 7.5);
        assert_eq!(h2.app.s.look.edge_alpha, 0.123);
        assert_eq!(h2.app.s.filter.expr_text, "_degree > 2");
        assert_eq!(h2.app.s.selected_list().len(), 1);
        assert_eq!(h2.app.s.g.ids[h2.app.s.selected_list()[0] as usize], want_id5);
        assert!(h2.app.s.sim.pinned[7]);
        assert!(h2.app.s.manual_hidden[9]);
        assert_eq!(h2.app.table.sort, Some(("_degree".to_string(), false)));
        assert_eq!(h2.app.cam.zoom, 2.5);
        assert!(!h2.app.auto_fit);
        for i in 0..n {
            assert!((h2.app.s.pos[i] - want_pos[i]).length() < 1e-3, "position {i} differs");
        }
        h2.frames(5);
        assert!((h2.app.s.pos[3] - want_pos[3]).length() < 1e-3, "layout must not move after restore");
        assert_eq!(h2.app.cam.zoom, 2.5, "camera must not be re-fitted");

        // a missing data file is reported, not fatal
        std::fs::remove_file(dir.join("n.csv")).unwrap();
        let mut h3 = Harness::new();
        h3.app.open_project(&file);
        assert!(h3.app.error.as_deref().is_some_and(|e| e.contains("Nodes file not found")));
        assert!(h3.app.s.g.n() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn click_selects_and_inspector_shows() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.app.s.layout.kind = crate::layout::LayoutKind::Circular;
        h.app.s.apply_layout_now();
        h.app.auto_fit = true;
        h.frames(60);
        // pick a node that is on screen and not covered: the first node of the circle
        let i = 0;
        let p = h.node_screen_pos(i);
        h.click(p, Modifiers::NONE);
        assert_eq!(h.app.s.sel_count, 1, "click should select exactly one node");
        assert!(h.app.s.sel[i]);
        // ctrl-click another adds to the selection
        let p2 = h.node_screen_pos(300);
        h.click(p2, Modifiers::CTRL);
        assert_eq!(h.app.s.sel_count, 2);
        // click on empty space clears
        let r = h.app.canvas_rect;
        h.click(r.min + Vec2::new(5.0, 5.0), Modifiers::NONE);
        assert_eq!(h.app.s.sel_count, 0);
    }

    #[test]
    fn wheel_zooms_and_drag_pans() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.app.s.layout.kind = crate::layout::LayoutKind::Circular;
        h.app.s.apply_layout_now();
        h.frames(60);
        let z0 = h.app.cam.zoom;
        let c = h.app.canvas_rect.center();
        h.frame(vec![Event::PointerMoved(c)]);
        h.frame(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, 120.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE }]);
        h.frames(3);
        assert!(h.app.cam.zoom > z0, "zoom {} → {}", z0, h.app.cam.zoom);
        // pan by dragging empty canvas corner area
        let center0 = h.app.cam.center;
        let a = h.app.canvas_rect.min + Vec2::new(8.0, 8.0);
        h.frame(vec![Event::PointerMoved(a)]);
        h.frame(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        h.frame(vec![Event::PointerMoved(a + Vec2::new(40.0, 25.0))]);
        h.frame(vec![Event::PointerMoved(a + Vec2::new(80.0, 50.0))]);
        h.frame(vec![Event::PointerButton { pos: a + Vec2::new(80.0, 50.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        h.frames(2);
        assert!((h.app.cam.center - center0).length() > 1.0, "camera should have panned");
    }

    #[test]
    fn dragging_a_node_moves_it() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.app.s.layout.kind = crate::layout::LayoutKind::Grid;
        h.app.s.apply_layout_now();
        h.frames(60);
        let i = 0;
        let before = h.app.s.pos[i];
        let p = h.node_screen_pos(i);
        h.frame(vec![Event::PointerMoved(p)]);
        h.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        h.frame(vec![Event::PointerMoved(p + Vec2::new(10.0, 10.0))]);
        h.frame(vec![Event::PointerMoved(p + Vec2::new(30.0, 20.0))]);
        h.frame(vec![Event::PointerButton { pos: p + Vec2::new(30.0, 20.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        h.frames(2);
        assert!((h.app.s.pos[i] - before).length() > 5.0, "node should have moved");
    }

    #[test]
    fn box_select_with_shift_drag() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.app.s.layout.kind = crate::layout::LayoutKind::Grid;
        h.app.s.apply_layout_now();
        h.frames(60);
        let r = h.app.canvas_rect;
        let a = r.min + Vec2::new(r.width() * 0.3, r.height() * 0.3);
        let b = r.min + Vec2::new(r.width() * 0.6, r.height() * 0.6);
        let shift = Modifiers::SHIFT;
        h.mods = shift;
        h.frame(vec![Event::PointerMoved(a)]);
        h.frame(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: shift }]);
        h.frame(vec![Event::PointerMoved((a + b.to_vec2()) * 0.5)]);
        h.frame(vec![Event::PointerMoved(b)]);
        h.frame(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: shift }]);
        h.mods = Modifiers::NONE;
        h.frames(2);
        assert!(h.app.s.sel_count > 20, "box select picked {}", h.app.s.sel_count);
    }

    #[test]
    fn keyboard_shortcuts() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.frames(10);
        let key = |k: egui::Key, mods: Modifiers| Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: mods };
        h.mods = Modifiers::CTRL;
        h.frame(vec![key(egui::Key::A, Modifiers::CTRL)]);
        h.mods = Modifiers::NONE;
        h.frames(1);
        assert_eq!(h.app.s.sel_count, 1200);
        h.frame(vec![key(egui::Key::Escape, Modifiers::NONE)]);
        h.frames(1);
        assert_eq!(h.app.s.sel_count, 0);
    }

    // ───────────────────────── view toolbar ─────────────────────────

    use crate::view::{NavMode, Tool};

    fn key_ev(k: egui::Key, pressed: bool) -> Event {
        Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: Modifiers::NONE }
    }

    fn grid_harness() -> Harness {
        let mut h = Harness::new();
        h.app.load_sample();
        h.app.s.layout.kind = crate::layout::LayoutKind::Grid;
        h.app.s.apply_layout_now();
        h.frames(80);
        h
    }

    fn harness_3d() -> Harness {
        use crate::data::*;
        let mut h = Harness::new();
        let mut nodes = String::from("id,x,y,z\n");
        let mut edges = String::from("source,target\n");
        for i in 0..60 {
            nodes += &format!("n{i},{},{},{}\n", (i % 10) * 10, (i / 10) * 10, (i % 7) * 5);
            if i > 0 {
                edges += &format!("n{},n{}\n", i - 1, i);
            }
        }
        let (e, n) = (parse_csv_text(&edges).unwrap(), parse_csv_text(&nodes).unwrap());
        let em = auto_edge_mapping(&e);
        h.app.s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        h.app.s.set_nodes("n".into(), n, nm);
        h.app.s.layout.kind = crate::layout::LayoutKind::Grid;
        h.app.rebuild();
        h.app.s.coords = crate::coords::CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
        h.app.s.apply_coords();
        h.app.auto_fit = true;
        h.frames(60);
        assert!(h.app.s.is_3d());
        h
    }

    impl Harness {
        fn tool(&self, t: Tool) -> Option<(egui::Rect, bool)> {
            self.app.view.buttons.iter().find(|(x, _, _)| *x == t).map(|(_, r, e)| (*r, *e))
        }

        fn click_tool(&mut self, t: Tool) {
            let (r, _) = self.tool(t).unwrap_or_else(|| panic!("no {t:?} button"));
            self.click(r.center(), Modifiers::NONE);
            self.frames(40); // the camera glides for about 0.3 s
        }

        fn drag(&mut self, a: Pos2, b: Pos2) {
            self.frame(vec![Event::PointerMoved(a)]);
            self.frame(vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            self.frame(vec![Event::PointerMoved((a + b.to_vec2()) * 0.5)]);
            self.frame(vec![Event::PointerMoved(b)]);
            self.frame(vec![Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
            self.frames(2);
        }

        fn press(&mut self, k: egui::Key) {
            self.frame(vec![key_ev(k, true)]);
            self.frame(vec![key_ev(k, false)]);
            self.frames(40);
        }
    }

    #[test]
    fn toolbar_has_the_usual_buttons_and_greys_out_what_does_not_apply() {
        let h = grid_harness();
        for t in [Tool::Fit, Tool::Selection, Tool::Region, Tool::ZoomIn, Tool::ZoomOut, Tool::Home, Tool::Back, Tool::Forward] {
            assert!(h.tool(t).is_some(), "{t:?} missing");
        }
        assert!(!h.tool(Tool::Selection).unwrap().1, "nothing selected: zoom to selection is greyed out");
        assert!(!h.tool(Tool::Back).unwrap().1 && !h.tool(Tool::Forward).unwrap().1, "no history yet");
        assert!(h.tool(Tool::Fit).unwrap().1 && h.tool(Tool::Region).unwrap().1);
        assert!(h.tool(Tool::Perspective).is_none() && h.tool(Tool::Preset(crate::coords::View3::Top)).is_none(), "3D tools are hidden in 2D");
        let bar = h.app.view.toolbar_rect.unwrap();
        let c = h.app.canvas_rect;
        assert!(c.contains_rect(bar) && bar.bottom() > c.bottom() - 40.0, "floats at the bottom of the graph");
        assert!((bar.center().x - c.center().x).abs() < 2.0, "in the middle");
        assert!(h.app.view.over_toolbar(bar.center()) && !h.app.view.over_toolbar(c.center()));
    }

    #[test]
    fn zoom_to_selection_button_centres_the_node_and_keeps_the_selection() {
        let mut h = grid_harness();
        h.app.s.select_only(500);
        h.frames(2);
        assert!(h.tool(Tool::Selection).unwrap().1);
        let before = h.app.cam.zoom;
        h.click_tool(Tool::Selection);
        assert_eq!(h.app.s.sel_count, 1, "clicking the toolbar must not clear the selection");
        assert!((h.app.cam.center - h.app.s.pos[500]).length() < 0.5, "{:?} vs {:?}", h.app.cam.center, h.app.s.pos[500]);
        assert!(h.app.cam.zoom > before && (1.5..=6.0).contains(&h.app.cam.zoom), "zoom {} -> {}", before, h.app.cam.zoom);
        assert!(!h.app.auto_fit);
        // a group: both nodes end up on screen
        let r = h.app.canvas_rect;
        h.app.s.select_nodes([0usize, 1199]);
        h.frames(2);
        h.click_tool(Tool::Selection);
        for i in [0usize, 1199] {
            let p = h.node_screen_pos(i);
            assert!(r.contains(p), "node {i} at {p:?} outside {r:?}");
        }
    }

    #[test]
    fn keyboard_shortcuts_s_and_home_and_f() {
        let mut h = grid_harness();
        let fit = h.app.cam;
        h.app.s.select_only(100);
        h.press(egui::Key::S);
        assert!((h.app.cam.center - h.app.s.pos[100]).length() < 0.5);
        h.press(egui::Key::F);
        assert!((h.app.cam.zoom / fit.zoom - 1.0).abs() < 0.02, "F fits again: {} vs {}", h.app.cam.zoom, fit.zoom);
        h.app.s.select_only(100);
        h.press(egui::Key::S);
        h.press(egui::Key::Home);
        assert!((h.app.cam.zoom / fit.zoom - 1.0).abs() < 0.02);
    }

    #[test]
    fn zoom_to_region_with_the_r_key_and_a_dragged_rectangle() {
        let mut h = grid_harness();
        h.app.s.select_only(3);
        h.frames(2);
        let before = h.app.cam;
        h.frame(vec![key_ev(egui::Key::R, true)]);
        h.frame(vec![key_ev(egui::Key::R, false)]);
        assert!(h.app.view.region_on);
        let r = h.app.canvas_rect;
        let a = r.min + Vec2::new(r.width() * 0.3, r.height() * 0.2);
        let b = r.min + Vec2::new(r.width() * 0.5, r.height() * 0.4);
        let world_a = before.to_world(a - r.min, r.size());
        let world_b = before.to_world(b - r.min, r.size());
        h.drag(a, b);
        h.frames(40);
        assert!(!h.app.view.region_on, "the tool switches itself off after one use");
        assert_eq!(h.app.s.sel_count, 1, "drawing the rectangle selected nothing");
        assert!(h.app.cam.zoom > before.zoom * 3.0, "{} -> {}", before.zoom, h.app.cam.zoom);
        let (sa, sb) = (h.app.cam.to_screen(world_a, r.size()), h.app.cam.to_screen(world_b, r.size()));
        assert!(sa.x.abs() < 3.0 && sa.y.abs() < 3.0 && (sb.x - r.width()).abs() < 3.0 && (sb.y - r.height()).abs() < 3.0, "{sa:?} {sb:?}");
    }

    #[test]
    fn region_tool_can_be_cancelled_and_escape_keeps_the_selection() {
        let mut h = grid_harness();
        h.app.s.select_only(3);
        h.click_tool(Tool::Region);
        assert!(h.app.view.region_on);
        let before = h.app.cam;
        h.frame(vec![key_ev(egui::Key::Escape, true)]);
        h.frame(vec![key_ev(egui::Key::Escape, false)]);
        assert!(!h.app.view.region_on);
        assert_eq!(h.app.s.sel_count, 1, "Esc cancelled the tool first");
        assert_eq!(h.app.cam.zoom, before.zoom);
        // a tiny drag is not a region
        h.click_tool(Tool::Region);
        let c = h.app.canvas_rect.center();
        h.drag(c, c + Vec2::new(2.0, 1.0));
        h.frames(30);
        assert!((h.app.cam.zoom / before.zoom - 1.0).abs() < 1e-3);
        // the button toggles the tool off again
        h.click_tool(Tool::Region);
        assert!(!h.app.view.region_on);
    }

    #[test]
    fn previous_and_next_view() {
        let mut h = grid_harness();
        let start = h.app.cam;
        h.app.s.select_only(500);
        h.click_tool(Tool::Selection);
        let zoomed = h.app.cam;
        assert!(zoomed.zoom > start.zoom);
        assert!(h.tool(Tool::Back).unwrap().1, "there is a view to go back to");
        h.click_tool(Tool::Back);
        assert!((h.app.cam.zoom / start.zoom - 1.0).abs() < 0.02, "back at the start: {} vs {}", h.app.cam.zoom, start.zoom);
        assert!(h.tool(Tool::Forward).unwrap().1);
        h.click_tool(Tool::Forward);
        assert!((h.app.cam.zoom / zoomed.zoom - 1.0).abs() < 0.02 && (h.app.cam.center - zoomed.center).length() < 1.0);
        // loading another graph forgets the history
        h.app.rebuild();
        h.frames(5);
        assert!(!h.app.view.hist.can_back());
    }

    #[test]
    fn zoom_buttons_step_around_the_middle_and_animation_can_be_switched_off() {
        let mut h = grid_harness();
        let z0 = h.app.cam.zoom;
        h.click_tool(Tool::ZoomIn);
        assert!((h.app.cam.zoom / z0 - crate::view::ZOOM_STEP).abs() < 0.01, "{}", h.app.cam.zoom / z0);
        h.click_tool(Tool::ZoomOut);
        assert!((h.app.cam.zoom / z0 - 1.0).abs() < 0.01);
        h.app.settings.animate_view = false;
        let (r, _) = h.tool(Tool::ZoomIn).unwrap();
        h.click(r.center(), Modifiers::NONE);
        assert!(h.app.view.anim.is_none() && h.app.cam.zoom > z0 * 1.4, "without animation the view jumps at once");
    }

    #[test]
    fn scrolling_stops_a_running_camera_move() {
        let mut h = grid_harness();
        h.app.s.select_only(700);
        let (r, _) = (h.app.canvas_rect, 0);
        h.app.run_tool(Tool::Selection);
        h.frames(3);
        assert!(h.app.view.anim.is_some(), "gliding");
        h.frame(vec![Event::PointerMoved(r.center() + Vec2::new(0.0, -100.0))]);
        h.frame(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, 40.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE }]);
        assert!(h.app.view.anim.is_none(), "the user takes over");
    }

    #[test]
    fn space_drag_pans_and_a_plain_space_still_pauses_the_layout() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.frames(10);
        assert!(h.app.s.sim.running);
        let c0 = h.app.cam.center;
        let r = h.app.canvas_rect;
        let a = r.min + Vec2::new(6.0, 6.0);
        h.frame(vec![key_ev(egui::Key::Space, true)]);
        h.drag(a, a + Vec2::new(120.0, 60.0));
        h.frame(vec![key_ev(egui::Key::Space, false)]);
        h.frames(2);
        assert!((h.app.cam.center - c0).length() > 5.0, "the view moved");
        assert!(h.app.s.sim.running && !h.app.sim_paused, "Space + drag does not also pause");
        h.frame(vec![key_ev(egui::Key::Space, true)]);
        h.frame(vec![key_ev(egui::Key::Space, false)]);
        h.frames(2);
        assert!(h.app.sim_paused && !h.app.s.sim.running, "a plain Space press pauses");
    }

    #[test]
    fn three_d_toolbar_presets_projection_and_drag_modes() {
        let mut h = harness_3d();
        for t in [Tool::Perspective, Tool::Nav(NavMode::Orbit), Tool::Nav(NavMode::Pan), Tool::Nav(NavMode::Zoom), Tool::Preset(crate::coords::View3::Top), Tool::Preset(crate::coords::View3::Iso)] {
            assert!(h.tool(t).is_some(), "{t:?} missing in 3D");
        }
        // the 3D row sits above the main row
        let top = h.tool(Tool::Preset(crate::coords::View3::Top)).unwrap().0;
        let fit = h.tool(Tool::Fit).unwrap().0;
        assert!(top.bottom() < fit.top());
        h.click_tool(Tool::Preset(crate::coords::View3::Top));
        assert!(h.app.s.cam3.pitch > 1.4, "looking down");
        assert!(!h.app.s.cam3.ortho);
        h.click_tool(Tool::Perspective);
        assert!(h.app.s.cam3.ortho, "orthographic now");
        h.frames(3);
        assert!(h.app.draw.nodes.len() > 10, "still draws in orthographic view");
        // pan mode: a plain drag moves the target, not the angle
        h.click_tool(Tool::Nav(NavMode::Pan));
        assert_eq!(h.app.view.nav, NavMode::Pan);
        let (t0, yaw0) = (h.app.s.cam3.target, h.app.s.cam3.yaw);
        let r = h.app.canvas_rect;
        let a = r.min + Vec2::new(10.0, 10.0);
        h.drag(a, a + Vec2::new(80.0, 40.0));
        let moved = (0..3).map(|k| (h.app.s.cam3.target[k] - t0[k]).abs()).sum::<f32>();
        assert!(moved > 1.0 && h.app.s.cam3.yaw == yaw0, "moved {moved}");
        // orbit mode turns it
        h.click_tool(Tool::Nav(NavMode::Orbit));
        h.drag(a, a + Vec2::new(80.0, 0.0));
        assert!((h.app.s.cam3.yaw - yaw0).abs() > 0.1);
        // zoom to selection in 3D
        h.app.s.select_only(30);
        h.frames(2);
        h.click_tool(Tool::Selection);
        assert_eq!(h.app.s.cam3.target, h.app.s.pos3[30]);
        // the view switches back to 2D: the 3D tools go away
        h.app.s.coords.three_d = false;
        h.app.s.apply_coords();
        h.frames(5);
        assert!(h.tool(Tool::Perspective).is_none());
    }

    #[test]
    fn query_panel_shows_and_the_result_follows_the_data() {
        use crate::query::{Cond, Op};
        let mut h = Harness::new();
        h.app.load_sample();
        h.frames(5);
        let mut dock = h.app.dock.take().unwrap();
        crate::dock::open_or_focus(&mut dock, crate::dock::Tab::Query);
        h.app.dock = Some(dock);
        h.frames(3);
        assert!(h.app.s.query_info.is_none(), "nothing asked yet");
        let group = h.app.s.g.node_col("group").unwrap().label_at(0).unwrap();
        h.app.s.query.a = Cond { col: Some("group".into()), op: Op::Is, value: group };
        h.app.s.query.b = Cond { col: None, op: Op::Is, value: String::new() };
        h.app.refilter();
        h.frames(3);
        let info = h.app.s.query_info.as_ref().expect("the query was answered");
        assert!(info.n_a > 0 && info.n_b == 1200 && info.n_a_conn > 0, "{info:?}");
        h.app.s.query.mode = crate::query::QueryMode::CrossTab;
        h.app.s.query.rows = Some("group".into());
        h.app.s.query.cols = Some("group".into());
        h.app.refilter();
        h.frames(3);
        assert!(h.app.s.xtab.is_some() && h.app.s.query_error.is_none());
    }

    #[test]
    fn panels_with_filters_sorting_and_every_layout() {
        let mut h = Harness::new();
        h.app.load_sample();
        h.frames(5);
        // filters of every kind
        let g = &h.app.s.g;
        let rules = vec![crate::model::Rule::new_for(g.node_col("age").unwrap()), crate::model::Rule::new_for(g.node_col("group").unwrap())];
        h.app.s.filter.rules = rules;
        if let crate::model::RuleKind::Range { lo, .. } = &mut h.app.s.filter.rules[0].kind {
            *lo = 30.0;
        }
        h.app.s.filter.expr_text = "score > 2 and remote == true".into();
        h.app.s.filter.search = "person".into();
        h.app.refilter();
        h.frames(3);
        assert!(h.app.s.n_vis > 0 && h.app.s.n_vis < 1200, "filtered to {}", h.app.s.n_vis);
        // dim mode and table sort
        h.app.s.look.filtered = crate::model::FilteredMode::Dim;
        h.app.table.sort = Some(("score".into(), false));
        h.app.table.dirty = true;
        h.frames(3);
        let first = h.app.table.order[0] as usize;
        let sc = h.app.s.g.node_col("score").unwrap();
        let top = h.app.table.order.iter().filter_map(|&i| sc.num(i as usize)).fold(f64::MIN, f64::max);
        assert_eq!(sc.num(first), Some(top));
        // every layout and colour mode renders
        for kind in crate::layout::LayoutKind::ALL {
            h.app.s.layout.kind = kind;
            h.app.s.layout.x_by = Some("age".into());
            h.app.s.layout.y_by = Some("role".into());
            h.app.s.apply_layout();
            h.frames(8);
        }
        for color in ["group", "score", "_pagerank", "age", "remote"] {
            h.app.s.look.color_by = Some(color.into());
            h.app.s.look.edge_mode = crate::model::EdgeColorMode::Blend;
            h.app.restyle();
            h.frames(2);
        }
        h.app.s.look.edge_mode = crate::model::EdgeColorMode::Attribute;
        h.app.s.look.edge_by = Some("weight".into());
        h.app.restyle();
        h.frames(2);
        // directed graphs draw arrows
        if let Some(src) = &mut h.app.s.edge_src {
            src.mapping.directed = true;
        }
        h.app.rebuild();
        h.frames(10);
        // dark/light
        h.app.s.look.dark = false;
        h.frames(2);
    }

    #[test]
    fn json_roundtrip_through_the_gui_loader() {
        let dir = std::env::temp_dir().join("grapht_ui_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("g.json");
        std::fs::write(&path, r#"{"directed":true,"nodes":[{"id":"a","kind":"x"},{"id":"b","kind":"y"},{"id":"c","kind":"x"}],"links":[{"source":"a","target":"b"},{"source":"b","target":"c"}]}"#).unwrap();
        let mut h = Harness::new();
        h.app.load_path(path, None);
        h.frames(10);
        assert_eq!(h.app.s.g.n(), 3);
        assert!(h.app.s.g.directed);
        assert!(h.app.s.g.node_col("kind").is_some());
    }

    #[test]
    fn csv_import_dialog_flow() {
        let dir = std::env::temp_dir().join("grapht_ui_test");
        std::fs::create_dir_all(&dir).unwrap();
        // unusual column names → the mapping dialog must appear instead of silently guessing
        let e = dir.join("odd_edges.csv");
        std::fs::write(&e, "p,q,strength\n1,2,5\n2,3,1\n").unwrap();
        let mut h = Harness::new();
        h.app.load_path(e, Some(PendKindHint::Edges));
        h.frames(2);
        assert!(h.app.pending.is_some(), "ambiguous headers should open the import dialog");
        let p = h.app.pending.take().unwrap();
        h.app.commit_import(p);
        h.frames(5);
        assert_eq!(h.app.s.g.n(), 3);
        // nodes with an obvious id column import directly
        let n = dir.join("nodes.csv");
        std::fs::write(&n, "id,colour\n1,red\n2,blue\n3,red\n").unwrap();
        h.app.load_path(n, None);
        h.frames(3);
        assert!(h.app.pending.is_none());
        assert!(h.app.s.g.node_col("colour").is_some());
    }

    /// Large-graph timings. Run with: cargo test --release bench -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_10k_nodes_50k_edges() {
        use std::time::Instant;
        let (n, e) = (10_000usize, 50_000usize);
        let mut rng = 0x9E3779B97F4A7C15u64;
        let mut next = move |m: usize| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng % m as u64) as usize
        };
        let groups = 12;
        let mut edges = String::from("source,target,weight\n");
        for _ in 0..e {
            let a = next(n);
            // 85 % of edges stay inside the node's group (group = index % groups)
            let b = if next(100) < 85 { (next(n / groups) * groups + a % groups) % n } else { next(n) };
            if a != b {
                edges += &format!("n{a},n{b},{}\n", 1 + next(9));
            }
        }
        let mut nodes = String::from("id,group,value\n");
        for i in 0..n {
            nodes += &format!("n{i},g{},{}\n", i % groups, next(1000));
        }
        let mut h = Harness::new();
        let et = parse_csv_text(&edges).unwrap();
        let nt = parse_csv_text(&nodes).unwrap();
        let em = auto_edge_mapping(&et);
        let nm = auto_node_mapping(&nt);
        h.app.s.set_edges("e".into(), et, em);
        h.app.s.set_nodes("n".into(), nt, nm);
        let t = Instant::now();
        h.app.rebuild();
        println!("build graph + metrics ({} nodes, {} edges): {:.0} ms", h.app.s.g.n(), h.app.s.g.edges.len(), t.elapsed().as_secs_f64() * 1e3);

        let p = h.app.s.layout.force.clone();
        let t = Instant::now();
        h.app.s.sim.step(&mut h.app.s.pos, &p, 20);
        println!("force layout: {:.1} ms per iteration", t.elapsed().as_secs_f64() * 1e3 / 20.0);
        let t = Instant::now();
        h.app.s.settle(400);
        println!("force layout to convergence: {:.2} s", t.elapsed().as_secs_f64());

        h.app.s.sim.running = false;
        h.app.auto_fit = true;
        h.frames(30);
        let size = h.app.canvas_rect.size();
        let none = vec![false; h.app.s.g.n()];
        let hi = Highlight { focus: &none, hood: &none, active: false };
        let mut dl = DrawList::default();
        let t = Instant::now();
        for _ in 0..20 {
            build_draw_list(&h.app.s, &h.app.cam, size, &hi, &mut dl);
        }
        println!("build draw list: {:.2} ms ({} nodes, {} edges on screen)", t.elapsed().as_secs_f64() * 1e3 / 20.0, dl.nodes.len(), dl.edges.len() + dl.edges_back.len());
        let t = Instant::now();
        h.frames(20);
        println!("full UI frame incl. meshes + tessellation: {:.2} ms", t.elapsed().as_secs_f64() * 1e3 / 20.0);
        h.app.s.layout.kind = crate::layout::LayoutKind::Circular;
        let t = Instant::now();
        h.app.s.apply_layout_now();
        println!("circular layout: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        h.app.s.layout.kind = crate::layout::LayoutKind::Hierarchical;
        let t = Instant::now();
        h.app.s.apply_layout_now();
        println!("hierarchical layout: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        h.app.s.layout.kind = crate::layout::LayoutKind::GroupBy;
        h.app.s.layout.group_by = Some("group".into());
        let t = Instant::now();
        h.app.s.apply_layout_now();
        println!("group layout: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        h.app.s.filter.expr_text = "value > 500 and group != \"g3\"".into();
        let t = Instant::now();
        h.app.refilter();
        println!("filter expression over 10k nodes: {:.2} ms ({} left)", t.elapsed().as_secs_f64() * 1e3, h.app.s.n_vis);
        let t = Instant::now();
        h.app.s.look.color_by = Some("_pagerank".into());
        h.app.restyle();
        println!("recolour: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    }

    // ───────────── performance benchmark (ignored by default) ─────────────
    // cargo test --release --bins perf_ -- --ignored --nocapture --test-threads=1

    fn pct(v: &[f64], p: f64) -> f64 {
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.total_cmp(b));
        s[((s.len() as f64 - 1.0) * p) as usize]
    }

    thread_local! {
        static UI_MS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
        static TESS_MS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }

    fn report(name: &str, phase: &str, ms: &[f64], verts: &[usize], ticks: usize) {
        let (ui, tess) = (UI_MS.with(|c| c.replace(0.0)) / ms.len() as f64, TESS_MS.with(|c| c.replace(0.0)) / ms.len() as f64);
        println!("BENCH {name} {phase} split: ui {ui:.2} ms, tessellate {tess:.2} ms");
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        let over = ms.iter().filter(|m| **m > 16.7).count();
        println!(
            "BENCH {name} {phase}: mean {mean:.2} p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} ms | over16.7 {over}/{} | verts {} | layout_ticks {ticks}",
            pct(ms, 0.5),
            pct(ms, 0.95),
            pct(ms, 0.99),
            ms.iter().cloned().fold(0.0, f64::max),
            ms.len(),
            verts.iter().sum::<usize>() / verts.len().max(1)
        );
    }

    fn run_bench(name: &str, load: impl FnOnce(&mut Harness), orbit: bool) {
        let mut h = Harness::new();
        let mb = |b: usize| b as f64 / 1048576.0;
        let (heap0, _) = crate::alloc_track::now();
        crate::alloc_track::reset_peak();
        load(&mut h);
        let (heap_loaded, peak_load) = crate::alloc_track::now();
        h.frames(25);
        let (heap_frames, _) = crate::alloc_track::now();
        println!("BENCH {name} loaded: nodes {} edges {} | heap after load +{:.1} MB (peak while loading +{:.1} MB), after 25 frames +{:.1} MB", h.app.s.g.n(), h.app.s.g.edges.len(), mb(heap_loaded - heap0), mb(peak_load - heap0), mb(heap_frames - heap0));
        crate::alloc_track::reset_peak();
        let rect = h.app.canvas_rect;
        let timed = |h: &mut Harness, events: Vec<Event>, ms: &mut Vec<f64>, verts: &mut Vec<usize>| {
            let t = std::time::Instant::now();
            h.frame(events);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            verts.push(h.last_verts);
            UI_MS.with(|c| c.set(c.get() + h.last_ui_ms));
            TESS_MS.with(|c| c.set(c.get() + h.last_tess_ms));
        };
        // pick the node closest to the middle of the canvas to grab
        let mid = rect.center();
        let grab = (0..h.app.s.g.n())
            .min_by(|&a, &b| (h.node_screen_pos(a) - mid).length().total_cmp(&(h.node_screen_pos(b) - mid).length()))
            .unwrap();
        let gp = h.node_screen_pos(grab);
        let empty = rect.min + Vec2::new(14.0, 14.0);
        for (phase, start) in [("drag", if orbit { empty } else { gp }), ("pan", empty)] {
            if phase == "drag" && orbit {
                continue;
            }
            let (mut ms, mut verts) = (vec![], vec![]);
            let ticks0 = h.app.s.sim.ticks;
            h.frame(vec![Event::PointerMoved(start)]);
            h.frame(vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            for k in 0..150 {
                let a = k as f32 * 0.12;
                let p = start + Vec2::new(a.cos() * 40.0, a.sin() * 30.0) * if phase == "pan" { 1.0 } else { 0.5 };
                timed(&mut h, vec![Event::PointerMoved(p)], &mut ms, &mut verts);
            }
            h.frame(vec![Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
            report(name, if orbit && phase == "pan" { "orbit" } else { phase }, &ms, &verts, h.app.s.sim.ticks - ticks0);
        }
        let (mut ms, mut verts) = (vec![], vec![]);
        h.frame(vec![Event::PointerMoved(mid)]);
        for k in 0..90 {
            let d = if (k / 15) % 2 == 0 { 40.0 } else { -40.0 };
            let ev = if k % 3 == 0 { vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, d), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE }] } else { vec![] };
            timed(&mut h, ev, &mut ms, &mut verts);
        }
        report(name, "zoom", &ms, &verts, 0);
        let (live_end, peak_end) = crate::alloc_track::now();
        println!("BENCH {name} end: heap +{:.1} MB, peak during interaction +{:.1} MB", mb(live_end - heap0), mb(peak_end - heap0));
    }

    /// A reproducible random graph as CSV text (no files needed): `n` nodes in clusters, `m` edges, mostly local.
    /// With `boxes` the nodes also carry a 3D position and a bounding box (min/max columns).
    fn synth(n: usize, m: usize, boxes: bool) -> (String, String) {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut nodes = String::from(if boxes { "id,group,kind,score,x,y,z,x0,y0,z0,x1,y1,z1\n" } else { "id,group,kind,score,label_text\n" });
        for i in 0..n {
            let score = rnd() * 100.0;
            if boxes {
                let (x, y, z) = (rnd() * 300.0, rnd() * 300.0, rnd() * 60.0);
                let (w, d, h) = (2.0 + rnd() * 7.0, 2.0 + rnd() * 7.0, 2.0 + rnd() * 4.0);
                nodes += &format!("n{i},g{},k{},{score:.2},{x:.2},{y:.2},{z:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}\n", (rnd() * 12.0) as u32, (rnd() * 5.0) as u32, x - w, y - d, z - h, x + w, y + d, z + h);
            } else {
                nodes += &format!("n{i},g{},k{},{score:.2},node number {i}\n", (rnd() * 24.0) as u32, (rnd() * 5.0) as u32);
            }
        }
        let mut edges = String::from("source,target,weight\n");
        for _ in 0..m {
            let a = (rnd() * n as f64) as usize % n;
            let b = if rnd() < 0.8 { (a + 1 + (rnd() * 59.0) as usize) % n } else { (rnd() * n as f64) as usize % n };
            edges += &format!("n{a},n{b},{:.3}\n", rnd());
        }
        (nodes, edges)
    }

    fn load_synth(h: &mut Harness, n: usize, m: usize, boxes: bool) {
        use crate::boxes::BoxSettings;
        use crate::coords::CoordSettings;
        let (nt, et) = synth(n, m, boxes);
        let (nn, ee) = (parse_csv_text(&nt).unwrap(), parse_csv_text(&et).unwrap());
        h.app.s.set_nodes("nodes".into(), nn.clone(), auto_node_mapping(&nn));
        h.app.s.set_edges("edges".into(), ee.clone(), auto_edge_mapping(&ee));
        h.app.rebuild();
        if boxes {
            let s = &mut h.app.s;
            s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
            s.apply_coords();
            let c = |k: &str| Some(k.to_string());
            s.boxes = BoxSettings { enabled: true, min: [c("x0"), c("y0"), c("z0")], max: [c("x1"), c("y1"), c("z1")], ..Default::default() };
            s.recompute_boxes();
            s.fit_cam3();
        }
    }

    #[test]
    #[ignore]
    fn perf_l() {
        run_bench("L30k", |h| load_synth(h, 30_000, 100_000, false), false);
    }

    #[test]
    #[ignore]
    fn perf_m() {
        run_bench("M5k", |h| load_synth(h, 5_000, 15_000, false), false);
    }

    #[test]
    #[ignore]
    fn perf_b700() {
        run_bench("B700", |h| load_synth(h, 700, 699, true), true);
    }

    #[test]
    #[ignore]
    fn perf_b5k() {
        run_bench("B5k", |h| load_synth(h, 5_000, 15_000, true), true);
    }
}
