//! Node shapes: ten flat shapes for the graph, and a solid version of each for the 3D coordinates view (circle and
//! sphere, square and cube, triangle and cone …). The flat outlines are drawn around (0, 0) with radius about 1,
//! y pointing down as on screen. The solids fill the cube from −1 to 1 on every axis (x right, y into the screen,
//! z up), so a node's box can stretch them to fill it; flat solids (prisms, the torus) are made thinner along y when
//! they are drawn at node size.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Pos2};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub type V3 = [f32; 3];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum NodeShape {
    #[default]
    Circle,
    Square,
    Triangle,
    TriangleDown,
    Diamond,
    Pentagon,
    Hexagon,
    Star,
    Plus,
    Ring,
}

impl NodeShape {
    pub const ALL: [NodeShape; 10] = [
        NodeShape::Circle,
        NodeShape::Square,
        NodeShape::Triangle,
        NodeShape::TriangleDown,
        NodeShape::Diamond,
        NodeShape::Pentagon,
        NodeShape::Hexagon,
        NodeShape::Star,
        NodeShape::Plus,
        NodeShape::Ring,
    ];

    pub fn from_index(i: u8) -> NodeShape {
        NodeShape::ALL[i as usize % NodeShape::ALL.len()]
    }

    pub fn index(self) -> u8 {
        NodeShape::ALL.iter().position(|&s| s == self).unwrap_or(0) as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeShape::Circle => "circle",
            NodeShape::Square => "square",
            NodeShape::Triangle => "triangle",
            NodeShape::TriangleDown => "triangle, point down",
            NodeShape::Diamond => "diamond",
            NodeShape::Pentagon => "pentagon",
            NodeShape::Hexagon => "hexagon",
            NodeShape::Star => "star",
            NodeShape::Plus => "plus",
            NodeShape::Ring => "ring",
        }
    }

    pub fn solid_name(self) -> &'static str {
        match self {
            NodeShape::Circle => "sphere",
            NodeShape::Square => "cube",
            NodeShape::Triangle => "cone",
            NodeShape::TriangleDown => "cone, point down",
            NodeShape::Diamond => "octahedron",
            NodeShape::Pentagon => "pentagonal prism",
            NodeShape::Hexagon => "hexagonal prism",
            NodeShape::Star => "star prism",
            NodeShape::Plus => "3D cross",
            NodeShape::Ring => "torus",
        }
    }

    /// How thick the solid is along y (into the screen) when it is drawn at node size: prisms and the torus stand
    /// like coins facing the viewer; the others are as deep as they are wide.
    pub fn depth(self) -> f32 {
        match self {
            NodeShape::Pentagon | NodeShape::Hexagon | NodeShape::Star => 0.45,
            NodeShape::Ring => 0.34,
            _ => 1.0,
        }
    }

    /// The flat outline around (0, 0), y down. Every outline is star-shaped from the centre, so a fan of triangles
    /// from the centre fills it. The ring is the outer circle (its hole is cut by `fill_flat`).
    pub fn outline(self) -> &'static [[f32; 2]] {
        static CACHE: OnceLock<Vec<Vec<[f32; 2]>>> = OnceLock::new();
        &CACHE.get_or_init(|| NodeShape::ALL.iter().map(|s| make_outline(*s)).collect())[self.index() as usize]
    }

    /// The solid as triangles, wound counter-clockwise seen from outside, filling −1 … 1 on every axis.
    pub fn solid(self) -> &'static [[V3; 3]] {
        static CACHE: OnceLock<Vec<Vec<[V3; 3]>>> = OnceLock::new();
        &CACHE.get_or_init(|| NodeShape::ALL.iter().map(|s| make_solid(*s)).collect())[self.index() as usize]
    }
}

fn regular(n: usize, r: f32, start: f32) -> Vec<[f32; 2]> {
    (0..n).map(|k| {
        let a = start + std::f32::consts::TAU * k as f32 / n as f32;
        [r * a.cos(), r * a.sin()]
    })
    .collect()
}

