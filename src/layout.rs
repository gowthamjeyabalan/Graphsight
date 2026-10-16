//! Graph layouts: a live force-directed simulation (Barnes–Hut, rayon) and several static layouts.

use crate::data::{Column, ColData, NULL_CODE};
use crate::graph::{Graph, components};
use egui::Vec2 as V2;
use rayon::prelude::*;
use std::collections::HashMap;

/// Typical spacing between neighbouring nodes in the static layouts (world units).
const UNIT: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutKind {
    Force,
    Circular,
    Grid,
    Concentric,
    Hierarchical,
    GroupBy,
    Scatter,
    Random,
}

impl LayoutKind {
    pub const ALL: [LayoutKind; 8] = [
        LayoutKind::Force,
        LayoutKind::Circular,
        LayoutKind::Grid,
        LayoutKind::Concentric,
        LayoutKind::Hierarchical,
        LayoutKind::GroupBy,
        LayoutKind::Scatter,
        LayoutKind::Random,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            LayoutKind::Force => "Force-directed (live)",
            LayoutKind::Circular => "Circular",
            LayoutKind::Grid => "Grid",
            LayoutKind::Concentric => "Concentric rings by attribute",
            LayoutKind::Hierarchical => "Hierarchical / tree",
            LayoutKind::GroupBy => "Group by attribute",
            LayoutKind::Scatter => "Scatter (X / Y by attribute)",
            LayoutKind::Random => "Random",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    TopDown,
    LeftRight,
    Radial,
}

#[derive(Clone, Debug)]
pub struct ForceParams {
    pub charge: f32,
    pub link_dist: f32,
    pub link_strength: f32,
    pub gravity: f32,
    pub friction: f32,
    pub theta: f32,
    pub group_pull: f32,
    pub use_weights: bool,
}

impl Default for ForceParams {
    fn default() -> Self {
        ForceParams {
            charge: 30.0,
            link_dist: 30.0,
            link_strength: 1.0,
            gravity: 0.04,
            friction: 0.4,
            theta: 0.9,
            group_pull: 0.0,
            use_weights: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayoutSettings {
    pub kind: LayoutKind,
    /// None = file order.
    pub order_by: Option<String>,
    pub order_desc: bool,
    pub ring_by: Option<String>,
    pub ring_bins: usize,
    pub center_high: bool,
    pub orientation: Orientation,
    pub group_by: Option<String>,
    pub x_by: Option<String>,
    pub y_by: Option<String>,
    pub x_log: bool,
    pub y_log: bool,
    /// Only lay out nodes that pass the current filters.
    pub visible_only: bool,
    pub force: ForceParams,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        LayoutSettings {
            kind: LayoutKind::Force,
            order_by: Some("_community".into()),
            order_desc: false,
            ring_by: Some("_degree".into()),
            ring_bins: 6,
            center_high: true,
            orientation: Orientation::TopDown,
            group_by: Some("_community".into()),
            x_by: None,
            y_by: None,
            x_log: false,
            y_log: false,
            visible_only: false,
            force: ForceParams::default(),
        }
    }
}

// ───────────────────────────── helpers ─────────────────────────────

fn sunflower(j: usize, c: f32) -> V2 {
    let r = c * (j as f32 + 0.5).sqrt();
    let a = j as f32 * 2.399_963_2;
    V2::new(r * a.cos(), r * a.sin())
}

pub fn initial_spiral(n: usize) -> Vec<V2> {
    (0..n).map(|i| sunflower(i, 10.0)).collect()
}

fn center_on_origin(pos: &mut [V2], active: &[u32]) {
    if active.is_empty() {
        return;
    }
    let (mut lo, mut hi) = (V2::splat(f32::INFINITY), V2::splat(f32::NEG_INFINITY));
    for &i in active {
        let p = pos[i as usize];
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let c = (lo + hi) * 0.5;
    for &i in active {
        pos[i as usize] -= c;
    }
}

/// Active nodes sorted by a column (stable on degree-descending), missing values last.
fn ordered(g: &Graph, active: &[u32], col: Option<&str>, desc: bool) -> Vec<u32> {
    let mut v: Vec<u32> = active.to_vec();
    v.sort_by(|&a, &b| g.degree(b as usize).cmp(&g.degree(a as usize)));
    if let Some(c) = col.and_then(|n| g.node_col(n)) {
        v.sort_by(|&a, &b| {
            let (ia, ib) = (a as usize, b as usize);
            let miss_a = c.num(ia).is_none() && c.code(ia).is_none();
            let miss_b = c.num(ib).is_none() && c.code(ib).is_none();
            match (miss_a, miss_b) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => {
                    let o = c.cmp_rows(ia, ib);
                    if desc { o.reverse() } else { o }
                }
            }
        });
    }
    v
}

fn group_codes(col: &Column) -> (Vec<u32>, usize) {
    let (codes, labels) = col.as_categories();
    (codes, labels.len())
}

// ───────────────────────────── static layouts ─────────────────────────────

pub fn random_layout(active: &[u32], pos: &mut [V2]) {
    let r = (active.len() as f32).sqrt() * UNIT * 1.2;
    let mut s = 0x2545F4914F6CDD1Du64;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f32 / (1u64 << 53) as f32
    };
    for &i in active {
        let a = rnd() * std::f32::consts::TAU;
        let d = rnd().sqrt() * r;
        pos[i as usize] = V2::new(a.cos() * d, a.sin() * d);
    }
}

pub fn circular(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    let order = ordered(g, active, s.order_by.as_deref(), s.order_desc);
    let n = order.len().max(1);
    let r = (n as f32 * UNIT / std::f32::consts::TAU).max(UNIT * 2.0);
    for (k, &i) in order.iter().enumerate() {
        let a = std::f32::consts::TAU * k as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
        pos[i as usize] = V2::new(a.cos() * r, a.sin() * r);
    }
}

pub fn grid(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    let order = ordered(g, active, s.order_by.as_deref(), s.order_desc);
    let n = order.len().max(1);
    let cols = (n as f32).sqrt().ceil() as usize;
    for (k, &i) in order.iter().enumerate() {
        pos[i as usize] = V2::new((k % cols) as f32, (k / cols) as f32) * UNIT * 1.2;
    }
    center_on_origin(pos, active);
}

pub fn concentric(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    if active.is_empty() {
        return;
    }
    let col = s.ring_by.as_deref().and_then(|n| g.node_col(n));
    // ring index per active node
    let mut ring_of: HashMap<u32, usize> = HashMap::new();
    let mut n_rings = 1usize;
    match col {
        None => {
            for &i in active {
                ring_of.insert(i, 0);
            }
        }
        Some(c) if c.is_numeric() && !c.hint_cat => {
            let bins = s.ring_bins.max(1);
            let mut vals: Vec<(f64, u32)> =
                active.iter().filter_map(|&i| c.num(i as usize).map(|v| (v, i))).collect();
            vals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let cnt = vals.len().max(1);
            let mut k = 0;
            while k < vals.len() {
                let mut e = k;
                while e < vals.len() && vals[e].0 == vals[k].0 {
                    e += 1;
                }
                let mut b = (k * bins / cnt).min(bins - 1);
                if s.center_high {
                    b = bins - 1 - b;
                }
                for &(_, i) in &vals[k..e] {
                    ring_of.insert(i, b);
                }
                k = e;
            }
            n_rings = bins + 1;
            for &i in active {
                ring_of.entry(i).or_insert(bins); // missing → outermost
            }
        }
        Some(c) => {
            let (codes, nl) = group_codes(c);
            for &i in active {
                let code = codes[i as usize];
                ring_of.insert(i, if code == NULL_CODE { nl } else { code as usize });
            }
            n_rings = nl + 1;
        }
    }
    let mut rings: Vec<Vec<u32>> = vec![Vec::new(); n_rings];
    for &i in active {
        rings[ring_of[&i]].push(i);
    }
    rings.retain(|r| !r.is_empty());
    let mut radius = 0.0f32;
    for (k, ring) in rings.iter().enumerate() {
        let ordered_ring = ordered(g, ring, s.order_by.as_deref(), s.order_desc);
        let m = ordered_ring.len();
        let need = m as f32 * UNIT / std::f32::consts::TAU;
        radius = if k == 0 {
            if m == 1 { 0.0 } else { need.max(UNIT) }
        } else {
            (radius + UNIT * 2.5).max(need)
        };
        let off = k as f32 * 0.6;
        for (j, &i) in ordered_ring.iter().enumerate() {
            let a = std::f32::consts::TAU * j as f32 / m as f32 + off;
            pos[i as usize] = V2::new(a.cos() * radius, a.sin() * radius);
        }
    }
}

pub fn group_by(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    if active.is_empty() {
        return;
    }
    let (codes, n_groups) = match s.group_by.as_deref().and_then(|n| g.node_col(n)) {
        Some(c) => group_codes(c),
        None => (vec![0; g.n()], 1),
    };
    let mut groups: Vec<Vec<u32>> = vec![Vec::new(); n_groups + 1];
    for &i in active {
        let c = codes[i as usize];
        groups[if c == NULL_CODE { n_groups } else { c as usize }].push(i);
    }
    groups.retain(|v| !v.is_empty());
    groups.sort_by(|a, b| b.len().cmp(&a.len()));
    let c = UNIT * 0.5;
    let radii: Vec<f32> = groups.iter().map(|v| c * (v.len() as f32).sqrt() + c).collect();
    let centers = pack_circles(&radii, UNIT);
    for (gi, grp) in groups.iter().enumerate() {
        let order = ordered(g, grp, None, false); // hubs first → centre of the disc
        for (j, &i) in order.iter().enumerate() {
            pos[i as usize] = centers[gi] + sunflower(j, c);
        }
    }
    center_on_origin(pos, active);
}

/// Greedy circle packing along an outward spiral. Radii must be sorted largest first.
pub fn pack_circles(radii: &[f32], gap: f32) -> Vec<V2> {
    let mut centers: Vec<V2> = Vec::with_capacity(radii.len());
    if radii.len() > 400 {
        // too many groups for the spiral search: simple grid
        let cell = radii.iter().cloned().fold(0.0, f32::max) * 2.0 + gap;
        let cols = (radii.len() as f32).sqrt().ceil() as usize;
        return (0..radii.len()).map(|k| V2::new((k % cols) as f32, (k / cols) as f32) * cell).collect();
    }
    for (k, &r) in radii.iter().enumerate() {
        if k == 0 {
            centers.push(V2::ZERO);
            continue;
        }
        let mut theta = 0.0f32;
        let mut dist = radii[0] + r + gap;
        loop {
            let p = V2::new(theta.cos(), theta.sin()) * dist;
            let ok = centers.iter().zip(radii).all(|(c, &rr)| (*c - p).length() >= rr + r + gap);
            if ok {
                centers.push(p);
                break;
            }
            theta += 0.2;
            dist += 0.2 * (r + gap) / std::f32::consts::TAU * 2.0;
        }
    }
    centers
}

pub fn scatter(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    if active.is_empty() {
        return;
    }
    let extent = ((active.len() as f32).sqrt() * UNIT * 3.0).max(600.0);
    let axis = |name: Option<&str>, jitter_salt: u32, log: bool| -> Vec<f32> {
        let mut out = vec![0.0f32; g.n()];
        let Some(c) = name.and_then(|n| g.node_col(n)) else { return out };
        match &c.data {
            ColData::Num(_) if !c.hint_cat => {
                let (lo, hi) = active
                    .iter()
                    .filter_map(|&i| c.num(i as usize))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
                let span = if hi > lo { hi - lo } else { 1.0 };
                for &i in active {
                    out[i as usize] = match c.num(i as usize) {
                        Some(v) => {
                            let mut t = ((v - lo) / span) as f32;
                            if log {
                                t = crate::style::log_curve(t);
                            }
                            (t - 0.5) * extent
                        }
                        None => (0.5 + 0.08) * extent, // missing parked beyond the max
                    };
                }
            }
            _ => {
                let (codes, labels) = c.as_categories();
                let k = labels.len().max(1);
                let step = extent / k as f32;
                for &i in active {
                    let code = codes[i as usize];
                    let slot = if code == NULL_CODE { k } else { code as usize };
                    // deterministic jitter inside the category band
                    let h = (i.wrapping_mul(2654435761).wrapping_add(jitter_salt.wrapping_mul(97))) >> 8;
                    let j = (h % 1000) as f32 / 1000.0 - 0.5;
                    out[i as usize] = (slot as f32 + 0.5) * step - extent * 0.5 + j * step * 0.7;
                }
            }
        }
        out
    };
    let xs = if s.x_by.is_some() {
        axis(s.x_by.as_deref(), 1, s.x_log)
    } else {
        // no X attribute: file order along the axis
        let mut out = vec![0.0; g.n()];
        let n = active.len().max(1) as f32;
        for (k, &i) in active.iter().enumerate() {
            out[i as usize] = (k as f32 / n - 0.5) * extent;
        }
        out
    };
    let ys = axis(s.y_by.as_deref(), 2, s.y_log);
    for &i in active {
        // screen Y grows downward, so flip so that "bigger" is up
        pos[i as usize] = V2::new(xs[i as usize], -ys[i as usize]);
    }
}

/// Layered layout: BFS layers (from sources / hubs), barycenter ordering, optional radial form.
pub fn hierarchical(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    if active.is_empty() {
        return;
    }
    let n = g.n();
    let mut in_active = vec![false; n];
    for &i in active {
        in_active[i as usize] = true;
    }
    // directed adjacency for source detection
    let mut out_adj: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut indeg = vec![0u32; n];
    if g.directed {
        for &(a, b) in &g.edges {
            if a != b && in_active[a as usize] && in_active[b as usize] {
                out_adj[a as usize].push(b);
                indeg[b as usize] += 1;
            }
        }
    }
    let comp = components(g);
    let mut comps: HashMap<u32, Vec<u32>> = HashMap::new();
    for &i in active {
        comps.entry(comp[i as usize]).or_default().push(i);
    }
    let mut comp_list: Vec<Vec<u32>> = comps.into_values().collect();
    comp_list.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));

