//! The interactive graph view: input handling and fast mesh-based drawing.

use crate::app::*;
use crate::layout::LayoutKind;
use crate::model::*;
use crate::render::arrow_points;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureId, Ui, Vec2};

const MAX_VERTS: usize = 60_000;

/// Accumulates triangles into meshes that stay below the 16-bit-friendly vertex limit.
struct MeshSink {
    tex: TextureId,
    done: Vec<Mesh>,
    cur: Mesh,
}

impl MeshSink {
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
            let params = self.s.layout.force.clone();
            let t = std::time::Instant::now();
            let mut ticks = 0;
            while ticks < 4 && self.s.sim.running {
                self.s.sim.step(&mut self.s.pos, &params, 1);
                ticks += 1;
                if t.elapsed().as_secs_f32() > 0.008 {
                    break;
                }
            }
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

        self.run_simulation(ui);
        self.handle_input(ui, &resp, rect);

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
        build_draw_list(&self.s, &self.cam, size, &hi, &mut self.draw);
        self.paint_graph(&painter, rect);
        self.paint_labels(&painter, rect);
        self.paint_overlays(&painter, rect);

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
        painter.text(rect.center() - Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, "Graphy", FontId::proportional(34.0), tc);
        painter.text(
            rect.center() + Vec2::new(0.0, 0.0),
            Align2::CENTER_CENTER,
            "Drop an edges CSV (source, target, …) and optionally a nodes CSV (id, attributes…) here,\nor open a JSON graph.",
            FontId::proportional(15.0),
            tc.gamma_multiply(0.7),
        );
        let btn = Rect::from_center_size(rect.center() + Vec2::new(0.0, 60.0), Vec2::new(190.0, 30.0));
        if ui.put(btn, egui::Button::new("Load sample dataset")).clicked() {
            self.load_sample();
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
            if mods.shift {
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
                    self.drag_node = Some(h);
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
            } else if self.box_start.is_none() {
                self.cam.center -= resp.drag_delta() / self.cam.zoom;
                self.auto_fit = false;
            } else {
                ui.ctx().request_repaint();
            }
        }
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            self.cam.center -= resp.drag_delta() / self.cam.zoom;
            self.auto_fit = false;
        }
        // drag end
        if resp.drag_stopped() {
            if let Some(start) = self.box_start.take() {
                if let Some(end) = resp.interact_pointer_pos() {
                    let r = Rect::from_two_pos(start, end);
                    if !(mods.ctrl || mods.command) {
                        self.s.clear_selection();
                    }
                    for i in 0..self.s.g.n() {
                        if self.s.vis[i] {
                            let sp = self.cam.to_screen(self.s.pos[i], size) + rect.min.to_vec2();
                            if r.contains(sp.to_pos2()) {
                                self.s.set_selected(i, true);
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
            let (f, esc, del, all, space) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::F),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Delete),
                    i.key_pressed(egui::Key::A) && (i.modifiers.ctrl || i.modifiers.command),
                    i.key_pressed(egui::Key::Space),
                )
            });
            if f {
                self.auto_fit = true;
            }
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

    fn paint_graph(&self, painter: &Painter, rect: Rect) {
        let off = rect.min.to_vec2();
        let look = &self.s.look;
        let width = look.edge_width;
        let n_edges = self.draw.edges.len() + self.draw.edges_back.len();
        let aa = n_edges <= 20_000;
        let arrows = self.s.g.directed && look.arrows && n_edges <= 15_000;
        for list in [&self.draw.edges_back, &self.draw.edges] {
            if list.is_empty() {
                continue;
            }
            let mut sink = MeshSink::new(TextureId::default());
            for e in list.iter() {
                let c = col(e.rgba);
                sink.line(Pos2::new(e.a[0], e.a[1]) + off, Pos2::new(e.b[0], e.b[1]) + off, width, c, aa);
                if arrows {
                    if let Some(t) = arrow_points(e.a, e.b, e.target_r) {
                        sink.triangle([Pos2::new(t[0][0], t[0][1]) + off, Pos2::new(t[1][0], t[1][1]) + off, Pos2::new(t[2][0], t[2][1]) + off], c);
                    }
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
        for n in &self.draw.nodes {
            let pos = Pos2::new(n.x, n.y) + off;
            if n.selected {
                let (k, half) = class_of(n.r + 3.0);
                halos[k].quad_uv(pos, half, halo);
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
        painter.text(rect.left_bottom() + Vec2::new(10.0, -10.0), Align2::LEFT_BOTTOM, txt, FontId::proportional(12.0), tc);

        if let Some(start) = self.box_start {
            if let Some(cur) = painter.ctx().pointer_latest_pos() {
                let r = Rect::from_two_pos(start, cur);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 170, 255, 36));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(110, 170, 255)), StrokeKind::Inside);
            }
        }
    }
}
