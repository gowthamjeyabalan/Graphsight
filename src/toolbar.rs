//! The floating view toolbar at the bottom of the graph: zoom to fit / selection / region, zoom steps, reset and
//! previous / next view; in 3D a second row with the drag mode, fixed viewing directions and the projection.
//! The icons are drawn with the painter, so they follow the theme and need no font or image files.

use crate::app::App;
use crate::coords::View3;
use crate::view::{NavMode, Tool};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};

const BTN: f32 = 32.0;
const GAP: f32 = 2.0;
const GROUP_GAP: f32 = 10.0;
const PAD: f32 = 5.0;
const MARGIN: f32 = 14.0;

/// Buttons are grouped; a gap separates groups.
fn main_row() -> Vec<Vec<Tool>> {
    vec![
        vec![Tool::Fit, Tool::Selection, Tool::Region],
        vec![Tool::ZoomIn, Tool::ZoomOut],
        vec![Tool::Home, Tool::Back, Tool::Forward],
    ]
}

fn row_3d() -> Vec<Vec<Tool>> {
    vec![
        vec![Tool::Nav(NavMode::Orbit), Tool::Nav(NavMode::Pan), Tool::Nav(NavMode::Zoom)],
        vec![Tool::Preset(View3::Top), Tool::Preset(View3::Front), Tool::Preset(View3::Side), Tool::Preset(View3::Iso)],
        vec![Tool::Perspective],
    ]
}

fn width_of(t: Tool) -> f32 {
    match t {
        Tool::Preset(_) => 42.0,
        Tool::Perspective => 52.0,
        _ => BTN,
    }
}

fn row_width(groups: &[Vec<Tool>]) -> f32 {
    let mut w = PAD * 2.0;
    for (gi, g) in groups.iter().enumerate() {
        if gi > 0 {
            w += GROUP_GAP;
        }
        for (ti, t) in g.iter().enumerate() {
            if ti > 0 {
                w += GAP;
            }
            w += width_of(*t);
        }
    }
    w
}