fn make_outline(s: NodeShape) -> Vec<[f32; 2]> {
    use std::f32::consts::FRAC_PI_2;
    // sizes chosen so the shapes look about as heavy as a circle of radius 1
    match s {
        NodeShape::Circle | NodeShape::Ring => regular(28, 1.0, 0.0),
        NodeShape::Square => vec![[-0.86, -0.86], [0.86, -0.86], [0.86, 0.86], [-0.86, 0.86]],
        // y down: a point at −y is the top; the triangles sit with their middle on the centre
        NodeShape::Triangle => regular(3, 1.2, -FRAC_PI_2).into_iter().map(|[x, y]| [x, y + 0.15]).collect(),
        NodeShape::TriangleDown => regular(3, 1.2, FRAC_PI_2).into_iter().map(|[x, y]| [x, y - 0.15]).collect(),
        NodeShape::Diamond => vec![[0.0, -1.15], [1.0, 0.0], [0.0, 1.15], [-1.0, 0.0]],
        NodeShape::Pentagon => regular(5, 1.06, -FRAC_PI_2),
        NodeShape::Hexagon => regular(6, 1.02, 0.0),
        NodeShape::Star => (0..10)
            .map(|k| {
                let r = if k % 2 == 0 { 1.22 } else { 0.52 };
                let a = -FRAC_PI_2 + std::f32::consts::PI * k as f32 / 5.0;
                [r * a.cos(), r * a.sin()]
            })
            .collect(),
        NodeShape::Plus => {
            let (l, w) = (1.05, 0.36);
            vec![[-w, -l], [w, -l], [w, -w], [l, -w], [l, w], [w, w], [w, l], [-w, l], [-w, w], [-l, w], [-l, -w], [-w, -w]]
        }
    }
}

// ── solids ──

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Turn every triangle to face away from `inside(centroid)`, the point it must face away from.
fn orient(tris: &mut [[V3; 3]], inside: impl Fn(V3) -> V3) {
    for t in tris.iter_mut() {
        let c = [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0, (t[0][2] + t[1][2] + t[2][2]) / 3.0];
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        if dot(n, sub(c, inside(c))) < 0.0 {
            t.swap(1, 2);
        }
    }
}

fn quad(out: &mut Vec<[V3; 3]>, a: V3, b: V3, c: V3, d: V3) {
    out.push([a, b, c]);
    out.push([a, c, d]);
}

/// A prism of a flat outline standing in the x–z plane (facing the viewer along y), from y = −1 to 1.
fn prism(outline: &[[f32; 2]]) -> Vec<[V3; 3]> {
    // fit the outline into −1 … 1 (y down on screen is z up here)
    let m = outline.iter().fold(0.0f32, |m, p| m.max(p[0].abs()).max(p[1].abs()));
    let pts: Vec<[f32; 2]> = outline.iter().map(|p| [p[0] / m, -p[1] / m]).collect();
    let n = pts.len();
    let mut t = Vec::new();
    for k in 0..n {
        let (p, q) = (pts[k], pts[(k + 1) % n]);
        let (pf, qf, pb, qb) = ([p[0], -1.0, p[1]], [q[0], -1.0, q[1]], [p[0], 1.0, p[1]], [q[0], 1.0, q[1]]);
        quad(&mut t, pf, qf, qb, pb);
        t.push([[0.0, -1.0, 0.0], pf, qf]);
        t.push([[0.0, 1.0, 0.0], pb, qb]);
    }
    orient(&mut t, |c| [0.0, if c[1].abs() > 0.999 { 0.0 } else { c[1] }, 0.0]);
    t
}