    let (dx, dy) = (UNIT * 1.3, UNIT * 3.0);
    let mut x_offset = 0.0f32;
    let gap = UNIT * 3.0;
    let mut placed: Vec<(Vec<u32>, f32)> = Vec::new(); // component nodes + width, for final recentre

    for nodes in &comp_list {
        let mut layer = vec![u32::MAX; n];
        let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
        // seeds
        let mut seeds: Vec<u32> = Vec::new();
        if g.directed {
            seeds = nodes.iter().copied().filter(|&i| indeg[i as usize] == 0 && !out_adj[i as usize].is_empty()).collect();
        }
        if seeds.is_empty() {
            let root = *nodes.iter().max_by_key(|&&i| (g.degree(i as usize), std::cmp::Reverse(i))).unwrap();
            seeds.push(root);
        }
        for &sd in &seeds {
            layer[sd as usize] = 0;
            queue.push_back(sd);
        }
        // phase 1: forward edges (directed) or any edges (undirected)
        let mut order_seen: Vec<u32> = Vec::new();
        while let Some(u) = queue.pop_front() {
            order_seen.push(u);
            let next: Vec<u32> = if g.directed {
                out_adj[u as usize].clone()
            } else {
                g.neighbors(u as usize).iter().copied().filter(|&v| in_active[v as usize]).collect()
            };
            for v in next {
                if layer[v as usize] == u32::MAX {
                    layer[v as usize] = layer[u as usize] + 1;
                    queue.push_back(v);
                }
            }
        }
        // phase 2: anything not reached follows undirected links from reached nodes
        if g.directed {
            for &u in &order_seen {
                queue.push_back(u);
            }
            while let Some(u) = queue.pop_front() {
                for &v in g.neighbors(u as usize) {
                    if in_active[v as usize] && layer[v as usize] == u32::MAX {
                        layer[v as usize] = layer[u as usize] + 1;
                        order_seen.push(v);
                        queue.push_back(v);
                    }
                }
            }
        }
        let max_layer = nodes.iter().map(|&i| layer[i as usize]).filter(|&l| l != u32::MAX).max().unwrap_or(0) as usize;
        let mut layers: Vec<Vec<u32>> = vec![Vec::new(); max_layer + 1];
        for &u in &order_seen {
            layers[layer[u as usize] as usize].push(u);
        }
        // barycentre sweeps
        let mut rank = vec![0f32; n];
        for l in &layers {
            for (k, &u) in l.iter().enumerate() {
                rank[u as usize] = k as f32;
            }
        }
        for _sweep in 0..4 {
            for dir in [1i32, -1] {
                let range: Vec<usize> = if dir == 1 { (1..layers.len()).collect() } else { (0..layers.len().saturating_sub(1)).rev().collect() };
                for l in range {
                    let want = (l as i32 - dir) as usize;
                    let mut keyed: Vec<(f32, u32)> = layers[l]
                        .iter()
                        .map(|&u| {
                            let (mut sum, mut cnt) = (0.0, 0.0);
                            for &v in g.neighbors(u as usize) {
                                if in_active[v as usize] && layer[v as usize] as usize == want {
                                    sum += rank[v as usize];
                                    cnt += 1.0;
                                }
                            }
                            (if cnt > 0.0 { sum / cnt } else { rank[u as usize] }, u)
                        })
                        .collect();
                    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
                    layers[l] = keyed.iter().map(|k| k.1).collect();
                    for (k, &u) in layers[l].iter().enumerate() {
                        rank[u as usize] = k as f32;
                    }
                }
            }
        }
        let widest = layers.iter().map(|l| l.len()).max().unwrap_or(1) as f32;
        let w = (widest * dx).max(dx);
        // very wide, shallow layer structures (hairballs) get more vertical room
        let dy = dy.max(w / (layers.len() as f32 * 2.5));
        match s.orientation {
            Orientation::TopDown | Orientation::LeftRight => {
                for (li, l) in layers.iter().enumerate() {
                    let m = l.len() as f32;
                    for (k, &u) in l.iter().enumerate() {
                        let x = x_offset + (k as f32 + 0.5) / m * w;
                        let y = li as f32 * dy;
                        pos[u as usize] = V2::new(x, y);
                    }
                }
                placed.push((nodes.clone(), w));
                x_offset += w + gap;
            }
            Orientation::Radial => {
                let mut radius = 0.0f32;
                for (li, l) in layers.iter().enumerate() {
                    let m = l.len() as f32;
                    let need = m * dx / std::f32::consts::TAU;
                    radius = if li == 0 { 0.0 } else { (radius + dy).max(need) };
                    for (k, &u) in l.iter().enumerate() {
                        let a = std::f32::consts::TAU * (k as f32 + 0.5) / m;
                        pos[u as usize] = V2::new(radius * a.cos(), radius * a.sin());
                    }
                }
                placed.push((nodes.clone(), radius * 2.0 + dy));
            }
        }
    }
    if s.orientation == Orientation::Radial {
        // components are laid out around the origin; slide each into its own slot on the x axis
        let mut cursor = 0.0f32;
        for (nodes, diameter) in &placed {
            for &i in nodes {
                pos[i as usize].x += cursor + diameter * 0.5;
            }
            cursor += diameter + gap;
        }
    }
    if s.orientation == Orientation::LeftRight {
        for &i in active {
            let p = pos[i as usize];
            pos[i as usize] = V2::new(p.y, p.x);
        }
    }
    center_on_origin(pos, active);
}

