//! Sub-graphs: keep only the nodes that match an attribute rule (plus optional neighbours), and join the
//! separate pieces back into one graph by walking the original graph along shortest paths.

use crate::graph::Graph;
use crate::model::{Rule, RuleKind};
use crate::data::NULL_CODE;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SubSettings {
    pub enabled: bool,
    pub rule: Option<Rule>,
    /// Also keep nodes up to this many steps away from a matching node.
    pub hops: u8,
    /// Add the nodes needed to join separate pieces into one connected graph.
    pub connect: bool,
}

impl Default for SubSettings {
    fn default() -> Self {
        SubSettings { enabled: false, rule: None, hops: 0, connect: true }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SubInfo {
    /// Nodes that stay: matches, their neighbours and the connecting nodes.
    pub member: Vec<bool>,
    /// Nodes that were added only to join pieces together.
    pub connector: Vec<bool>,
    pub n_match: usize,
    pub n_neighbours: usize,
    pub n_connectors: usize,
    pub pieces_before: usize,
    /// Read by the tests; the panel reports `unreachable` instead.
    #[allow(dead_code)]
    pub pieces_after: usize,
    /// Pieces that could not be joined because the original graph itself does not connect them.
    pub unreachable: usize,
}

/// Clear `vis[i]` for every node that fails the rule.
pub fn rule_pass(g: &Graph, r: &Rule, vis: &mut [bool]) {
    let n = g.n();
    let Some(col) = g.node_col(&r.col) else { return };
    match &r.kind {
        RuleKind::Range { lo, hi, keep_missing } => {
            if !col.is_numeric() {
                return;
            }
            for i in 0..n {
                let ok = match col.num(i) {
                    Some(v) => v >= *lo && v <= *hi,
                    None => *keep_missing,
                };
                vis[i] &= ok;
            }
        }
        RuleKind::Cats { allowed } => {
            let (codes, labels) = col.as_categories();
            if allowed.len() != labels.len() + 1 {
                return;
            }
            for i in 0..n {
                let k = if codes[i] == NULL_CODE { labels.len() } else { codes[i] as usize };
                vis[i] &= allowed[k];
            }
        }
    }
}

pub struct Joined {
    pub added_node: Vec<bool>,
    /// Per edge index: part of a joining path.
    pub added_edge: Vec<bool>,
    pub pieces_before: usize,
    pub pieces_after: usize,
    pub unreachable: usize,
}

struct Dsu(Vec<u32>);
impl Dsu {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[x as usize];
            self.0[x as usize] = self.0[p as usize];
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra as usize] = rb;
        }
    }
}

/// Join the separate pieces of the node set `core` into one connected graph. Starting from the largest piece,
/// the nearest other piece is found with a breadth-first walk through the original graph (either direction of
/// every edge, only through `allowed` nodes if given; `core_edges`, if given, are the only edges that count as
/// connecting nodes of `core`); the nodes along that shortest path are added, the pieces
/// merge, and the walk repeats until one piece is left or no piece can be reached.
pub fn join_pieces(g: &Graph, core: &[bool], allowed: Option<&[bool]>, core_edges: Option<&[bool]>) -> Joined {
    let n = g.n();
    let ok = |i: usize| allowed.is_none_or(|a| a[i]);
    let mut adj: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s != t {
            adj[s as usize].push((t, e as u32));
            adj[t as usize].push((s, e as u32));
        }
    }
    let mut dsu = Dsu((0..n as u32).collect());
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if core[s as usize] && core[t as usize] && core_edges.is_none_or(|c| c[e]) {
            dsu.union(s, t);
        }
    }
    let mut groups: HashMap<u32, Vec<u32>> = HashMap::new();
    for i in 0..n {
        if core[i] {
            groups.entry(dsu.find(i as u32)).or_default().push(i as u32);
        }
    }
    let mut comps: Vec<Vec<u32>> = groups.into_values().collect();
    comps.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    let mut out = Joined { added_node: vec![false; n], added_edge: vec![false; g.edges.len()], pieces_before: comps.len(), pieces_after: comps.len(), unreachable: 0 };
    if comps.len() <= 1 {
        return out;
    }
    let mut comp_of = vec![u32::MAX; n];
    for (ci, c) in comps.iter().enumerate() {
        for &v in c {
            comp_of[v as usize] = ci as u32;
        }
    }
    let mut in_tree = vec![false; n];
    let mut tree: Vec<u32> = comps[0].clone();
    for &v in &tree {
        in_tree[v as usize] = true;
    }
    let mut remaining = comps.len() - 1;
    let mut par: Vec<(u32, u32)> = vec![(u32::MAX, u32::MAX); n];
    while remaining > 0 {
        let mut seen = in_tree.clone();
        let mut q: VecDeque<u32> = tree.iter().copied().collect();
        let mut found: Option<u32> = None;
        'bfs: while let Some(u) = q.pop_front() {
            for &(v, e) in &adj[u as usize] {
                let vi = v as usize;
                if seen[vi] || !ok(vi) {
                    continue;
                }
                seen[vi] = true;
                par[vi] = (u, e);
                if core[vi] {
                    found = Some(v);
                    break 'bfs;
                }
                q.push_back(v);
            }
        }
        let Some(v) = found else { break };
        let mut x = v;
        while !in_tree[x as usize] {
            let (p, e) = par[x as usize];
            out.added_edge[e as usize] = true;
            if !core[x as usize] {
                out.added_node[x as usize] = true;
            }
            in_tree[x as usize] = true;
            tree.push(x);
            x = p;
        }
        for &m in &comps[comp_of[v as usize] as usize] {
            if !in_tree[m as usize] {
                in_tree[m as usize] = true;
                tree.push(m);
            }
        }
        remaining -= 1;
    }
    out.unreachable = remaining;
    out.pieces_after = 1 + remaining;
    out
}

