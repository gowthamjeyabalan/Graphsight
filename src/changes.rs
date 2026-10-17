//! "Where does an attribute change?": finds the connections whose two ends have different values of a
//! chosen node attribute, counts them per transition (e.g. Level 1 → 2) and tells the canvas what to highlight.

use crate::data::{Column, NULL_CODE};
use crate::graph::Graph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Numeric columns with at most this many distinct values get a per-transition table.
const MAX_TRANSITION_VALUES: usize = 40;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ChangeSettings {
    pub enabled: bool,
    pub col: Option<String>,
    /// For numeric columns: differences up to this size are not counted as a change (0 = any difference).
    pub min_diff: f64,
    /// Hide every connection (and node) that is not part of a change.
    pub only: bool,
    /// Highlight just one transition, as (from label, to label).
    pub pick: Option<(String, String)>,
    /// With `only`: also add the connections needed to join the separate pieces into one graph.
    pub connect: bool,
}

impl Default for ChangeSettings {
    fn default() -> Self {
        ChangeSettings { enabled: false, col: None, min_diff: 0.0, only: false, pick: None, connect: true }
    }
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub from: String,
    pub to: String,
    pub count: usize,
    /// For numeric columns: +1 if the value rises from `from` to `to`, -1 if it falls, 0 otherwise.
    pub dir: i8,
}

#[derive(Clone, Debug, Default)]
pub struct ChangeInfo {
    /// Per edge: true if it is a change (and matches the picked transition, if one is picked).
    pub edge_hi: Vec<bool>,
    /// Per node: true if at least one highlighted edge touches it.
    pub node_hi: Vec<bool>,
    pub transitions: Vec<Transition>,
    /// Visible connections that were compared / are changes / could not be compared (a value is missing).
    pub n_compared: usize,
    pub n_change: usize,
    pub n_unknown: usize,
    pub n_boundary_nodes: usize,
    pub numeric: bool,
    pub mean_abs_diff: Option<f64>,
    /// Joining paths (only filled when "only" and "connect" are on): extra connections / nodes to keep.
    pub edge_path: Vec<bool>,
    pub node_path: Vec<bool>,
    pub n_connectors: usize,
    pub pieces_before: usize,
    pub pieces_after: usize,
}

