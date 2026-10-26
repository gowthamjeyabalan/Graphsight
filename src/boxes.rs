//! Bounding boxes: each node can carry a 3D box (a minimum and a maximum value for X, Y and Z, stored as six
//! numeric node attributes). The boxes of the nodes that match an attribute rule are drawn in the 3D view as
//! translucent boxes behind the nodes.

use crate::coords::Xform;
use crate::graph::Graph;
use crate::model::Rule;
use crate::style::Rgb;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoxColor {
    /// One colour for every box.
    Single,
    /// Each box takes the colour of its node (so "colour by attribute" in Appearance colours the boxes too).
    Node,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxSettings {
    pub enabled: bool,
    /// Minimum X, Y, Z columns.
    pub min: [Option<String>; 3],
    /// Maximum X, Y, Z columns.
    pub max: [Option<String>; 3],
    /// Only the nodes that match get a box. None: every node does.
    pub rule: Option<Rule>,
    pub color_mode: BoxColor,
    pub color: Rgb,
    /// Opacity of the box faces, 0 to 1.
    pub fill: f32,
    /// Opacity of the box edges, 0 to 1.
    pub outline: f32,
    pub line_width: f32,
    /// Draw the nodes and edges as well. Off shows the boxes alone.
    pub show_nodes: bool,
    /// Each node with a box is drawn as its solid shape stretched to fill the box.
    pub fit_shapes: bool,
}

impl Default for BoxSettings {
    fn default() -> Self {
        BoxSettings {
            enabled: false,
            min: Default::default(),
            max: Default::default(),
            rule: None,
            color_mode: BoxColor::Single,
            color: [10, 132, 255],
            fill: 0.14,
            outline: 0.85,
            line_width: 1.4,
            show_nodes: true,
            fit_shapes: false,
        }
    }
}

impl BoxSettings {
    /// All six columns are chosen.
    pub fn has_columns(&self) -> bool {
        self.min.iter().chain(self.max.iter()).all(|c| c.is_some())
    }
}

/// One box in the same units as the node positions in 3D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxItem {
    pub node: u32,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxInfo {
    /// Nodes that match the rule.
    pub n_match: usize,
    /// Of those, nodes that actually have a box.
    pub n_boxes: usize,
    /// Matching nodes with a missing or unreadable value in one of the six columns (no box).
    pub n_incomplete: usize,
    /// Boxes that lie far outside the cloud of nodes (placeholder values, a site outline, another datum). The
    /// camera ignores them when it frames the view.
    pub n_far: usize,
}

/// Work out the boxes for the nodes that match the rule. `xf` is the transform the node positions use.
pub fn compute(g: &Graph, xf: &Xform, st: &BoxSettings) -> (Vec<BoxItem>, BoxInfo) {
    let n = g.n();
    let cols: Vec<Option<&crate::data::Column>> = st
        .min
        .iter()
        .chain(st.max.iter())
        .map(|name| name.as_deref().and_then(|c| g.node_col(c)).filter(|c| c.is_numeric()))
        .collect();
    if n == 0 || cols.iter().any(|c| c.is_none()) {
        return (vec![], BoxInfo::default());
    }
    let cols: Vec<&crate::data::Column> = cols.into_iter().flatten().collect();
    let mut keep = vec![true; n];
    if let Some(r) = st.rule.as_ref().filter(|r| r.enabled) {
        crate::subgraph::rule_pass(g, r, &mut keep);
    }
    let mut info = BoxInfo::default();
    let mut items = Vec::new();
    for i in 0..n {
        if !keep[i] {
            continue;
        }
        info.n_match += 1;
        let mut v = [0.0f64; 6];
        let mut ok = true;
        for (k, c) in cols.iter().enumerate() {
            match c.num(i).filter(|x| x.is_finite()) {
                Some(x) => v[k] = x,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            info.n_incomplete += 1;
            continue;
        }
        // a box given with its corners the wrong way round is still a box
        let (mut a, mut b) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
        for k in 0..3 {
            if a[k] > b[k] {
                std::mem::swap(&mut a[k], &mut b[k]);
            }
        }
        items.push(BoxItem { node: i as u32, lo: xf.apply(a), hi: xf.apply(b) });
        info.n_boxes += 1;
    }
    (items, info)
}

/// Pick likely min / max columns by name (`bbox_min_x`, `xmin`, `min_x`, `bounds_max_z`, …).
pub fn guess_columns(g: &Graph) -> ([Option<String>; 3], [Option<String>; 3]) {
    const PREFIXES: &[&str] = &["boundingbox", "bounds", "bbox", "box", "bb"];
    let mut min: [Option<String>; 3] = Default::default();
    let mut max: [Option<String>; 3] = Default::default();
    for c in g.node_cols.iter().filter(|c| c.is_numeric() && !c.is_metric) {
        let mut name: String = c.name.to_lowercase().chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
        for p in PREFIXES {
            if let Some(rest) = name.strip_prefix(p) {
                name = rest.to_string();
                break;
            }
        }
        for (k, axis) in ['x', 'y', 'z'].into_iter().enumerate() {
            let is = |word: &str| name == format!("{word}{axis}") || name == format!("{axis}{word}");
            if (is("min") || is("minimum") || is("lo") || is("low")) && min[k].is_none() {
                min[k] = Some(c.name.clone());
            } else if (is("max") || is("maximum") || is("hi") || is("high")) && max[k].is_none() {
                max[k] = Some(c.name.clone());
            }
        }
    }
    (min, max)
}

/// The six faces of a box as corner indices. Corner `k` has its X at the high end if bit 0 is set, its Y if bit 1
/// is set and its Z if bit 2 is set. Faces in order: X low, X high, Y low, Y high, Z low, Z high.
pub const FACES: [[usize; 4]; 6] = [[0, 2, 6, 4], [1, 3, 7, 5], [0, 1, 5, 4], [2, 3, 7, 6], [0, 1, 3, 2], [4, 5, 7, 6]];

/// The twelve edges: the two corners, and the two faces the edge lies on (indices into `FACES`).
pub const EDGES: [(usize, usize, usize, usize); 12] = [
    (0, 1, 2, 4),
    (2, 3, 3, 4),
    (4, 5, 2, 5),
    (6, 7, 3, 5),
    (0, 2, 0, 4),
    (1, 3, 1, 4),
    (4, 6, 0, 5),
    (5, 7, 1, 5),
    (0, 4, 0, 2),
    (1, 5, 1, 2),
    (2, 6, 0, 3),
    (3, 7, 1, 3),
];

pub fn corner(b: &BoxItem, k: usize) -> [f32; 3] {
    [if k & 1 != 0 { b.hi[0] } else { b.lo[0] }, if k & 2 != 0 { b.hi[1] } else { b.lo[1] }, if k & 4 != 0 { b.hi[2] } else { b.lo[2] }]
}

/// Which faces look towards the viewer at `eye`.
pub fn front_faces(b: &BoxItem, eye: [f32; 3]) -> [bool; 6] {
    [eye[0] < b.lo[0], eye[0] > b.hi[0], eye[1] < b.lo[1], eye[1] > b.hi[1], eye[2] < b.lo[2], eye[2] > b.hi[2]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::{self, CoordSettings};
    use crate::data::*;
    use crate::model::{RuleKind, Session};

    fn session() -> Session {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,d\n").unwrap();
        let n = parse_csv_text(
            "id,x,y,z,kind,bbox_min_x,bbox_min_y,bbox_min_z,bbox_max_x,bbox_max_y,bbox_max_z\n\
             a,0,0,0,room,-1,-1,0,1,1,2\n\
             b,10,0,5,room,9,-2,4,11,2,6\n\
             c,10,20,10,hall,8,18,9,12,22,11\n\
             d,0,20,0,hall,,,,,,\n",
        )
        .unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    fn on(s: &mut Session) {
        s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
        s.apply_coords();
    }

    fn settings(s: &Session) -> BoxSettings {
        let (min, max) = guess_columns(&s.g);
        BoxSettings { enabled: true, min, max, ..Default::default() }
    }

    #[test]
    fn columns_are_guessed_by_name() {
        let s = session();
        let (min, max) = guess_columns(&s.g);
        assert_eq!(min, [Some("bbox_min_x".into()), Some("bbox_min_y".into()), Some("bbox_min_z".into())]);
        assert_eq!(max, [Some("bbox_max_x".into()), Some("bbox_max_y".into()), Some("bbox_max_z".into())]);
        // x / y / z themselves are not mistaken for boxes
        let e = parse_csv_text("source,target\na,b\n").unwrap();
        let n = parse_csv_text("id,xmin,xmax,ymin,ymax,zmin,zmax,Min Z\na,0,1,0,1,0,1,5\nb,0,1,0,1,0,1,5\n").unwrap();
        let mut t = Session::new();
        t.set_edges("e".into(), e.clone(), auto_edge_mapping(&e));
        t.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
        t.rebuild();
        let (min, max) = guess_columns(&t.g);
        assert_eq!(min[0].as_deref(), Some("xmin"));
        assert_eq!(max[2].as_deref(), Some("zmax"));
        assert_eq!(min[2].as_deref(), Some("zmin"), "the first match wins");
    }

    #[test]
    fn boxes_use_the_node_transform_and_skip_incomplete_rows() {
        let mut s = session();
        on(&mut s);
        let st = settings(&s);
        let xf = coords::place(&s.g, &s.coords).unwrap().xform;
        let (items, info) = compute(&s.g, &xf, &st);
        assert_eq!(info, BoxInfo { n_match: 4, n_boxes: 3, n_incomplete: 1, n_far: 0 });
        let a = s.g.index["a"] as usize;
        let ia = items.iter().find(|b| b.node as usize == a).unwrap();
        // a's box runs −1..1 in x and y around the node, so its centre in x and y is the node's position
        let p = s.pos3[a];
        assert!(((ia.lo[0] + ia.hi[0]) * 0.5 - p[0]).abs() < 1e-3);
        assert!(((ia.lo[1] + ia.hi[1]) * 0.5 - p[1]).abs() < 1e-3);
        // it is 2 units long in each of x, y and z, so with one scale for all axes the sides are equal
        let (dx, dy, dz) = (ia.hi[0] - ia.lo[0], ia.hi[1] - ia.lo[1], ia.hi[2] - ia.lo[2]);
        assert!((dx - dy).abs() < 1e-3 && (dx - dz).abs() < 1e-3 && dx > 0.0);
    }

    #[test]
    fn the_rule_picks_which_nodes_get_a_box() {
        let mut s = session();
        on(&mut s);
        let mut st = settings(&s);
        let col = s.g.node_col("kind").unwrap();
        let mut r = Rule::new_for(col);
        let labels = col.as_categories().1;
        let room = labels.iter().position(|l| l == "room").unwrap();
        if let RuleKind::Cats { allowed } = &mut r.kind {
            allowed.iter_mut().for_each(|a| *a = false);
            allowed[room] = true;
        }
        st.rule = Some(r);
        let xf = coords::place(&s.g, &s.coords).unwrap().xform;
        let (items, info) = compute(&s.g, &xf, &st);
        assert_eq!(info.n_match, 2);
        assert_eq!(items.len(), 2);
        let ids: Vec<&str> = items.iter().map(|b| s.g.ids[b.node as usize].as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b"));
        // a disabled rule matches everything again
        st.rule.as_mut().unwrap().enabled = false;
        assert_eq!(compute(&s.g, &xf, &st).1.n_match, 4);
    }

    #[test]
    fn swapped_corners_and_missing_columns() {
        let e = parse_csv_text("source,target\na,b\n").unwrap();
        let n = parse_csv_text("id,x,y,z,x0,y0,z0,x1,y1,z1\na,0,0,0,5,5,5,1,1,1\nb,9,9,9,0,0,0,9,9,9\n").unwrap();
        let mut s = Session::new();
        s.set_edges("e".into(), e.clone(), auto_edge_mapping(&e));
        s.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
        s.rebuild();
        on(&mut s);
        let name = |x: &str| Some(x.to_string());
        let st = BoxSettings { enabled: true, min: [name("x0"), name("y0"), name("z0")], max: [name("x1"), name("y1"), name("z1")], ..Default::default() };
        let xf = coords::place(&s.g, &s.coords).unwrap().xform;
        let (items, _) = compute(&s.g, &xf, &st);
        let a = items.iter().find(|b| s.g.ids[b.node as usize] == "a").unwrap();
        assert!(a.lo.iter().zip(a.hi.iter()).all(|(l, h)| l < h), "a box given back to front is turned round");
        // one column missing: no boxes
        let st2 = BoxSettings { min: [None, name("y0"), name("z0")], ..st.clone() };
        assert!(!st2.has_columns());
        assert!(compute(&s.g, &xf, &st2).0.is_empty());
        // a text column is not usable
        let st3 = BoxSettings { min: [name("id"), name("y0"), name("z0")], ..st };
        assert!(compute(&s.g, &xf, &st3).0.is_empty());
    }

    #[test]
    fn corners_faces_and_edges_are_consistent() {
        let b = BoxItem { node: 0, lo: [0.0, 0.0, 0.0], hi: [1.0, 2.0, 3.0] };
        // every face is flat: its four corners share one coordinate
        let flat_axis = [(0, 0.0), (0, 1.0), (1, 0.0), (1, 2.0), (2, 0.0), (2, 3.0)];
        for (f, (axis, value)) in FACES.iter().zip(flat_axis) {
            assert!(f.iter().all(|&k| corner(&b, k)[axis] == value), "{f:?}");
        }
        // every edge joins two corners that differ in exactly one axis, and both its faces contain both corners
        for &(a, c, f1, f2) in &EDGES {
            let (pa, pc) = (corner(&b, a), corner(&b, c));
            assert_eq!((0..3).filter(|&k| pa[k] != pc[k]).count(), 1);
            assert!(FACES[f1].contains(&a) && FACES[f1].contains(&c) && FACES[f2].contains(&a) && FACES[f2].contains(&c), "{a}-{c}");
        }
        // each corner is used by three edges
        for k in 0..8 {
            assert_eq!(EDGES.iter().filter(|e| e.0 == k || e.1 == k).count(), 3);
        }
        // looking from the high-X, low-Y, high-Z side shows exactly those three faces
        let front = front_faces(&b, [5.0, -5.0, 9.0]);
        assert_eq!(front, [false, true, true, false, false, true]);
    }

    fn shown(s: &Session) -> crate::model::DrawList {
        use crate::model::*;
        let size = egui::Vec2::new(800.0, 600.0);
        let none = vec![false; s.g.n()];
        let hi = Highlight { focus: &none, hood: &none, active: false };
        let mut dl = DrawList::default();
        build_draw_list(s, &ViewCam::default(), size, &hi, &mut dl);
        dl
    }

    #[test]
    fn the_session_draws_boxes_far_to_near_and_follows_filters() {
        let mut s = session();
        on(&mut s);
        s.boxes = settings(&s);
        s.recompute_boxes();
        assert!(s.boxes_shown());
        assert_eq!(s.box_items.len(), 3);
        s.fit_cam3();
        let dl = shown(&s);
        assert_eq!(dl.boxes.len(), 3);
        assert!(dl.boxes.windows(2).all(|w| w[0].depth >= w[1].depth), "far to near");
        assert_eq!(dl.nodes.len(), 4, "nodes are drawn as well by default");
        // a hidden node takes its box with it
        s.manual_hidden[s.g.index["a"] as usize] = true;
        s.recompute_visibility();
        assert_eq!(shown(&s).boxes.len(), 2);
        // boxes alone: no nodes or edges, and nothing to click on
        s.boxes.show_nodes = false;
        let dl = shown(&s);
        assert!(dl.nodes.is_empty() && dl.edges.is_empty() && dl.boxes.len() == 2);
        assert!(s.graph_hidden());
        assert_eq!(crate::model::pick_node(&s, &crate::model::ViewCam::default(), egui::Vec2::new(800.0, 600.0), egui::Vec2::new(400.0, 300.0)), None);
        // the camera frames the boxes, which reach further than the nodes
        s.manual_hidden.iter_mut().for_each(|h| *h = false);
        s.recompute_visibility();
        s.fit_cam3();
        let o = s.cam3.orbit(egui::Vec2::new(800.0, 600.0));
        for b in &s.box_items {
            for k in 0..8 {
                let (p, _, _) = o.project(corner(b, k)).unwrap();
                assert!(p.x > 0.0 && p.x < 800.0 && p.y > 0.0 && p.y < 600.0, "{p:?}");
            }
        }
    }

    #[test]
    fn boxes_are_off_in_2d_and_when_switched_off() {
        let mut s = session();
        on(&mut s);
        s.boxes = settings(&s);
        s.recompute_boxes();
        s.coords.three_d = false; // flat view: no boxes
        assert!(!s.boxes_shown());
        assert!(shown(&s).boxes.is_empty());
        s.coords.three_d = true;
        s.boxes.enabled = false;
        s.recompute_boxes();
        assert!(s.box_items.is_empty() && !s.boxes_shown());
        // coordinates off: nothing stays behind
        s.boxes.enabled = true;
        s.recompute_boxes();
        assert!(!s.box_items.is_empty());
        s.coords.enabled = false;
        s.apply_coords();
        assert!(s.box_items.is_empty());
    }

    #[test]
    fn opacity_and_colour_settings_reach_the_draw_list() {
        let mut s = session();
        on(&mut s);
        s.boxes = BoxSettings { fill: 0.5, outline: 0.2, color: [1, 2, 3], line_width: 3.0, ..settings(&s) };
        s.recompute_boxes();
        s.fit_cam3();
        let dl = shown(&s);
        assert!(dl.boxes.iter().all(|b| b.rgb == [1, 2, 3] && b.fill == 128 && b.line == 51 && (b.width - 3.0).abs() < 1e-6));
        s.boxes.fill = 0.0;
        assert!(shown(&s).boxes.iter().all(|b| b.fill == 0));
        // the node colour mode follows the node style
        s.boxes.color_mode = BoxColor::Node;
        let dl = shown(&s);
        assert!(dl.boxes.iter().any(|b| b.rgb != [1, 2, 3]));
        // a selected node's box stands out
        s.boxes.color_mode = BoxColor::Single;
        s.select_only(s.g.index["a"] as usize);
        let dl = shown(&s);
        assert_eq!(dl.boxes.iter().filter(|b| b.line == 255).count(), 1);
    }

    #[test]
    fn exports_contain_the_boxes_and_settings_survive_a_session_file() {
        let mut s = session();
        on(&mut s);
        s.boxes = settings(&s);
        s.recompute_boxes();
        s.fit_cam3();
        let svg = crate::render::render_svg(&s, &s.fit_camera(egui::Vec2::new(800.0, 600.0)), 800.0, 600.0);
        assert_eq!(svg.matches("<polygon").count(), 18, "six faces for each of three boxes");
        let png = crate::render::render_png(&s, &s.fit_camera(egui::Vec2::new(400.0, 300.0)), 400, 300, 1.0).unwrap();
        assert!(png.pixels().iter().any(|p| p.blue() > p.red()), "something blue was painted");
        // the settings are written to a session and read back
        let json = serde_json::to_string(&s.boxes).unwrap();
        let back: BoxSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.min, s.boxes.min);
        assert_eq!(back.color_mode, BoxColor::Single);
        // an old session file without box settings still loads
        let old: BoxSettings = serde_json::from_str("{}").unwrap();
        assert!(!old.enabled && old.show_nodes);
    }

    #[test]
    fn shapes_by_attribute_flat_in_2d_solid_in_3d_and_filling_their_boxes() {
        use crate::model::{DrawList, Highlight, build_draw_list};
        let draw = |s: &Session| {
            let size = egui::Vec2::new(800.0, 600.0);
            let none = vec![false; s.g.n()];
            let mut dl = DrawList::default();
            build_draw_list(s, &s.fit_camera(size), size, &Highlight { focus: &none, hood: &none, active: false }, &mut dl);
            dl
        };
        let mut s = session();
        s.look.node_size = 12.0;
        s.look.shape_by = Some("kind".into());
        s.recompute_style();
        // 2D: "hall" and "room" get two different flat shapes, no solids
        let dl = draw(&s);
        let shapes: std::collections::HashSet<u8> = dl.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(shapes.len(), 2);
        assert!(dl.nodes.iter().all(|n| n.solid.is_empty()));
        assert_eq!(s.style.shape_legend.len(), 2);
        let svg = crate::render::render_svg(&s, &s.fit_camera(egui::Vec2::new(800.0, 600.0)), 800.0, 600.0);
        assert!(svg.contains("<polygon"), "a square is a polygon in the SVG");
        // 3D: big enough nodes become shaded solids
        on(&mut s);
        s.fit_cam3();
        s.recompute_style();
        let dl = draw(&s);
        assert!(dl.nodes.iter().any(|n| !n.solid.is_empty()), "solids in 3D");
        let shades: std::collections::HashSet<[u8; 4]> = dl.nodes.iter().flat_map(|n| n.solid.iter().map(|t| t.1)).collect();
        assert!(shades.len() > 3, "faces are lit differently");
        // filling the boxes: a node's solid covers about as much screen as its box, a node without a box stays small
        s.boxes = settings(&s);
        s.boxes.fit_shapes = true;
        s.look.node_size = 1.0;
        s.recompute_boxes();
        s.recompute_style();
        let dl = draw(&s);
        let extent = |n: &crate::model::DrawNode| {
            let xs = n.solid.iter().flat_map(|t| t.0.iter().map(|p| p[0]));
            let (lo, hi) = xs.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
            hi - lo
        };
        let by_id = |id: &str| dl.nodes.iter().find(|n| s.g.ids[n.idx as usize] == id).unwrap();
        let (a, c, d) = (by_id("a"), by_id("c"), by_id("d"));
        assert!(!a.solid.is_empty() && !c.solid.is_empty(), "boxed nodes are solids");
        assert!(d.solid.is_empty(), "d has no box and is small: drawn flat");
        let box_px = dl.boxes.iter().map(|b| b.polygons().flat_map(|p| p.iter().map(|q| q[0])).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))).map(|(lo, hi)| hi - lo).fold(0.0f32, f32::max);
        assert!(extent(a).max(extent(c)) > box_px * 0.5, "a solid reaches across its box ({} vs {box_px})", extent(a).max(extent(c)));
        let png = crate::render::render_png(&s, &s.fit_camera(egui::Vec2::new(400.0, 300.0)), 400, 300, 1.0);
        assert!(png.is_some());
    }

    /// Many small boxes plus a few very large ones (a site, a floor slab): the small ones must still be drawn.
    fn crowded(n_small: usize, n_big: usize) -> Session {
        let mut nodes = String::from("id,x,y,z,kind,bx0,by0,bz0,bx1,by1,bz1\n");
        let mut edges = String::from("source,target\n");
        for i in 0..n_small {
            let (x, y, z) = ((i % 30) as f64 * 4.0, ((i / 30) % 30) as f64 * 4.0, (i / 900) as f64 * 4.0);
            nodes += &format!("r{i},{x},{y},{z},room,{},{},{},{},{},{}\n", x - 1.5, y - 1.5, z, x + 1.5, y + 1.5, z + 3.0);
            if i > 0 {
                edges += &format!("r{},r{i}\n", i - 1);
            }
        }
        for k in 0..n_big {
            let (x, y, z) = (60.0 + k as f64, 60.0, 0.0);
            nodes += &format!("big{k},{x},{y},{z},site,-3000,-3000,-500,3000,3000,500\n");
            edges += &format!("r0,big{k}\n");
        }
        let e = parse_csv_text(&edges).unwrap();
        let n = parse_csv_text(&nodes).unwrap();
        let mut s = Session::new();
        s.set_edges("e".into(), e.clone(), auto_edge_mapping(&e));
        s.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
        s.rebuild();
        s
    }

    fn on_screen(s: &Session) -> (usize, usize) {
        let dl = shown(s);
        let size = egui::Vec2::new(800.0, 600.0);
        let small = dl
            .boxes
            .iter()
            .filter(|b| {
                let pts: Vec<[f32; 2]> = b.polygons().flatten().copied().collect();
                let (x0, x1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
                let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
                let (w, h) = (x1 - x0, y1 - y0);
                w >= 4.0 && h >= 4.0 && w < size.x * 0.5 && h < size.y * 0.5 && x0 >= 0.0 && x1 <= size.x && y0 >= 0.0 && y1 <= size.y
            })
            .count();
        (dl.boxes.len(), small)
    }

    #[test]
    fn small_boxes_survive_next_to_huge_ones() {
        for big in [0usize, 3, 8, 20] {
            let mut s = crowded(700, big);
            s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
            s.apply_coords();
            let name = |x: &str| Some(x.to_string());
            s.boxes = BoxSettings { enabled: true, min: [name("bx0"), name("by0"), name("bz0")], max: [name("bx1"), name("by1"), name("bz1")], ..Default::default() };
            s.recompute_boxes();
            s.fit_cam3();
            let (total, small) = on_screen(&s);
            eprintln!("big={big}: boxes={} drawn={total} small-and-on-screen={small} dist={}", s.box_items.len(), s.cam3.dist);
            assert!(small >= 600, "big={big}: only {small} of 700 small boxes are visible ({total} drawn)");
        }
    }

    /// A few nodes with absurd box values (a placeholder such as 99999 or -9999) must not ruin the view of the rest.
    #[test]
    fn placeholder_values_do_not_hide_the_other_boxes() {
        for (bad, value) in [(1usize, 99999.0), (4, -9999.0), (12, 1e9), (30, 1e6)] {
            let mut s = crowded(700, 0);
            // overwrite the boxes of the first `bad` nodes
            let mut text = String::from("id,x,y,z,kind,bx0,by0,bz0,bx1,by1,bz1\n");
            for i in 0..700usize {
                let (x, y, z) = ((i % 30) as f64 * 4.0, ((i / 30) % 30) as f64 * 4.0, (i / 900) as f64 * 4.0);
                if i < bad {
                    text += &format!("r{i},{x},{y},{z},room,{value},{value},{value},{},{},{}\n", value + 5.0, value + 5.0, value + 5.0);
                } else {
                    text += &format!("r{i},{x},{y},{z},room,{},{},{},{},{},{}\n", x - 1.5, y - 1.5, z, x + 1.5, y + 1.5, z + 3.0);
                }
            }
            let n = parse_csv_text(&text).unwrap();
            s.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
            s.rebuild();
            s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
            s.apply_coords();
            let name = |x: &str| Some(x.to_string());
            s.boxes = BoxSettings { enabled: true, min: [name("bx0"), name("by0"), name("bz0")], max: [name("bx1"), name("by1"), name("bz1")], ..Default::default() };
            s.recompute_boxes();
            s.fit_cam3();
            let (total, small) = on_screen(&s);
            eprintln!("bad={bad} value={value}: drawn={total} readable-on-screen={small} dist={}", s.cam3.dist);
            assert!(small >= 690 - bad, "{bad} boxes at {value}: only {small} of the normal boxes can be seen");
        }
    }

    fn set_up(s: &mut Session, boxcols: [&str; 6]) {
        s.coords = CoordSettings { enabled: true, x: Some("x".into()), y: Some("y".into()), z: Some("z".into()), three_d: true, ..Default::default() };
        s.apply_coords();
        let n = |i: usize| Some(boxcols[i].to_string());
        s.boxes = BoxSettings { enabled: true, min: [n(0), n(1), n(2)], max: [n(3), n(4), n(5)], ..Default::default() };
        s.recompute_boxes();
        s.fit_cam3();
    }

    fn session_from(nodes: &str) -> Session {
        let ids: Vec<&str> = nodes.lines().skip(1).map(|l| l.split(',').next().unwrap()).collect();
        let mut edges = String::from("source,target\n");
        for w in ids.windows(2) {
            edges += &format!("{},{}\n", w[0], w[1]);
        }
        let e = parse_csv_text(&edges).unwrap();
        let n = parse_csv_text(nodes).unwrap();
        let mut s = Session::new();
        s.set_edges("e".into(), e.clone(), auto_edge_mapping(&e));
        s.set_nodes("n".into(), n.clone(), auto_node_mapping(&n));
        s.rebuild();
        s
    }

    const COLS: [&str; 6] = ["x0", "y0", "z0", "x1", "y1", "z1"];

    #[test]
    fn real_world_coordinates_with_large_values() {
        // coordinates like a national grid: hundreds of thousands, boxes of 216 x 103 x 75 and smaller ones
        let mut text = String::from("id,x,y,z,x0,y0,z0,x1,y1,z1\n");
        for i in 0..200usize {
            let (x, y, z) = (279500.0 + (i % 20) as f64 * 40.0, 201300.0 + (i / 20) as f64 * 40.0, 22500.0 + (i % 3) as f64 * 30.0);
            let (w, d, h) = if i % 10 == 0 { (216.0, 102.8, 75.15) } else { (20.0, 12.0, 8.0) };
            text += &format!("n{i},{x},{y},{z},{},{},{},{},{},{}\n", x - w / 2.0, y - d / 2.0, z - h / 2.0, x + w / 2.0, y + d / 2.0, z + h / 2.0);
        }
        let mut s = session_from(&text);
        set_up(&mut s, COLS);
        assert_eq!(s.box_info.n_boxes, 200);
        assert_eq!(s.box_info.n_far, 0);
        let (total, readable) = on_screen(&s);
        assert_eq!(total, 200);
        assert!(readable >= 150, "{readable} of 200 boxes are readable");
    }

    #[test]
    fn boxes_in_another_origin_are_still_shown_and_reported() {
        // the nodes use a local frame (0..300) while the boxes use the grid values of the sample row
        let mut text = String::from("id,x,y,z,x0,y0,z0,x1,y1,z1\n");
        for i in 0..100usize {
            let (x, y, z) = ((i % 10) as f64 * 30.0, (i / 10) as f64 * 30.0, (i % 4) as f64 * 20.0);
            let (gx, gy, gz) = (279530.4 + x, 201373.4 + y, 22511.28 + z);
            text += &format!("n{i},{x},{y},{z},{gx},{gy},{gz},{},{},{}\n", gx + 216.0, gy + 102.8, gz + 75.15);
        }
        let mut s = session_from(&text);
        set_up(&mut s, COLS);
        assert_eq!(s.box_info.n_boxes, 100);
        assert_eq!(s.box_info.n_far, 100, "none of the boxes is near the nodes");
        let (total, readable) = on_screen(&s);
        assert_eq!(total, 100);
        assert!(readable >= 90, "the boxes are framed even though they sit elsewhere: {readable} readable");
    }

    #[test]
    fn a_box_around_the_camera_is_clipped_not_dropped() {
        let mut s = session();
        on(&mut s);
        s.boxes = settings(&s);
        s.recompute_boxes();
        // put the camera inside the first box, looking along it
        let b = s.box_items[0];
        s.cam3.target = [(b.lo[0] + b.hi[0]) * 0.5, (b.lo[1] + b.hi[1]) * 0.5, (b.lo[2] + b.hi[2]) * 0.5];
        s.cam3.dist = 0.2;
        s.cam3.yaw = 0.7;
        let dl = shown(&s);
        let inside = dl.boxes.iter().find(|d| d.lines().count() > 0 && d.depth < 1.0).expect("the box around the camera is still drawn");
        for (p, q, _) in inside.lines() {
            assert!(p.iter().chain(q.iter()).all(|v| v.is_finite()));
        }
        assert!(inside.polygons().all(|poly| poly.iter().all(|p| p[0].is_finite() && p[1].is_finite())));
        // nothing is drawn for a box entirely behind the camera
        s.cam3.target = [1.0e5, 1.0e5, 1.0e5];
        s.cam3.dist = 5.0;
        assert!(shown(&s).boxes.is_empty());
    }
}