impl App {
    /// Draw the toolbar over the canvas and report the button that was clicked.
    pub fn toolbar(&mut self, ui: &mut Ui, canvas: Rect) -> Option<Tool> {
        self.view.buttons.clear();
        self.view.toolbar_rect = None;
        if canvas.width() < 120.0 || canvas.height() < 90.0 {
            return None;
        }
        let three_d = self.s.is_3d();
        let mut rows = vec![main_row()];
        if three_d {
            rows.push(row_3d());
        }
        let row_h = BTN + PAD * 2.0;
        let mut clicked = None;
        let mut whole: Option<Rect> = None;
        // the main row sits at the bottom, the 3D row above it
        for (ri, groups) in rows.iter().enumerate() {
            let w = row_width(groups).min(canvas.width() - 8.0);
            let y1 = canvas.bottom() - MARGIN - ri as f32 * (row_h + 6.0);
            let x0 = canvas.center().x - w * 0.5;
            let bar = Rect::from_min_max(Pos2::new(x0, y1 - row_h), Pos2::new(x0 + w, y1));
            whole = Some(whole.map_or(bar, |r| r.union(bar)));
            self.bar_background(ui, bar);
            let mut x = bar.left() + PAD;
            for (gi, g) in groups.iter().enumerate() {
                if gi > 0 {
                    x += GROUP_GAP;
                    let sx = x - GROUP_GAP * 0.5;
                    ui.painter().line_segment([Pos2::new(sx, bar.top() + 9.0), Pos2::new(sx, bar.bottom() - 9.0)], Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
                }
                for (ti, t) in g.iter().enumerate() {
                    if ti > 0 {
                        x += GAP;
                    }
                    let r = Rect::from_min_size(Pos2::new(x, bar.top() + PAD), Vec2::new(width_of(*t), BTN));
                    x += r.width();
                    if self.button(ui, *t, r) {
                        clicked = Some(*t);
                    }
                }
            }
        }
        self.view.toolbar_rect = whole;
        clicked
    }

    /// Background of one row. It also takes the clicks and drags that miss a button, so they do not reach the graph.
    fn bar_background(&self, ui: &mut Ui, bar: Rect) {
        let id = ui.make_persistent_id(("view_bar", (bar.top() * 10.0) as i32));
        ui.interact(bar, id, Sense::click_and_drag());
        let v = ui.visuals();
        let fill = v.window_fill().gamma_multiply(0.92);
        ui.painter().rect_filled(bar, 8.0, fill);
        ui.painter().rect_stroke(bar, 8.0, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
    }

    fn is_active(&self, t: Tool) -> bool {
        match t {
            Tool::Region => self.view.region_on,
            Tool::Nav(m) => self.view.nav == m,
            Tool::Perspective => self.s.cam3.ortho,
            _ => false,
        }
    }

    /// One button. Returns true when it was clicked and is usable.
    fn button(&mut self, ui: &mut Ui, t: Tool, r: Rect) -> bool {
        let enabled = self.tool_enabled(t);
        self.view.buttons.push((t, r, enabled));
        let id = ui.make_persistent_id(("view_btn", t));
        let resp = ui.interact(r, id, Sense::click());
        let resp = resp.on_hover_text(t.tip());
        let v = ui.visuals();
        let active = self.is_active(t);
        let fill = if active {
            v.selection.bg_fill
        } else if enabled && resp.is_pointer_button_down_on() {
            v.widgets.active.bg_fill
        } else if enabled && resp.hovered() {
            v.widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect_filled(r, 6.0, fill);
        let ink = if !enabled {
            v.weak_text_color().gamma_multiply(0.6)
        } else if active {
            v.selection.stroke.color
        } else {
            v.text_color()
        };
        draw_icon(painter, t, r, ink, self.s.cam3.ortho);
        enabled && resp.clicked()
    }
}

fn draw_icon(p: &Painter, t: Tool, r: Rect, ink: Color32, ortho: bool) {
    let c = r.center();
    let st = Stroke::new(1.6, ink);
    let pt = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    // four corner brackets around a box of half-size h
    let brackets = |h: f32, l: f32| {
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            line(pt(sx * h, sy * h), pt(sx * (h - l), sy * h));
            line(pt(sx * h, sy * h), pt(sx * h, sy * (h - l)));
        }
    };
    let lens = |sign: Option<bool>| {
        p.circle_stroke(pt(-1.5, -1.5), 6.5, st);
        line(pt(3.2, 3.2), pt(8.0, 8.0));
        match sign {
            Some(plus) => {
                line(pt(-4.5, -1.5), pt(1.5, -1.5));
                if plus {
                    line(pt(-1.5, -4.5), pt(-1.5, 1.5));
                }
            }
            None => {}
        }
    };
    let arrow = |dx: f32| {
        line(pt(-7.0 * dx, 0.0), pt(7.0 * dx, 0.0));
        line(pt(7.0 * dx, 0.0), pt(2.0 * dx, -5.0));
        line(pt(7.0 * dx, 0.0), pt(2.0 * dx, 5.0));
    };
    let text = |s: &str| {
        p.text(c, Align2::CENTER_CENTER, s, FontId::proportional(12.0), ink);
    };
    match t {
        Tool::Fit => {
            brackets(8.0, 5.0);
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(6.0)), 1.0, ink);
        }
        Tool::Selection => {
            brackets(8.0, 4.0);
            p.circle_filled(pt(-2.5, 1.5), 2.6, ink);
            p.circle_filled(pt(3.0, -2.5), 2.6, ink);
            line(pt(-2.5, 1.5), pt(3.0, -2.5));
        }
        Tool::Region => {
            let r = Rect::from_center_size(c, Vec2::new(17.0, 13.0));
            p.add(egui::Shape::dashed_line(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], st, 3.0, 2.5));
            // a corner handle and a cursor arrow
            p.rect_filled(Rect::from_center_size(r.right_bottom(), Vec2::splat(4.0)), 0.0, ink);
            p.rect_filled(Rect::from_center_size(r.left_top(), Vec2::splat(4.0)), 0.0, ink);
        }
        Tool::ZoomIn => lens(Some(true)),
        Tool::ZoomOut => lens(Some(false)),
        Tool::Home => {
            let roof = [pt(-9.0, -0.5), pt(0.0, -8.5), pt(9.0, -0.5)];
            p.add(egui::Shape::line(roof.to_vec(), st));
            p.add(egui::Shape::line(vec![pt(-6.0, -2.5), pt(-6.0, 7.5), pt(6.0, 7.5), pt(6.0, -2.5)], st));
            line(pt(-1.5, 7.5), pt(-1.5, 2.5));
            line(pt(1.5, 7.5), pt(1.5, 2.5));
            line(pt(-1.5, 2.5), pt(1.5, 2.5));
        }
        Tool::Back => arrow(-1.0),
        Tool::Forward => arrow(1.0),
        Tool::Nav(NavMode::Orbit) => {
            // a ball with a ring around it
            p.circle_stroke(c, 4.5, st);
            let ring: Vec<Pos2> = (0..=24).map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                pt(a.cos() * 9.5, a.sin() * 3.5)
            }).collect();
            p.add(egui::Shape::line(ring, st));
        }
        Tool::Nav(NavMode::Pan) => {
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                line(pt(0.0, 0.0), pt(dx * 8.5, dy * 8.5));
                let tip = pt(dx * 8.5, dy * 8.5);
                line(tip, pt(dx * 5.0 - dy * 3.0, dy * 5.0 + dx * 3.0));
                line(tip, pt(dx * 5.0 + dy * 3.0, dy * 5.0 - dx * 3.0));
            }
        }
        Tool::Nav(NavMode::Zoom) => {
            lens(None);
            line(pt(-1.5, -4.0), pt(-1.5, 1.0));
            line(pt(-4.0, -1.0), pt(-1.5, 1.5));
            line(pt(1.0, -1.0), pt(-1.5, 1.5));
        }
        Tool::Preset(View3::Top) => text("Top"),
        Tool::Preset(View3::Front) => text("Front"),
        Tool::Preset(View3::Side) => text("Side"),
        Tool::Preset(View3::Iso) => text("Iso"),
        Tool::Perspective => text(if ortho { "Ortho" } else { "Persp" }),
    }
}
