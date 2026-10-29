//! The interactive graph view: input handling and fast mesh-based drawing.

use crate::app::*;
use crate::layout::LayoutKind;
use crate::model::*;
use crate::render::arrow_points;
use crate::view::{NavMode, Tool};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, Vec2};

const MAX_VERTS: usize = 60_000;

/// Accumulates triangles into meshes that stay below the 16-bit-friendly vertex limit.
struct MeshSink {
    tex: TextureId,
    done: Vec<Mesh>,
    cur: Mesh,
}

/// Above this many edges the view is drawn thinner while it is being dragged or zoomed.
const LOD_EDGES: usize = 40_000;
/// Edges per piece when the mesh is built on several cores.
const MESH_CHUNK: usize = 8192;

impl MeshSink {
    fn with_capacity(tex: TextureId, verts: usize, idx: usize) -> Self {
        let mut cur = Mesh::with_texture(tex);
        cur.vertices.reserve(verts.min(MAX_VERTS));
        cur.indices.reserve(idx);
        MeshSink { tex, done: vec![], cur }
    }

    fn new(tex: TextureId) -> Self {
        MeshSink { tex, done: vec![], cur: Mesh::with_texture(tex) }
    }

    #[inline]
    fn reserve(&mut self, extra: usize) {
        if self.cur.vertices.len() + extra > MAX_VERTS {
            let full = std::mem::replace(&mut self.cur, Mesh::with_texture(self.tex));
            self.done.push(full);
        }
    }

    #[inline]
    fn vert(&mut self, p: Pos2, uv: Pos2, c: Color32) {
        self.cur.vertices.push(Vertex { pos: p, uv, color: c });
    }

    #[inline]
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.cur.indices.extend_from_slice(&[a, b, c]);
    }

    fn quad_uv(&mut self, center: Pos2, half: f32, c: Color32) {
        self.reserve(4);
        let base = self.cur.vertices.len() as u32;
        self.vert(Pos2::new(center.x - half, center.y - half), Pos2::new(0.0, 0.0), c);
        self.vert(Pos2::new(center.x + half, center.y - half), Pos2::new(1.0, 0.0), c);
        self.vert(Pos2::new(center.x + half, center.y + half), Pos2::new(1.0, 1.0), c);
        self.vert(Pos2::new(center.x - half, center.y + half), Pos2::new(0.0, 1.0), c);
        self.tri(base, base + 1, base + 2);
        self.tri(base, base + 2, base + 3);
    }

    /// A line segment. With `aa` the edges fade out over about a pixel.
    fn line(&mut self, a: Pos2, b: Pos2, width: f32, c: Color32, aa: bool) {
        let d = b - a;
        let len = d.length();
        if len < 0.01 {
            return;
        }
        let n = Vec2::new(-d.y, d.x) / len;
        if aa {
            self.reserve(8);
            let half = width * 0.5;
            let inner = (half - 0.5).max(0.0);
            let outer = half + 0.5;
            let base = self.cur.vertices.len() as u32;
            for p in [a, b] {
                self.vert(p - n * outer, WHITE_UV, Color32::TRANSPARENT);
                self.vert(p - n * inner, WHITE_UV, c);
                self.vert(p + n * inner, WHITE_UV, c);
                self.vert(p + n * outer, WHITE_UV, Color32::TRANSPARENT);
            }
            for k in 0..3u32 {
                self.tri(base + k, base + k + 1, base + 4 + k);
                self.tri(base + k + 1, base + 5 + k, base + 4 + k);
            }
        } else {
            self.reserve(4);
            let (half, c) = if width < 1.0 { (0.5, scale_alpha(c, width)) } else { (width * 0.5, c) };
            let base = self.cur.vertices.len() as u32;
            self.vert(a - n * half, WHITE_UV, c);
            self.vert(a + n * half, WHITE_UV, c);
            self.vert(b + n * half, WHITE_UV, c);
            self.vert(b - n * half, WHITE_UV, c);
            self.tri(base, base + 1, base + 2);
            self.tri(base, base + 2, base + 3);
        }
    }

    /// A filled convex polygon (corners in order round the outline), shifted by `off`.
    fn fan(&mut self, pts: &[[f32; 2]], off: Vec2, c: Color32) {
        if pts.len() < 3 {
            return;
        }
        self.reserve(pts.len());
        let base = self.cur.vertices.len() as u32;
        for q in pts {
            self.vert(Pos2::new(q[0], q[1]) + off, WHITE_UV, c);
        }
        for k in 1..pts.len() as u32 - 1 {
            self.tri(base, base + k, base + k + 1);
        }
    }

    fn triangle(&mut self, p: [Pos2; 3], c: Color32) {
        self.reserve(3);
        let base = self.cur.vertices.len() as u32;
        for q in p {
            self.vert(q, WHITE_UV, c);
        }
        self.tri(base, base + 1, base + 2);
    }

    fn finish(mut self) -> Vec<Mesh> {
        if !self.cur.vertices.is_empty() {
            self.done.push(self.cur);
        }
        self.done
    }
}

fn scale_alpha(c: Color32, k: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * k) as u8,
        (c.g() as f32 * k) as u8,
        (c.b() as f32 * k) as u8,
        (c.a() as f32 * k) as u8,
    )
}

