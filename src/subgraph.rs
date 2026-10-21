//! Sub-graphs: keep only the nodes that match an attribute rule (plus optional neighbours), and join the
//! separate pieces back into one graph by walking the original graph along shortest paths.

use crate::graph::Graph;
use crate::model::{Rule, RuleKind};
use crate::data::NULL_CODE;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// A link standing in for removed nodes: from, to, and whether it points from the first to the second.
pub type Bridge = (u32, u32, bool);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SubSettings {
    pub enabled: bool,
    pub rule: Option<Rule>,
    /// Also keep nodes up to this many steps away from a matching node.
    pub hops: u8,
    /// Join separate pieces into one connected graph.
    pub connect: bool,
    /// How: contract the removed nodes that lie between the pieces (they disappear and their neighbours are linked
    /// directly), or keep those nodes on screen as joining nodes.
    pub contract: bool,
}

impl Default for SubSettings {
    fn default() -> Self {
        SubSettings { enabled: false, rule: None, hops: 0, connect: true, contract: false }
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
    /// Removed nodes that were contracted away to join pieces (they are not shown).
    /// New links between kept nodes that stand in for a path through contracted nodes.
    pub bridges: Vec<Bridge>,
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

/// Contract the nodes that `join_pieces` picked to connect the pieces: they disappear, and the kept nodes they
/// linked get a direct link instead. Returns those links (each one joins two pieces that were apart) and the number
/// of pieces left.
pub fn contract_joins(g: &Graph, core: &[bool], j: &Joined) -> (Vec<Bridge>, usize) {
    let n = g.n();
    // pieces of the kept nodes
    let mut pieces = Dsu((0..n as u32).collect());
    for &(s, t) in &g.edges {
        if core[s as usize] && core[t as usize] {
            pieces.union(s, t);
        }
    }
    // groups of removed nodes that touch each other along the joining paths
    let mut groups = Dsu((0..n as u32).collect());
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if j.added_edge[e] && j.added_node[s as usize] && j.added_node[t as usize] {
            groups.union(s, t);
        }
    }
    // the kept nodes each group was walked to
    let mut touches: HashMap<u32, Vec<u32>> = HashMap::new();
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if !j.added_edge[e] {
            continue;
        }
        let (kept, gone) = match (core[s as usize], core[t as usize]) {
            (true, false) if j.added_node[t as usize] => (s, t),
            (false, true) if j.added_node[s as usize] => (t, s),
            _ => continue,
        };
        touches.entry(groups.find(gone)).or_default().push(kept);
    }
    let mut keys: Vec<u32> = touches.keys().copied().collect();
    keys.sort_unstable();
    let mut bridges = Vec::new();
    for k in keys {
        let mut ends = touches.remove(&k).unwrap_or_default();
        ends.sort_unstable();
        ends.dedup();
        // link the ends one after the other, but only where that joins two pieces
        let first = ends[0];
        for &other in &ends[1..] {
            if pieces.find(first) != pieces.find(other) {
                bridges.push((first.min(other), first.max(other), false));
                pieces.union(first, other);
            }
        }
    }
    let mut roots: Vec<u32> = (0..n as u32).filter(|&i| core[i as usize]).map(|i| pieces.find(i)).collect();
    roots.sort_unstable();
    roots.dedup();
    (bridges, roots.len())
}

pub fn compute(g: &Graph, rule: &Rule, hops: u8, connect: bool, contract: bool) -> SubInfo {
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
    let mut connector = vec![false; n];
    let mut n_connectors = 0;
    let mut bridges = Vec::new();
    let (mut before, mut after, mut unreachable) = (j.pieces_before, j.pieces_before, 0);
    let mut member = core.clone();
    if connect {
        unreachable = j.unreachable;
        if contract {
            // the removed nodes between the pieces vanish and their neighbours are linked directly
            let (b, left) = contract_joins(g, &core, &j);
            bridges = b;
            after = left;
        } else {
            for i in 0..n {
                if j.added_node[i] {
                    member[i] = true;
                    connector[i] = true;
                    n_connectors += 1;
                }
            }
            after = j.pieces_after;
        }
    }
    if n_match == 0 {
        before = 0;
        after = 0;
    }
    SubInfo { member, connector, n_match, n_neighbours, n_connectors, bridges, pieces_before: before, pieces_after: after, unreachable }
}