pub fn compute(g: &Graph, rule: &Rule, hops: u8, connect: bool) -> SubInfo {
    let n = g.n();
    let mut core = vec![true; n];
    rule_pass(g, rule, &mut core);
    let n_match = core.iter().filter(|b| **b).count();
    // neighbours, `hops` steps out (either direction)
    let mut frontier: Vec<u32> = (0..n as u32).filter(|&i| core[i as usize]).collect();
    let mut n_neighbours = 0;
    for _ in 0..hops {
        let mut next = Vec::new();
        for &u in &frontier {
            for &v in g.neighbors(u as usize) {
                if !core[v as usize] {
                    core[v as usize] = true;
                    n_neighbours += 1;
                    next.push(v);
                }
            }
        }
        frontier = next;
    }
    let j = join_pieces(g, &core, None, None);
    let mut member = core;
    let mut connector = vec![false; n];
    let mut n_connectors = 0;
    let (mut before, mut after, mut unreachable) = (j.pieces_before, j.pieces_before, 0);
    if connect {
        for i in 0..n {
            if j.added_node[i] {
                member[i] = true;
                connector[i] = true;
                n_connectors += 1;
            }
        }
        after = j.pieces_after;
        unreachable = j.unreachable;
    }
    if n_match == 0 {
        before = 0;
        after = 0;
    }
    SubInfo { member, connector, n_match, n_neighbours, n_connectors, pieces_before: before, pieces_after: after, unreachable }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::model::Session;

    /// Chain a-b-c-d-e-f-g with a side branch c-x-y; colour red at a,b | e | g.
    fn session() -> Session {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,d\nd,e\ne,f\nf,g\nc,x\nx,y\n").unwrap();
        let n = parse_csv_text("id,col\na,red\nb,red\nc,blue\nd,blue\ne,red\nf,blue\ng,red\nx,blue\ny,blue\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    fn red_rule(s: &Session) -> Rule {
        let mut r = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut r.kind {
            let (_, labels) = s.g.node_col("col").unwrap().as_categories();
            for (k, a) in allowed.iter_mut().enumerate() {
                *a = labels.get(k).is_some_and(|l| l == "red");
            }
        }
        r
    }

    fn ids(s: &Session, flags: &[bool]) -> Vec<String> {
        let mut v: Vec<String> = (0..s.g.n()).filter(|&i| flags[i]).map(|i| s.g.ids[i].clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn matches_only_without_joining() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 0, false);
        assert_eq!(ids(&s, &i.member), ["a", "b", "e", "g"]);
        assert_eq!((i.n_match, i.pieces_before, i.pieces_after, i.n_connectors), (4, 3, 3, 0));
    }

    #[test]
    fn joining_walks_the_shortest_path_through_the_original_graph() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 0, true);
        // a,b | e | g  →  join with c,d (b..e) and f (e..g); the side branch x,y is not needed
        assert_eq!(ids(&s, &i.member), ["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(ids(&s, &i.connector), ["c", "d", "f"]);
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable, i.n_connectors), (3, 1, 0, 3));
    }

    #[test]
    fn neighbours_and_hops() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 1, false);
        assert_eq!(ids(&s, &i.member), ["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(i.n_neighbours, 3);
        let one = compute(&s.g, &red_rule(&s), 0, false);
        assert_eq!(one.n_neighbours, 0);
    }

    #[test]
    fn pieces_in_different_original_components_stay_apart() {
        let e = parse_csv_text("source,target\na,b\nc,d\n").unwrap();
        let n = parse_csv_text("id,col\na,red\nb,red\nc,red\nd,red\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        let i = compute(&s.g, &red_rule(&s), 0, true);
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable, i.n_connectors), (2, 2, 1, 0));
    }

    #[test]
    fn session_applies_layout_and_restores_it() {
        let mut s = session();
        let before = s.pos.clone();
        s.sub = SubSettings { enabled: true, rule: Some(red_rule(&s)), hops: 0, connect: true };
        s.apply_subgraph();
        assert_eq!(s.n_vis, 7);
        assert_eq!(s.vis.iter().filter(|v| **v).count(), 7);
        s.settle(50);
        s.sub.enabled = false;
        s.apply_subgraph();
        assert_eq!(s.n_vis, 9);
        assert_eq!(s.pos, before, "the full layout comes back");
    }

    #[test]
    fn change_highlight_can_join_its_pieces() {
        use crate::changes::ChangeSettings;
        let mut s = session();
        // change edges for `col`: b-c (red/blue), d-e, e-f, f-g; node a is never on a change
        s.change = ChangeSettings { enabled: true, col: Some("col".into()), only: true, connect: true, ..Default::default() };
        s.recompute_changes();
        let i = s.change_info.clone().unwrap();
        assert_eq!(i.n_change, 4);
        // pieces of change nodes: {b,c} and {d,e,f,g}; c-d joins them
        assert_eq!((i.pieces_before, i.pieces_after, i.n_connectors), (2, 1, 0));
        assert!(i.edge_path.iter().filter(|b| **b).count() == 1);
    }
}