/// Dispatch for every non-force layout.
pub fn apply_static(g: &Graph, active: &[u32], s: &LayoutSettings, pos: &mut [V2]) {
    match s.kind {
        LayoutKind::Random => random_layout(active, pos),
        LayoutKind::Circular => circular(g, active, s, pos),
        LayoutKind::Grid => grid(g, active, s, pos),
        LayoutKind::Concentric => concentric(g, active, s, pos),
        LayoutKind::Hierarchical => hierarchical(g, active, s, pos),
        LayoutKind::GroupBy => group_by(g, active, s, pos),
        LayoutKind::Scatter => scatter(g, active, s, pos),
        LayoutKind::Force => {}
    }
}

/// Anchor positions (one per node) used to pull nodes of the same group together in the force layout.
pub fn group_anchors(g: &Graph, group_col: &str, active: &[u32]) -> Option<Vec<V2>> {
    let col = g.node_col(group_col)?;
    let mut tmp = vec![V2::ZERO; g.n()];
    let s = LayoutSettings { group_by: Some(group_col.to_string()), ..Default::default() };
    // reuse the group packing but spread groups wider so the force layout has room
    group_by(g, active, &s, &mut tmp);
    let (codes, labels) = col.as_categories();
    let nl = labels.len();
    let mut sum = vec![V2::ZERO; nl + 1];
    let mut cnt = vec![0f32; nl + 1];
    for &i in active {
        let c = codes[i as usize];
        let k = if c == NULL_CODE { nl } else { c as usize };
        sum[k] += tmp[i as usize];
        cnt[k] += 1.0;
    }
    let mut anchors = vec![V2::ZERO; g.n()];
    for &i in active {
        let c = codes[i as usize];
        let k = if c == NULL_CODE { nl } else { c as usize };
        anchors[i as usize] = sum[k] / cnt[k].max(1.0) * 2.5;
    }
    Some(anchors)
}