// ───────────────────────────── contracting nodes ─────────────────────────────

/// Remove the nodes that match a rule and rebuild the graph around them, as in graph contraction: nodes that were
/// linked through removed nodes get a direct link (drawn dashed), so what was connected stays connected.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ContractSettings {
    pub enabled: bool,
    /// The nodes that match are removed (for categories: the ticked values).
    pub rule: Option<Rule>,
    /// Draw the dashed links that stand in for the removed nodes.
    pub show_links: bool,
}

impl Default for ContractSettings {
    fn default() -> Self {
        ContractSettings { enabled: false, rule: None, show_links: true }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContractInfo {
    pub removed: Vec<bool>,
    pub n_removed: usize,
    /// The links that stand in for the removed nodes.
    pub bridges: Vec<Bridge>,
}

/// Most links contraction will add; a removed hub with thousands of neighbours on each side would otherwise
/// make millions.
const MAX_BRIDGES: usize = 250_000;

/// Remove the nodes the rule matches, keeping the graph's own logic: wherever a path ran from one kept node through
/// removed nodes to another, the two now join directly, in the same direction. With `a → b` and `b → c`, removing
/// `b` leaves `a → c`. Undirected edges work both ways, so they bridge both ways (one undirected link).
pub fn contract_nodes(g: &Graph, rule: &Rule) -> ContractInfo {
    let n = g.n();
    // the rule keeps what matches; here what matches is what goes
    let mut removed = vec![true; n];
    rule_pass(g, rule, &mut removed);
    let n_removed = removed.iter().filter(|b| **b).count();
    if n_removed == 0 || n_removed == n {
        return ContractInfo { removed, n_removed, bridges: vec![] };
    }
    // who can be reached in one step, following each edge's own direction
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s == t {
            continue;
        }
        out[s as usize].push(t);
        if !g.edge_directed(e) {
            out[t as usize].push(s);
        }
    }
    // links the kept nodes already have among themselves
    let real: std::collections::HashSet<(u32, u32)> = (0..n as u32)
        .filter(|&u| !removed[u as usize])
        .flat_map(|u| out[u as usize].iter().filter(|&&v| !removed[v as usize]).map(move |&v| (u, v)).collect::<Vec<_>>())
        .collect();
    // from every kept node: which kept nodes does a walk through removed nodes alone reach?
    let mut reach: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut seen = vec![u32::MAX; n];
    let mut stack: Vec<u32> = Vec::new();
    'sources: for u in 0..n as u32 {
        if removed[u as usize] {
            continue;
        }
        stack.clear();
        for &w in &out[u as usize] {
            if removed[w as usize] && seen[w as usize] != u {
                seen[w as usize] = u;
                stack.push(w);
            }
        }
        while let Some(x) = stack.pop() {
            for &y in &out[x as usize] {
                if removed[y as usize] {
                    if seen[y as usize] != u {
                        seen[y as usize] = u;
                        stack.push(y);
                    }
                } else if y != u && !real.contains(&(u, y)) {
                    reach.insert((u, y));
                    if reach.len() > 2 * MAX_BRIDGES {
                        break 'sources;
                    }
                }
            }
        }
    }
    // one link per pair of nodes: undirected if the path runs both ways, otherwise pointing the way it ran
    let mut bridges: Vec<Bridge> = Vec::new();
    let mut done: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut pairs: Vec<(u32, u32)> = reach.iter().copied().collect();
    pairs.sort_unstable();
    for (u, v) in pairs {
        if !done.insert((u.min(v), u.max(v))) {
            continue;
        }
        let fwd = reach.contains(&(u, v));
        let bwd = reach.contains(&(v, u));
        match (fwd, bwd) {
            (true, true) => bridges.push((u.min(v), u.max(v), false)),
            (true, false) => bridges.push((u, v, true)),
            (false, true) => bridges.push((v, u, true)),
            _ => {}
        }
        if bridges.len() >= MAX_BRIDGES {
            break;
        }
    }
    ContractInfo { removed, n_removed, bridges }
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
        let i = compute(&s.g, &red_rule(&s), 0, false, false);
        assert_eq!(ids(&s, &i.member), ["a", "b", "e", "g"]);
        assert_eq!((i.n_match, i.pieces_before, i.pieces_after, i.n_connectors), (4, 3, 3, 0));
    }

    #[test]
    fn joining_walks_the_shortest_path_through_the_original_graph() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 0, true, false);
        // a,b | e | g  →  join with c,d (b..e) and f (e..g); the side branch x,y is not needed
        assert_eq!(ids(&s, &i.member), ["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(ids(&s, &i.connector), ["c", "d", "f"]);
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable, i.n_connectors), (3, 1, 0, 3));
    }

    #[test]
    fn contracting_removes_the_nodes_in_between_and_links_their_neighbours() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 0, true, true);
        // a,b | e | g: the removed nodes c,d and f vanish; b-e and e-g stand in for the paths through them
        assert_eq!(ids(&s, &i.member), ["a", "b", "e", "g"], "no removed node stays on screen");
        assert!(i.connector.iter().all(|c| !c));
        assert_eq!(i.n_connectors, 0);
        let mut b: Vec<(String, String)> = i.bridges.iter().map(|&(x, y, _)| (s.g.ids[x as usize].clone(), s.g.ids[y as usize].clone())).collect();
        b.sort();
        assert_eq!(b, [("b".to_string(), "e".to_string()), ("e".to_string(), "g".to_string())]);
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable), (3, 1, 0));
    }

    #[test]
    fn contracting_does_not_link_pieces_that_the_full_graph_keeps_apart() {
        let e = parse_csv_text("source,target\na,b\nc,d\n").unwrap();
        let n = parse_csv_text("id,col\na,red\nb,red\nc,red\nd,red\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        let i = compute(&s.g, &red_rule(&s), 0, true, true);
        assert!(i.bridges.is_empty());
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable), (2, 2, 1));
    }

    #[test]
    fn a_removed_hub_joining_three_pieces_links_them_in_one_go() {
        // hub h connects a, b and c; only a, b, c are kept
        let e = parse_csv_text("source,target\nh,a\nh,b\nh,c\n").unwrap();
        let n = parse_csv_text("id,col\na,red\nb,red\nc,red\nh,blue\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        let i = compute(&s.g, &red_rule(&s), 0, true, true);
        assert_eq!(ids(&s, &i.member), ["a", "b", "c"]);
        assert_eq!((i.bridges.len(), i.pieces_before, i.pieces_after), (2, 3, 1));
    }

    #[test]
    fn session_draws_the_bridges_and_never_the_removed_nodes() {
        let mut s = session();
        s.sub = SubSettings { enabled: true, rule: Some(red_rule(&s)), hops: 0, connect: true, contract: true };
        s.apply_subgraph();
        assert_eq!(s.n_vis, 4);
        assert_eq!(s.n_vis_edges, 3, "the real edge a-b plus the two bridges");
        let hi = crate::model::Highlight { focus: &vec![false; 9], hood: &vec![false; 9], active: false };
        let mut dl = crate::model::DrawList::default();
        crate::model::build_draw_list(&s, &crate::model::ViewCam::default(), egui::Vec2::new(800.0, 600.0), &hi, &mut dl);
        assert_eq!(dl.nodes.len(), 4);
        assert!(!dl.edges.is_empty(), "the bridges are drawn");
    }

    #[test]
    fn contracting_a_node_links_its_neighbours_and_keeps_them_connected() {
        let s = session();
        // remove the blue nodes: c, d, f, x, y  → a,b | e | g: b-e through c,d; e-g through f
        let mut r = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut r.kind {
            let (_, labels) = s.g.node_col("col").unwrap().as_categories();
            for (k, a) in allowed.iter_mut().enumerate() {
                *a = labels.get(k).is_some_and(|l| l == "blue");
            }
        }
        let c = contract_nodes(&s.g, &r);
        assert_eq!(c.n_removed, 5);
        let mut b: Vec<(String, String)> = c.bridges.iter().map(|&(x, y, _)| (s.g.ids[x as usize].clone(), s.g.ids[y as usize].clone())).collect();
        b.sort();
        // the existing edge a-b stays, so only b-e and e-g are new
        assert_eq!(b, [("b".to_string(), "e".to_string()), ("e".to_string(), "g".to_string())]);
    }

    #[test]
    fn contracting_a_hub_links_all_its_neighbours_without_doubling_real_edges() {
        let e = parse_csv_text("source,target\nh,a\nh,b\nh,c\na,b\n").unwrap();
        let n = parse_csv_text("id,col\na,keep\nb,keep\nc,keep\nh,drop\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        let mut r = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut r.kind {
            let (_, labels) = s.g.node_col("col").unwrap().as_categories();
            for (k, a) in allowed.iter_mut().enumerate() {
                *a = labels.get(k).is_some_and(|l| l == "drop");
            }
        }
        let c = contract_nodes(&s.g, &r);
        // a-b already exists; a-c and b-c are new
        assert_eq!(c.n_removed, 1);
        assert_eq!(c.bridges.len(), 2);
    }

    #[test]
    fn removing_everything_or_nothing_changes_nothing() {
        let s = session();
        let all = Rule::new_for(s.g.node_col("col").unwrap()); // every value ticked
        let c = contract_nodes(&s.g, &all);
        assert!(c.n_removed == s.g.n() && c.bridges.is_empty());
        let mut none = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut none.kind {
            allowed.iter_mut().for_each(|a| *a = false);
        }
        assert_eq!(contract_nodes(&s.g, &none).n_removed, 0);
    }

    #[test]
    fn session_contracts_and_restores_the_full_graph() {
        let mut s = session();
        let before = s.pos.clone();
        let mut r = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut r.kind {
            let (_, labels) = s.g.node_col("col").unwrap().as_categories();
            for (k, a) in allowed.iter_mut().enumerate() {
                *a = labels.get(k).is_some_and(|l| l == "blue");
            }
        }
        s.contract = ContractSettings { enabled: true, rule: Some(r), ..Default::default() };
        s.apply_subgraph();
        assert_eq!(s.n_vis, 4, "a, b, e, g remain");
        assert_eq!(s.n_vis_edges, 3, "a-b plus the two new links");
        assert!(s.contract_active());
        s.settle(30);
        s.contract.enabled = false;
        s.apply_subgraph();
        assert_eq!(s.n_vis, 9);
        assert_eq!(s.pos, before, "the full layout comes back");
    }

    fn directed_session(edges: &str, nodes: &str) -> Session {
        let e = parse_csv_text(edges).unwrap();
        let n = parse_csv_text(nodes).unwrap();
        let mut s = Session::new();
        let mut em = auto_edge_mapping(&e);
        em.directed = true;
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    fn drop_rule(s: &Session, value: &str) -> Rule {
        let mut r = Rule::new_for(s.g.node_col("col").unwrap());
        if let RuleKind::Cats { allowed } = &mut r.kind {
            let (_, labels) = s.g.node_col("col").unwrap().as_categories();
            for (k, a) in allowed.iter_mut().enumerate() {
                *a = labels.get(k).is_some_and(|l| l == value);
            }
        }
        r
    }

    fn named(s: &Session, c: &ContractInfo) -> Vec<(String, String, bool)> {
        let mut v: Vec<_> = c.bridges.iter().map(|&(x, y, d)| (s.g.ids[x as usize].clone(), s.g.ids[y as usize].clone(), d)).collect();
        v.sort();
        v
    }

    #[test]
    fn a_to_b_to_c_without_b_is_a_to_c() {
        let s = directed_session("source,target\na,b\nb,c\n", "id,col\na,keep\nb,drop\nc,keep\n");
        let c = contract_nodes(&s.g, &drop_rule(&s, "drop"));
        assert_eq!(named(&s, &c), [("a".to_string(), "c".to_string(), true)]);
    }

    #[test]
    fn directions_are_respected_so_unrelated_neighbours_stay_unlinked() {
        // a -> b <- c : nothing runs from a to c through b
        let s = directed_session("source,target\na,b\nc,b\n", "id,col\na,keep\nb,drop\nc,keep\n");
        assert!(contract_nodes(&s.g, &drop_rule(&s, "drop")).bridges.is_empty());
        // a <- b -> c : b is a source, and a and c are not linked either
        let s = directed_session("source,target\nb,a\nb,c\n", "id,col\na,keep\nb,drop\nc,keep\n");
        assert!(contract_nodes(&s.g, &drop_rule(&s, "drop")).bridges.is_empty());
    }

    #[test]
    fn a_whole_chain_of_removed_nodes_collapses_and_keeps_its_direction() {
        // a -> b -> c -> d, with b and c removed: a -> d; and a branch c -> e adds a -> e
        let s = directed_session("source,target\na,b\nb,c\nc,d\nc,e\n", "id,col\na,keep\nb,drop\nc,drop\nd,keep\ne,keep\n");
        let c = contract_nodes(&s.g, &drop_rule(&s, "drop"));
        assert_eq!(named(&s, &c), [("a".to_string(), "d".to_string(), true), ("a".to_string(), "e".to_string(), true)]);
    }

    #[test]
    fn a_path_that_runs_both_ways_becomes_one_undirected_link_and_real_edges_are_not_doubled() {
        // a -> b -> c and c -> b -> a  → a -- c ;  also a -> c already exists elsewhere in the second case
        let s = directed_session("source,target\na,b\nb,c\nc,b\nb,a\n", "id,col\na,keep\nb,drop\nc,keep\n");
        let c = contract_nodes(&s.g, &drop_rule(&s, "drop"));
        assert_eq!(named(&s, &c), [("a".to_string(), "c".to_string(), false)]);
        let s = directed_session("source,target\na,b\nb,c\na,c\n", "id,col\na,keep\nb,drop\nc,keep\n");
        assert!(contract_nodes(&s.g, &drop_rule(&s, "drop")).bridges.is_empty(), "a -> c is already there");
    }

    #[test]
    fn the_new_link_is_drawn_with_an_arrow_in_the_session() {
        let mut s = directed_session("source,target\na,b\nb,c\n", "id,col\na,keep\nb,drop\nc,keep\n");
        s.contract = ContractSettings { enabled: true, rule: Some(drop_rule(&s, "drop")), ..Default::default() };
        s.apply_subgraph();
        assert_eq!((s.n_vis, s.n_vis_edges), (2, 1));
        let none = vec![false; 3];
        let hi = crate::model::Highlight { focus: &none, hood: &none, active: false };
        let mut dl = crate::model::DrawList::default();
        crate::model::build_draw_list(&s, &crate::model::ViewCam::default(), egui::Vec2::new(800.0, 600.0), &hi, &mut dl);
        assert!(dl.edges.iter().any(|e| e.directed), "the arrow head sits on the new link");
    }

    #[test]
    fn neighbours_and_hops() {
        let s = session();
        let i = compute(&s.g, &red_rule(&s), 1, false, false);
        assert_eq!(ids(&s, &i.member), ["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(i.n_neighbours, 3);
        let one = compute(&s.g, &red_rule(&s), 0, false, false);
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
        let i = compute(&s.g, &red_rule(&s), 0, true, false);
        assert_eq!((i.pieces_before, i.pieces_after, i.unreachable, i.n_connectors), (2, 2, 1, 0));
    }

    #[test]
    fn session_applies_layout_and_restores_it() {
        let mut s = session();
        let before = s.pos.clone();
        s.sub = SubSettings { enabled: true, rule: Some(red_rule(&s)), hops: 0, connect: true, contract: false };
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
