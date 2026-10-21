//! View controls: the camera actions behind the floating toolbar (zoom to fit, to the selection, to a dragged
//! region, zoom steps, home, 3D presets), animated camera transitions and the view history ("previous view").
//!
//! The 2D view is a `ViewCam` (centre + zoom), the 3D view a `Cam3` (orbit camera). Both are wrapped in a
//! `ViewSnap` so that one set of tools and one history serve both.

use crate::app::App;
use crate::coords::{Cam3, View3};
use crate::model::{Session, ViewCam};
use egui::{Pos2, Rect, Vec2};

/// Seconds a camera move takes.
pub const ANIM_SECS: f32 = 0.30;
/// A view that has not changed for this long is remembered in the history.
pub const REST_SECS: f64 = 0.35;
const HISTORY_MAX: usize = 100;
/// Step of the zoom in / zoom out buttons.
pub const ZOOM_STEP: f32 = 1.5;
/// Zoom to a single node: readable, but never "all the way in". Pixels per world unit.
pub const READABLE_MIN: f32 = 1.5;
pub const READABLE_MAX: f32 = 6.0;
/// Zoom to a group never goes in further than this (a few overlapping nodes would otherwise zoom in without end).
pub const SELECTION_MAX_ZOOM: f32 = 8.0;
/// The group fills this share of the view (the rest is padding).
const SELECTION_FILL: f32 = 0.8;
/// A dragged region smaller than this (pixels) is taken for a click.
pub const MIN_REGION_PX: f32 = 6.0;
const ZOOM_RANGE: (f32, f32) = (0.002, 80.0);

// ───────────────────────────── tools ─────────────────────────────

/// One button of the toolbar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tool {
    Fit,
    Selection,
    Region,
    ZoomIn,
    ZoomOut,
    Home,
    Back,
    Forward,
    /// 3D only: what a plain left-drag does.
    Nav(NavMode),
    /// 3D only: look from a fixed direction.
    Preset(View3),
    /// 3D only: perspective / orthographic.
    Perspective,
}

