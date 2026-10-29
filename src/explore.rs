//! Exploring the graph: how far nodes are from some starting nodes (focus mode and "what's downstream", played wave
//! by wave), bottlenecks (nodes and connections whose loss splits the network) and routes between two nodes.

use crate::data::Column;
use crate::graph::Graph;
use crate::walk::Follow;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Not reached.
pub const FAR: u32 = u32::MAX;

/// For every node, the edges that may be followed out of it: (neighbour, edge).
pub fn steps_out(g: &Graph, follow: Follow) -> Vec<Vec<(u32, u32)>> {
    let mut out = vec![Vec::new(); g.n()];
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s == t {
            continue;
        }
        let both = !g.edge_directed(e) || follow == Follow::Both;
        if both || follow == Follow::Out {
            out[s as usize].push((t, e as u32));
        }
        if both || follow == Follow::In {
            out[t as usize].push((s, e as u32));
        }
    }
    out
}

/// How many steps every node is from the nearest of `from` (FAR: not within `max_steps`).
pub fn reach(g: &Graph, from: &[u32], follow: Follow, max_steps: u32) -> Vec<u32> {
    let out = steps_out(g, follow);
    let mut level = vec![FAR; g.n()];
    let mut front: Vec<u32> = Vec::new();
    for &v in from {
        if let Some(l) = level.get_mut(v as usize) {
            *l = 0;
            front.push(v);
        }
    }
    let mut d = 0;
    while !front.is_empty() && d < max_steps {
        d += 1;
        let mut next = Vec::new();
        for v in front {
            for &(w, _) in &out[v as usize] {
                if level[w as usize] == FAR {
                    level[w as usize] = d;
                    next.push(w);
                }
            }
        }
        front = next;
    }
    level
}

/// Nodes and connections whose loss splits the network, each with how many nodes it would cut off from the rest of
/// its piece (largest first).
#[derive(Clone, Debug, Default)]
pub struct Cuts {
    pub nodes: Vec<(u32, usize)>,
    pub edges: Vec<(u32, usize)>,
}

/// Bottlenecks of the network, taking every connection as two-way (articulation points and bridges, Tarjan).
pub fn bottlenecks(g: &Graph) -> Cuts {
    let n = g.n();
    // how many edges join each pair (two parallel edges between the same nodes are not a bridge) and one of them
    let mut pairs: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s != t {
            let k = (s.min(t), s.max(t));
            let entry = pairs.entry(k).or_insert((0, e as u32));
            entry.0 += 1;
        }
    }
    let mut disc = vec![FAR; n];
    let mut low = vec![0u32; n];
    let mut parent = vec![FAR; n];
    let mut size = vec![1usize; n];
    let mut cut_off = vec![0usize; n]; // nodes a non-root articulation point cuts off
    let mut edges = Vec::new();
    let mut nodes = Vec::new();
    let mut time = 0u32;
    for root in 0..n as u32 {
        if disc[root as usize] != FAR {
            continue;
        }
        disc[root as usize] = time;
        low[root as usize] = time;
        time += 1;
        let mut child_sizes: Vec<usize> = Vec::new();
        let mut stack: Vec<(u32, usize)> = vec![(root, 0)];
        while let Some(top) = stack.last_mut() {
            let v = top.0;
            let nb = g.neighbors(v as usize);
            if top.1 < nb.len() {
                let w = nb[top.1];
                top.1 += 1;
                if disc[w as usize] == FAR {
                    parent[w as usize] = v;
                    disc[w as usize] = time;
                    low[w as usize] = time;
                    time += 1;
                    stack.push((w, 0));
                } else if w != parent[v as usize] {
                    low[v as usize] = low[v as usize].min(disc[w as usize]);
                }
            } else {
                stack.pop();
                let p = parent[v as usize];
                if p == FAR {
                    continue;
                }
                let (pu, vu) = (p as usize, v as usize);
                low[pu] = low[pu].min(low[vu]);
                size[pu] += size[vu];
                if low[vu] > disc[pu] {
                    if let Some(&(count, e)) = pairs.get(&(p.min(v), p.max(v))) {
                        if count == 1 {
                            edges.push((e, size[vu]));
                        }
                    }
                }
                if p == root {
                    child_sizes.push(size[vu]);
                } else if low[vu] >= disc[pu] {
                    cut_off[pu] += size[vu];
                }
            }
        }
        // the start of a piece splits it when it has two or more separate branches: all but the largest are cut off
        if child_sizes.len() >= 2 {
            let total: usize = child_sizes.iter().sum();
            let largest = child_sizes.iter().copied().max().unwrap_or(0);
            nodes.push((root, total - largest));
        }
    }
    nodes.extend((0..n).filter(|&v| cut_off[v] > 0).map(|v| (v as u32, cut_off[v])));
    nodes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    edges.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Cuts { nodes, edges }
}

