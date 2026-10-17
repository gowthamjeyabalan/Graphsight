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

    fn pick_and_load(&mut self, hint: Option<PendKindHint>) {
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
                if ui.button("Fit graph to window   (F)").clicked() {
                    ui.close();
                    self.auto_fit = true;
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
                        ui.checkbox(&mut m.directed, "Directed edges (source → target)");
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
            ui.label("• F fit · Esc clear selection · Del hide selected · Ctrl+A select visible · Space pause/resume layout");
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
        self.handle_drops(&ctx);
        self.handle_shortcuts(&ctx);
        self.update_window_title(&ctx);

        egui::Panel::top("menu").frame(egui::Frame::new().fill(crate::theme::BG).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| self.menu_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::theme::BG)).show(ui, |ui| self.show_dock(ui));

        self.persist_layout(&ctx);
        self.dialogs(&ctx);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.frame_ms = self.frame_ms * 0.9 + ms * 0.1;
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
    }

    impl Harness {
        fn new() -> Self {
            let ctx = Context::default();
            let app = App::new_with_ctx(&ctx, Args { edges: None, nodes: None, json: None, directed: false, project: None });
            Harness { ctx, app, t: 0.0, mods: Modifiers::NONE, applied: Modifiers::NONE }
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
            let mut out = self.ctx.run_ui(input, |ui| app.ui_inner(ui));
            out.textures_delta.clear(); // not uploading anywhere; egui panics on dropping unapplied deltas
            let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
            let mut verts = 0;
            for p in &prims {
                if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                    assert!(m.is_valid(), "invalid mesh produced");
                    verts += m.vertices.len();
                }
            }
            assert!(verts > 0, "nothing was drawn");
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
}