fn col(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

impl App {
    fn update_highlight(&mut self) {
        let n = self.s.g.n();
        // a walk stops when the selection is no longer its current node
        if self.walk.as_ref().is_some_and(|w| self.s.sel_count != 1 || !self.s.sel.get(w.current() as usize).copied().unwrap_or(false)) {
            self.walk = None;
        }
        if let Some(w) = &self.walk {
            // while walking: the path and the next steps stand out; the rest as chosen
            let key = (w.version, self.walk_rest as u8);
            if Some(key) != self.walk_hi_key || self.focus.len() != n {
                let (nodes, edges) = w.marks(n, self.s.g.edges.len());
                self.focus = nodes.clone();
                self.hood = nodes;
                self.hi_edges = edges;
                self.hi_active = self.walk_rest != crate::walk::Rest::AsIs;
                self.walk_hi_key = Some(key);
                self.hi_key = (None, u64::MAX, false);
            }
            return;
        }
        self.walk_hi_key = None;
        let hover_key = if self.s.sel_count == 0 { self.hover } else { None };
        let key = (hover_key, self.s.sel_version, self.s.look.highlight_hood);
        if key == self.hi_key && self.focus.len() == n {
            return;
        }
        self.hi_key = key;
        self.focus = vec![false; n];
        self.hood = vec![false; n];
        let mut any = false;
        if self.s.sel_count > 0 {
            for i in 0..n {
                if self.s.sel[i] {
                    self.focus[i] = true;
                    any = true;
                }
            }
        } else if let Some(h) = hover_key {
            self.focus[h] = true;
            any = true;
        }
        if any {
            for i in 0..n {
                if self.focus[i] {
                    self.hood[i] = true;
                    for &v in self.s.g.neighbors(i) {
                        self.hood[v as usize] = true;
                    }
                }
            }
        }
        self.hi_active = any && self.s.look.highlight_hood;
    }

    fn run_simulation(&mut self, ui: &Ui) {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if self.s.tick_anim(dt) {
            ui.ctx().request_repaint();
        }
        if self.s.sim.running && !self.sim_paused {
            // a few milliseconds of layout per frame; a big layout takes several frames per iteration, so
            // dragging and drawing stay smooth however large the graph is
            let params = self.s.layout.force.clone();
            self.s.sim.step_for(&mut self.s.pos, &params, std::time::Duration::from_millis(6), 4);
            ui.ctx().request_repaint();
        }
    }

    pub fn canvas(&mut self, ui: &mut Ui) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        let size = rect.size();
        self.canvas_size = size;
        self.canvas_rect = rect;
        let painter = ui.painter_at(rect);
        let bg = self.s.look.background();
        // the graph may be see-through (Settings ▸ Translucency ▸ Graph)
        let board = crate::theme::translucency(ui.ctx()).board;
        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], (board * 255.0).round() as u8));

        if self.s.fit_requested {
            self.auto_fit = true;
            self.s.fit_requested = false;
        }
        if self.s.g.n() == 0 {
            self.empty_state(ui, rect, &painter);
            self.maximize_button(ui, rect);
            return;
        }

        let t_sim = std::time::Instant::now();
        self.run_simulation(ui);
        self.prof.add(0, t_sim);
        self.tick_view(ui);
        let t_in = std::time::Instant::now();
        self.handle_input(ui, &resp, rect);
        self.prof.add(1, t_in);

        // the 3D camera simply re-frames the visible nodes
        if self.auto_fit && self.s.is_3d() {
            self.s.fit_cam3();
            self.auto_fit = false;
        }
        // camera follows the layout while it is still moving
        if self.auto_fit {
            let target = self.s.fit_camera(size);
            let moving = self.s.sim.running || self.s.is_animating();
            let k = if moving { 0.18 } else { 0.3 };
            self.cam.center += (target.center - self.cam.center) * k;
            self.cam.zoom *= (target.zoom / self.cam.zoom).powf(k);
            if !moving && (target.zoom / self.cam.zoom - 1.0).abs() < 0.002 && (target.center - self.cam.center).length() * self.cam.zoom < 0.5 {
                self.cam = target;
                self.auto_fit = false;
            } else {
                ui.ctx().request_repaint();
            }
        }

        self.update_highlight();
        let walk_edges = self.walk.is_some() && self.walk_rest != crate::walk::Rest::AsIs;
        let hi = Highlight {
            focus: &self.focus,
            hood: &self.hood,
            active: self.hi_active,
            edges: if walk_edges { Some(&self.hi_edges) } else { None },
            hide: walk_edges && self.walk_rest == crate::walk::Rest::Hide,
        };
        // a huge graph is drawn thinner while it is dragged or zoomed, and in full as soon as the pointer rests
        let moving = resp.dragged() || (resp.hovered() && ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO));
        let many = self.s.g.edges.len() / LOD_EDGES;
        self.draw.edge_stride = if moving && many >= 1 { (many as u32 + 1).min(4) } else { 1 };
        if self.draw.edge_stride > 1 {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
        let t_b = std::time::Instant::now();
        build_draw_list(&self.s, &self.cam, size, &hi, &mut self.draw);
        self.prof.add(2, t_b);
        let t_p = std::time::Instant::now();
        self.paint_graph(&painter, rect);
        self.paint_walk(&painter, rect);
        self.paint_labels(&painter, rect);
        self.paint_section(&painter, rect);
        self.paint_overlays(&painter, rect);
        self.paint_region(&painter, rect);
        self.prof.add(3, t_p);
        if self.settings.show_toolbar || self.maximized {
            if let Some(tool) = self.toolbar(ui, rect) {
                self.run_tool(tool);
            }
        } else {
            self.view.buttons.clear();
            self.view.toolbar_rect = None;
        }

        self.maximize_button(ui, rect);
        self.walk_panel(ui, rect);

        // tooltip
        if let (Some(e), None, None, true) = (self.hover_edge, self.hover, self.box_start, self.settings.show_tooltips && self.section_drag.is_none()) {
            resp.clone().on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(320.0);
                self.edge_details(ui, e, false);
            });
        }
        if let (Some(h), None, None, true) = (self.hover, self.drag_node, self.box_start, self.settings.show_tooltips && self.section_drag.is_none()) {
            resp.clone().on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(320.0);
                ui.strong(self.s.label_of(h));
                egui::Grid::new("tip").num_columns(2).show(ui, |ui| {
                    ui.weak("id");
                    ui.label(&self.s.g.ids[h]);
                    ui.end_row();
                    ui.weak("connections");
                    ui.label(self.s.g.degree(h).to_string());
                    ui.end_row();
                    for c in self.s.g.node_cols.iter().filter(|c| !c.is_metric).take(10) {
                        ui.weak(&c.name);
                        ui.label(c.label_at(h).unwrap_or_else(|| "—".into()));
                        ui.end_row();
                    }
                });
            });
        }
    }

    /// The button in the top-right corner of the graph: full screen, or (when the graph is full screen) the way back.
    fn maximize_button(&mut self, ui: &mut Ui, canvas: Rect) {
        let on = self.maximized;
        let size = if on { Vec2::new(154.0, 30.0) } else { Vec2::splat(30.0) };
        let r = Rect::from_min_size(Pos2::new(canvas.right() - 12.0 - size.x, canvas.top() + 10.0), size);
        let id = ui.make_persistent_id("maximize_graph");
        let resp = ui.interact(r, id, Sense::click());
        let tip = if on { "Leave full screen  (Esc or F11)" } else { "Maximise the graph: full screen with its view controls  (F11)" };
        let resp = resp.on_hover_text(tip);
        let v = ui.visuals();
        let fill = if resp.is_pointer_button_down_on() {
            v.widgets.active.bg_fill
        } else if resp.hovered() {
            v.widgets.hovered.bg_fill
        } else {
            crate::theme::faded(v.window_fill(), 0.92)
        };
        let ink = if resp.hovered() { v.widgets.hovered.fg_stroke.color } else { v.text_color() };
        let line = v.widgets.noninteractive.bg_stroke.color;
        let p = ui.painter();
        p.rect_filled(r, 8.0, fill);
        p.rect_stroke(r, 8.0, Stroke::new(1.0, line), StrokeKind::Inside);
        let st = Stroke::new(1.6, ink);
        if on {
            // a cross, and what it does
            let c = Pos2::new(r.left() + 17.0, r.center().y);
            p.line_segment([c + Vec2::new(-4.5, -4.5), c + Vec2::new(4.5, 4.5)], st);
            p.line_segment([c + Vec2::new(-4.5, 4.5), c + Vec2::new(4.5, -4.5)], st);
            p.text(Pos2::new(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, "Leave full screen", FontId::proportional(12.5), ink);
        } else {
            // four corner arrows pointing outwards
            let c = r.center();
            for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = c + Vec2::new(sx * 6.5, sy * 6.5);
                p.line_segment([corner, corner + Vec2::new(-sx * 4.0, 0.0)], st);
                p.line_segment([corner, corner + Vec2::new(0.0, -sy * 4.0)], st);
            }
        }
        if resp.clicked() {
            let ctx = ui.ctx().clone();
            self.set_maximized(&ctx, !on);
        }
    }

    fn empty_state(&mut self, ui: &mut Ui, rect: Rect, painter: &Painter) {
        let c = self.s.look.text_color();
        let tc = Color32::from_rgb(c[0], c[1], c[2]);
        let top = rect.center().y - 150.0;
        painter.text(Pos2::new(rect.center().x, top), Align2::CENTER_CENTER, "Graphsight", FontId::proportional(34.0), tc);
        painter.text(
            Pos2::new(rect.center().x, top + 42.0),
            Align2::CENTER_CENTER,
            "Open your files, or drop an edges CSV (source, target, …),\nan optional nodes CSV (id, attributes…) or a JSON graph here.",
            FontId::proportional(15.0),
            tc.gamma_multiply(0.7),
        );
        // the three ways in
        let (w, h, gap) = (170.0, 34.0, 10.0);
        let y = top + 100.0;
        let x0 = rect.center().x - (3.0 * w + 2.0 * gap) * 0.5;
        let place = |k: f32| Rect::from_min_size(Pos2::new(x0 + k * (w + gap), y), Vec2::new(w, h));
        if ui.put(place(0.0), egui::Button::new("Open edges file…")).clicked() {
            self.pick_and_load(Some(PendKindHint::Edges));
        }
        if ui.put(place(1.0), egui::Button::new("Open nodes file…")).clicked() {
            self.pick_and_load(Some(PendKindHint::Nodes));
        }
        if ui.put(place(2.0), egui::Button::new("Open session…")).clicked() {
            self.open_project_dialog();
        }
        // recent sessions
        let recent: Vec<String> = self.settings.recent.iter().take(6).cloned().collect();
        if !recent.is_empty() {
            let mut y = y + h + 34.0;
            painter.text(Pos2::new(rect.center().x, y), Align2::CENTER_CENTER, "Recent sessions", FontId::proportional(13.0), tc.gamma_multiply(0.6));
            y += 16.0;
            for r in recent {
                let path = std::path::PathBuf::from(&r);
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.clone());
                let row = Rect::from_min_size(Pos2::new(rect.center().x - 170.0, y), Vec2::new(340.0, 26.0));
                if ui.put(row, egui::Button::new(name).frame(false)).on_hover_text(&r).clicked() {
                    self.open_project(&path);
                }
                y += 28.0;
            }
        }
    }

    fn handle_input(&mut self, ui: &mut Ui, resp: &egui::Response, rect: Rect) {
        let size = rect.size();
        let ptr = resp.hover_pos().or_else(|| resp.interact_pointer_pos()).map(|p| p - rect.min);
        let mods = ui.input(|i| i.modifiers);

        // zoom around the cursor
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta().y);
            let pinch = ui.input(|i| i.zoom_delta());
            let factor = pinch * (scroll * 0.0016).exp();
            if (factor - 1.0).abs() > 1e-4 {
                self.view.anim = None;
            }
            if (factor - 1.0).abs() > 1e-4 && self.s.is_3d() {
                let c = ptr.map_or(Vec2::ZERO, |p| Vec2::new(p.x, p.y) - size * 0.5);
                self.s.cam3.zoom_at(factor, c, size);
                self.s.cam3.clamp();
            } else if (factor - 1.0).abs() > 1e-4 {
                if let Some(p) = ptr {
                    let p = Vec2::new(p.x, p.y);
                    let before = self.cam.to_world(p, size);
                    self.cam.zoom = (self.cam.zoom * factor).clamp(0.002, 80.0);
                    let after = self.cam.to_world(p, size);
                    self.cam.center += before - after;
                    self.auto_fit = false;
                }
            }
        }

        if resp.dragged() {
            self.view.anim = None;
        }
        // zoom-to-region tool: the drag draws a rectangle instead of moving the view
        if self.view.region_on {
            self.region_input(ui, resp, rect);
            self.pan_input(resp, size);
            self.view_keys(ui);
            return;
        }

        // a section line under the pointer is grabbed before any node
        let on_line = match (resp.hovered(), ptr, self.section_drag) {
            (_, _, Some(h)) => h,
            (true, Some(p), None) => self.section_hit(Vec2::new(p.x, p.y), size),
            _ => [None, None],
        };
        if on_line != [None, None] {
            ui.ctx().set_cursor_icon(match on_line {
                [Some(_), Some(_)] => egui::CursorIcon::ResizeNwSe,
                [Some(_), None] => egui::CursorIcon::ResizeHorizontal,
                _ => egui::CursorIcon::ResizeVertical,
            });
        }

        let on_arrow = match (resp.hovered(), ptr, self.section_drag3) {
            (_, _, Some(h)) => Some(h),
            (true, Some(p), None) => self.section_hit3(Vec2::new(p.x, p.y), size),
            _ => None,
        };
        if on_arrow.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // hover
        if self.drag_node.is_none() && self.box_start.is_none() {
            self.hover = match (resp.hovered(), ptr) {
                (true, Some(p)) if on_line == [None, None] && on_arrow.is_none() => pick_node(&self.s, &self.cam, size, Vec2::new(p.x, p.y)),
                _ => None,
            };
            if self.hover.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            // with no node under the pointer, an edge may be: looked up again only when the pointer or the view moved
            let busy = ui.input(|i| i.pointer.any_down());
            let probe = match (self.hover, resp.hovered(), ptr) {
                (None, true, Some(p)) if !busy && on_line == [None, None] && on_arrow.is_none() && !self.view.region_on => Some(Vec2::new(p.x, p.y)),
                _ => None,
            };
            let same = |at: &Option<(Vec2, ViewCam)>, p: Vec2| at.is_some_and(|(q, c)| q == p && c.center == self.cam.center && c.zoom == self.cam.zoom);
            self.hover_edge = match probe {
                Some(p) if same(&self.hover_edge_at, p) => self.hover_edge,
                Some(p) => {
                    self.hover_edge_at = Some((p, self.cam));
                    if self.s.g.edges.len() <= 300_000 { pick_edge(&self.s, &self.cam, size, p) } else { None }
                }
                None => {
                    self.hover_edge_at = None;
                    None
                }
            };
        }

        // drag start: egui reports it only after the pointer has moved a few pixels, so look at where
        // the button went down rather than where the pointer is now
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            let space = ui.input(|i| i.key_down(egui::Key::Space));
            self.view.pan_drag = space;
            let line = origin.map_or([None, None], |o| self.section_hit(o - rect.min, size));
            if space {
                self.view.space_used = true;
            } else if line != [None, None] {
                self.section_drag = Some(line);
            } else if let Some(h) = origin.and_then(|o| self.section_hit3(o - rect.min, size)) {
                self.section_drag3 = Some(h);
            } else if mods.shift {
                self.box_start = origin.or_else(|| resp.interact_pointer_pos());
            } else {
                let grabbed = origin.and_then(|o| pick_node(&self.s, &self.cam, size, o - rect.min));
                if let Some(h) = grabbed {
                    if !self.s.sel[h] {
                        if !(mods.ctrl || mods.command) {
                            self.s.clear_selection();
                        }
                        self.s.set_selected(h, true);
                    }
                    // with coordinates the nodes stay where their data puts them
                    if !self.s.coords_active() {
                        self.drag_node = Some(h);
                    }
                }
            }
        }
        // dragging
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some((axis, side)) = self.section_drag3 {
                if let Some((_, _, _, _, sv)) = self.section_handles(size).into_iter().find(|h| h.0 == axis && h.1 == side) {
                    let d = resp.drag_delta();
                    let t = (d.x * sv.x + d.y * sv.y) / sv.length_sq().max(1e-9);
                    let sec = &mut self.s.section;
                    if side == 0 { sec.min[axis] += t } else { sec.max[axis] += t }
                    sec.normalise();
                }
                ui.ctx().request_repaint();
            } else if let Some(handle) = self.section_drag {
                let delta = resp.drag_delta() / self.cam.zoom;
                let sec = &mut self.s.section;
                for (axis, side) in handle.iter().enumerate() {
                    if let Some(side) = side {
                        let d = if axis == 0 { delta.x } else { delta.y };
                        if *side == 0 { sec.min[axis] += d } else { sec.max[axis] += d }
                    }
                }
                self.s.section.normalise();
                ui.ctx().request_repaint();
            } else if let Some(h) = self.drag_node {
                let delta = resp.drag_delta() / self.cam.zoom;
                if self.s.sel[h] && self.s.sel_count > 1 {
                    for i in 0..self.s.g.n() {
                        if self.s.sel[i] {
                            self.s.pos[i] += delta;
                            self.s.sim.pin(i, true);
                        }
                    }
                } else {
                    self.s.pos[h] += delta;
                    self.s.sim.pin(h, true);
                }
                if self.s.layout.kind == LayoutKind::Force {
                    self.s.sim.reheat(0.3);
                    self.sim_paused = false;
                }
                self.auto_fit = false;
                ui.ctx().request_repaint();
            } else if self.box_start.is_none() && (self.view.pan_drag || (self.s.is_3d() && self.view.nav == NavMode::Pan)) {
                self.pan_by(resp.drag_delta(), size);
                ui.ctx().request_repaint();
            } else if self.box_start.is_none() && self.s.is_3d() && self.view.nav == NavMode::Zoom {
                let at = ui.input(|i| i.pointer.press_origin()).map_or(Vec2::ZERO, |o| o - rect.min - size * 0.5);
                self.s.cam3.zoom_at((-resp.drag_delta().y * 0.01).exp(), at, size);
                self.s.cam3.clamp();
                ui.ctx().request_repaint();
            } else if self.box_start.is_none() && self.s.is_3d() {
                let d = resp.drag_delta();
                // with something selected the view turns around the selection, otherwise around the view's own centre
                match self.s.selection_pivot3() {
                    Some(pivot) => self.s.cam3.orbit_about(pivot, -d.x * 0.008, d.y * 0.008, size),
                    None => {
                        self.s.cam3.yaw -= d.x * 0.008;
                        self.s.cam3.pitch += d.y * 0.008;
                    }
                }
                self.s.cam3.clamp();
                ui.ctx().request_repaint();
            } else if self.box_start.is_none() {
                self.cam.center -= resp.drag_delta() / self.cam.zoom;
                self.auto_fit = false;
            } else {
                ui.ctx().request_repaint();
            }
        }
        self.pan_input(resp, size);
        // drag end
        if resp.drag_stopped() {
            self.view.pan_drag = false;
            self.section_drag = None;
            self.section_drag3 = None;
            if let Some(start) = self.box_start.take() {
                if let Some(end) = resp.interact_pointer_pos() {
                    let r = Rect::from_two_pos(start, end);
                    if !(mods.ctrl || mods.command) {
                        self.s.clear_selection();
                    }
                    let proj = self.s.projector(&self.cam, size);
                    for i in 0..self.s.g.n() {
                        if self.s.vis[i] && !self.s.section_hides(i) {
                            if let Some((sp, _, _)) = proj.at(&self.s, i) {
                                if r.contains((sp + rect.min.to_vec2()).to_pos2()) {
                                    self.s.set_selected(i, true);
                                }
                            }
                        }
                    }
                }
            }
            if self.drag_node.take().is_some() && !self.pin_after_drag {
                self.s.sim.unpin_all();
            }
        }

        // clicks
        if resp.clicked() {
            match self.hover {
                Some(h) => {
                    if mods.shift || mods.ctrl || mods.command {
                        self.s.toggle_selected(h);
                    } else {
                        self.s.select_only(h);
                    }
                }
                None => {
                    let extend = mods.shift || mods.ctrl || mods.command;
                    // no node under the pointer: an edge may be
                    let edge = resp.interact_pointer_pos().and_then(|p| {
                        let p = p - rect.min;
                        pick_edge(&self.s, &self.cam, size, Vec2::new(p.x, p.y))
                    });
                    match edge {
                        Some(e) if extend => self.s.toggle_edge_selected(e),
                        Some(e) => self.s.select_only_edge(e),
                        None if !extend => self.s.clear_selection(),
                        None => {}
                    }
                }
            }
        }
        if resp.double_clicked() {
            match self.hover {
                Some(h) => {
                    self.s.select_only(h);
                    self.s.select_neighbors();
                }
                None => self.auto_fit = true,
            }
        }

        // keyboard (not while typing, nor in the frame a text box let go of the keys: Esc that leaves a search box
        // must not also clear the selection)
        let typing = ui.ctx().egui_wants_keyboard_input();
        let was_typing = std::mem::replace(&mut self.was_typing, typing);
        if !typing && !was_typing {
            self.view_keys(ui);
            let (esc, del, all, space) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Delete),
                    i.key_pressed(egui::Key::A) && (i.modifiers.ctrl || i.modifiers.command),
                    i.key_released(egui::Key::Space),
                )
            });
            // Space + drag pans, so the layout is paused or resumed when Space is let go without a drag
            let space_pan = space && std::mem::take(&mut self.view.space_used);
            let space = space && !space_pan;
            // arrow keys walk the graph from the selected node
            let (left, right, up, down) = ui.input(|i| {
                let plain = i.modifiers.is_none();
                (
                    plain && i.key_pressed(egui::Key::ArrowLeft),
                    plain && i.key_pressed(egui::Key::ArrowRight),
                    plain && i.key_pressed(egui::Key::ArrowUp),
                    plain && i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if left || right || up || down {
                self.walk_key(ui.ctx(), left, right, up, down);
            }
            if esc {
                // the first Esc stops a walk (keeping the node selected), the next clears the selection
                if self.walk.is_some() {
                    self.walk = None;
                } else {
                    self.s.clear_selection();
                }
            }
            if del && self.s.sel_count > 0 {
                self.s.hide_selected();
                self.table.dirty = true;
            }
            if all {
                for i in 0..self.s.g.n() {
                    if self.s.vis[i] {
                        self.s.set_selected(i, true);
                    }
                }
            }
            if space && self.s.layout.kind == LayoutKind::Force {
                if self.s.sim.running {
                    self.s.sim.running = false;
                    self.sim_paused = true;
                } else {
                    self.s.sim.reheat(0.5);
                    self.sim_paused = false;
                }
            }
        }
    }

    /// Slide the view by a screen-space drag.
    fn pan_by(&mut self, delta: Vec2, size: Vec2) {
        if self.s.is_3d() {
            let o = self.s.cam3.orbit(size);
            let d = delta / (o.focal / self.s.cam3.dist);
            for k in 0..3 {
                self.s.cam3.target[k] += -o.right[k] * d.x + o.up[k] * d.y;
            }
        } else {
            self.cam.center -= delta / self.cam.zoom;
            self.auto_fit = false;
        }
    }

    /// Middle- or right-drag pans.
    fn pan_input(&mut self, resp: &egui::Response, size: Vec2) {
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            self.pan_by(resp.drag_delta(), size);
        }
    }

    /// Keyboard shortcuts of the view tools. Plain keys only: Ctrl+S and friends belong to the menus.
    fn view_keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        use egui::Key;
        let (f, s, r, home, back, fwd, esc, plain) = ui.input(|i| {
            let plain = !i.modifiers.ctrl && !i.modifiers.command && !i.modifiers.alt && !i.modifiers.shift;
            (
                i.key_pressed(Key::F) && plain,
                i.key_pressed(Key::S) && plain,
                i.key_pressed(Key::R) && plain,
                i.key_pressed(Key::Home) && plain,
                i.key_pressed(Key::ArrowLeft) && i.modifiers.alt,
                i.key_pressed(Key::ArrowRight) && i.modifiers.alt,
                i.key_pressed(Key::Escape),
                plain,
            )
        });
        let _ = plain;
        if esc && self.view.cancel_region() {
            return;
        }
        for (hit, tool) in [(f, Tool::Fit), (s, Tool::Selection), (r, Tool::Region), (home, Tool::Home), (back, Tool::Back), (fwd, Tool::Forward)] {
            if hit {
                self.run_tool(tool);
            }
        }
    }

    /// While the region tool is on: a left-drag draws the rectangle, letting go zooms to it.
    fn region_input(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        self.hover = None;
        if resp.drag_started_by(egui::PointerButton::Primary) {
            self.view.region_start = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos());
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            ui.ctx().request_repaint();
        }
        if resp.drag_stopped() {
            if let (Some(a), Some(b)) = (self.view.region_start.take(), resp.interact_pointer_pos()) {
                let r = Rect::from_two_pos(a, b).intersect(rect).translate(-rect.min.to_vec2());
                self.zoom_to_rect(r);
            }
        }
    }

    /// The rectangle being dragged for zoom-to-region.
    fn paint_region(&self, painter: &Painter, rect: Rect) {
        if !self.view.region_on {
            return;
        }
        let tc = self.s.look.text_color();
        let c = Color32::from_rgb(tc[0], tc[1], tc[2]);
        if let (Some(a), Some(b)) = (self.view.region_start, painter.ctx().input(|i| i.pointer.latest_pos())) {
            let r = Rect::from_two_pos(a, b).intersect(rect);
            painter.rect_filled(r, 0.0, c.gamma_multiply(0.08));
            painter.add(egui::Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], egui::Stroke::new(1.5, c), 5.0, 4.0));
        } else {
            painter.text(rect.center_top() + Vec2::new(0.0, 14.0), Align2::CENTER_TOP, "Drag a rectangle to zoom to it  (Esc cancels)", FontId::proportional(13.0), c.gamma_multiply(0.85));
        }
    }

    fn paint_graph(&self, painter: &Painter, rect: Rect) {
        let off = rect.min.to_vec2();
        let look = &self.s.look;
        let width = look.edge_width;
        let n_edges = self.draw.edges.len() + self.draw.edges_back.len() + self.draw.edges_hi.len();
        let aa = n_edges <= 20_000;
        let arrows = self.s.g.directed && look.arrows && n_edges <= 15_000;
        self.paint_boxes(painter, off);
        for (li, list) in [&self.draw.edges_back, &self.draw.edges, &self.draw.edges_hi].into_iter().enumerate() {
            let width = if li == 2 { width * 2.0 + 0.4 } else { width };
            if list.is_empty() {
                continue;
            }
            let fill = |sink: &mut MeshSink, e: &DrawEdge| {
                let c = col(e.rgba);
                let (a, b) = (Pos2::new(e.a[0], e.a[1]) + off, Pos2::new(e.b[0], e.b[1]) + off);
                let w = width * e.w;
                if e.dash == 0 {
                    sink.line(a, b, w, c, aa);
                } else {
                    dashed_line(sink, a, b, w, e.dash, c, aa);
                }
                if arrows && e.directed {
                    if let Some(t) = arrow_points(e.a, e.b, e.target_r) {
                        sink.triangle([Pos2::new(t[0][0], t[0][1]) + off, Pos2::new(t[1][0], t[1][1]) + off, Pos2::new(t[2][0], t[2][1]) + off], c);
                    }
                }
            };
            if list.len() >= 2 * MESH_CHUNK {
                // build the pieces side by side, then add them in their original order
                use rayon::prelude::*;
                let pieces: Vec<Vec<Mesh>> = list
                    .par_chunks(MESH_CHUNK)
                    .map(|chunk| {
                        let per = if aa { 8 } else { 4 };
                        let mut sink = MeshSink::with_capacity(TextureId::default(), chunk.len() * per, chunk.len() * per * 3 / 2);
                        for e in chunk {
                            fill(&mut sink, e);
                        }
                        sink.finish()
                    })
                    .collect();
                for m in pieces.into_iter().flatten() {
                    painter.add(Shape::mesh(m));
                }
            } else {
                let mut sink = MeshSink::new(TextureId::default());
                for e in list.iter() {
                    fill(&mut sink, e);
                }
                for m in sink.finish() {
                    painter.add(Shape::mesh(m));
                }
            }
        }

        // the edge under the pointer: a lighter line over it
        if let Some(&(a, b)) = self.hover_edge.and_then(|e| self.s.g.edges.get(e)) {
            let proj = self.s.projector(&self.cam, self.canvas_size);
            if let (Some((pa, _, _)), Some((pb, _, _))) = (proj.at(&self.s, a as usize), proj.at(&self.s, b as usize)) {
                let t = look.text_color();
                let mut sink = MeshSink::new(TextureId::default());
                sink.line(Pos2::new(pa.x, pa.y) + off, Pos2::new(pb.x, pb.y) + off, (width * 2.0).max(2.5), Color32::from_rgba_unmultiplied(t[0], t[1], t[2], 150), true);
                for m in sink.finish() {
                    painter.add(Shape::mesh(m));
                }
            }
        }

        // selected edges: a thick accent line over the ordinary edges, under the nodes
        if !self.s.sel_edges.is_empty() {
            let proj = self.s.projector(&self.cam, self.canvas_size);
            let accent = Color32::from_rgb(10, 132, 255);
            let w = (width * 2.5).max(3.0);
            let mut sink = MeshSink::new(TextureId::default());
            for &e in &self.s.sel_edges {
                let Some(&(a, b)) = self.s.g.edges.get(e as usize) else { continue };
                if let (Some((pa, _, _)), Some((pb, _, _))) = (proj.at(&self.s, a as usize), proj.at(&self.s, b as usize)) {
                    sink.line(Pos2::new(pa.x, pa.y) + off, Pos2::new(pb.x, pb.y) + off, w, accent, true);
                }
            }
            for m in sink.finish() {
                painter.add(Shape::mesh(m));
            }
        }

        // nodes in three size classes (each uses a disc texture of matching resolution);
        // selection halos go first so they never cover a node of another size class
        let tex = [self.tex.small.id(), self.tex.mid.id(), self.tex.big.id()];
        let mut halos = [MeshSink::new(tex[0]), MeshSink::new(tex[1]), MeshSink::new(tex[2])];
        let mut nodes = [MeshSink::new(tex[0]), MeshSink::new(tex[1]), MeshSink::new(tex[2])];
        let halo = {
            let t = look.text_color();
            Color32::from_rgb(t[0], t[1], t[2])
        };
        // quad half-size so that the disc inside the texture ends up with radius r pixels
        let class_of = |r: f32| -> (usize, f32) {
            let (k, size) = if r < 4.5 { (0, 16.0) } else if r < 18.0 { (1, 64.0) } else { (2, 256.0) };
            (k, r * (size / 2.0) / (size / 2.0 - 1.5))
        };
        // other shapes than circles, and (in 3D) shaded solids, far ones first
        let mut flat = MeshSink::new(TextureId::default());
        let mut solids = MeshSink::new(TextureId::default());
        for n in &self.draw.nodes {
            let pos = Pos2::new(n.x, n.y) + off;
            if !n.solid.is_empty() {
                solids.reserve(n.solid.len() * 3);
                for (t, c) in &n.solid {
                    let base = solids.cur.vertices.len() as u32;
                    for p in t {
                        solids.vert(Pos2::new(p[0], p[1]) + off, WHITE_UV, col(*c));
                    }
                    solids.tri(base, base + 1, base + 2);
                }
                continue;
            }
            if n.selected {
                let (k, half) = class_of(n.r + 3.0);
                halos[k].quad_uv(pos, half, halo);
            } else if n.ring {
                let (k, half) = class_of(n.r + 2.5);
                let c = crate::model::CHANGE_COLOR;
                halos[k].quad_uv(pos, half, Color32::from_rgb(c[0], c[1], c[2]));
            }
            if n.shape != 0 {
                let shape = crate::shapes::NodeShape::from_index(n.shape);
                flat.reserve(crate::shapes::flat_vertices(shape));
                crate::shapes::fill_flat(&mut flat.cur, shape, pos, n.r, col(n.rgba));
                continue;
            }
            let (k, half) = class_of(n.r);
            nodes[k].quad_uv(pos, half, col(n.rgba));
        }
        for sink in halos.into_iter().chain(nodes).chain([flat, solids]) {
            for m in sink.finish() {
                painter.add(Shape::mesh(m));
            }
        }
    }

    /// Bounding boxes, far to near, behind the edges and nodes: translucent faces, then the outline. Edges that
    /// run along the far side of a box are drawn fainter, which gives the box its depth.
    fn paint_boxes(&self, painter: &Painter, off: Vec2) {
        if self.draw.boxes.is_empty() {
            return;
        }
        let aa = self.draw.boxes.len() <= 2_000;
        let tex = TextureId::default();
        let fill_box = |sink: &mut MeshSink, b: &DrawBox| {
            if b.fill > 0 {
                let fill = Color32::from_rgba_unmultiplied(b.rgb[0], b.rgb[1], b.rgb[2], b.fill);
                for poly in b.polygons() {
                    sink.fan(poly, off, fill);
                }
            }
            if b.line > 0 {
                for (p, q, alpha) in b.lines() {
                    let c = Color32::from_rgba_unmultiplied(b.rgb[0], b.rgb[1], b.rgb[2], alpha);
                    sink.line(Pos2::new(p[0], p[1]) + off, Pos2::new(q[0], q[1]) + off, b.width, c, aa);
                }
            }
        };
        // far-to-near order is kept: one mesh (split at the vertex limit) in drawing order
        let mut sink = MeshSink::new(tex);
        for b in &self.draw.boxes {
            fill_box(&mut sink, b);
        }
        for m in sink.finish() {
            painter.add(Shape::mesh(m));
        }
    }

    /// Which section lines (flat view) are within reach of `p`, given relative to the canvas: `[x line, y line]`, each
    /// the side (0 = lower, 1 = upper) that was hit. Both set means a corner.
    pub fn section_hit(&self, p: Vec2, size: Vec2) -> [Option<usize>; 2] {
        let sec = &self.s.section;
        if !sec.enabled || !sec.placed || !sec.show_frame || self.s.is_3d() || self.view.region_on {
            return [None, None];
        }
        let grab = 6.0;
        let at = |x: f32, y: f32| self.cam.to_screen(Vec2::new(x, y), size);
        let (lo, hi) = (at(sec.min[0], sec.min[1]), at(sec.max[0], sec.max[1]));
        // the span of a line along the other axis: the box, or unlimited where the box has no end
        let span = |axis_on: [bool; 2], a: f32, b: f32| -> (f32, f32) {
            (if axis_on[0] { a - grab } else { f32::NEG_INFINITY }, if axis_on[1] { b + grab } else { f32::INFINITY })
        };
        let (y0, y1) = span(sec.side_on[1], lo.y, hi.y);
        let (x0, x1) = span(sec.side_on[0], lo.x, hi.x);
        let mut hit = [None, None];
        for (side, x) in [(0usize, lo.x), (1, hi.x)] {
            if sec.side_on[0][side] && (p.x - x).abs() <= grab && p.y >= y0 && p.y <= y1 {
                hit[0] = Some(side);
            }
        }
        for (side, y) in [(0usize, lo.y), (1, hi.y)] {
            if sec.side_on[1][side] && (p.y - y).abs() <= grab && p.x >= x0 && p.x <= x1 {
                hit[1] = Some(side);
            }
        }
        hit
    }

    /// The 3D section arrows: (axis, side, centre on screen, arrow tip, screen movement per world unit along +axis).
    pub fn section_handles(&self, size: Vec2) -> Vec<(usize, usize, Pos2, Pos2, Vec2)> {
        let sec = &self.s.section;
        let mut out = Vec::new();
        if !self.s.is_3d() || !sec.enabled || !sec.placed || !sec.show_frame || self.view.region_on {
            return out;
        }
        let o = self.s.cam3.orbit(size);
        for axis in 0..3 {
            for side in 0..2 {
                if !sec.side_on[axis][side] {
                    continue;
                }
                let mut c3 = [0.0f32; 3];
                for j in 0..3 {
                    c3[j] = if j == axis { if side == 0 { sec.min[j] } else { sec.max[j] } } else { (sec.min[j] + sec.max[j]) * 0.5 };
                }
                let unit = ((sec.max[axis] - sec.min[axis]) * 0.1).max(1e-3);
                let mut c4 = c3;
                c4[axis] += unit;
                let (Some((p0, _, _)), Some((p1, _, _))) = (o.project(c3), o.project(c4)) else { continue };
                let sv = Vec2::new(p1.x - p0.x, p1.y - p0.y) / unit;
                let len = sv.length() * unit;
                if len < 3.0 {
                    continue; // looking straight along this axis: use the sliders
                }
                let dir = sv.normalized() * if side == 0 { -1.0 } else { 1.0 };
                let c = Pos2::new(p0.x, p0.y);
                out.push((axis, side, c, c + dir * 40.0, sv));
            }
        }
        out
    }

    /// The 3D section arrow within reach of `p` (relative to the canvas).
    pub fn section_hit3(&self, p: Vec2, size: Vec2) -> Option<(usize, usize)> {
        let p = Pos2::new(p.x, p.y);
        let mut best: Option<(f32, usize, usize)> = None;
        for (axis, side, c, tip, _) in self.section_handles(size) {
            let ab = tip - c;
            let t = ((p - c).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            let d = (p - (c + ab * t)).length();
            let d = if (p - tip).length() < 12.0 { d.min((p - tip).length() - 4.0) } else { d };
            if d <= 8.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, axis, side));
            }
        }
        best.map(|b| (b.1, b.2))
    }

    /// The section box: a rectangle with draggable sides in the flat view, a wire box in 3D.
    fn paint_section(&self, painter: &Painter, rect: Rect) {
        let sec = &self.s.section;
        if !self.s.section_active() || !sec.show_frame {
            return;
        }
        let off = rect.min.to_vec2();
        let size = rect.size();
        let t = self.s.look.text_color();
        let ink = Color32::from_rgb(t[0], t[1], t[2]);
        let on_line = self.section_drag.unwrap_or([None, None]);
        if !self.s.is_3d() {
            let at = |x: f32, y: f32| self.cam.to_screen(Vec2::new(x, y), size) + off;
            let (lo, hi) = (at(sec.min[0], sec.min[1]), at(sec.max[0], sec.max[1]));
            let bg = self.s.look.background();
            let shade = Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], 70);
            // shade the part that is cut away
            if !sec.invert {
                if sec.side_on[0][0] {
                    painter.rect_filled(Rect::from_min_max(rect.left_top(), Pos2::new(lo.x.clamp(rect.left(), rect.right()), rect.bottom())), 0.0, shade);
                }
                if sec.side_on[0][1] {
                    painter.rect_filled(Rect::from_min_max(Pos2::new(hi.x.clamp(rect.left(), rect.right()), rect.top()), rect.right_bottom()), 0.0, shade);
                }
                if sec.side_on[1][0] {
                    painter.rect_filled(Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), lo.y.clamp(rect.top(), rect.bottom()))), 0.0, shade);
                }
                if sec.side_on[1][1] {
                    painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), hi.y.clamp(rect.top(), rect.bottom())), rect.right_bottom()), 0.0, shade);
                }
            }
            let (y_from, y_to) = (if sec.side_on[1][0] { lo.y } else { rect.top() }, if sec.side_on[1][1] { hi.y } else { rect.bottom() });
            let (x_from, x_to) = (if sec.side_on[0][0] { lo.x } else { rect.left() }, if sec.side_on[0][1] { hi.x } else { rect.right() });
            let clip = painter.with_clip_rect(rect);
            for (side, x) in [(0usize, lo.x), (1, hi.x)] {
                if sec.side_on[0][side] {
                    let w = if on_line[0] == Some(side) { 2.6 } else { 1.6 };
                    clip.add(Shape::dashed_line(&[Pos2::new(x, y_from), Pos2::new(x, y_to)], Stroke::new(w, ink), 8.0, 5.0));
                    {
                        let mid = Pos2::new(x, (y_from + y_to) * 0.5);
                        let s = if side == 0 { -1.0 } else { 1.0 };
                        let (shaft_to, tip) = (mid + Vec2::new(s * 20.0, 0.0), mid + Vec2::new(s * 30.0, 0.0));
                        clip.line_segment([mid, shaft_to], Stroke::new(w, ink));
                        clip.add(Shape::convex_polygon(vec![tip, shaft_to + Vec2::new(0.0, -6.0), shaft_to + Vec2::new(0.0, 6.0)], ink, Stroke::NONE));
                    }
                }
            }
            for (side, y) in [(0usize, lo.y), (1, hi.y)] {
                if sec.side_on[1][side] {
                    let w = if on_line[1] == Some(side) { 2.6 } else { 1.6 };
                    clip.add(Shape::dashed_line(&[Pos2::new(x_from, y), Pos2::new(x_to, y)], Stroke::new(w, ink), 8.0, 5.0));
                    {
                        let mid = Pos2::new((x_from + x_to) * 0.5, y);
                        let s = if side == 0 { -1.0 } else { 1.0 };
                        let (shaft_to, tip) = (mid + Vec2::new(0.0, s * 20.0), mid + Vec2::new(0.0, s * 30.0));
                        clip.line_segment([mid, shaft_to], Stroke::new(w, ink));
                        clip.add(Shape::convex_polygon(vec![tip, shaft_to + Vec2::new(-6.0, 0.0), shaft_to + Vec2::new(6.0, 0.0)], ink, Stroke::NONE));
                    }
                }
            }
            return;
        }
        // 3D: the twelve edges; one that lies in two switched-on planes is drawn strongly, in one plane faintly
        let o = self.s.cam3.orbit(size);
        for (k, &(a, b, axis)) in crate::section::EDGES.iter().enumerate() {
            let _ = k;
            let (p, q) = (crate::section::corner(sec, a), crate::section::corner(sec, b));
            let mut planes_on = 0;
            for j in (0..3).filter(|&j| j != axis) {
                let side = if p[j] == sec.min[j] { 0 } else { 1 };
                if sec.side_on[j][side] {
                    planes_on += 1;
                }
            }
            if planes_on == 0 {
                continue;
            }
            if let Some((s0, s1)) = o.clip_segment(p, q) {
                let alpha = if planes_on == 2 { 230 } else { 90 };
                painter.line_segment([s0.to_pos2() + off, s1.to_pos2() + off], Stroke::new(1.5, Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), alpha)));
            }
        }
        // an arrow on every active side: drag it to push the side into the graph or pull it out
        for (axis, side, c, tip, _) in self.section_handles(size) {
            let w = if self.section_drag3 == Some((axis, side)) { 3.2 } else { 2.2 };
            let (c, tip) = (c + off, tip + off);
            let d = (tip - c).normalized();
            let n = Vec2::new(-d.y, d.x);
            painter.circle_filled(c, 3.5, ink);
            painter.line_segment([c, tip - d * 8.0], Stroke::new(w, ink));
            painter.add(Shape::convex_polygon(vec![tip, tip - d * 12.0 + n * 6.0, tip - d * 12.0 - n * 6.0], ink, Stroke::NONE));
        }
    }

    fn paint_labels(&self, painter: &Painter, rect: Rect) {
        if self.s.look.labels == LabelMode::Never && self.hover.is_none() && self.s.sel_count == 0 {
            return;
        }
        let off = rect.min.to_vec2();
        let t = self.s.look.text_color();
        let font = FontId::proportional(11.5);
        let all = self.s.look.labels == LabelMode::Auto && self.draw.nodes.len() <= 160 && self.cam.zoom > 0.25;
        let mut shown = 0;
        let hood_labels = self.hi_active && self.hood.iter().filter(|h| **h).count() <= 60;
        for n in &self.draw.nodes {
            let i = n.idx as usize;
            let want = all && n.rgba[3] > 100
                || n.selected && self.s.sel_count <= 150
                || Some(i) == self.hover
                || hood_labels && self.hood[i];
            if !want {
                continue;
            }
            shown += 1;
            if shown > 400 {
                break;
            }
            let alpha = if n.rgba[3] < 255 { 90 } else { 230 };
            let color = Color32::from_rgba_unmultiplied(t[0], t[1], t[2], alpha);
            let p = Pos2::new(n.x + n.r + 3.0, n.y) + off;
            painter.text(p, Align2::LEFT_CENTER, self.s.label_of(i), font.clone(), color);
        }
    }

    /// Small X / Y / Z axes in the corner that turn with the camera.
    fn paint_gizmo(&self, painter: &Painter, rect: Rect, tc: Color32) {
        let o = self.s.cam3.orbit(rect.size());
        let centre = rect.right_bottom() + Vec2::new(-58.0, -58.0);
        let len = 34.0;
        painter.circle_filled(centre, len + 12.0, Color32::from_rgba_unmultiplied(128, 128, 140, 22));
        let mut axes = [
            ("X", [1.0f32, 0.0, 0.0], Color32::from_rgb(255, 99, 90)),
            ("Y", [0.0, 1.0, 0.0], Color32::from_rgb(90, 210, 130)),
            ("Z", [0.0, 0.0, 1.0], Color32::from_rgb(10, 132, 255)),
        ];
        // axes pointing away from us go first so the near ones sit on top
        let depth = |a: &[f32; 3]| a[0] * o.fwd[0] + a[1] * o.fwd[1] + a[2] * o.fwd[2];
        axes.sort_by(|a, b| depth(&b.1).total_cmp(&depth(&a.1)));
        for (name, a, c) in axes {
            let (dx, dy) = o.axis(a);
            let end = centre + Vec2::new(dx, -dy) * len;
            painter.line_segment([centre, end], Stroke::new(2.2, c));
            painter.circle_filled(end, 7.0, c);
            painter.text(end, Align2::CENTER_CENTER, name, FontId::proportional(9.5), Color32::WHITE);
        }
        let _ = tc;
    }

    fn paint_overlays(&self, painter: &Painter, rect: Rect) {
        let t = self.s.look.text_color();
        let tc = Color32::from_rgba_unmultiplied(t[0], t[1], t[2], 170);
        let hidden = self.s.n_vis != self.s.g.n();
        let mut txt = format!("{} nodes · {} edges", self.s.n_vis, self.s.n_vis_edges);
        if hidden {
            txt = format!("{} of {} nodes · {} of {} edges", self.s.n_vis, self.s.g.n(), self.s.n_vis_edges, self.s.g.edges.len());
        }
        if self.s.sel_count > 0 {
            txt += &format!(" · {} selected", self.s.sel_count);
        }
        if !self.s.sel_edges.is_empty() {
            txt += &format!(" · {} edge{} selected", self.s.sel_edges.len(), if self.s.sel_edges.len() == 1 { "" } else { "s" });
        }
        if self.s.section_active() {
            txt += if self.s.section.outside == crate::section::Outside::Hide { " · section box (outside hidden)" } else { " · section box" };
        }
        if self.s.sim.running && !self.sim_paused {
            txt += &format!(" · layout running ({:.0}%)", (1.0 - (self.s.sim.alpha - crate::layout::ALPHA_MIN).max(0.0)) * 100.0);
        } else if self.sim_paused && self.s.layout.kind == LayoutKind::Force {
            txt += " · layout paused";
        }
        if self.s.is_3d() {
            txt += if self.s.has_selection() { " · drag to orbit around the selection · right-drag to pan · scroll to zoom" } else { " · drag to orbit · right-drag to pan · scroll to zoom" };
        }
        painter.text(rect.left_top() + Vec2::new(10.0, 10.0), Align2::LEFT_TOP, txt, FontId::proportional(12.0), tc);
        if self.s.is_3d() {
            self.paint_gizmo(painter, rect, tc);
        }

        if let Some(start) = self.box_start {
            if let Some(cur) = painter.ctx().pointer_latest_pos() {
                let r = Rect::from_two_pos(start, cur);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 170, 255, 36));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(110, 170, 255)), StrokeKind::Inside);
            }
        }
    }
}

