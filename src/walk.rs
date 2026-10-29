//! Walking the graph with the arrow keys: from the selected node, → steps along an edge (following edge directions:
//! out of the node, into it, or both), ↑ / ↓ choose which neighbour comes next, ← goes back the way you came. The path
//! is drawn over the graph; the rest can stay as it is, be dimmed or be hidden.

use crate::graph::Graph;
use crate::data::natural_cmp;

/// Which edges a step may follow from the current node.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Follow {
    /// Along edge directions: source → target (undirected edges both ways).
    #[default]
    Out,
    /// Against them: target → source.
    In,
    /// Any edge.
    Both,
}

/// What the rest of the graph looks like while walking.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rest {
    #[default]
    AsIs,
    Dim,
    Hide,
}

/// The walk so far: the nodes, each with the edge that led to it, and the possible next steps.
pub struct Walk {
    /// (node, edge it was reached by); the first has no edge.
    pub path: Vec<(u32, Option<u32>)>,
    /// The next steps from the current node: (edge, node), not-yet-visited nodes first.
    pub next: Vec<(u32, u32)>,
    /// Which of `next` → takes.
    pub pick: usize,
    /// Changes whenever the walk does (so the highlight is worked out again).
    pub version: u64,
}

impl Walk {
    pub fn start(g: &Graph, node: u32, follow: Follow, shown: &[bool]) -> Walk {
        let mut w = Walk { path: vec![(node, None)], next: vec![], pick: 0, version: 0 };
        w.refresh(g, follow, shown);
        w
    }

    pub fn current(&self) -> u32 {
        self.path.last().map(|p| p.0).unwrap_or(0)
    }

    /// The step → would take.
    pub fn picked(&self) -> Option<(u32, u32)> {
        self.next.get(self.pick).copied()
    }

    /// Work out the next steps from the current node again (after a step, or when "follow" changes).
    pub fn refresh(&mut self, g: &Graph, follow: Follow, shown: &[bool]) {
        let at = self.current();
        let visited: std::collections::HashSet<u32> = self.path.iter().map(|p| p.0).collect();
        let mut seen = std::collections::HashSet::new();
        let mut next = Vec::new();
        for (e, &(s, t)) in g.edges.iter().enumerate() {
            if s == t || (s != at && t != at) {
                continue;
            }
            let other = if s == at { t } else { s };
            let directed = g.edge_directed(e);
            let ok = match follow {
                Follow::Both => true,
                Follow::Out => !directed || s == at,
                Follow::In => !directed || t == at,
            };
            if ok && shown.get(other as usize).copied().unwrap_or(true) && seen.insert(other) {
                next.push((e as u32, other));
            }
        }
        // new places first, then back where you have been; by name within each
        next.sort_by(|a, b| visited.contains(&a.1).cmp(&visited.contains(&b.1)).then_with(|| natural_cmp(&g.ids[a.1 as usize], &g.ids[b.1 as usize])));
        self.next = next;
        self.pick = 0;
        self.version += 1;
    }

    /// ↑ / ↓: choose another next step.
    pub fn choose(&mut self, delta: i32) {
        if !self.next.is_empty() {
            let k = self.next.len() as i32;
            self.pick = (self.pick as i32 + delta).rem_euclid(k) as usize;
            self.version += 1;
        }
    }

    /// →: take the chosen step. False when there is nowhere to go.
    pub fn forward(&mut self, g: &Graph, follow: Follow, shown: &[bool]) -> bool {
        let Some((e, v)) = self.picked() else { return false };
        self.path.push((v, Some(e)));
        self.refresh(g, follow, shown);
        true
    }

    /// ←: one step back. The step just undone is chosen again, so → returns to it.
    pub fn back(&mut self, g: &Graph, follow: Follow, shown: &[bool]) -> bool {
        if self.path.len() < 2 {
            return false;
        }
        let left = self.path.pop().unwrap().0;
        self.refresh(g, follow, shown);
        if let Some(k) = self.next.iter().position(|x| x.1 == left) {
            self.pick = k;
        }
        true
    }

    /// The nodes and edges to stand out: the path and the next steps.
    pub fn marks(&self, n: usize, n_edges: usize) -> (Vec<bool>, Vec<bool>) {
        let mut nodes = vec![false; n];
        let mut edges = vec![false; n_edges];
        for &(v, e) in &self.path {
            if let Some(x) = nodes.get_mut(v as usize) {
                *x = true;
            }
            if let Some(e) = e.and_then(|e| edges.get_mut(e as usize)) {
                *e = true;
            }
        }
        for &(e, v) in &self.next {
            if let Some(x) = nodes.get_mut(v as usize) {
                *x = true;
            }
            if let Some(x) = edges.get_mut(e as usize) {
                *x = true;
            }
        }
        (nodes, edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{auto_edge_mapping, auto_node_mapping, parse_csv_text};

    fn graph() -> Graph {
        // a → b → c, a → d, and an undirected b – e
        let e = parse_csv_text("source,target,directed\na,b,yes\nb,c,yes\na,d,yes\nb,e,no\n").unwrap();
        let n = parse_csv_text("id\na\nb\nc\nd\ne\n").unwrap();
        crate::graph::build_graph(Some((&e, &auto_edge_mapping(&e))), Some((&n, &auto_node_mapping(&n))))
    }

    #[test]
    fn steps_follow_directions_choose_and_go_back() {
        let g = graph();
        let id = |s: &str| g.index[s];
        let name = |v: u32| g.ids[v as usize].clone();
        let shown = vec![true; g.n()];
        let mut w = Walk::start(&g, id("a"), Follow::Out, &shown);
        assert_eq!(w.next.iter().map(|x| name(x.1)).collect::<Vec<_>>(), ["b", "d"]);
        w.choose(1);
        assert_eq!(name(w.picked().unwrap().1), "d");
        w.choose(1);
        assert!(w.forward(&g, Follow::Out, &shown));
        assert_eq!(name(w.current()), "b");
        // out of b: c (directed) and e (undirected); a is upstream, so not offered
        assert_eq!(w.next.iter().map(|x| name(x.1)).collect::<Vec<_>>(), ["c", "e"]);
        w.choose(1);
        w.forward(&g, Follow::Out, &shown);
        assert_eq!(name(w.current()), "e");
        // back to b, with e chosen again
        assert!(w.back(&g, Follow::Out, &shown));
        assert_eq!(name(w.current()), "b");
        assert_eq!(name(w.picked().unwrap().1), "e");
        // against the arrows from b: only a (and the undirected e)
        w.refresh(&g, Follow::In, &shown);
        assert_eq!(w.next.iter().map(|x| name(x.1)).collect::<Vec<_>>(), ["e", "a"], "a was visited, so it comes last");
        // the path and the next steps are marked
        let (nodes, edges) = w.marks(g.n(), g.edges.len());
        assert!(nodes[id("a") as usize] && nodes[id("b") as usize] && !nodes[id("c") as usize]);
        assert_eq!(edges.iter().filter(|x| **x).count(), 2, "a → b on the path, b – e next (a → b counted once)");
        // the start has no way back
        let mut w2 = Walk::start(&g, id("c"), Follow::Out, &shown);
        assert!(w2.next.is_empty() && !w2.forward(&g, Follow::Out, &shown) && !w2.back(&g, Follow::Out, &shown));
    }
}