/// A route: its nodes, the edges between them, and what it costs.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub nodes: Vec<u32>,
    pub edges: Vec<u32>,
    pub cost: f64,
}

#[derive(PartialEq)]
struct Item(f64, u32);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Cheapest route from `a` to `b` (Dijkstra), not using banned nodes or edges.
fn cheapest(out: &[Vec<(u32, u32)>], cost: &[f64], a: u32, b: u32, banned_nodes: &HashSet<u32>, banned_edges: &HashSet<u32>) -> Option<Route> {
    let n = out.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut prev: Vec<Option<(u32, u32)>> = vec![None; n];
    let mut heap = BinaryHeap::new();
    dist[a as usize] = 0.0;
    heap.push(Item(0.0, a));
    while let Some(Item(d, v)) = heap.pop() {
        if v == b {
            break;
        }
        if d > dist[v as usize] {
            continue;
        }
        for &(w, e) in &out[v as usize] {
            if banned_nodes.contains(&w) || banned_edges.contains(&e) {
                continue;
            }
            let nd = d + cost[e as usize];
            if nd < dist[w as usize] {
                dist[w as usize] = nd;
                prev[w as usize] = Some((v, e));
                heap.push(Item(nd, w));
            }
        }
    }
    if !dist[b as usize].is_finite() {
        return None;
    }
    let (mut nodes, mut edges) = (vec![b], vec![]);
    let mut at = b;
    while let Some((p, e)) = prev[at as usize] {
        nodes.push(p);
        edges.push(e);
        at = p;
        if at == a {
            break;
        }
    }
    nodes.reverse();
    edges.reverse();
    Some(Route { nodes, edges, cost: dist[b as usize] })
}

/// Up to `k` routes from `a` to `b`, cheapest first (Yen's method): the best one and genuine alternatives. Each
/// step costs 1, or the edge's value in `cost_col` (missing and negative values count as 0).
pub fn routes(g: &Graph, a: u32, b: u32, follow: Follow, cost_col: Option<&Column>, k: usize) -> Vec<Route> {
    let out = steps_out(g, follow);
    let cost: Vec<f64> = (0..g.edges.len()).map(|e| cost_col.map_or(1.0, |c| c.num(e).unwrap_or(0.0).max(0.0))).collect();
    let none = HashSet::new();
    let Some(first) = cheapest(&out, &cost, a, b, &none, &none) else { return vec![] };
    let mut found = vec![first];
    let mut candidates: Vec<Route> = Vec::new();
    while found.len() < k {
        let last = found.last().unwrap().clone();
        for i in 0..last.nodes.len().saturating_sub(1) {
            let spur = last.nodes[i];
            let root_nodes = &last.nodes[..=i];
            let root_edges = &last.edges[..i];
            let banned_edges: HashSet<u32> = found.iter().filter(|r| r.nodes.len() > i + 1 && r.nodes[..=i] == *root_nodes).map(|r| r.edges[i]).collect();
            let banned_nodes: HashSet<u32> = root_nodes[..i].iter().copied().collect();
            if let Some(tail) = cheapest(&out, &cost, spur, b, &banned_nodes, &banned_edges) {
                let mut nodes = root_nodes.to_vec();
                nodes.extend_from_slice(&tail.nodes[1..]);
                let mut edges = root_edges.to_vec();
                edges.extend_from_slice(&tail.edges);
                let c = edges.iter().map(|&e| cost[e as usize]).sum();
                let r = Route { nodes, edges, cost: c };
                if !found.contains(&r) && !candidates.contains(&r) {
                    candidates.push(r);
                }
            }
        }
        if candidates.is_empty() {
            break;
        }
        let best = (0..candidates.len()).min_by(|&x, &y| candidates[x].cost.total_cmp(&candidates[y].cost).then(candidates[x].nodes.len().cmp(&candidates[y].nodes.len()))).unwrap();
        found.push(candidates.swap_remove(best));
    }
    found
}

