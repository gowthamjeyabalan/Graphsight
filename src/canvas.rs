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
        painter.rect_filled(rect, 0.0, Color32::from_rgb(bg[0], bg[1], bg[2]));

        if self.s.fit_requested {
            self.auto_fit = true;
            self.s.fit_requested = false;
        }
        if self.s.g.n() == 0 {
            self.empty_state(ui, rect, &painter);
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
        let hi = Highlight { focus: &self.focus, hood: &self.hood, active: self.hi_active };
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
        self.paint_labels(&painter, rect);
        self.paint_overlays(&painter, rect);
        self.paint_region(&painter, rect);
        self.prof.add(3, t_p);
        if let Some(tool) = self.toolbar(ui, rect) {
            self.run_tool(tool);
        }

        // tooltip
        if let (Some(h), None, None) = (self.hover, self.drag_node, self.box_start) {
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

    fn empty_state(&mut self, ui: &mut Ui, rect: Rect, painter: &Painter) {
        let c = self.s.look.text_color();
        let tc = Color32::from_rgb(c[0], c[1], c[2]);
        let top = rect.center().y - 150.0;
        painter.text(Pos2::new(rect.center().x, top), Align2::CENTER_CENTER, "Grapht", FontId::proportional(34.0), tc);
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

        // hover
        if self.drag_node.is_none() && self.box_start.is_none() {
            self.hover = match (resp.hovered(), ptr) {
                (true, Some(p)) => pick_node(&self.s, &self.cam, size, Vec2::new(p.x, p.y)),
                _ => None,
            };
            if self.hover.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }

        // drag start: egui reports it only after the pointer has moved a few pixels, so look at where
        // the button went down rather than where the pointer is now
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin());
            let space = ui.input(|i| i.key_down(egui::Key::Space));
            self.view.pan_drag = space;
            if space {
                self.view.space_used = true;
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
            if let Some(h) = self.drag_node {
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
                self.s.cam3.yaw -= d.x * 0.008;
                self.s.cam3.pitch += d.y * 0.008;
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
            if let Some(start) = self.box_start.take() {
                if let Some(end) = resp.interact_pointer_pos() {
                    let r = Rect::from_two_pos(start, end);
                    if !(mods.ctrl || mods.command) {
                        self.s.clear_selection();
                    }
                    let proj = self.s.projector(&self.cam, size);
                    for i in 0..self.s.g.n() {
                        if self.s.vis[i] {
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
                    if !(mods.shift || mods.ctrl || mods.command) {
                        self.s.clear_selection();
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

        // keyboard
        if !ui.ctx().egui_wants_keyboard_input() {
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
            if esc {
                self.s.clear_selection();
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
                sink.line(Pos2::new(e.a[0], e.a[1]) + off, Pos2::new(e.b[0], e.b[1]) + off, width, c, aa);
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
        for n in &self.draw.nodes {
            let pos = Pos2::new(n.x, n.y) + off;
            if n.selected {
                let (k, half) = class_of(n.r + 3.0);
                halos[k].quad_uv(pos, half, halo);
            } else if n.ring {
                let (k, half) = class_of(n.r + 2.5);
                let c = crate::model::CHANGE_COLOR;
                halos[k].quad_uv(pos, half, Color32::from_rgb(c[0], c[1], c[2]));
            }
            let (k, half) = class_of(n.r);
            nodes[k].quad_uv(pos, half, col(n.rgba));
        }
        for sink in halos.into_iter().chain(nodes) {
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
        if self.s.sim.running && !self.sim_paused {
            txt += &format!(" · layout running ({:.0}%)", (1.0 - (self.s.sim.alpha - crate::layout::ALPHA_MIN).max(0.0)) * 100.0);
        } else if self.sim_paused && self.s.layout.kind == LayoutKind::Force {
            txt += " · layout paused";
        }
        if self.s.is_3d() {
            txt += " · drag to orbit · right-drag to pan · scroll to zoom";
        }
        painter.text(rect.left_bottom() + Vec2::new(10.0, -10.0), Align2::LEFT_BOTTOM, txt, FontId::proportional(12.0), tc);
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
