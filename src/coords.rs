//! Place nodes at coordinates stored as node attributes (X, Y and optionally Z), and look at them in 2D or in a
//! 3D orbit view. The 3D camera is a plain perspective camera orbiting a target point (Z is up).

use crate::graph::Graph;
use egui::Vec2 as V2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CoordSettings {
    pub enabled: bool,
    pub x: Option<String>,
    pub y: Option<String>,
    pub z: Option<String>,
    /// Look at the nodes in 3D (only when a Z column is chosen).
    pub three_d: bool,
    /// Use the same scale on every axis (true proportions). Off stretches each axis to the same extent.
    pub keep_aspect: bool,
}

impl Default for CoordSettings {
    fn default() -> Self {
        CoordSettings { enabled: false, x: None, y: None, z: None, three_d: false, keep_aspect: true }
    }
}

/// How a raw coordinate value becomes a position: `(value − mid) × scale` on each axis. Bounding boxes use the
/// same transform, so they line up with the nodes.
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub mid: [f64; 3],
    pub scale: [f64; 3],
}

impl Xform {
    pub fn apply(&self, p: [f64; 3]) -> [f32; 3] {
        [0, 1, 2].map(|k| ((p[k] - self.mid[k]) * self.scale[k]) as f32)
    }
}

/// Positions worked out from the coordinate columns.
pub struct Placed {
    pub xform: Xform,
    /// Screen-plane positions for the 2D view: x to the right, y up (so stored as −y).
    pub pos2: Vec<V2>,
    /// 3D positions: x right, y into the screen, z up.
    pub pos3: Vec<[f32; 3]>,
    /// Nodes missing a coordinate: they are put in the middle.
    pub missing: usize,
}

fn numeric<'a>(g: &'a Graph, name: &Option<String>) -> Option<&'a crate::data::Column> {
    name.as_deref().and_then(|n| g.node_col(n)).filter(|c| c.is_numeric())
}