fn make_solid(s: NodeShape) -> Vec<[V3; 3]> {
    use std::f32::consts::{PI, TAU};
    let mut t: Vec<[V3; 3]> = Vec::new();
    match s {
        NodeShape::Circle => {
            let (seg, rings) = (14, 9);
            let at = |i: usize, j: usize| -> V3 {
                let (th, ph) = (TAU * i as f32 / seg as f32, PI * j as f32 / rings as f32);
                [ph.sin() * th.cos(), ph.sin() * th.sin(), ph.cos()]
            };
            for j in 0..rings {
                for i in 0..seg {
                    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    if j > 0 {
                        t.push([a, b, c]);
                    }
                    if j + 1 < rings {
                        t.push([a, c, d]);
                    }
                }
            }
            orient(&mut t, |_| [0.0; 3]);
        }
        NodeShape::Square => {
            let c = |x: f32, y: f32, z: f32| [x, y, z];
            let (l, h) = (-1.0, 1.0);
            quad(&mut t, c(l, l, l), c(h, l, l), c(h, h, l), c(l, h, l));
            quad(&mut t, c(l, l, h), c(h, l, h), c(h, h, h), c(l, h, h));
            quad(&mut t, c(l, l, l), c(h, l, l), c(h, l, h), c(l, l, h));
            quad(&mut t, c(l, h, l), c(h, h, l), c(h, h, h), c(l, h, h));
            quad(&mut t, c(l, l, l), c(l, h, l), c(l, h, h), c(l, l, h));
            quad(&mut t, c(h, l, l), c(h, h, l), c(h, h, h), c(h, l, h));
            orient(&mut t, |_| [0.0; 3]);
        }
        NodeShape::Triangle | NodeShape::TriangleDown => {
            let up = if s == NodeShape::Triangle { 1.0 } else { -1.0 };
            let seg = 18;
            let (tip, base) = ([0.0, 0.0, up], -up);
            for i in 0..seg {
                let a = TAU * i as f32 / seg as f32;
                let b = TAU * (i + 1) as f32 / seg as f32;
                let (p, q) = ([a.cos(), a.sin(), base], [b.cos(), b.sin(), base]);
                t.push([tip, p, q]);
                t.push([[0.0, 0.0, base], p, q]);
            }
            // the middle of the cone's volume sits a quarter of the way up from the base
            orient(&mut t, move |_| [0.0, 0.0, base * 0.5]);
        }
        NodeShape::Diamond => {
            let v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]];
            for k in 0..4 {
                t.push([[0.0, 0.0, 1.0], v[k], v[(k + 1) % 4]]);
                t.push([[0.0, 0.0, -1.0], v[k], v[(k + 1) % 4]]);
            }
            orient(&mut t, |_| [0.0; 3]);
        }
        NodeShape::Pentagon | NodeShape::Hexagon | NodeShape::Star => t = prism(s.outline()),
        NodeShape::Plus => {
            // six arms from a hidden middle cube: each arm is a box with its inner face left off
            let w = 0.3;
            for axis in 0..3 {
                for sign in [-1.0f32, 1.0] {
                    let corner = |a: f32, b: f32, c: f32| -> V3 {
                        // a along the arm, b and c across it
                        let mut p = [0.0; 3];
                        p[axis] = a;
                        p[(axis + 1) % 3] = b;
                        p[(axis + 2) % 3] = c;
                        p
                    };
                    let (i, o) = (w * sign, sign);
                    let mut arm = Vec::new();
                    quad(&mut arm, corner(i, -w, -w), corner(o, -w, -w), corner(o, w, -w), corner(i, w, -w));
                    quad(&mut arm, corner(i, -w, w), corner(o, -w, w), corner(o, w, w), corner(i, w, w));
                    quad(&mut arm, corner(i, -w, -w), corner(o, -w, -w), corner(o, -w, w), corner(i, -w, w));
                    quad(&mut arm, corner(i, w, -w), corner(o, w, -w), corner(o, w, w), corner(i, w, w));
                    quad(&mut arm, corner(o, -w, -w), corner(o, w, -w), corner(o, w, w), corner(o, -w, w));
                    let mid = corner((i + o) * 0.5, 0.0, 0.0);
                    orient(&mut arm, move |_| mid);
                    t.extend(arm);
                }
            }
        }
        NodeShape::Ring => {
            // a torus standing in the x–z plane, its hole facing the viewer; the tube is stretched along y to ±1
            let (big, tube, seg, sides) = (0.68f32, 0.32f32, 22, 9);
            let at = |i: usize, j: usize| -> V3 {
                let (u, v) = (TAU * i as f32 / seg as f32, TAU * j as f32 / sides as f32);
                let r = big + tube * v.cos();
                [r * u.cos(), v.sin(), r * u.sin()]
            };
            for i in 0..seg {
                for j in 0..sides {
                    quad(&mut t, at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                }
            }
            orient(&mut t, move |c| {
                let l = (c[0] * c[0] + c[2] * c[2]).sqrt().max(1e-6);
                [big * c[0] / l, 0.0, big * c[2] / l]
            });
        }
    }
    t
}

// ── drawing ──

/// Add a flat shape to `mesh` (untextured: it must use the white texel), with a soft edge about a pixel wide.
pub fn fill_flat(mesh: &mut Mesh, shape: NodeShape, c: Pos2, r: f32, color: Color32) {
    let out = shape.outline();
    let clear = Color32::TRANSPARENT;
    let feather = 0.7f32.min(r * 0.5);
    let base = mesh.vertices.len() as u32;
    let push = |mesh: &mut Mesh, k: f32, p: [f32; 2], col: Color32| {
        mesh.vertices.push(Vertex { pos: Pos2::new(c.x + p[0] * k, c.y + p[1] * k), uv: WHITE_UV, color: col });
    };
    let n = out.len() as u32;
    let (k_in, k_out) = ((r - feather).max(0.0), r + feather);
    if shape == NodeShape::Ring {
        // the band between the outer circle and a hole of 55 %, feathered on both sides
        let hole = r * 0.55;
        for p in out {
            push(mesh, hole - feather, *p, clear);
            push(mesh, hole + feather, *p, color);
            push(mesh, k_in, *p, color);
            push(mesh, k_out, *p, clear);
        }
        for k in 0..n {
            let (a, b) = (base + k * 4, base + ((k + 1) % n) * 4);
            for s in 0..3 {
                mesh.indices.extend_from_slice(&[a + s, b + s, b + s + 1, a + s, b + s + 1, a + s + 1]);
            }
        }
        return;
    }
    mesh.vertices.push(Vertex { pos: c, uv: WHITE_UV, color });
    for p in out {
        push(mesh, k_in, *p, color);
        push(mesh, k_out, *p, clear);
    }
    for k in 0..n {
        let (a, b) = (base + 1 + k * 2, base + 1 + ((k + 1) % n) * 2);
        mesh.indices.extend_from_slice(&[base, a, b, a, b, b + 1, a, b + 1, a + 1]);
    }
}