// ───────────────────────────── force simulation ─────────────────────────────

const NONE: u32 = u32::MAX;

#[derive(Clone)]
struct Cell {
    cx: f32,
    cy: f32,
    half: f32,
    mass: f32,
    mx: f32,
    my: f32,
    lx: f32,
    ly: f32,
    leaf: u32,
    child: [u32; 4],
}

struct QuadTree {
    cells: Vec<Cell>,
}

impl QuadTree {
    fn new_cell(cx: f32, cy: f32, half: f32) -> Cell {
        Cell { cx, cy, half, mass: 0.0, mx: 0.0, my: 0.0, lx: 0.0, ly: 0.0, leaf: NONE, child: [0; 4] }
    }

    fn build(pts: &[(u32, V2)]) -> QuadTree {
        let (mut lo, mut hi) = (V2::splat(f32::INFINITY), V2::splat(f32::NEG_INFINITY));
        for (_, p) in pts {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if pts.is_empty() {
            lo = V2::ZERO;
            hi = V2::ZERO;
        }
        let c = (lo + hi) * 0.5;
        let half = ((hi.x - lo.x).max(hi.y - lo.y) * 0.5).max(1.0) * 1.001 + 1.0;
        let mut t = QuadTree { cells: Vec::with_capacity(pts.len() * 2 + 8) };
        t.cells.push(Self::new_cell(c.x, c.y, half));
        for &(id, p) in pts {
            t.insert(id, p);
        }
        t
    }

    fn quadrant(c: &Cell, x: f32, y: f32) -> usize {
        (x >= c.cx) as usize | (((y >= c.cy) as usize) << 1)
    }

    fn make_child(&mut self, parent: usize, q: usize) -> usize {
        let (cx, cy, h) = {
            let p = &self.cells[parent];
            let h = p.half * 0.5;
            (p.cx + if q & 1 == 1 { h } else { -h }, p.cy + if q & 2 == 2 { h } else { -h }, h)
        };
        self.cells.push(Self::new_cell(cx, cy, h));
        let idx = self.cells.len() - 1;
        self.cells[parent].child[q] = idx as u32;
        idx
    }

    fn insert(&mut self, id: u32, p: V2) {
        let mut c = 0usize;
        let mut depth = 0;
        loop {
            let was_empty = self.cells[c].mass == 0.0;
            {
                let cell = &mut self.cells[c];
                cell.mass += 1.0;
                cell.mx += p.x;
                cell.my += p.y;
            }
            if was_empty {
                let cell = &mut self.cells[c];
                cell.leaf = id;
                cell.lx = p.x;
                cell.ly = p.y;
                return;
            }
            let is_leaf = self.cells[c].child == [0; 4];
            if is_leaf {
                if depth >= 32 {
                    return; // coincident points are merged into this leaf
                }
                let (lid, lx, ly) = (self.cells[c].leaf, self.cells[c].lx, self.cells[c].ly);
                self.cells[c].leaf = NONE;
                let q = Self::quadrant(&self.cells[c], lx, ly);
                let ch = self.make_child(c, q);
                let cell = &mut self.cells[ch];
                cell.mass = 1.0;
                cell.mx = lx;
                cell.my = ly;
                cell.lx = lx;
                cell.ly = ly;
                cell.leaf = lid;
            }
            let q = Self::quadrant(&self.cells[c], p.x, p.y);
            let next = self.cells[c].child[q] as usize;
            c = if next == 0 { self.make_child(c, q) } else { next };
            depth += 1;
        }
    }

    /// Repulsion on node `i` at `p`. Returns the velocity increment (already scaled by alpha).
    fn force(&self, i: u32, p: V2, charge: f32, alpha: f32, theta2: f32) -> V2 {
        let mut v = V2::ZERO;
        let mut stack: [u32; 128] = [0; 128];
        let mut sp = 1usize;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let cell = &self.cells[stack[sp] as usize];
            if cell.mass == 0.0 {
                continue;
            }
            let is_leaf = cell.child == [0; 4];
            let mut dx = cell.mx / cell.mass - p.x;
            let mut dy = cell.my / cell.mass - p.y;
            let mut l = dx * dx + dy * dy;
            let w = cell.half * 2.0;
            if is_leaf || w * w < theta2 * l {
                if is_leaf && cell.leaf == i {
                    continue;
                }
                if dx == 0.0 {
                    dx = jiggle(i, 1);
                    l += dx * dx;
                }
                if dy == 0.0 {
                    dy = jiggle(i, 2);
                    l += dy * dy;
                }
                if l < 1.0 {
                    l = (1.0 * l).sqrt();
                }
                let k = -charge * cell.mass * alpha / l;
                v.x += dx * k;
                v.y += dy * k;
            } else if sp + 4 <= stack.len() {
                for &ch in &cell.child {
                    if ch != 0 {
                        stack[sp] = ch;
                        sp += 1;
                    }
                }
            }
        }
        v
    }
}