/// A line drawn in one of the `DASHES` patterns; the pattern grows with the line width.
fn dashed_line(sink: &mut MeshSink, a: Pos2, b: Pos2, w: f32, dash: u8, c: Color32, aa: bool) {
    let (d, g, _) = crate::model::DASHES[dash as usize % crate::model::DASHES.len()];
    let unit = w.max(1.2);
    let (d, g) = (d * unit, g * unit);
    let v = b - a;
    let len = v.length();
    if !len.is_finite() || len < 0.5 || d <= 0.0 {
        sink.line(a, b, w, c, aa);
        return;
    }
    let dir = v / len;
    // dash-dot: a dash, then a dot, in one period
    let dot = dash == 4;
    let period = if dot { d + g + unit + g } else { d + g };
    let mut t = 0.0f32;
    let mut guard = 0;
    while t < len && guard < 600 {
        let e = (t + d).min(len);
        sink.line(a + dir * t, a + dir * e, w, c, aa);
        if dot {
            let s0 = t + d + g;
            if s0 < len {
                sink.line(a + dir * s0, a + dir * (s0 + unit).min(len), w, c, aa);
            }
        }
        t += period;
        guard += 1;
    }
}

// ───────────────────────── walking the graph ─────────────────────────

impl App {
    /// An arrow key: start a walk at the selected node, or step, go back, or choose the next step.
    pub fn walk_key(&mut self, ctx: &egui::Context, left: bool, right: bool, _up: bool, down: bool) {
        use crate::walk::Walk;
        let sel = self.s.selected_list();
        let on_track = self.walk.as_ref().is_some_and(|w| sel.len() == 1 && sel[0] == w.current());
        if !on_track {
            if sel.len() != 1 {
                self.toast(ctx, "Select one node, then use the arrow keys to walk the graph from it");
                return;
            }
            self.walk = Some(Walk::start(&self.s.g, sel[0], self.walk_follow, &self.walk_shown()));
            if !right && !left {
                // the first ↑ / ↓ only starts the walk and shows the choices
                return;
            }
        }
        let follow = self.walk_follow;
        let shown = self.walk_shown();
        let Some(w) = self.walk.as_mut() else { return };
        let mut stuck = false;
        let moved = if right {
            let ok = w.forward(&self.s.g, follow, &shown);
            stuck = !ok;
            ok
        } else if left {
            w.back(&self.s.g, follow, &shown)
        } else {
            w.choose(if down { 1 } else { -1 });
            false
        };
        let at = w.current() as usize;
        if moved {
            self.s.select_only(at);
            self.keep_in_view(at);
        }
        if stuck {
            self.toast(ctx, "No way on from here (try another direction in the walk panel)");
        }
    }