/// The edge joining `a` and `b` (either way), if there is one.
pub fn edge_between(g: &Graph, a: u32, b: u32) -> Option<u32> {
    g.edges.iter().position(|&(s, t)| (s == a && t == b) || (s == b && t == a)).map(|e| e as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{auto_edge_mapping, auto_node_mapping, parse_csv_text};

    fn graph(edges: &str) -> Graph {
        let e = parse_csv_text(edges).unwrap();
        let n = parse_csv_text("id\na\nb\nc\nd\ne\nf\ng\n").unwrap();
        crate::graph::build_graph(Some((&e, &auto_edge_mapping(&e))), Some((&n, &auto_node_mapping(&n))))
    }

    #[test]
    fn reach_follows_directions_and_stops() {
        let g = graph("source,target,directed\na,b,yes\nb,c,yes\nc,d,yes\ne,b,yes\n");
        let id = |s: &str| g.index[s];
        let down = reach(&g, &[id("b")], Follow::Out, 10);
        assert_eq!((down[id("c") as usize], down[id("d") as usize], down[id("a") as usize]), (1, 2, FAR));
        let up = reach(&g, &[id("b")], Follow::In, 10);
        assert_eq!((up[id("a") as usize], up[id("e") as usize], up[id("c") as usize]), (1, 1, FAR));
        let near = reach(&g, &[id("b")], Follow::Both, 1);
        assert_eq!(near[id("d") as usize], FAR, "two steps away, past the limit");
    }

    #[test]
    fn bottlenecks_find_cut_nodes_and_bridges() {
        // a triangle a-b-c, a bridge c-d, then d-e-f in a line; g alone
        let g = graph("source,target\na,b\nb,c\nc,a\nc,d\nd,e\ne,f\n");
        let id = |s: &str| g.index[s];
        let cuts = bottlenecks(&g);
        let nodes: HashMap<u32, usize> = cuts.nodes.iter().copied().collect();
        assert_eq!(nodes.get(&id("c")), Some(&3), "c cuts off d, e, f");
        assert_eq!(nodes.get(&id("e")), Some(&1));
        assert!(!nodes.contains_key(&id("a")) && !nodes.contains_key(&id("f")));
        assert_eq!(cuts.edges.len(), 3, "c-d, d-e, e-f; the triangle has none");
        assert_eq!(cuts.edges[0].1, 3, "c-d cuts off the most");
        // two parallel edges are not a bridge
        let g2 = graph("source,target\na,b\na,b\nb,c\n");
        assert_eq!(bottlenecks(&g2).edges.len(), 1);
    }

    #[test]
    fn routes_cheapest_first_with_alternatives() {
        // a-b-d (2 steps), a-c-d (2 steps, but heavy), a-e-f-d (3 steps)
        let g = graph("source,target,w\na,b,5\nb,d,5\na,c,1\nc,d,1\na,e,1\ne,f,1\nf,d,1\n");
        let id = |s: &str| g.index[s];
        let name = |r: &Route| r.nodes.iter().map(|&v| g.ids[v as usize].clone()).collect::<Vec<_>>().join("");
        let steps = routes(&g, id("a"), id("d"), Follow::Both, None, 5);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].nodes.len(), 3);
        assert_eq!(name(&steps[2]), "aefd");
        let w = g.edge_col("w");
        let cheap = routes(&g, id("a"), id("d"), Follow::Both, w, 2);
        assert_eq!((name(&cheap[0]), cheap[0].cost), ("acd".to_string(), 2.0));
        assert_eq!(name(&cheap[1]), "aefd");
        assert_eq!(cheap[0].edges.len() + 1, cheap[0].nodes.len());
        assert!(routes(&g, id("a"), id("g"), Follow::Both, None, 3).is_empty(), "g is not connected");
    }
}