fn jiggle(i: u32, salt: u32) -> f32 {
    let h = i.wrapping_mul(2246822519).wrapping_add(salt.wrapping_mul(3266489917));
    ((h >> 8) % 1000) as f32 / 1000.0 * 1e-3 - 5e-4
}

struct Link {
    s: u32,
    t: u32,
    strength: f32,
    bias: f32,
}

pub struct ForceSim {
    pub alpha: f32,
    pub running: bool,
    vel: Vec<V2>,
    pub pinned: Vec<bool>,
    active: Vec<u32>,
    links: Vec<Link>,
    anchors: Option<Vec<V2>>,
    isolated: Vec<bool>,
    pub ticks: usize,
}

pub const ALPHA_MIN: f32 = 0.001;

impl ForceSim {
    pub fn new() -> Self {
        ForceSim { alpha: 1.0, running: false, vel: vec![], pinned: vec![], active: vec![], links: vec![], anchors: None, isolated: vec![], ticks: 0 }
    }

    pub fn reheat(&mut self, alpha: f32) {
        self.alpha = self.alpha.max(alpha).min(1.0);
        self.running = !self.active.is_empty();
    }

    /// (Re)build the problem from the graph. Keeps pins if the node count is unchanged.
    pub fn setup(&mut self, g: &Graph, active: &[u32], p: &ForceParams, anchors: Option<Vec<V2>>) {
        let n = g.n();
        if self.pinned.len() != n {
            self.pinned = vec![false; n];
        }
        self.vel = vec![V2::ZERO; n];
        self.active = active.to_vec();
        self.anchors = anchors;
        let mut in_active = vec![false; n];
        for &i in active {
            in_active[i as usize] = true;
        }
        // unique undirected pairs, optionally weighted
        let weights = if p.use_weights { g.weights() } else { None };
        let mut pairs: HashMap<(u32, u32), f32> = HashMap::with_capacity(g.edges.len());
        for (e, &(s, t)) in g.edges.iter().enumerate() {
            if s == t || !in_active[s as usize] || !in_active[t as usize] {
                continue;
            }
            let key = (s.min(t), s.max(t));
            let w = weights.as_ref().map(|w| w[e].max(0.0)).unwrap_or(1.0);
            let slot = pairs.entry(key).or_insert(0.0);
            if weights.is_some() {
                *slot += w;
            } else {
                *slot = 1.0;
            }
        }
        let mean_w = if pairs.is_empty() { 1.0 } else { pairs.values().sum::<f32>() / pairs.len() as f32 };
        let mut count = vec![0u32; n];
        for &(s, t) in pairs.keys() {
            count[s as usize] += 1;
            count[t as usize] += 1;
        }
        let mut keys: Vec<(&(u32, u32), &f32)> = pairs.iter().collect();
        keys.sort_by_key(|(k, _)| **k); // deterministic
        self.links = keys
            .into_iter()
            .map(|(&(s, t), &w)| {
                let (cs, ct) = (count[s as usize] as f32, count[t as usize] as f32);
                let wf = if weights.is_some() && mean_w > 0.0 { (w / mean_w).clamp(0.25, 4.0) } else { 1.0 };
                Link { s, t, strength: p.link_strength * wf / cs.min(ct), bias: cs / (cs + ct) }
            })
            .collect();
        self.isolated = (0..n).map(|i| count[i] == 0).collect();
        self.ticks = 0;
    }