    /// The nodes a walk may go through: everything on show, and what only the filter takes out (a node picked with
    /// the search box can still be walked from), but not nodes hidden by hand or outside a sub-graph.
    fn walk_shown(&self) -> Vec<bool> {
        (0..self.s.g.n()).map(|i| self.s.vis[i] || self.s.dimmed.get(i).copied().unwrap_or(false)).collect()
    }

    /// Bring a node into view when it is near the edge of the graph or outside it.
    fn keep_in_view(&mut self, i: usize) {
        let size = self.canvas_size;
        let Some((p, _, _)) = self.s.projector(&self.cam, size).at(&self.s, i) else { return };
        let inside = p.x > size.x * 0.12 && p.x < size.x * 0.88 && p.y > size.y * 0.12 && p.y < size.y * 0.88;
        if inside {
            return;
        }
        if self.s.is_3d() {
            self.s.cam3.target = self.s.pos3[i];
        } else {
            self.cam.center = self.s.pos[i];
            self.auto_fit = false;
        }
    }

    /// The walk drawn over the graph: the path as a bright band with step numbers, rings on its nodes, and thin
    /// lines to the possible next steps (the chosen one stronger). Colours of the graph itself are left alone.
    fn paint_walk(&self, painter: &Painter, rect: Rect) {
        let Some(w) = &self.walk else { return };
        let off = rect.min.to_vec2();
        let proj = self.s.projector(&self.cam, self.canvas_size);
        let at = |i: u32| proj.at(&self.s, i as usize).map(|(p, f, _)| (Pos2::new(p.x, p.y) + off, node_radius_px(self.s.style.node_r[i as usize], f)));
        let accent = Color32::from_rgb(10, 132, 255);
        let glow = Color32::from_rgba_unmultiplied(10, 132, 255, 70);
        let width = self.s.look.edge_width;
        // the next steps: thin lines, the chosen one stronger and dashed
        if let Some((from, _)) = at(w.current()) {
            for (k, &(_, v)) in w.next.iter().enumerate() {
                let Some((to, r)) = at(v) else { continue };
                if k == w.pick {
                    painter.extend(Shape::dashed_line(&[from, to], Stroke::new((width * 2.0).max(2.5), accent), 7.0, 4.0));
                    painter.circle_stroke(to, r + 4.0, Stroke::new(2.0, accent));
                } else {
                    painter.line_segment([from, to], Stroke::new((width * 1.2).max(1.2), Color32::from_rgba_unmultiplied(10, 132, 255, 110)));
                    painter.circle_stroke(to, r + 3.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(10, 132, 255, 150)));
                }
            }
        }
        // the path: a soft wide band under a firm line
        for pair in w.path.windows(2) {
            if let (Some((a, _)), Some((b, _))) = (at(pair[0].0), at(pair[1].0)) {
                painter.line_segment([a, b], Stroke::new((width * 5.0).max(7.0), glow));
                painter.line_segment([a, b], Stroke::new((width * 2.0).max(2.5), accent));
            }
        }
        let font = egui::FontId::proportional(11.0);
        let last = w.path.len() - 1;
        for (k, &(v, _)) in w.path.iter().enumerate() {
            let Some((p, r)) = at(v) else { continue };
            if k == last {
                painter.circle_stroke(p, r + 6.0, Stroke::new(3.0, accent));
                painter.circle_stroke(p, r + 10.0, Stroke::new(1.0, glow));
            } else {
                painter.circle_stroke(p, r + 3.5, Stroke::new(2.0, accent));
            }
            let label = Pos2::new(p.x + r + 7.0, p.y - r - 7.0);
            painter.circle_filled(label, 8.0, accent);
            painter.text(label, Align2::CENTER_CENTER, (k + 1).to_string(), font.clone(), Color32::WHITE);
        }
    }

    /// The small panel at the top of the graph while walking: where you are, what is next, and the choices.
    fn walk_panel(&mut self, ui: &mut Ui, rect: Rect) {
        use crate::theme::UiExt;
        use crate::walk::{Follow, Rest};
        let Some(w) = &self.walk else { return };
        let here = self.s.label_of(w.current() as usize);
        let next = w.picked().map(|(_, v)| self.s.label_of(v as usize));
        let (step, count, pick) = (w.path.len(), w.next.len(), w.pick);
        let mut follow = self.walk_follow;
        let mut rest = self.walk_rest;
        let mut stop = false;
        // bottom left, where it covers the least of the graph
        egui::Area::new(egui::Id::unique("walk_panel")).pivot(Align2::LEFT_BOTTOM).fixed_pos(rect.left_bottom() + Vec2::new(10.0, -64.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(330.0);
                ui.horizontal(|ui| {
                    ui.label(crate::theme::semibold(format!("Walking · step {step}")));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Stop").on_hover_text("Esc").clicked() {
                            stop = true;
                        }
                    });
                });
                ui.label(format!("At {here}"));
                match &next {
                    Some(n) => ui.label(format!("Next ({} of {count}): {n}", pick + 1)),
                    None => ui.weak("No way on from here"),
                };
                ui.add_space(4.0);
                ui.segmented(&mut follow, &[(Follow::Out, "Along arrows"), (Follow::In, "Against"), (Follow::Both, "Any edge")]);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.weak("Rest of the graph");
                    ui.segmented(&mut rest, &[(Rest::AsIs, "As is"), (Rest::Dim, "Dimmed"), (Rest::Hide, "Hidden")]);
                });
                ui.add_space(2.0);
                ui.weak("→ step   ← back   ↑ ↓ choose the next step   Esc stop");
            });
        });
        if stop {
            self.walk = None;
            return;
        }
        if rest != self.walk_rest {
            self.walk_rest = rest;
            self.walk_hi_key = None;
        }
        if follow != self.walk_follow {
            self.walk_follow = follow;
            let shown = self.walk_shown();
            if let Some(w) = self.walk.as_mut() {
                w.refresh(&self.s.g, follow, &shown);
            }
        }
    }
}