/// Turn the chosen columns into positions. None if X or Y is not set to a numeric column.
pub fn place(g: &Graph, c: &CoordSettings) -> Option<Placed> {
    let n = g.n();
    let (cx, cy) = (numeric(g, &c.x)?, numeric(g, &c.y)?);
    let cz = numeric(g, &c.z);
    if n == 0 {
        return None;
    }
    let val = |col: Option<&crate::data::Column>, i: usize| -> Option<f64> { col.and_then(|c| c.num(i)).filter(|v| v.is_finite()) };
    let ranges: Vec<Option<(f64, f64)>> = [Some(cx), Some(cy), cz]
        .iter()
        .map(|col| {
            let col = (*col)?;
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for i in 0..n {
                if let Some(v) = val(Some(col), i) {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            (lo <= hi).then_some((lo, hi))
        })
        .collect();
    let (Some(rx), Some(ry)) = (ranges[0], ranges[1]) else { return None };
    let rz = ranges[2];
    // the whole picture is about as wide as a laid-out graph of this many nodes
    let span = (n as f64).sqrt() * 24.0 * 3.0;
    let ext = |r: Option<(f64, f64)>| r.map(|(a, b)| (b - a).max(1e-12)).unwrap_or(1.0);
    let max_ext = ext(Some(rx)).max(ext(Some(ry))).max(rz.map(|r| ext(Some(r))).unwrap_or(0.0));
    let scale = |r: Option<(f64, f64)>| -> f64 {
        if c.keep_aspect {
            span / max_ext
        } else {
            span / ext(r)
        }
    };
    let (sx, sy, sz) = (scale(Some(rx)), scale(Some(ry)), scale(rz));
    let mid = |r: Option<(f64, f64)>| r.map(|(a, b)| (a + b) * 0.5).unwrap_or(0.0);
    let (mx, my, mz) = (mid(Some(rx)), mid(Some(ry)), mid(rz));
    let mut pos2 = Vec::with_capacity(n);
    let mut pos3 = Vec::with_capacity(n);
    let mut missing = 0;
    for i in 0..n {
        let (x, y) = (val(Some(cx), i), val(Some(cy), i));
        if x.is_none() || y.is_none() {
            missing += 1;
        }
        let x = (x.unwrap_or(mx) - mx) * sx;
        let y = (y.unwrap_or(my) - my) * sy;
        let z = (val(cz, i).unwrap_or(mz) - mz) * sz;
        pos2.push(V2::new(x as f32, -(y as f32)));
        pos3.push([x as f32, y as f32, z as f32]);
    }
    let xform = Xform { mid: [mx, my, mz], scale: [sx, sy, sz] };
    Some(Placed { xform, pos2, pos3, missing })
}

/// Pick likely X / Y / Z columns by name (x, pos_x, lon, easting, …). Only numeric columns qualify.
pub fn guess_columns(g: &Graph) -> (Option<String>, Option<String>, Option<String>) {
    const XS: &[&str] = &["x", "pos_x", "px", "coord_x", "x_coord", "xcoord", "lon", "long", "lng", "longitude", "easting"];
    const YS: &[&str] = &["y", "pos_y", "py", "coord_y", "y_coord", "ycoord", "lat", "latitude", "northing"];
    const ZS: &[&str] = &["z", "pos_z", "pz", "coord_z", "z_coord", "zcoord", "height", "elevation", "alt", "altitude"];
    let find = |names: &[&str]| -> Option<String> {
        g.node_cols
            .iter()
            .filter(|c| c.is_numeric() && !c.is_metric)
            .find(|c| names.iter().any(|n| c.name.trim().eq_ignore_ascii_case(n)))
            .map(|c| c.name.clone())
    };
    (find(XS), find(YS), find(ZS))
}

// ───────────────────────────── 3D camera ─────────────────────────────

/// Vertical field of view of the 3D camera.
const FOV: f32 = 0.7; // radians, about 40°
const MAX_PITCH: f32 = 1.5;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Cam3 {
    pub target: [f32; 3],
    /// Turn around the vertical axis, radians (0 = looking along +Y).
    pub yaw: f32,
    /// Elevation of the camera above the horizon, radians (positive = looking down).
    pub pitch: f32,
    /// Distance from the target. (In orthographic view it only sets the scale.)
    pub dist: f32,
    /// Orthographic projection: no perspective shrinking, parallel lines stay parallel.
    pub ortho: bool,
}

impl Default for Cam3 {
    fn default() -> Self {
        Cam3 { target: [0.0; 3], yaw: 0.45, pitch: 0.4, dist: 1000.0, ortho: false }
    }
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// What the renderer needs to turn a 3D point into a screen point.
#[derive(Clone, Copy)]
pub struct Orbit {
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub fwd: V3,
    pub focal: f32,
    pub size: V2,
    /// Nearest distance in front of the camera that is drawn (for clipping boxes that reach past the camera).
    pub near: f32,
    /// Orthographic: every depth has the same scale, `focal / dist`.
    pub ortho: bool,
    pub dist: f32,
}

impl Cam3 {
    pub fn clamp(&mut self) {
        self.pitch = self.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        self.dist = self.dist.clamp(1e-3, 1e9);
        self.yaw = self.yaw.rem_euclid(std::f32::consts::TAU);
    }

    pub fn orbit(&self, size: V2) -> Orbit {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.clamp(-MAX_PITCH, MAX_PITCH).sin_cos();
        let t = self.target;
        let eye = [t[0] + self.dist * cp * sy, t[1] - self.dist * cp * cy, t[2] + self.dist * sp];
        let fwd = norm(sub(t, eye));
        let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
        let up = cross(right, fwd);
        let focal = Self::focal_for(size);
        // orthographic: nothing is cut off in front of the camera, even what lies behind the eye
        let near = if self.ortho { -1e30 } else { (self.dist * 0.02).max(1e-3) };
        Orbit { eye, right, up, fwd, focal, size, near, ortho: self.ortho, dist: self.dist }
    }

    /// Focal length in pixels for a canvas of this size.
    pub fn focal_for(size: V2) -> f32 {
        (size.x.min(size.y) * 0.5) / (FOV * 0.5).tan()
    }

    /// Frame these points with padding, keeping the viewing angle: the target moves to the middle of their box and
    /// the camera backs off just far enough for all of them to be on screen. Never closer than `max_zoom`
    /// pixels per unit, so a few nodes on top of each other do not zoom in without end.
    pub fn frame_group(&mut self, pts: &[V3], size: V2, max_zoom: f32) {
        if pts.is_empty() {
            return;
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in pts {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        self.target = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let o = self.orbit(size);
        let focal = o.focal;
        let (hw, hh) = (size.x * 0.5 * 0.8, size.y * 0.5 * 0.8);
        let mut need = focal / max_zoom;
        for p in pts {
            let d = sub(*p, self.target);
            let (x, y, dz) = (dot(d, o.right).abs(), dot(d, o.up).abs(), dot(d, o.fwd));
            let dz = if self.ortho { 0.0 } else { dz };
            need = need.max(focal * x / hw - dz).max(focal * y / hh - dz).max(-dz / 0.9);
        }
        self.dist = need;
    }

    /// Turn the camera by `d_yaw` / `d_pitch` around `pivot` instead of around the target: the pivot stays exactly
    /// where it is on the screen and everything else swings around it. With the pivot at the target this is the
    /// plain orbit.
    pub fn orbit_about(&mut self, pivot: V3, d_yaw: f32, d_pitch: f32, size: V2) {
        let o = self.orbit(size);
        let rel = sub(pivot, self.target);
        let seen = [dot(rel, o.right), dot(rel, o.up), dot(rel, o.fwd)];
        self.yaw += d_yaw;
        self.pitch += d_pitch;
        self.clamp();
        let o2 = self.orbit(size);
        for k in 0..3 {
            self.target[k] = pivot[k] - (seen[0] * o2.right[k] + seen[1] * o2.up[k] + seen[2] * o2.fwd[k]);
        }
    }

    /// Zoom so that the (canvas-local, from the middle) rectangle fills the view.
    pub fn zoom_to_rect(&mut self, centre: V2, rect_size: V2, size: V2) {
        let o = self.orbit(size);
        let f = o.focal / self.dist;
        for k in 0..3 {
            self.target[k] += o.right[k] * (centre.x / f) - o.up[k] * (centre.y / f);
        }
        let k = (size.x / rect_size.x.max(1.0)).min(size.y / rect_size.y.max(1.0));
        self.dist /= k;
    }

    /// Zoom by `factor` keeping the point under the cursor (canvas-local, from the middle) where it is.
    pub fn zoom_at(&mut self, factor: f32, cursor: V2, size: V2) {
        let o = self.orbit(size);
        let f = o.focal / self.dist;
        let f2 = f * factor;
        for k in 0..3 {
            self.target[k] += o.right[k] * (cursor.x / f - cursor.x / f2) - o.up[k] * (cursor.y / f - cursor.y / f2);
        }
        self.dist /= factor;
    }

    /// Look from a fixed direction. `Top` looks straight down (x right, y up).
    pub fn preset(&mut self, which: View3) {
        let (yaw, pitch) = match which {
            View3::Top => (0.0, MAX_PITCH),
            View3::Front => (0.0, 0.0),
            View3::Back => (std::f32::consts::PI, 0.0),
            View3::Right => (std::f32::consts::FRAC_PI_2, 0.0),
            View3::Left => (std::f32::consts::PI * 1.5, 0.0),
            View3::Iso => (0.45, 0.4),
        };
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Frame the given points (a robust radius, so a few far-away nodes do not shrink everything).
    pub fn fit(&mut self, pts: &[V3]) {
        if pts.is_empty() {
            return;
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in pts {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let mut d: Vec<f32> = pts.iter().map(|p| dot(sub(*p, c), sub(*p, c)).sqrt()).collect();
        d.sort_by(|a, b| a.total_cmp(b));
        let r = d[((d.len() - 1) as f32 * 0.99) as usize].max(1.0);
        self.target = c;
        self.dist = r / (FOV * 0.5).sin() * 1.15;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum View3 {
    Top,
    Front,
    Back,
    Left,
    Right,
    Iso,
}

impl Orbit {
    /// Screen position, pixels per world unit at that depth, and depth (distance along the view direction).
    /// None if the point is behind (or too close to) the camera.
    #[inline]
    pub fn project(&self, p: V3) -> Option<(V2, f32, f32)> {
        let d = sub(p, self.eye);
        let cz = dot(d, self.fwd);
        if !self.ortho && cz < 1e-3 {
            return None;
        }
        let f = if self.ortho { self.focal / self.dist } else { self.focal / cz };
        Some((V2::new(self.size.x * 0.5 + dot(d, self.right) * f, self.size.y * 0.5 - dot(d, self.up) * f), f, cz))
    }

    /// Camera-space coordinates of a world point: right, up, forward.
    #[inline]
    fn to_cam(&self, p: V3) -> V3 {
        let d = sub(p, self.eye);
        [dot(d, self.right), dot(d, self.up), dot(d, self.fwd)]
    }

    #[inline]
    fn cam_to_screen(&self, c: V3) -> V2 {
        let f = if self.ortho { self.focal / self.dist } else { self.focal / c[2] };
        V2::new(self.size.x * 0.5 + c[0] * f, self.size.y * 0.5 - c[1] * f)
    }

    /// A flat polygon (at most 4 corners) cut off at the near plane, as screen points. Returns the points and how
    /// many there are; 0 means nothing of it is in front of the camera.
    pub fn clip_polygon(&self, pts: &[V3]) -> ([V2; 8], usize) {
        let mut out = [V2::ZERO; 8];
        let mut n = 0;
        let k = pts.len();
        for i in 0..k {
            let a = self.to_cam(pts[i]);
            let b = self.to_cam(pts[(i + 1) % k]);
            let (a_in, b_in) = (a[2] >= self.near, b[2] >= self.near);
            if a_in {
                out[n] = self.cam_to_screen(a);
                n += 1;
            }
            if a_in != b_in {
                let t = (self.near - a[2]) / (b[2] - a[2]);
                out[n] = self.cam_to_screen([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, self.near]);
                n += 1;
            }
        }
        (out, n)
    }

    /// A line segment cut off at the near plane, as screen points. None if it is entirely behind the camera.
    pub fn clip_segment(&self, p: V3, q: V3) -> Option<(V2, V2)> {
        let (mut a, mut b) = (self.to_cam(p), self.to_cam(q));
        let (a_in, b_in) = (a[2] >= self.near, b[2] >= self.near);
        if !a_in && !b_in {
            return None;
        }
        if a_in != b_in {
            let t = (self.near - a[2]) / (b[2] - a[2]);
            let m = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, self.near];
            if a_in { b = m } else { a = m }
        }
        Some((self.cam_to_screen(a), self.cam_to_screen(b)))
    }

    /// Screen offset of a unit step along a world axis (for the axes gizmo): (right, up) components.
    pub fn axis(&self, a: V3) -> (f32, f32) {
        (dot(a, self.right), dot(a, self.up))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::model::Session;

    fn session(extra: &str) -> Session {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,d\n").unwrap();
        let n = parse_csv_text(&format!("id,x,y,z,t\n{extra}")).unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    #[test]
    fn places_nodes_with_true_proportions_and_y_up() {
        let s = session("a,0,0,0,p\nb,10,0,5,p\nc,10,20,10,q\nd,0,20,0,q\n");
        let c = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), ..Default::default() };
        let p = place(&s.g, &c).unwrap();
        assert_eq!(p.missing, 0);
        let (a, b, cc) = (s.g.index["a"] as usize, s.g.index["b"] as usize, s.g.index["c"] as usize);
        // y is 20 and x is 10: the picture is twice as tall as it is wide
        let w = (p.pos3[b][0] - p.pos3[a][0]).abs();
        let h = (p.pos3[cc][1] - p.pos3[b][1]).abs();
        assert!((h / w - 2.0).abs() < 1e-4, "{h} / {w}");
        assert!((p.pos3[cc][2] - p.pos3[a][2] - h * 0.5).abs() < 1e-3, "z (0..10) is half of y (0..20)");
        // 2D: higher y means higher on screen, which is a smaller screen y
        assert!(p.pos2[cc].y < p.pos2[b].y);
        // centred
        let mean_x: f32 = p.pos3.iter().map(|q| q[0]).sum::<f32>() / 4.0;
        assert!(mean_x.abs() < 1e-3);
        let stretched = place(&s.g, &CoordSettings { keep_aspect: false, ..c.clone() }).unwrap();
        let w2 = (stretched.pos3[b][0] - stretched.pos3[a][0]).abs();
        let h2 = (stretched.pos3[cc][1] - stretched.pos3[b][1]).abs();
        assert!((h2 / w2 - 1.0).abs() < 1e-4, "each axis fills the same extent");
    }

    #[test]
    fn missing_values_go_to_the_middle_and_are_counted() {
        let s = session("a,0,0,0,p\nb,,5,5,p\nc,10,x,10,q\nd,4,4,4,q\n");
        let c = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), ..Default::default() };
        // y holds text in one row, so the column is not numeric: nothing can be placed
        assert!(place(&s.g, &c).is_none());
        let s = session("a,0,0,0,p\nb,,5,5,p\nc,10,8,10,q\nd,4,4,4,q\n");
        let p = place(&s.g, &c).unwrap();
        assert_eq!(p.missing, 1);
        assert!(p.pos3.iter().all(|q| q.iter().all(|v| v.is_finite())));
        assert!(place(&s.g, &CoordSettings { x: None, ..c.clone() }).is_none());
        assert!(place(&s.g, &CoordSettings { x: Some("t".into()), ..c.clone() }).is_none(), "text column");
    }

    #[test]
    fn columns_are_guessed_by_name() {
        let s = session("a,0,0,0,p\nb,1,1,1,p\nc,2,2,2,q\nd,3,3,3,q\n");
        assert_eq!(guess_columns(&s.g), (Some("x".into()), Some("y".into()), Some("z".into())));
    }

    #[test]
    fn projection_basics() {
        let size = V2::new(800.0, 600.0);
        let mut cam = Cam3 { target: [0.0; 3], yaw: 0.0, pitch: 0.0, dist: 100.0, ortho: false };
        let o = cam.orbit(size);
        let (c, _, depth) = o.project([0.0; 3]).unwrap();
        assert!((c - size * 0.5).length() < 1e-3);
        assert!((depth - 100.0).abs() < 1e-3);
        let (right, _, _) = o.project([10.0, 0.0, 0.0]).unwrap();
        let (up, _, _) = o.project([0.0, 0.0, 10.0]).unwrap();
        assert!(right.x > c.x + 1.0 && (right.y - c.y).abs() < 1e-3, "x points right");
        assert!(up.y < c.y - 1.0 && (up.x - c.x).abs() < 1e-3, "z points up the screen");
        let (near, fnear, _) = o.project([0.0, -50.0, 0.0]).unwrap();
        let (far, ffar, _) = o.project([0.0, 50.0, 0.0]).unwrap();
        assert!(fnear > ffar, "closer things are bigger");
        assert!((near - c).length() < 1e-3 && (far - c).length() < 1e-3);
        assert!(o.project([0.0, -200.0, 0.0]).is_none(), "behind the camera");
        // top view: x right, y up the screen
        cam.preset(View3::Top);
        let o = cam.orbit(size);
        let (px, _, _) = o.project([10.0, 0.0, 0.0]).unwrap();
        let (py, _, _) = o.project([0.0, 10.0, 0.0]).unwrap();
        assert!(px.x > size.x * 0.5 + 1.0 && py.y < size.y * 0.5 - 1.0, "{px:?} {py:?}");
    }

    #[test]
    fn fit_frames_everything() {
        let pts: Vec<V3> = (0..100).map(|i| [i as f32, (i % 10) as f32 * 3.0, (i % 7) as f32]).collect();
        let mut cam = Cam3::default();
        cam.fit(&pts);
        let o = cam.orbit(V2::new(800.0, 600.0));
        for p in &pts {
            let (s, _, _) = o.project(*p).unwrap();
            assert!(s.x > 0.0 && s.x < 800.0 && s.y > 0.0 && s.y < 600.0, "{s:?}");
        }
    }

    fn on(s: &mut Session, z: bool) {
        s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: z.then(|| "z".into()), three_d: z, ..Default::default() };
        s.apply_coords();
    }

    #[test]
    fn session_places_nodes_and_gives_the_layout_back() {
        let mut s = session("a,0,0,0,p\nb,10,0,5,p\nc,10,20,10,q\nd,0,20,0,q\n");
        let before = s.pos.clone();
        on(&mut s, true);
        assert!(s.coords_active() && s.is_3d());
        assert_eq!(s.pos3.len(), 4);
        assert_ne!(s.pos, before);
        // layouts do nothing while coordinates are in use
        let placed = s.pos.clone();
        s.apply_layout_now();
        assert_eq!(s.pos, placed);
        assert!(!s.sim.running);
        // without a Z column there is no 3D view
        on(&mut s, false);
        assert!(s.coords_active() && !s.is_3d());
        // switching off brings the old layout back
        s.coords.enabled = false;
        s.apply_coords();
        assert!(!s.coords_active());
        assert_eq!(s.pos, before);
    }

    #[test]
    fn a_rebuild_keeps_the_coordinates() {
        let mut s = session("a,0,0,0,p\nb,10,0,5,p\nc,10,20,10,q\nd,0,20,0,q\n");
        on(&mut s, true);
        let p3 = s.pos3.clone();
        s.rebuild();
        assert!(s.is_3d());
        assert_eq!(s.pos3, p3);
        // a column that disappears switches coordinates off instead of leaving a broken state
        s.coords.x = Some("gone".into());
        s.rebuild();
        assert!(!s.coords.enabled && !s.coords_active());
    }

    #[test]
    fn three_d_drawing_is_far_to_near_and_picking_takes_the_nearest() {
        use crate::model::*;
        // a and b line up behind each other when seen from the front; c is off to the side
        let mut s = session("a,0,0,0,p\nb,0,10,0,p\nc,8,5,3,q\nd,-8,2,-3,q\n");
        on(&mut s, true);
        s.cam3.yaw = 0.0;
        s.cam3.pitch = 0.0;
        s.fit_cam3();
        let size = V2::new(800.0, 600.0);
        let cam = ViewCam::default();
        let none = vec![false; 4];
        let hi = Highlight { focus: &none, hood: &none, active: false, edges: None, hide: false };
        let mut dl = DrawList::default();
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.nodes.len(), 4);
        assert!(dl.nodes.windows(2).all(|w| w[0].depth >= w[1].depth), "painter's order");
        let (a, b) = (s.g.index["a"] as usize, s.g.index["b"] as usize);
        let na = dl.nodes.iter().find(|n| n.idx as usize == a).unwrap();
        let nb = dl.nodes.iter().find(|n| n.idx as usize == b).unwrap();
        assert!((na.x - nb.x).abs() < 0.5 && (na.y - nb.y).abs() < 0.5, "they overlap on screen");
        assert!(na.depth < nb.depth, "a is nearer (the camera looks along +y)");
        let hit = pick_node(&s, &cam, size, V2::new(na.x, na.y)).unwrap();
        assert_eq!(hit, a, "the nearer of two overlapping nodes is picked");
        // seen from behind, b is the nearer one
        s.cam3.yaw = std::f32::consts::PI;
        let p = s.projector(&cam, size);
        let (sb, _, _) = p.at(&s, b).unwrap();
        assert_eq!(pick_node(&s, &cam, size, sb), Some(b));
    }

    #[test]
    fn projector_is_flat_without_coordinates() {
        use crate::model::*;
        let s = session("a,0,0,0,p\nb,1,1,1,p\nc,2,2,2,q\nd,3,3,3,q\n");
        assert!(!s.is_3d());
        assert!(matches!(s.projector(&ViewCam::default(), V2::new(100.0, 100.0)), Projector::Flat { .. }));
    }

    #[test]
    fn clipping_cuts_at_the_near_plane() {
        let size = V2::new(800.0, 600.0);
        let cam = Cam3 { target: [0.0; 3], yaw: 0.0, pitch: 0.0, dist: 100.0, ortho: false };
        let o = cam.orbit(size);
        // the camera looks along +y from y = -100; near is 2 units in front of it
        assert!((o.near - 2.0).abs() < 1e-4);
        // a segment entirely in front is untouched; one entirely behind is gone
        assert!(o.clip_segment([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]).is_some());
        assert!(o.clip_segment([0.0, -300.0, 0.0], [10.0, -200.0, 0.0]).is_none());
        // a segment running from far behind to far in front is cut to the near plane: finite, and on screen centre line
        let (a, b) = o.clip_segment([0.0, -500.0, 0.0], [0.0, 500.0, 0.0]).unwrap();
        assert!(a.x.is_finite() && b.x.is_finite() && (a.x - 400.0).abs() < 1e-3 && (b.x - 400.0).abs() < 1e-3);
        // a square that reaches behind the camera loses the corners there and gains cut points: no more than 5
        let quad = [[-50.0, -500.0, -50.0], [50.0, -500.0, -50.0], [50.0, 0.0, -50.0], [-50.0, 0.0, -50.0]];
        let (pts, n) = o.clip_polygon(&quad);
        assert!((3..=5).contains(&n), "{n}");
        assert!(pts[..n].iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        // a polygon wholly in front keeps its corners; one wholly behind has none
        assert_eq!(o.clip_polygon(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]).1, 3);
        assert_eq!(o.clip_polygon(&[[0.0, -400.0, 0.0], [1.0, -400.0, 0.0], [1.0, -400.0, 1.0]]).1, 0);
    }
}