    pub fn pin(&mut self, i: usize, on: bool) {
        if i < self.pinned.len() {
            self.pinned[i] = on;
        }
    }

    pub fn unpin_all(&mut self) {
        self.pinned.iter_mut().for_each(|p| *p = false);
    }

    /// Advance the simulation by `ticks` iterations.
    pub fn step(&mut self, pos: &mut [V2], p: &ForceParams, ticks: usize) {
        if self.active.is_empty() {
            self.running = false;
            return;
        }
        let decay = 1.0 - ALPHA_MIN.powf(1.0 / 300.0);
        let theta2 = p.theta * p.theta;
        for _ in 0..ticks {
            self.alpha -= self.alpha * decay;
            if self.alpha < ALPHA_MIN {
                self.running = false;
                self.alpha = ALPHA_MIN;
                break;
            }
            let alpha = self.alpha;
            // many-body repulsion (Barnes–Hut)
            let pts: Vec<(u32, V2)> = self.active.iter().map(|&i| (i, pos[i as usize])).collect();
            let tree = QuadTree::build(&pts);
            let dv: Vec<V2> = if pts.len() > 1500 {
                pts.par_iter().map(|&(i, q)| tree.force(i, q, p.charge, alpha, theta2)).collect()
            } else {
                pts.iter().map(|&(i, q)| tree.force(i, q, p.charge, alpha, theta2)).collect()
            };
            for (k, &(i, _)) in pts.iter().enumerate() {
                self.vel[i as usize] += dv[k];
            }
            // springs
            for l in &self.links {
                let (s, t) = (l.s as usize, l.t as usize);
                let mut d = pos[t] + self.vel[t] - pos[s] - self.vel[s];
                if d.x == 0.0 {
                    d.x = jiggle(l.s, 3);
                }
                if d.y == 0.0 {
                    d.y = jiggle(l.t, 4);
                }
                let len = d.length();
                let f = (len - p.link_dist) / len * alpha * l.strength;
                d *= f;
                self.vel[t] -= d * l.bias;
                self.vel[s] += d * (1.0 - l.bias);
            }
            // gravity and group anchors
            for &i in &self.active {
                let i = i as usize;
                let gk = if self.isolated[i] { 4.0 } else { 1.0 };
                self.vel[i] -= pos[i] * (p.gravity * gk * alpha);
                if let Some(a) = &self.anchors {
                    if p.group_pull > 0.0 {
                        self.vel[i] += (a[i] - pos[i]) * (p.group_pull * alpha);
                    }
                }
            }
            // integrate
            let keep = 1.0 - p.friction;
            let mut mean = V2::ZERO;
            let mut any_pinned = false;
            for &i in &self.active {
                let i = i as usize;
                if self.pinned[i] {
                    any_pinned = true;
                    self.vel[i] = V2::ZERO;
                } else {
                    self.vel[i] *= keep;
                    pos[i] += self.vel[i];
                }
                if !pos[i].x.is_finite() || !pos[i].y.is_finite() {
                    pos[i] = sunflower(i, 10.0);
                    self.vel[i] = V2::ZERO;
                }
                mean += pos[i];
            }
            if !any_pinned {
                mean /= self.active.len() as f32;
                for &i in &self.active {
                    pos[i as usize] -= mean;
                }
            }
            self.ticks += 1;
        }
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::graph::build_graph;

    fn path_graph(n: usize, directed: bool) -> Graph {
        let mut s = String::from("source,target\n");
        for i in 0..n - 1 {
            s += &format!("n{},n{}\n", i, i + 1);
        }
        let t = parse_csv_text(&s).unwrap();
        let mut m = auto_edge_mapping(&t);
        m.directed = directed;
        build_graph(Some((&t, &m)), None)
    }

    fn all(g: &Graph) -> Vec<u32> {
        (0..g.n() as u32).collect()
    }

    fn finite_and_distinct(pos: &[V2]) {
        assert!(pos.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        let mut seen = std::collections::HashSet::new();
        for p in pos {
            seen.insert(((p.x * 10.0) as i64, (p.y * 10.0) as i64));
        }
        assert_eq!(seen.len(), pos.len(), "overlapping nodes");
    }

    #[test]
    fn every_static_layout_places_every_node() {
        let g = crate::graph::tests::tiny(false);
        let act = all(&g);
        for kind in LayoutKind::ALL {
            if kind == LayoutKind::Force {
                continue;
            }
            let s = LayoutSettings { kind, x_by: Some("score".into()), y_by: Some("group".into()), ..Default::default() };
            let mut pos = vec![V2::ZERO; g.n()];
            apply_static(&g, &act, &s, &mut pos);
            assert!(pos.iter().all(|p| p.x.is_finite() && p.y.is_finite()), "{kind:?}");
            if kind != LayoutKind::Scatter {
                finite_and_distinct(&pos);
            }
        }
    }

    #[test]
    fn layouts_leave_inactive_nodes_alone() {
        let g = crate::graph::tests::tiny(false);
        let act: Vec<u32> = vec![0, 1, 2];
        let mut pos = vec![V2::new(7.0, 7.0); g.n()];
        let s = LayoutSettings { kind: LayoutKind::Circular, ..Default::default() };
        apply_static(&g, &act, &s, &mut pos);
        for i in 3..g.n() {
            assert_eq!(pos[i], V2::new(7.0, 7.0));
        }
    }

    #[test]
    fn hierarchy_layers_increase_along_a_path() {
        let g = path_graph(6, true);
        let mut pos = vec![V2::ZERO; g.n()];
        let s = LayoutSettings { kind: LayoutKind::Hierarchical, orientation: Orientation::TopDown, ..Default::default() };
        apply_static(&g, &all(&g), &s, &mut pos);
        for i in 0..5 {
            assert!(pos[g.index[&format!("n{}", i + 1)] as usize].y > pos[g.index[&format!("n{i}")] as usize].y);
        }
    }

    #[test]
    fn concentric_puts_hubs_in_the_middle() {
        // star: hub + 8 leaves
        let mut s = String::from("source,target\n");
        for i in 0..8 {
            s += &format!("hub,l{i}\n");
        }
        let t = parse_csv_text(&s).unwrap();
        let g = build_graph(Some((&t, &auto_edge_mapping(&t))), None);
        let st = LayoutSettings { kind: LayoutKind::Concentric, ring_by: Some("_degree".into()), ring_bins: 2, ..Default::default() };
        let mut pos = vec![V2::ZERO; g.n()];
        apply_static(&g, &all(&g), &st, &mut pos);
        let hub = pos[g.index["hub"] as usize].length();
        let leaf = pos[g.index["l0"] as usize].length();
        assert!(hub < leaf, "hub {hub} leaf {leaf}");
    }

    #[test]
    fn group_layout_separates_groups() {
        let g = crate::graph::tests::tiny(false);
        let s = LayoutSettings { kind: LayoutKind::GroupBy, group_by: Some("group".into()), ..Default::default() };
        let mut pos = vec![V2::ZERO; g.n()];
        apply_static(&g, &all(&g), &s, &mut pos);
        let a = pos[g.index["a"] as usize];
        let b = pos[g.index["b"] as usize];
        let d = pos[g.index["d"] as usize];
        assert!((a - b).length() < (a - d).length());
    }

    #[test]
    fn barnes_hut_matches_brute_force_when_exact() {
        let mut pts = Vec::new();
        let mut s = 12345u64;
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 33) as f32 / (1u64 << 31) as f32) * 200.0 - 100.0
        };
        for i in 0..150u32 {
            pts.push((i, V2::new(rnd(), rnd())));
        }
        let tree = QuadTree::build(&pts);
        for &(i, p) in pts.iter().take(10) {
            let bh = tree.force(i, p, 30.0, 1.0, 0.0);
            let mut brute = V2::ZERO;
            for &(j, q) in &pts {
                if j == i {
                    continue;
                }
                let d = q - p;
                let l = d.length_sq();
                let l = if l < 1.0 { l.sqrt() } else { l };
                brute += d * (-30.0 / l);
            }
            assert!((bh - brute).length() < 1e-3 * (1.0 + brute.length()), "bh {bh:?} brute {brute:?}");
        }
        // and a realistic theta stays close
        let (i, p) = pts[0];
        let approx = tree.force(i, p, 30.0, 1.0, 0.81);
        let exact = tree.force(i, p, 30.0, 1.0, 0.0);
        assert!((approx - exact).length() < 0.25 * (1.0 + exact.length()));
    }

    #[test]
    fn force_layout_settles_without_nans() {
        let g = path_graph(30, false);
        let mut pos = initial_spiral(g.n());
        let p = ForceParams::default();
        let mut sim = ForceSim::new();
        sim.setup(&g, &all(&g), &p, None);
        sim.reheat(1.0);
        sim.step(&mut pos, &p, 400);
        assert!(!sim.running, "alpha should have cooled below the minimum");
        assert!(pos.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
        // path edges end up roughly at the rest length, neighbours are not on top of each other
        let mean: f32 = (0..29).map(|i| (pos[i + 1] - pos[i]).length()).sum::<f32>() / 29.0;
        assert!(mean > 10.0 && mean < 90.0, "mean edge length {mean}");
    }

    #[test]
    fn pinned_nodes_do_not_move() {
        let g = path_graph(10, false);
        let mut pos = initial_spiral(g.n());
        let p = ForceParams::default();
        let mut sim = ForceSim::new();
        sim.setup(&g, &all(&g), &p, None);
        sim.pin(3, true);
        let before = pos[3];
        sim.reheat(1.0);
        sim.step(&mut pos, &p, 60);
        assert_eq!(pos[3], before);
        assert!(pos.iter().all(|q| q.x.is_finite() && q.y.is_finite()));
    }
}