pub fn compute(g: &Graph, col: &Column, min_diff: f64, vis: &[bool], pick: &Option<(String, String)>, join: bool) -> ChangeInfo {
    let ne = g.edges.len();
    let numeric = col.is_numeric() && !col.hint_cat;
    let (codes, labels) = col.as_categories();
    let use_table = !numeric || labels.len() <= MAX_TRANSITION_VALUES;
    let mut info = ChangeInfo { edge_hi: vec![false; ne], node_hi: vec![false; g.n()], numeric, ..Default::default() };
    let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
    let mut diff_sum = 0.0;
    let mut change_edge = vec![None::<(u32, u32)>; ne];
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        let (si, ti) = (s as usize, t as usize);
        if si == ti || !vis[si] || !vis[ti] {
            continue;
        }
        let (cs, ct) = (codes[si], codes[ti]);
        if cs == NULL_CODE || ct == NULL_CODE {
            info.n_unknown += 1;
            continue;
        }
        info.n_compared += 1;
        let changed = if numeric {
            match (col.num(si), col.num(ti)) {
                (Some(a), Some(b)) => {
                    let d = (a - b).abs();
                    let c = if min_diff > 0.0 { d > min_diff } else { cs != ct };
                    if c {
                        diff_sum += d;
                    }
                    c
                }
                _ => false,
            }
        } else {
            cs != ct
        };
        if !changed {
            continue;
        }
        info.n_change += 1;
        // directed graphs keep the direction of the edge; undirected ones list each pair once
        let key = if g.edge_directed(e) || cs <= ct { (cs, ct) } else { (ct, cs) };
        change_edge[e] = Some(key);
        if use_table {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    if numeric && info.n_change > 0 {
        info.mean_abs_diff = Some(diff_sum / info.n_change as f64);
    }
    let picked: Option<(u32, u32)> = pick.as_ref().and_then(|(a, b)| {
        let ia = labels.iter().position(|l| l == a)? as u32;
        let ib = labels.iter().position(|l| l == b)? as u32;
        Some((ia, ib))
    });
    for (e, key) in change_edge.iter().enumerate() {
        let Some(k) = key else { continue };
        if picked.is_none_or(|p| p == *k) {
            info.edge_hi[e] = true;
            let (s, t) = g.edges[e];
            info.node_hi[s as usize] = true;
            info.node_hi[t as usize] = true;
        }
    }
    info.n_boundary_nodes = info.node_hi.iter().filter(|b| **b).count();
    info.edge_path = vec![false; ne];
    info.node_path = vec![false; g.n()];
    if join && info.n_boundary_nodes > 0 {
        let j = crate::subgraph::join_pieces(g, &info.node_hi, Some(vis), Some(&info.edge_hi));
        info.n_connectors = j.added_node.iter().filter(|b| **b).count();
        info.pieces_before = j.pieces_before;
        info.pieces_after = j.pieces_after;
        info.edge_path = j.added_edge;
        info.node_path = j.added_node;
    }
    let mut tr: Vec<Transition> = counts
        .into_iter()
        .map(|((a, b), count)| {
            let dir = if numeric { (b > a) as i8 - (b < a) as i8 } else { 0 };
            Transition { from: labels[a as usize].clone(), to: labels[b as usize].clone(), count, dir }
        })
        .collect();
    // biggest first, ties in label order
    tr.sort_by(|x, y| y.count.cmp(&x.count).then_with(|| crate::data::natural_cmp(&x.from, &y.from)).then_with(|| crate::data::natural_cmp(&x.to, &y.to)));
    info.transitions = tr;
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::model::Session;

    /// a-b-c-d-e chain plus c-f; Level: a=1 b=1 c=2 d=2 e=3 f=3; Kind text on the same nodes.
    fn session(directed: bool) -> Session {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,d\nd,e\nc,f\n").unwrap();
        let n = parse_csv_text("id,Level,Kind\na,1,x\nb,1,x\nc,2,y\nd,2,y\ne,3,x\nf,3,\n").unwrap();
        let mut s = Session::new();
        let mut em = auto_edge_mapping(&e);
        em.directed = directed;
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    fn run(s: &mut Session, col: &str, min_diff: f64, pick: Option<(&str, &str)>) -> ChangeInfo {
        s.change = ChangeSettings { enabled: true, col: Some(col.into()), min_diff, only: false, pick: pick.map(|(a, b)| (a.into(), b.into())), connect: true };
        s.recompute_changes();
        s.change_info.clone().unwrap()
    }

    #[test]
    fn numeric_level_changes_and_transition_table() {
        let mut s = session(true);
        let i = run(&mut s, "Level", 0.0, None);
        // b-c (1→2), c-f (2→3), d-e (2→3) change; a-b and c-d do not
        assert_eq!((i.n_compared, i.n_change, i.n_unknown), (5, 3, 0));
        assert_eq!(i.transitions.len(), 2);
        assert_eq!((i.transitions[0].from.as_str(), i.transitions[0].to.as_str(), i.transitions[0].count), ("2", "3", 2));
        assert_eq!((i.transitions[1].from.as_str(), i.transitions[1].to.as_str(), i.transitions[1].count), ("1", "2", 1));
        assert_eq!(i.transitions[0].dir, 1);
        // nodes at the boundary: b c d e f
        assert_eq!(i.n_boundary_nodes, 5);
        assert!(!i.node_hi[s.g.index["a"] as usize]);
    }

    #[test]
    fn min_diff_ignores_small_steps() {
        let mut s = session(false);
        let i = run(&mut s, "Level", 1.0, None); // every step is exactly 1, so nothing counts
        assert_eq!(i.n_change, 0);
        let i = run(&mut s, "Level", 0.5, None);
        assert_eq!(i.n_change, 3);
    }

    #[test]
    fn picking_one_transition_highlights_only_it() {
        let mut s = session(true);
        let i = run(&mut s, "Level", 0.0, Some(("1", "2")));
        assert_eq!(i.edge_hi.iter().filter(|b| **b).count(), 1);
        assert_eq!(i.n_boundary_nodes, 2);
        assert_eq!(i.n_change, 3, "the totals still describe every change");
    }

    #[test]
    fn direction_is_kept_only_for_directed_graphs() {
        // reverse the edge d→c: directed shows 3→2 separately, undirected folds it into 2-3
        let e = parse_csv_text("source,target\nb,c\ne,d\n").unwrap();
        let n = parse_csv_text("id,Level\nb,1\nc,2\nd,2\ne,3\n").unwrap();
        for directed in [true, false] {
            let mut s = Session::new();
            let mut em = auto_edge_mapping(&e);
            em.directed = directed;
            s.set_edges("e".into(), e.clone(), em);
            let nm = auto_node_mapping(&n);
            s.set_nodes("n".into(), n.clone(), nm);
            s.rebuild();
            let i = run(&mut s, "Level", 0.0, None);
            let t: Vec<_> = i.transitions.iter().map(|t| (t.from.clone(), t.to.clone(), t.dir)).collect();
            if directed {
                assert!(t.contains(&("3".into(), "2".into(), -1)), "{t:?}");
            } else {
                assert!(t.iter().all(|x| x.2 >= 0), "{t:?}");
            }
        }
    }

    #[test]
    fn text_attribute_and_missing_values() {
        let mut s = session(false);
        let i = run(&mut s, "Kind", 0.0, None);
        // f has no Kind, so c-f is skipped; a-b same, b-c x/y, c-d same, d-e y/x
        assert_eq!((i.n_compared, i.n_change, i.n_unknown), (4, 2, 1));
        assert!(!i.numeric);
        assert_eq!(i.transitions.len(), 1, "x-y and y-x are the same pair when undirected");
        assert_eq!(i.transitions[0].count, 2);
    }

    #[test]
    fn filtering_limits_the_comparison_and_draw_list_marks_changes() {
        use crate::model::*;
        let mut s = session(false);
        run(&mut s, "Level", 0.0, None);
        s.filter.expr_text = "Level < 3".into();
        s.recompute_visibility();
        let i = s.change_info.clone().unwrap();
        assert_eq!((i.n_compared, i.n_change), (3, 1)); // a-b, b-c, c-d among visible nodes
        // draw list: the change edge goes to the highlight list, ring on its two ends
        let size = egui::Vec2::new(400.0, 300.0);
        let cam = s.fit_camera(size);
        let none = vec![false; s.g.n()];
        let hi = Highlight { focus: &none, hood: &none, active: false };
        let mut dl = DrawList::default();
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!(dl.edges_hi.len(), 1);
        assert_eq!(dl.nodes.iter().filter(|n| n.ring).count(), 2);
        s.change.only = true;
        build_draw_list(&s, &cam, size, &hi, &mut dl);
        assert_eq!((dl.edges_hi.len(), dl.edges.len() + dl.edges_back.len(), dl.nodes.len()), (1, 0, 2));
    }

    #[test]
    fn many_distinct_numbers_skip_the_table() {
        let mut rows = String::from("id,v\n");
        let mut edges = String::from("source,target\n");
        for k in 0..100 {
            rows += &format!("n{k},{}\n", k as f64 * 1.5);
            if k > 0 {
                edges += &format!("n{},n{k}\n", k - 1);
            }
        }
        let (e, n) = (parse_csv_text(&edges).unwrap(), parse_csv_text(&rows).unwrap());
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        let i = run(&mut s, "v", 0.0, None);
        assert_eq!(i.n_change, 99);
        assert!(i.transitions.is_empty());
        assert!((i.mean_abs_diff.unwrap() - 1.5).abs() < 1e-9);
    }
}