/// How many vertices `fill_flat` adds for a shape.
pub fn flat_vertices(shape: NodeShape) -> usize {
    let n = shape.outline().len();
    if shape == NodeShape::Ring { n * 4 } else { 1 + n * 2 }
}

/// A shape's outline as screen points around `c` (for exports); the ring's hole is the same circle at 55 %.
pub fn outline_at(shape: NodeShape, c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    shape.outline().iter().map(|p| [c[0] + p[0] * r, c[1] + p[1] * r]).collect()
}

/// One solid placed in the world: its centre and how far it reaches along each axis.
pub struct Placed {
    pub centre: V3,
    pub half: V3,
}

/// The visible triangles of a solid as screen triangles with their shaded colours, far ones first.
/// `project` maps a world point to the screen and its depth (None behind the camera); `eye` and `fwd` are the
/// camera position and view direction, `light` the direction the light comes from.
pub fn solid_on_screen(
    shape: NodeShape,
    at: &Placed,
    project: impl Fn(V3) -> Option<([f32; 2], f32)>,
    eye: V3,
    fwd: V3,
    ortho: bool,
    light: V3,
    rgba: [u8; 4],
) -> Vec<([[f32; 2]; 3], [u8; 4], f32)> {
    let place = |p: V3| [at.centre[0] + p[0] * at.half[0], at.centre[1] + p[1] * at.half[1], at.centre[2] + p[2] * at.half[2]];
    let mut out = Vec::with_capacity(shape.solid().len() / 2);
    for t in shape.solid() {
        let w = [place(t[0]), place(t[1]), place(t[2])];
        let n = cross(sub(w[1], w[0]), sub(w[2], w[0]));
        let nl = dot(n, n).sqrt();
        if nl < 1e-12 {
            continue;
        }
        let n = [n[0] / nl, n[1] / nl, n[2] / nl];
        let cen = [(w[0][0] + w[1][0] + w[2][0]) / 3.0, (w[0][1] + w[1][1] + w[2][1]) / 3.0, (w[0][2] + w[1][2] + w[2][2]) / 3.0];
        let view = if ortho { fwd } else { sub(cen, eye) };
        if dot(n, view) >= 0.0 {
            continue; // facing away
        }
        let (Some(a), Some(b), Some(c)) = (project(w[0]), project(w[1]), project(w[2])) else { continue };
        let lit = 0.38 + 0.62 * dot(n, light).max(0.0);
        let shade = |v: u8| ((v as f32 * lit).round()).clamp(0.0, 255.0) as u8;
        out.push(([a.0, b.0, c.0], [shade(rgba[0]), shade(rgba[1]), shade(rgba[2]), rgba[3]], (a.1 + b.1 + c.1) / 3.0));
    }
    // painter's order inside the shape (needed for the cross, the star and the torus)
    out.sort_by(|x, y| y.2.total_cmp(&x.2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solids_fill_the_unit_cube_and_face_outwards() {
        for s in NodeShape::ALL {
            let tris = s.solid();
            assert!(!tris.is_empty(), "{s:?}");
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for t in tris {
                for p in t {
                    for a in 0..3 {
                        lo[a] = lo[a].min(p[a]);
                        hi[a] = hi[a].max(p[a]);
                    }
                }
            }
            for a in 0..3 {
                assert!(lo[a] >= -1.001 && hi[a] <= 1.001, "{s:?} leaves the cube");
            }
            // seen from far away along any axis, a solid shows some faces and hides others
            for eye in [[0.0, -50.0, 0.0], [50.0, 0.0, 0.0], [0.0, 0.0, 50.0]] {
                let at = Placed { centre: [0.0; 3], half: [1.0; 3] };
                let fwd = [-eye[0] / 50.0, -eye[1] / 50.0, -eye[2] / 50.0];
                let seen = solid_on_screen(s, &at, |p| Some(([p[0], p[2]], dot(sub(p, eye), fwd))), eye, fwd, false, [0.0, -1.0, 0.0], [200, 100, 50, 255]);
                assert!(!seen.is_empty() && seen.len() < tris.len(), "{s:?} from {eye:?}: {} of {}", seen.len(), tris.len());
            }
        }
    }

    #[test]
    fn flat_shapes_have_a_mesh() {
        for s in NodeShape::ALL {
            let mut m = Mesh::default();
            fill_flat(&mut m, s, Pos2::new(10.0, 10.0), 6.0, Color32::WHITE);
            assert_eq!(m.vertices.len(), flat_vertices(s), "{s:?}");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        }
    }
}