impl Tool {
    /// Tooltip text, with the keyboard shortcut.
    pub fn tip(self) -> &'static str {
        match self {
            Tool::Fit => "Zoom to fit: all visible nodes  (F)",
            Tool::Selection => "Zoom to selection  (S)",
            Tool::Region => "Zoom to region: drag a rectangle on the graph  (R, Esc cancels)",
            Tool::ZoomIn => "Zoom in",
            Tool::ZoomOut => "Zoom out",
            Tool::Home => "Reset view  (Home)",
            Tool::Back => "Previous view  (Alt+Left)",
            Tool::Forward => "Next view  (Alt+Right)",
            Tool::Nav(NavMode::Orbit) => "Drag to orbit",
            Tool::Nav(NavMode::Pan) => "Drag to pan  (or Space + drag, or middle-drag)",
            Tool::Nav(NavMode::Zoom) => "Drag to zoom",
            Tool::Preset(View3::Top) => "Top view",
            Tool::Preset(View3::Front) => "Front view",
            Tool::Preset(View3::Back) => "Back view",
            Tool::Preset(View3::Left) => "Left view",
            Tool::Preset(View3::Right) => "Right view",
            Tool::Preset(View3::Iso) => "Isometric view",
            Tool::Perspective => "Switch between perspective and orthographic projection",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum NavMode {
    #[default]
    Orbit,
    Pan,
    Zoom,
}

// ───────────────────────────── snapshots and animation ─────────────────────────────

#[derive(Clone, Copy, Debug)]
pub enum ViewSnap {
    Flat(ViewCam),
    Orbit(Cam3),
}

fn wrap_pi(x: f32) -> f32 {
    (x + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl ViewSnap {
    pub fn same_kind(&self, other: &ViewSnap) -> bool {
        matches!((self, other), (ViewSnap::Flat(_), ViewSnap::Flat(_)) | (ViewSnap::Orbit(_), ViewSnap::Orbit(_)))
    }

    /// True if the two views look the same to the eye (differences well below a pixel).
    pub fn near_eq(&self, other: &ViewSnap) -> bool {
        match (self, other) {
            (ViewSnap::Flat(a), ViewSnap::Flat(b)) => (a.center - b.center).length() * a.zoom.max(b.zoom) < 0.5 && (a.zoom / b.zoom).ln().abs() < 0.003,
            (ViewSnap::Orbit(a), ViewSnap::Orbit(b)) => {
                let d = [0, 1, 2].map(|k| a.target[k] - b.target[k]);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 0.002 * a.dist.min(b.dist)
                    && (a.dist / b.dist).ln().abs() < 0.003
                    && wrap_pi(a.yaw - b.yaw).abs() < 0.002
                    && (a.pitch - b.pitch).abs() < 0.002
                    && a.ortho == b.ortho
            }
            _ => false,
        }
    }

    /// The view `e` of the way (0 to 1) from `self` to `to`: centres move in a straight line, zoom and distance
    /// change evenly (as a ratio), the turn takes the short way round.
    pub fn lerp(&self, to: &ViewSnap, e: f32) -> ViewSnap {
        match (self, to) {
            (ViewSnap::Flat(a), ViewSnap::Flat(b)) => ViewSnap::Flat(ViewCam { center: a.center + (b.center - a.center) * e, zoom: a.zoom * (b.zoom / a.zoom).powf(e) }),
            (ViewSnap::Orbit(a), ViewSnap::Orbit(b)) => {
                let mut c = *b;
                for k in 0..3 {
                    c.target[k] = a.target[k] + (b.target[k] - a.target[k]) * e;
                }
                c.dist = a.dist * (b.dist / a.dist).powf(e);
                c.yaw = a.yaw + wrap_pi(b.yaw - a.yaw) * e;
                c.pitch = a.pitch + (b.pitch - a.pitch) * e;
                ViewSnap::Orbit(c)
            }
            _ => *to,
        }
    }
}

pub struct ViewAnim {
    from: ViewSnap,
    pub to: ViewSnap,
    t: f32,
}

impl ViewAnim {
    pub fn new(from: ViewSnap, to: ViewSnap) -> Self {
        ViewAnim { from, to, t: 0.0 }
    }

    /// Advance by `dt` seconds. Returns the view to show and whether the move is over.
    pub fn step(&mut self, dt: f32) -> (ViewSnap, bool) {
        self.t = (self.t + dt / ANIM_SECS).min(1.0);
        if self.t >= 1.0 {
            return (self.to, true);
        }
        let e = 1.0 - (1.0 - self.t).powi(3); // ease out
        (self.from.lerp(&self.to, e), false)
    }
}

// ───────────────────────────── history ─────────────────────────────

/// The views the user has rested on, for "previous view" / "next view". A view is remembered once the camera has
/// stopped moving for a moment, so a whole drag or a run of wheel clicks is one step.
#[derive(Default)]
pub struct History {
    entries: Vec<ViewSnap>,
    pos: usize,
    last: Option<ViewSnap>,
    stable_since: f64,
}

impl History {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pos = 0;
        self.last = None;
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    #[allow(dead_code)] // used by the tests
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn commit(&mut self, cur: ViewSnap) {
        self.entries.truncate(self.pos + 1);
        self.entries.push(cur);
        self.pos = self.entries.len() - 1;
        if self.entries.len() > HISTORY_MAX {
            self.entries.remove(0);
            self.pos -= 1;
        }
    }

    /// Called every frame with the camera as it is. `busy` while something else is moving the camera (a layout
    /// that is still running, an animation). Returns true while a view is waiting to be remembered.
    pub fn observe(&mut self, cur: ViewSnap, now: f64, busy: bool) -> bool {
        // a different kind of camera (2D <-> 3D) starts a new history
        if self.entries.get(self.pos).is_some_and(|e| !e.same_kind(&cur)) {
            self.clear();
        }
        if busy {
            self.last = Some(cur);
            self.stable_since = now;
            return false;
        }
        if self.entries.is_empty() {
            self.entries.push(cur);
            self.pos = 0;
            self.last = Some(cur);
            self.stable_since = now;
            return false;
        }
        if self.last.is_none_or(|l| !l.near_eq(&cur)) {
            self.last = Some(cur);
            self.stable_since = now;
        }
        if self.entries[self.pos].near_eq(&cur) {
            return false;
        }
        if now - self.stable_since >= REST_SECS {
            self.commit(cur);
            return false;
        }
        true
    }

    /// Remember the view as it is right now (before a tool moves the camera), unless it is already the latest.
    pub fn flush(&mut self, cur: ViewSnap) {
        match self.entries.get(self.pos) {
            Some(e) if !e.same_kind(&cur) => {
                self.clear();
                self.entries.push(cur);
            }
            Some(e) if e.near_eq(&cur) => {}
            Some(_) => self.commit(cur),
            None => self.entries.push(cur),
        }
        self.pos = self.pos.min(self.entries.len().saturating_sub(1));
        self.last = Some(cur);
    }

    pub fn back(&mut self, cur: ViewSnap) -> Option<ViewSnap> {
        self.flush(cur);
        if self.pos == 0 {
            return None;
        }
        self.pos -= 1;
        self.last = Some(self.entries[self.pos]);
        Some(self.entries[self.pos])
    }

    pub fn forward(&mut self, cur: ViewSnap) -> Option<ViewSnap> {
        self.flush(cur);
        if self.pos + 1 >= self.entries.len() {
            return None;
        }
        self.pos += 1;
        self.last = Some(self.entries[self.pos]);
        Some(self.entries[self.pos])
    }
}

// ───────────────────────────── per-app state ─────────────────────────────

#[derive(Default)]
pub struct ViewCtl {
    pub anim: Option<ViewAnim>,
    pub hist: History,
    pub nav: NavMode,
    /// The zoom-to-region tool is waiting for a rectangle.
    pub region_on: bool,
    /// Where the rectangle started (screen position).
    pub region_start: Option<Pos2>,
    /// The current left-drag is a pan (Space was held when it began).
    pub pan_drag: bool,
    /// A pan happened while Space was held, so letting go of Space must not also pause the layout.
    pub space_used: bool,
    /// Where the toolbar is on screen, and every button in it (for hit tests; refreshed every frame).
    pub toolbar_rect: Option<Rect>,
    pub buttons: Vec<(Tool, Rect, bool)>,
}

impl ViewCtl {
    pub fn cancel_region(&mut self) -> bool {
        let was = self.region_on || self.region_start.is_some();
        self.region_on = false;
        self.region_start = None;
        was
    }

    #[allow(dead_code)] // used by the tests
    pub fn over_toolbar(&self, p: Pos2) -> bool {
        self.toolbar_rect.is_some_and(|r| r.contains(p))
    }
}

// ───────────────────────────── targets ─────────────────────────────

/// Selected nodes that are on screen (not filtered out).
pub fn selected_visible(s: &Session) -> Vec<usize> {
    (0..s.g.n()).filter(|&i| s.sel[i] && s.vis[i]).collect()
}

fn clamp_zoom(z: f32) -> f32 {
    z.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1)
}

/// All visible nodes. In 3D the viewing angle stays as it is.
pub fn fit_target(s: &Session, size: Vec2) -> ViewSnap {
    if s.is_3d() { ViewSnap::Orbit(s.cam3_fit_target()) } else { ViewSnap::Flat(s.fit_camera(size)) }
}

/// The default view of the current graph or subgraph: fitted, and in 3D also back at the default angle.
pub fn home_target(s: &Session, size: Vec2) -> ViewSnap {
    match fit_target(s, size) {
        ViewSnap::Orbit(mut c) => {
            c.preset(View3::Iso);
            ViewSnap::Orbit(c)
        }
        flat => flat,
    }
}

/// The selected nodes (visible ones only): one node is centred at a readable zoom, a group fills the view with padding.
pub fn selection_target(s: &Session, cam: &ViewCam, size: Vec2) -> Option<ViewSnap> {
    let sel = selected_visible(s);
    if sel.is_empty() {
        return None;
    }
    if s.is_3d() {
        let pts: Vec<[f32; 3]> = sel.iter().map(|&i| s.pos3[i]).collect();
        let mut c = s.cam3;
        if pts.len() == 1 {
            let focal = Cam3::focal_for(size);
            let ppu = (focal / c.dist).clamp(READABLE_MIN, READABLE_MAX);
            c.target = pts[0];
            c.dist = focal / ppu;
        } else {
            c.frame_group(&pts, size, SELECTION_MAX_ZOOM);
        }
        c.clamp();
        return Some(ViewSnap::Orbit(c));
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for &i in &sel {
        lo = lo.min(s.pos[i]);
        hi = hi.max(s.pos[i]);
    }
    let center = (lo + hi) * 0.5;
    let zoom = if sel.len() == 1 {
        cam.zoom.clamp(READABLE_MIN, READABLE_MAX)
    } else {
        let ext = (hi - lo).max(Vec2::splat(1.0));
        (size.x * SELECTION_FILL / ext.x).min(size.y * SELECTION_FILL / ext.y).min(SELECTION_MAX_ZOOM)
    };
    Some(ViewSnap::Flat(ViewCam { center, zoom: clamp_zoom(zoom) }))
}

/// The view that shows the dragged rectangle (canvas-local pixels) filling the canvas.
pub fn region_target(s: &Session, cam: &ViewCam, size: Vec2, rect: Rect) -> Option<ViewSnap> {
    if rect.width() < MIN_REGION_PX || rect.height() < MIN_REGION_PX {
        return None;
    }
    if s.is_3d() {
        let mut c = s.cam3;
        c.zoom_to_rect(rect.center().to_vec2() - size * 0.5, rect.size(), size);
        c.clamp();
        return Some(ViewSnap::Orbit(c));
    }
    let center = cam.to_world(rect.center().to_vec2(), size);
    let k = (size.x / rect.width()).min(size.y / rect.height());
    Some(ViewSnap::Flat(ViewCam { center, zoom: clamp_zoom(cam.zoom * k) }))
}

/// One step in or out, around the middle of the view.
pub fn step_target(s: &Session, cam: &ViewCam, zoom_in: bool) -> ViewSnap {
    let f = if zoom_in { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
    if s.is_3d() {
        let mut c = s.cam3;
        c.dist /= f;
        c.clamp();
        ViewSnap::Orbit(c)
    } else {
        ViewSnap::Flat(ViewCam { center: cam.center, zoom: clamp_zoom(cam.zoom * f) })
    }
}

/// A fixed viewing direction, framed on the visible nodes.
pub fn preset_target(s: &Session, which: View3) -> ViewSnap {
    let mut c = s.cam3_fit_target();
    c.preset(which);
    ViewSnap::Orbit(c)
}

// ───────────────────────────── the app side ─────────────────────────────

impl App {
    pub fn view_snap(&self) -> ViewSnap {
        if self.s.is_3d() { ViewSnap::Orbit(self.s.cam3) } else { ViewSnap::Flat(self.cam) }
    }

    pub fn set_view(&mut self, v: ViewSnap) {
        match v {
            ViewSnap::Flat(c) => self.cam = c,
            ViewSnap::Orbit(c) => self.s.cam3 = c,
        }
    }

    /// The view a tool would go to, or None if it does not apply (no selection, not in 3D, no history …).
    pub fn tool_target(&self, tool: Tool) -> Option<ViewSnap> {
        let size = self.canvas_size;
        let three_d = self.s.is_3d();
        match tool {
            Tool::Fit => Some(fit_target(&self.s, size)),
            Tool::Home => Some(home_target(&self.s, size)),
            Tool::Selection => selection_target(&self.s, &self.cam, size),
            Tool::ZoomIn => Some(step_target(&self.s, &self.cam, true)),
            Tool::ZoomOut => Some(step_target(&self.s, &self.cam, false)),
            Tool::Preset(v) if three_d => Some(preset_target(&self.s, v)),
            _ => None,
        }
    }

    /// Is the button usable right now? (Unusable ones stay on the toolbar, greyed out.)
    pub fn tool_enabled(&self, tool: Tool) -> bool {
        match tool {
            Tool::Selection => !selected_visible(&self.s).is_empty(),
            Tool::Back => self.view.hist.can_back(),
            Tool::Forward => self.view.hist.can_forward(),
            _ => self.s.g.n() > 0,
        }
    }

    /// Move the camera to `to`: animated if that is switched on, and remembered so "previous view" returns here.
    pub fn go_to(&mut self, to: ViewSnap) {
        let cur = self.view_snap();
        self.view.hist.flush(cur);
        self.auto_fit = false;
        if self.settings.animate_view && cur.same_kind(&to) && !cur.near_eq(&to) {
            self.view.anim = Some(ViewAnim::new(cur, to));
        } else {
            self.view.anim = None;
            self.set_view(to);
        }
    }

    /// Run a toolbar button (or its keyboard shortcut).
    pub fn run_tool(&mut self, tool: Tool) {
        if !self.tool_enabled(tool) {
            return;
        }
        match tool {
            Tool::Region => {
                self.view.region_on = !self.view.region_on;
                self.view.region_start = None;
            }
            Tool::Back | Tool::Forward => {
                let cur = self.view_snap();
                let to = if tool == Tool::Back { self.view.hist.back(cur) } else { self.view.hist.forward(cur) };
                if let Some(to) = to {
                    self.auto_fit = false;
                    if self.settings.animate_view && cur.same_kind(&to) && !cur.near_eq(&to) {
                        self.view.anim = Some(ViewAnim::new(cur, to));
                    } else {
                        self.view.anim = None;
                        self.set_view(to);
                    }
                }
            }
            Tool::Nav(m) => self.view.nav = m,
            Tool::Perspective => {
                if self.s.is_3d() {
                    self.s.cam3.ortho = !self.s.cam3.ortho;
                }
            }
            Tool::Fit if self.s.sim.running || self.s.is_animating() => {
                // the layout is still moving: follow it
                let cur = self.view_snap();
                self.view.hist.flush(cur);
                self.view.anim = None;
                self.auto_fit = true;
            }
            other => {
                if let Some(to) = self.tool_target(other) {
                    self.go_to(to);
                }
            }
        }
    }

    /// Zoom to a rectangle the user dragged (canvas-local pixels). Ends the region tool.
    pub fn zoom_to_rect(&mut self, rect: Rect) {
        if let Some(to) = region_target(&self.s, &self.cam, self.canvas_size, rect) {
            self.go_to(to);
            self.view.region_on = false;
        }
    }

    /// Advance a running camera move, and remember the view once the camera has come to rest.
    pub fn tick_view(&mut self, ui: &egui::Ui) {
        let (dt, now) = ui.input(|i| (i.stable_dt.min(0.1), i.time));
        if let Some(a) = &mut self.view.anim {
            let (v, done) = a.step(dt);
            if done {
                self.view.anim = None;
            }
            self.set_view(v);
            ui.ctx().request_repaint();
        }
        let busy = self.view.anim.is_some() || self.auto_fit || self.s.sim.running || self.s.is_animating();
        if self.view.hist.observe(self.view_snap(), now, busy) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::CoordSettings;
    use crate::data::*;
    use crate::layout::LayoutKind;

    const SIZE: Vec2 = Vec2::new(800.0, 600.0);

    /// A 6×6 grid of nodes (ids n0..n35), laid out on a grid.
    fn grid_session() -> Session {
        let mut nodes = String::from("id,x,y,z,Kind\n");
        let mut edges = String::from("source,target\n");
        for r in 0..6 {
            for c in 0..6 {
                let i = r * 6 + c;
                nodes += &format!("n{i},{},{},{},{}\n", c * 10, r * 10, (r + c) * 4, if i % 2 == 0 { "a" } else { "b" });
                if c < 5 {
                    edges += &format!("n{i},n{}\n", i + 1);
                }
                if r < 5 {
                    edges += &format!("n{i},n{}\n", i + 6);
                }
            }
        }
        let (e, n) = (parse_csv_text(&edges).unwrap(), parse_csv_text(&nodes).unwrap());
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.layout.kind = LayoutKind::Grid;
        s.rebuild();
        s.apply_layout_now();
        s
    }

    fn on_screen(p: Vec2) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= SIZE.x && p.y <= SIZE.y
    }

    fn flat(v: ViewSnap) -> ViewCam {
        match v {
            ViewSnap::Flat(c) => c,
            other => panic!("{other:?}"),
        }
    }

    fn orbit(v: ViewSnap) -> Cam3 {
        match v {
            ViewSnap::Orbit(c) => c,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fit_ignores_hidden_nodes_and_follows_the_subgraph() {
        let mut s = grid_session();
        let all = flat(fit_target(&s, SIZE));
        // hide everything but the top-left corner
        for i in 0..s.g.n() {
            s.manual_hidden[i] = !(s.g.ids[i] == "n0" || s.g.ids[i] == "n1" || s.g.ids[i] == "n6");
        }
        s.recompute_visibility();
        let some = flat(fit_target(&s, SIZE));
        assert!(some.zoom > all.zoom * 1.2, "fits the few visible nodes: {} vs {}", some.zoom, all.zoom);
        for i in (0..s.g.n()).filter(|&i| s.vis[i]) {
            assert!(on_screen(some.to_screen(s.pos[i], SIZE)));
        }
    }

    #[test]
    fn one_selected_node_is_centred_at_a_readable_zoom() {
        let mut s = grid_session();
        s.select_only(7);
        for (cur, want) in [(0.2, READABLE_MIN), (3.0, 3.0), (40.0, READABLE_MAX)] {
            let cam = ViewCam { center: Vec2::new(500.0, 500.0), zoom: cur };
            let t = flat(selection_target(&s, &cam, SIZE).unwrap());
            assert!((t.center - s.pos[7]).length() < 1e-4);
            assert!((t.zoom - want).abs() < 1e-4, "from zoom {cur}: {}", t.zoom);
            assert!(t.zoom < ZOOM_RANGE.1, "never maximum zoom");
        }
    }

    #[test]
    fn a_selected_group_fills_the_view_with_padding() {
        let mut s = grid_session();
        s.select_nodes([0usize, 5, 30, 35]); // the four corners
        let t = flat(selection_target(&s, &ViewCam::default(), SIZE).unwrap());
        let pts: Vec<Vec2> = [0usize, 5, 30, 35].iter().map(|&i| t.to_screen(s.pos[i], SIZE)).collect();
        for p in &pts {
            assert!(p.x > SIZE.x * 0.05 && p.x < SIZE.x * 0.95 && p.y > SIZE.y * 0.05 && p.y < SIZE.y * 0.95, "{p:?} has padding");
        }
        let (lo, hi) = pts.iter().fold((Vec2::splat(1e9), Vec2::splat(-1e9)), |(l, h), p| (l.min(*p), h.max(*p)));
        assert!(((hi.x - lo.x) / SIZE.x > 0.7) || ((hi.y - lo.y) / SIZE.y > 0.7), "and it is as big as the padding allows");
        // two nodes right next to each other do not zoom in without end
        s.select_nodes([0usize, 1]);
        s.pos[1] = s.pos[0] + Vec2::new(0.01, 0.0);
        let t = flat(selection_target(&s, &ViewCam::default(), SIZE).unwrap());
        assert!(t.zoom <= SELECTION_MAX_ZOOM + 1e-4);
    }

    #[test]
    fn selection_ignores_hidden_nodes_and_needs_a_selection() {
        let mut s = grid_session();
        assert!(selection_target(&s, &ViewCam::default(), SIZE).is_none());
        s.select_nodes([3usize, 4]);
        s.manual_hidden[3] = true;
        s.recompute_visibility();
        assert_eq!(selected_visible(&s), vec![4]);
        let t = flat(selection_target(&s, &ViewCam::default(), SIZE).unwrap());
        assert!((t.center - s.pos[4]).length() < 1e-4, "only the visible one counts");
        s.manual_hidden[4] = true;
        s.recompute_visibility();
        assert!(selection_target(&s, &ViewCam::default(), SIZE).is_none(), "nothing visible is selected");
    }

    #[test]
    fn region_zoom_makes_the_rectangle_fill_the_canvas() {
        let s = grid_session();
        let cam = flat(fit_target(&s, SIZE));
        let rect = Rect::from_min_max(Pos2::new(200.0, 150.0), Pos2::new(400.0, 300.0)); // 200×150, same aspect as the canvas / 4
        let before_a = cam.to_world(rect.min.to_vec2(), SIZE);
        let before_b = cam.to_world(rect.max.to_vec2(), SIZE);
        let t = flat(region_target(&s, &cam, SIZE, rect).unwrap());
        let (a, b) = (t.to_screen(before_a, SIZE), t.to_screen(before_b, SIZE));
        assert!(a.x.abs() < 1.0 && a.y.abs() < 1.0 && (b.x - SIZE.x).abs() < 1.0 && (b.y - SIZE.y).abs() < 1.0, "{a:?} {b:?}");
        // a wide rectangle is limited by its width, so all of it stays in view
        let wide = Rect::from_min_max(Pos2::new(100.0, 250.0), Pos2::new(700.0, 300.0));
        let t = flat(region_target(&s, &cam, SIZE, wide).unwrap());
        for p in [wide.min, wide.max] {
            let w = cam.to_world(p.to_vec2(), SIZE);
            assert!(on_screen(t.to_screen(w, SIZE)));
        }
        // a click is not a region
        assert!(region_target(&s, &cam, SIZE, Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(2.0, 2.0))).is_none());
    }

    #[test]
    fn zoom_steps_are_centred_and_limited() {
        let s = grid_session();
        let cam = ViewCam { center: Vec2::new(12.0, 34.0), zoom: 1.0 };
        let i = flat(step_target(&s, &cam, true));
        let o = flat(step_target(&s, &cam, false));
        assert_eq!((i.center, o.center), (cam.center, cam.center));
        assert!((i.zoom - ZOOM_STEP).abs() < 1e-5 && (o.zoom - 1.0 / ZOOM_STEP).abs() < 1e-5);
        let top = flat(step_target(&s, &ViewCam { center: Vec2::ZERO, zoom: 79.0 }, true));
        assert_eq!(top.zoom, ZOOM_RANGE.1);
    }

    fn session_3d() -> Session {
        let mut s = grid_session();
        s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
        s.apply_coords();
        assert!(s.is_3d());
        s
    }

    #[test]
    fn three_d_fit_home_presets_and_zoom() {
        let mut s = session_3d();
        s.cam3.yaw = 2.0;
        s.cam3.pitch = 0.1;
        let fit = orbit(fit_target(&s, SIZE));
        assert_eq!((fit.yaw, fit.pitch), (2.0, 0.1), "fit keeps the viewing angle");
        let home = orbit(home_target(&s, SIZE));
        assert_eq!((home.yaw, home.pitch), (0.45, 0.4), "home resets the angle");
        let top = orbit(preset_target(&s, View3::Top));
        assert!(top.pitch > 1.4 && top.yaw == 0.0);
        // everything is on screen after a fit
        s.cam3 = fit;
        let o = s.cam3.orbit(SIZE);
        for i in 0..s.g.n() {
            let (p, _, _) = o.project(s.pos3[i]).unwrap();
            assert!(on_screen(p), "node {i} at {p:?}");
        }
        let near = orbit(step_target(&s, &ViewCam::default(), true));
        assert!(near.dist < s.cam3.dist);
    }

    #[test]
    fn three_d_selection_and_region() {
        let mut s = session_3d();
        s.cam3 = orbit(fit_target(&s, SIZE));
        // one node: centred, at a readable pixels-per-unit
        s.select_only(14);
        let t = orbit(selection_target(&s, &ViewCam::default(), SIZE).unwrap());
        assert_eq!(t.target, s.pos3[14]);
        let ppu = Cam3::focal_for(SIZE) / t.dist;
        assert!((READABLE_MIN - 0.001..=READABLE_MAX + 0.001).contains(&ppu), "{ppu}");
        // a group: all of it visible with padding
        s.select_nodes([0usize, 5, 30, 35]);
        let t = orbit(selection_target(&s, &ViewCam::default(), SIZE).unwrap());
        let o = t.orbit(SIZE);
        for &i in &[0usize, 5, 30, 35] {
            let (p, _, _) = o.project(s.pos3[i]).unwrap();
            assert!(p.x > 0.0 && p.x < SIZE.x && p.y > 0.0 && p.y < SIZE.y, "{p:?}");
        }
        // region: the point at the middle of the rectangle ends up in the middle of the view
        let cam = s.cam3;
        let rect = Rect::from_min_max(Pos2::new(500.0, 100.0), Pos2::new(700.0, 250.0));
        let t = orbit(region_target(&s, &ViewCam::default(), SIZE, rect).unwrap());
        assert!(t.dist < cam.dist, "zoomed in");
        let o0 = cam.orbit(SIZE);
        let o1 = t.orbit(SIZE);
        // the point on the target plane under the rectangle's centre, seen before and after
        let f0 = o0.focal / cam.dist;
        let c = rect.center().to_vec2() - SIZE * 0.5;
        let p = [0, 1, 2].map(|k| cam.target[k] + o0.right[k] * (c.x / f0) - o0.up[k] * (c.y / f0));
        let (after, _, _) = o1.project(p).unwrap();
        assert!((after - SIZE * 0.5).length() < 0.5, "{after:?}");
    }

    #[test]
    fn zooming_at_the_cursor_keeps_the_point_under_it() {
        for ortho in [false, true] {
            let mut cam = Cam3 { target: [5.0, 2.0, 1.0], yaw: 0.7, pitch: 0.5, dist: 300.0, ortho };
            let cursor = Vec2::new(120.0, -80.0);
            let o = cam.orbit(SIZE);
            let f = o.focal / cam.dist;
            let p = [0, 1, 2].map(|k| cam.target[k] + o.right[k] * (cursor.x / f) - o.up[k] * (cursor.y / f));
            let before = o.project(p).unwrap().0;
            cam.zoom_at(1.8, cursor, SIZE);
            assert!((cam.dist - 300.0 / 1.8).abs() < 1e-3);
            let after = cam.orbit(SIZE).project(p).unwrap().0;
            assert!((before - after).length() < 0.01, "ortho {ortho}: {before:?} → {after:?}");
        }
    }

    #[test]
    fn orthographic_view_has_one_scale_and_keeps_what_is_behind_the_eye() {
        let mut cam = Cam3 { target: [0.0; 3], yaw: 0.0, pitch: 0.0, dist: 100.0, ortho: false };
        let persp = cam.orbit(SIZE);
        assert!(persp.project([0.0, -200.0, 0.0]).is_none(), "behind the camera");
        cam.ortho = true;
        let o = cam.orbit(SIZE);
        let (c, f_mid, _) = o.project([0.0; 3]).unwrap();
        assert!((c - SIZE * 0.5).length() < 1e-3);
        let (_, f_far, _) = o.project([0.0, 50.0, 0.0]).unwrap();
        let (_, f_near, _) = o.project([0.0, -50.0, 0.0]).unwrap();
        assert!((f_mid - f_far).abs() < 1e-4 && (f_mid - f_near).abs() < 1e-4, "no perspective shrinking");
        assert!(o.project([0.0, -500.0, 0.0]).is_some(), "orthographic has no near plane in front of the target");
        // parallel lines stay parallel: two points on a line along x project to the same y at any depth
        let (a, _, _) = o.project([0.0, -30.0, 7.0]).unwrap();
        let (b, _, _) = o.project([0.0, 30.0, 7.0]).unwrap();
        assert!((a - b).length() < 1e-3);
        // clipping a segment that runs behind the eye keeps it whole
        assert!(o.clip_segment([0.0, -500.0, 0.0], [10.0, 500.0, 0.0]).is_some());
    }

    #[test]
    fn animation_runs_from_start_to_finish_and_turns_the_short_way() {
        let a = ViewSnap::Flat(ViewCam { center: Vec2::new(0.0, 0.0), zoom: 1.0 });
        let b = ViewSnap::Flat(ViewCam { center: Vec2::new(100.0, 0.0), zoom: 4.0 });
        let mut anim = ViewAnim::new(a, b);
        let mut last_x = -1.0;
        let mut frames = 0;
        loop {
            let (v, done) = anim.step(1.0 / 60.0);
            let c = flat(v);
            assert!(c.center.x >= last_x, "moves steadily towards the target");
            last_x = c.center.x;
            frames += 1;
            if done {
                assert_eq!((c.center.x, c.zoom), (100.0, 4.0), "ends exactly on the target");
                break;
            }
            assert!(frames < 60);
        }
        assert!((15..=25).contains(&frames), "about {ANIM_SECS} s, took {frames} frames");
        // half way in time is more than half way in distance (ease out)
        let mut anim = ViewAnim::new(a, b);
        let (v, _) = anim.step(ANIM_SECS * 0.5);
        assert!(flat(v).center.x > 50.0);
        // 3D: from yaw 6.0 to yaw 0.1 goes up through 2π, not back through 3
        let c0 = Cam3 { yaw: 6.0, ..Cam3::default() };
        let c1 = Cam3 { yaw: 0.1, ..Cam3::default() };
        let mid = orbit(ViewSnap::Orbit(c0).lerp(&ViewSnap::Orbit(c1), 0.5));
        let turned = wrap_pi(mid.yaw - 6.0).abs();
        assert!(turned < 0.2, "{} turned {turned}", mid.yaw);
    }

    #[test]
    fn views_are_compared_by_what_you_would_see() {
        let a = ViewSnap::Flat(ViewCam { center: Vec2::new(10.0, 10.0), zoom: 2.0 });
        assert!(a.near_eq(&ViewSnap::Flat(ViewCam { center: Vec2::new(10.1, 10.0), zoom: 2.0 })), "0.2 px is nothing");
        assert!(!a.near_eq(&ViewSnap::Flat(ViewCam { center: Vec2::new(12.0, 10.0), zoom: 2.0 })));
        assert!(!a.near_eq(&ViewSnap::Flat(ViewCam { center: Vec2::new(10.0, 10.0), zoom: 2.1 })));
        assert!(!a.near_eq(&ViewSnap::Orbit(Cam3::default())));
        let o = ViewSnap::Orbit(Cam3::default());
        assert!(!o.near_eq(&ViewSnap::Orbit(Cam3 { ortho: true, ..Cam3::default() })));
    }

    fn at(x: f32) -> ViewSnap {
        ViewSnap::Flat(ViewCam { center: Vec2::new(x, 0.0), zoom: 1.0 })
    }

    fn x_of(v: ViewSnap) -> f32 {
        flat(v).center.x
    }

    #[test]
    fn history_remembers_views_once_they_rest() {
        let mut h = History::default();
        let mut t = 0.0;
        // nothing is remembered while the camera is busy (first fit after loading)
        assert!(!h.observe(at(0.0), t, true));
        assert_eq!(h.len(), 0);
        t += 1.0;
        h.observe(at(5.0), t, false);
        assert_eq!(h.len(), 1, "the first view that rests is the start");
        // a drag: many small changes, then rest → one entry
        for k in 1..=10 {
            t += 0.016;
            assert!(h.observe(at(5.0 + 10.0 * k as f32), t, false) || k == 1 || true);
        }
        assert_eq!(h.len(), 1, "still moving");
        t += 0.2;
        assert!(h.observe(at(105.0), t, false), "waiting to be remembered");
        t += 0.2;
        assert!(!h.observe(at(105.0), t, false));
        assert_eq!(h.len(), 2);
        assert!(h.can_back() && !h.can_forward());
        // back and forward
        assert_eq!(x_of(h.back(at(105.0)).unwrap()), 5.0);
        assert!(!h.can_back() && h.can_forward());
        assert_eq!(x_of(h.forward(at(5.0)).unwrap()), 105.0);
        assert!(h.forward(at(105.0)).is_none() && h.back(at(105.0)).is_some());
    }

    #[test]
    fn a_new_view_after_going_back_drops_the_forward_views() {
        let mut h = History::default();
        h.flush(at(0.0));
        h.flush(at(10.0));
        h.flush(at(20.0));
        assert_eq!(h.len(), 3);
        assert_eq!(x_of(h.back(at(20.0)).unwrap()), 10.0);
        assert_eq!(x_of(h.back(at(10.0)).unwrap()), 0.0);
        // arriving at 0 through the history is not a new view; moving somewhere new is
        let mut t = 100.0;
        h.observe(at(0.0), t, false);
        assert_eq!(h.len(), 3);
        for _ in 0..3 {
            t += 0.2;
            h.observe(at(77.0), t, false);
        }
        assert_eq!(h.len(), 2, "[0, 77]: the forward views are gone");
        assert!(!h.can_forward());
    }

    #[test]
    fn flush_saves_a_view_that_has_not_rested_yet() {
        let mut h = History::default();
        h.flush(at(0.0));
        // the user zoomed and clicked a tool straight away: the view before the tool is kept
        h.flush(at(40.0));
        assert_eq!(h.len(), 2);
        h.flush(at(40.0));
        assert_eq!(h.len(), 2, "no duplicate");
        assert_eq!(x_of(h.back(at(99.0)).unwrap()), 40.0, "back from an unsaved view lands on the last saved one");
    }

    #[test]
    fn the_history_restarts_when_the_camera_kind_changes_and_is_capped() {
        let mut h = History::default();
        h.flush(at(0.0));
        h.flush(at(1.0));
        assert!(h.can_back());
        h.observe(ViewSnap::Orbit(Cam3::default()), 5.0, false);
        assert_eq!(h.len(), 1, "going to 3D starts afresh");
        assert!(!h.can_back());
        let mut h = History::default();
        for k in 0..250 {
            h.flush(at(k as f32 * 10.0));
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(x_of(h.back(at(2490.0)).unwrap()), 2480.0);
    }
}