// ───────────────────────── state and panels ─────────────────────────

use crate::app::App;
use crate::theme::UiExt;
use egui::Ui;

/// Focus mode: a node (or a few) and everything within `steps` of it stand out.
pub struct FocusMode {
    pub center: Vec<u32>,
    pub steps: u32,
}

/// "What's downstream": how far every node is from the start, and how far the wave has got.
pub struct Down {
    pub sources: Vec<u32>,
    pub follow: Follow,
    pub levels: Vec<u32>,
    pub max: u32,
    pub upto: u32,
    pub counts: Vec<usize>,
    pub playing: bool,
    pub next_at: f64,
}

#[derive(Default)]
pub struct ExploreState {
    pub down_follow: Follow,
    pub down: Option<Down>,
    pub path_from: String,
    pub path_to: String,
    pub path_follow: Follow,
    pub path_cost: Option<String>,
    pub routes: Vec<Route>,
    pub route_shown: Option<usize>,
    pub path_msg: Option<String>,
    pub cuts: Option<Cuts>,
}

impl App {
    /// A node by its id, or by its label (ignoring case).
    pub fn find_node(&self, text: &str) -> Option<u32> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        if let Some(&i) = self.s.g.index.get(t) {
            return Some(i);
        }
        let lower = t.to_lowercase();
        (0..self.s.g.n()).find(|&i| self.s.g.ids[i].to_lowercase() == lower || self.s.label_of(i).to_lowercase() == lower).map(|i| i as u32)
    }

    /// Start a downstream (or upstream) wave from these nodes.
    pub fn start_down(&mut self, sources: Vec<u32>, follow: Follow, now: f64) {
        let levels = reach(&self.s.g, &sources, follow, u32::MAX);
        let max = levels.iter().copied().filter(|&l| l != FAR).max().unwrap_or(0);
        let mut counts = vec![0usize; max as usize + 1];
        for &l in &levels {
            if l != FAR {
                counts[l as usize] += 1;
            }
        }
        self.walk = None;
        self.focus_mode = None;
        self.explore.down = Some(Down { sources, follow, levels, max, upto: 0, counts, playing: max > 0, next_at: now + 0.6 });
        self.spot_version += 1;
    }

    /// Show one of the routes found: as a walk along it, ending at its last node.
    pub fn show_route(&mut self, k: usize) {
        let Some(r) = self.explore.routes.get(k).cloned() else { return };
        let mut path: Vec<(u32, Option<u32>)> = vec![(r.nodes[0], None)];
        for (i, &v) in r.nodes.iter().enumerate().skip(1) {
            path.push((v, r.edges.get(i - 1).copied()));
        }
        let last = *r.nodes.last().unwrap();
        self.focus_mode = None;
        self.explore.down = None;
        self.s.select_only(last as usize);
        self.walk = Some(crate::walk::Walk { path, next: vec![], pick: 0, version: self.spot_version + 1 });
        self.spot_version += 1;
        self.explore.route_shown = Some(k);
    }

    /// The Query tab's exploring tools, under the connection query.
    pub fn explore_sections(&mut self, ui: &mut Ui) {
        if self.s.g.n() == 0 {
            return;
        }
        let now = ui.input(|i| i.time);
        let red = egui::Color32::from_rgb(235, 100, 90);

        // ── what's downstream ──
        ui.add_space(10.0);
        crate::theme::heading(ui, "What's downstream");
        ui.weak("From the selected nodes: everything they lead to, played step by step.");
        ui.segmented(&mut self.explore.down_follow, &[(Follow::Out, "Downstream"), (Follow::In, "Upstream"), (Follow::Both, "Both ways")]);
        let sel = self.s.selected_list();
        ui.horizontal(|ui| {
            if ui.add_enabled(!sel.is_empty(), egui::Button::new(format!("Start from the {} selected", sel.len()))).clicked() {
                let follow = self.explore.down_follow;
                self.start_down(sel.clone(), follow, now);
            }
            if self.explore.down.is_some() && ui.button("Stop").clicked() {
                self.explore.down = None;
                self.spot_version += 1;
            }
        });
        let mut select_reached = None;
        if let Some(d) = &mut self.explore.down {
            let reached: usize = d.counts.iter().skip(1).sum();
            ui.label(format!("{reached} nodes reached in {} steps", d.max));
            let per: Vec<String> = d.counts.iter().skip(1).take(14).map(|c| c.to_string()).collect();
            if !per.is_empty() {
                ui.weak(format!("Per step: {}{}", per.join(" · "), if d.counts.len() > 15 { " …" } else { "" }));
            }
            let mut upto = d.upto;
            ui.horizontal(|ui| {
                if ui.button(if d.playing { "❚❚" } else { "▶" }).on_hover_text("Play the wave step by step").clicked() {
                    d.playing = !d.playing;
                    if d.playing && d.upto >= d.max {
                        upto = 0;
                    }
                    d.next_at = now + 0.6;
                }
                ui.add(egui::Slider::new(&mut upto, 0..=d.max.max(1)).text("step"));
            });
            if upto != d.upto {
                d.upto = upto.min(d.max);
                self.spot_version += 1;
            }
            if ui.button("Select all reached").clicked() {
                select_reached = Some(d.levels.clone());
            }
        }
        if let Some(levels) = select_reached {
            self.walk = None;
            self.explore.down = None;
            self.spot_version += 1;
            self.s.clear_selection();
            for (i, &l) in levels.iter().enumerate() {
                if l != FAR {
                    self.s.set_selected(i, true);
                }
            }
        }

        // ── path between two nodes ──
        ui.add_space(10.0);
        crate::theme::heading(ui, "Path between two nodes");
        let sel = self.s.selected_list();
        let one = (sel.len() == 1).then(|| self.s.g.ids[sel[0] as usize].clone());
        for (label, which) in [("From", 0), ("To", 1)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let text = if which == 0 { &mut self.explore.path_from } else { &mut self.explore.path_to };
                ui.add(egui::TextEdit::singleline(text).id_salt(("path_end", which)).hint_text("node id or name").desired_width(150.0));
                if ui.add_enabled(one.is_some(), egui::Button::new("selected")).on_hover_text("Use the selected node").clicked() {
                    *text = one.clone().unwrap_or_default();
                }
            });
        }
        if ui.add_enabled(sel.len() == 2, egui::Button::new("Use the two selected nodes")).clicked() {
            self.explore.path_from = self.s.g.ids[sel[0] as usize].clone();
            self.explore.path_to = self.s.g.ids[sel[1] as usize].clone();
        }
        ui.segmented(&mut self.explore.path_follow, &[(Follow::Out, "Along arrows"), (Follow::Both, "Any way")]);
        let numeric: Vec<String> = self.s.g.edge_cols.iter().filter(|c| c.is_numeric()).map(|c| c.name.clone()).collect();
        ui.horizontal(|ui| {
            ui.label("Cost");
            egui::ComboBox::from_id_salt("path_cost").selected_text(self.explore.path_cost.clone().unwrap_or_else(|| "(fewest steps)".into())).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.explore.path_cost, None, "(fewest steps)");
                for c in &numeric {
                    ui.selectable_value(&mut self.explore.path_cost, Some(c.clone()), c);
                }
            });
        });
        if ui.button("Find routes").clicked() {
            let (a, b) = (self.find_node(&self.explore.path_from), self.find_node(&self.explore.path_to));
            self.explore.routes.clear();
            self.explore.route_shown = None;
            self.explore.path_msg = match (a, b) {
                (None, _) => Some("The start is not a node id or name in this graph.".into()),
                (_, None) => Some("The end is not a node id or name in this graph.".into()),
                (Some(a), Some(b)) if a == b => Some("Start and end are the same node.".into()),
                (Some(a), Some(b)) => {
                    let col = self.explore.path_cost.as_deref().and_then(|c| self.s.g.edge_col(c));
                    self.explore.routes = routes(&self.s.g, a, b, self.explore.path_follow, col, 5);
                    if self.explore.routes.is_empty() { Some("No route joins them (try Any way).".into()) } else { None }
                }
            };
            if !self.explore.routes.is_empty() {
                self.show_route(0);
            }
        }
        if let Some(m) = &self.explore.path_msg {
            ui.colored_label(red, m);
        }
        let mut show = None;
        let costed = self.explore.path_cost.is_some();
        for (k, r) in self.explore.routes.iter().enumerate() {
            let steps = r.nodes.len() - 1;
            let text = if costed { format!("Route {}: {steps} steps, cost {}", k + 1, crate::data::fmt_num(r.cost)) } else { format!("Route {}: {steps} steps", k + 1) };
            if ui.selectable_label(self.explore.route_shown == Some(k) && self.walk.is_some(), text).on_hover_text("Show it as a walk; the left arrow steps back along it").clicked() {
                show = Some(k);
            }
        }
        if let Some(k) = show {
            self.show_route(k);
        }

        // ── bottlenecks ──
        ui.add_space(10.0);
        crate::theme::heading(ui, "Bottlenecks");
        ui.weak("Nodes and connections whose loss splits the network into pieces (every connection taken as two-way).");
        if ui.button("Find bottlenecks").clicked() {
            self.explore.cuts = Some(bottlenecks(&self.s.g));
        }
        if let Some(c) = self.explore.cuts.clone() {
            ui.label(format!("{} nodes and {} connections", c.nodes.len(), c.edges.len()));
            ui.horizontal(|ui| {
                if ui.add_enabled(!c.nodes.is_empty(), egui::Button::new("Select the nodes")).clicked() {
                    self.walk = None;
                    self.s.clear_selection();
                    for &(v, _) in &c.nodes {
                        self.s.set_selected(v as usize, true);
                    }
                }
                if ui.add_enabled(!c.edges.is_empty(), egui::Button::new("Select the connections")).clicked() {
                    self.walk = None;
                    self.s.clear_selection();
                    for &(e, _) in &c.edges {
                        self.s.set_edge_selected(e as usize, true);
                    }
                }
            });
            let mut go_node = None;
            let mut go_edge = None;
            egui::CollapsingHeader::new(format!("Nodes ({})", c.nodes.len())).id_salt("cut_nodes").default_open(true).show(ui, |ui| {
                for &(v, k) in c.nodes.iter().take(40) {
                    ui.horizontal(|ui| {
                        if ui.link(self.s.label_of(v as usize)).clicked() {
                            go_node = Some(v);
                        }
                        ui.weak(format!("cuts off {k}"));
                    });
                }
                if c.nodes.len() > 40 {
                    ui.weak(format!("… and {} more", c.nodes.len() - 40));
                }
            });
            egui::CollapsingHeader::new(format!("Connections ({})", c.edges.len())).id_salt("cut_edges").show(ui, |ui| {
                for &(e, k) in c.edges.iter().take(30) {
                    let (a, b) = self.s.g.edges[e as usize];
                    ui.horizontal(|ui| {
                        if ui.link(format!("{} — {}", self.s.label_of(a as usize), self.s.label_of(b as usize))).clicked() {
                            go_edge = Some(e);
                        }
                        ui.weak(format!("cuts off {k}"));
                    });
                }
                if c.edges.len() > 30 {
                    ui.weak(format!("… and {} more", c.edges.len() - 30));
                }
            });
            if let Some(v) = go_node {
                self.select_and_focus(v as usize);
            }
            if let Some(e) = go_edge {
                self.s.select_only_edge(e as usize);
                self.run_tool(crate::view::Tool::Selection);
            }
        }
    }
}
