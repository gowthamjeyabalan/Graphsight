//! "How many nodes with attribute A connect to nodes with attribute B?": a two-condition query over the
//! connections, and a cross-tab (attribute against attribute) with the number of connections in each cell.
//!
//! A condition is a small builder (column, operator, value), not free text. It is turned into a filter
//! expression and evaluated by the same engine as the Filter panel (`expr.rs`).

use crate::data::NULL_CODE;
use crate::expr;
use crate::graph::Graph;
use serde::{Deserialize, Serialize};

/// A cross-tab lists at most this many different values along each side.
pub const MAX_XTAB_VALUES: usize = 40;

/// The node id as a pseudo-column (unless the nodes file has an attribute of that name).
pub const ID_COLUMN: &str = "id";

// ───────────────────────────── conditions ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Op {
    Is,
    IsNot,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    Empty,
    NotEmpty,
}

impl Op {
    pub const ALL: [Op; 10] = [Op::Is, Op::IsNot, Op::Lt, Op::Le, Op::Gt, Op::Ge, Op::Contains, Op::StartsWith, Op::Empty, Op::NotEmpty];

    pub fn label(self) -> &'static str {
        match self {
            Op::Is => "is",
            Op::IsNot => "is not",
            Op::Lt => "<",
            Op::Le => "≤",
            Op::Gt => ">",
            Op::Ge => "≥",
            Op::Contains => "contains",
            Op::StartsWith => "starts with",
            Op::Empty => "is empty",
            Op::NotEmpty => "is not empty",
        }
    }

    pub fn needs_value(self) -> bool {
        !matches!(self, Op::Empty | Op::NotEmpty)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Cond {
    /// None = every node.
    pub col: Option<String>,
    pub op: Op,
    pub value: String,
}

impl Default for Cond {
    fn default() -> Self {
        Cond { col: None, op: Op::Is, value: String::new() }
    }
}

impl Cond {
    pub fn is_any(&self) -> bool {
        self.col.is_none()
    }

    /// The filter expression this condition stands for; "" means every node.
    pub fn expr_text(&self) -> Result<String, String> {
        let Some(col) = &self.col else { return Ok(String::new()) };
        if col.contains('`') {
            return Err(format!("the column name `{col}` contains a backtick, which filters cannot express"));
        }
        let name = format!("`{col}`");
        let value = self.value.trim();
        if self.op.needs_value() && value.is_empty() {
            return Err("choose or type a value".into());
        }
        let quoted = if !value.contains('"') {
            format!("\"{value}\"")
        } else if !value.contains('\'') {
            format!("'{value}'")
        } else {
            return Err("the value cannot contain both kinds of quote marks".into());
        };
        Ok(match self.op {
            Op::Is => format!("{name} == {quoted}"),
            Op::IsNot => format!("{name} != {quoted}"),
            Op::Lt => format!("{name} < {quoted}"),
            Op::Le => format!("{name} <= {quoted}"),
            Op::Gt => format!("{name} > {quoted}"),
            Op::Ge => format!("{name} >= {quoted}"),
            Op::Contains => format!("{name} contains {quoted}"),
            Op::StartsWith => format!("{name} startswith {quoted}"),
            Op::Empty => format!("{name} == null"),
            Op::NotEmpty => format!("{name} != null"),
        })
    }

    /// Short human wording: "Room" for `Type is Room`, "age > 30" otherwise, "any node" for no condition.
    pub fn label(&self, g: &Graph) -> String {
        let Some(col) = &self.col else { return "any node".into() };
        let text_col = col == ID_COLUMN || g.node_col(col).is_some_and(|c| !c.is_numeric() || c.hint_cat);
        let v = self.value.trim();
        match self.op {
            Op::Is if text_col && col != ID_COLUMN => v.to_string(),
            Op::Is => format!("{col} = {v}"),
            Op::IsNot => format!("{col} ≠ {v}"),
            Op::Lt => format!("{col} < {v}"),
            Op::Le => format!("{col} ≤ {v}"),
            Op::Gt => format!("{col} > {v}"),
            Op::Ge => format!("{col} ≥ {v}"),
            Op::Contains => format!("{col} contains {v}"),
            Op::StartsWith => format!("{col} starts with {v}"),
            Op::Empty => format!("{col} is empty"),
            Op::NotEmpty => format!("{col} is not empty"),
        }
    }

    /// Which nodes meet the condition (and are visible, when `vis` is given).
    pub fn matching(&self, g: &Graph, vis: Option<&[bool]>) -> Result<Vec<bool>, String> {
        let text = self.expr_text()?;
        let compiled = expr::compile(&text, g)?;
        Ok((0..g.n())
            .map(|i| vis.is_none_or(|v| v[i]) && compiled.as_ref().is_none_or(|c| c.matches(g, i)))
            .collect())
    }
}

// ───────────────────────────── settings ─────────────────────────────

/// Which way a connection has to point, seen from the nodes of the first condition. Directed edges count as
/// outgoing for their source and incoming for their target; undirected edges count both ways.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Dir {
    #[default]
    Either,
    Out,
    In,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum QueryMode {
    #[default]
    Count,
    CrossTab,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct QuerySettings {
    pub mode: QueryMode,
    pub a: Cond,
    pub b: Cond,
    pub dir: Dir,
    /// Count only nodes that pass the current filter / subgraph, and the connections between them.
    pub visible_only: bool,
    /// Cross-tab: the attribute down the rows and the one across the columns.
    pub rows: Option<String>,
    pub cols: Option<String>,
}

impl Default for QuerySettings {
    fn default() -> Self {
        QuerySettings { mode: QueryMode::Count, a: Cond::default(), b: Cond::default(), dir: Dir::Either, visible_only: true, rows: None, cols: None }
    }
}

impl QuerySettings {
    /// True once there is something to work out (the panel shows a hint until then).
    pub fn is_set(&self) -> bool {
        match self.mode {
            QueryMode::Count => self.a.col.is_some() || self.b.col.is_some(),
            QueryMode::CrossTab => self.rows.is_some() && self.cols.is_some(),
        }
    }
}

/// Which ways round an edge can be read: (as written, reversed). In the pair (a = source, b = target) `a` has an
/// outgoing connection to `b`; in the reversed pair (a = target, b = source) it has an incoming one.
fn orientations(dir: Dir, directed: bool) -> (bool, bool) {
    if !directed {
        return (true, true);
    }
    match dir {
        Dir::Out => (true, false),
        Dir::In => (false, true),
        Dir::Either => (true, true),
    }
}

// ───────────────────────────── connection count ─────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct QueryInfo {
    pub in_a: Vec<bool>,
    /// First-condition nodes with at least one connection to a second-condition node.
    pub a_conn: Vec<bool>,
    /// Second-condition nodes that are connected to a first-condition node.
    pub b_reached: Vec<bool>,
    pub n_a: usize,
    pub n_b: usize,
    pub n_a_conn: usize,
    pub n_b_reached: usize,
    /// Connections that run between the two groups (each counted once).
    pub n_edges: usize,
}

impl QueryInfo {
    pub fn n_a_none(&self) -> usize {
        self.n_a - self.n_a_conn
    }

    pub fn connected(&self) -> Vec<usize> {
        (0..self.a_conn.len()).filter(|&i| self.a_conn[i]).collect()
    }

    pub fn unconnected(&self) -> Vec<usize> {
        (0..self.a_conn.len()).filter(|&i| self.in_a[i] && !self.a_conn[i]).collect()
    }

    pub fn reached(&self) -> Vec<usize> {
        (0..self.b_reached.len()).filter(|&i| self.b_reached[i]).collect()
    }
}

pub fn run(g: &Graph, q: &QuerySettings, vis: &[bool]) -> Result<QueryInfo, String> {
    let v = q.visible_only.then_some(vis);
    let in_a = q.a.matching(g, v).map_err(|e| format!("First condition: {e}"))?;
    let in_b = q.b.matching(g, v).map_err(|e| format!("Second condition: {e}"))?;
    let n = g.n();
    let mut a_conn = vec![false; n];
    let mut b_reached = vec![false; n];
    let mut n_edges = 0;
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s == t {
            continue;
        }
        let (si, ti) = (s as usize, t as usize);
        let (fw, rev) = orientations(q.dir, g.edge_directed(e));
        let mut hit = false;
        if fw && in_a[si] && in_b[ti] {
            a_conn[si] = true;
            b_reached[ti] = true;
            hit = true;
        }
        if rev && in_a[ti] && in_b[si] {
            a_conn[ti] = true;
            b_reached[si] = true;
            hit = true;
        }
        n_edges += hit as usize;
    }
    let count = |f: &[bool]| f.iter().filter(|b| **b).count();
    Ok(QueryInfo {
        n_a: count(&in_a),
        n_b: count(&in_b),
        n_a_conn: count(&a_conn),
        n_b_reached: count(&b_reached),
        n_edges,
        in_a,

        a_conn,
        b_reached,
    })
}

/// "65 of 85 Room nodes connect to Sensor nodes." and "20 have no connection to Sensor nodes." (None if a group is empty).
pub fn sentences(g: &Graph, q: &QuerySettings, info: &QueryInfo) -> (String, Option<String>) {
    let a = if q.a.is_any() { "nodes".to_string() } else if matches!(q.a.op, Op::Is) && q.a.label(g) == q.a.value.trim() { format!("{} nodes", q.a.label(g)) } else { format!("nodes where {}", q.a.label(g)) };
    let b = if q.b.is_any() {
        "other nodes".to_string()
    } else if matches!(q.b.op, Op::Is) && q.b.label(g) == q.b.value.trim() {
        format!("{} nodes", q.b.label(g))
    } else {
        format!("nodes where {}", q.b.label(g))
    };
    if info.n_a == 0 {
        return ("No node meets the first condition.".into(), None);
    }
    if info.n_b == 0 {
        return ("No node meets the second condition.".into(), None);
    }
    let (yes, no) = match q.dir {
        Dir::Either => ("connect to", "have no connection to"),
        Dir::Out => ("have an outgoing connection to", "have no outgoing connection to"),
        Dir::In => ("have an incoming connection from", "have no incoming connection from"),
    };
    let first = format!("{} of {} {a} {yes} {b}.", info.n_a_conn, info.n_a);
    let second = (info.n_a_none() > 0).then(|| format!("{} {no} {b}.", info.n_a_none()));
    (first, second)
}

// ───────────────────────────── cross-tab ─────────────────────────────

#[derive(Clone, Debug)]
pub struct XTab {
    pub rows_name: String,
    pub cols_name: String,
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    /// Row-major: `counts[r * cols.len() + c]`.
    pub counts: Vec<usize>,
    /// Connections that fall in at least one cell.
    pub n_edges: usize,
    /// Per node: its row / column value (an index into `rows` / `cols`, or `u32::MAX` if it is not counted).
    row_of: Vec<u32>,
    col_of: Vec<u32>,
    dir: Dir,
}

impl XTab {
    pub fn count(&self, r: usize, c: usize) -> usize {
        self.counts[r * self.cols.len() + c]
    }

    pub fn max_count(&self) -> usize {
        self.counts.iter().copied().max().unwrap_or(0)
    }

    /// The cells (row, column) an edge falls in, at most two (once if both readings give the same cell).
    fn cells_of(&self, g: &Graph, e: usize) -> [Option<(usize, usize)>; 2] {
        let (s, t) = g.edges[e];
        let (si, ti) = (s as usize, t as usize);
        let (fw, rev) = orientations(self.dir, g.edge_directed(e));
        let cell = |a: usize, b: usize| -> Option<(usize, usize)> {
            let (r, c) = (self.row_of[a], self.col_of[b]);
            (r != u32::MAX && c != u32::MAX).then_some((r as usize, c as usize))
        };
        let first = if fw && si != ti { cell(si, ti) } else { None };
        let mut second = if rev && si != ti { cell(ti, si) } else { None };
        if second == first {
            second = None;
        }
        [first, second]
    }

    /// The nodes at both ends of the connections counted in one cell.
    pub fn cell_nodes(&self, g: &Graph, r: usize, c: usize) -> Vec<usize> {
        let mut mark = vec![false; g.n()];
        for (e, &(s, t)) in g.edges.iter().enumerate() {
            if self.cells_of(g, e).iter().flatten().any(|&x| x == (r, c)) {
                mark[s as usize] = true;
                mark[t as usize] = true;
            }
        }
        (0..g.n()).filter(|&i| mark[i]).collect()
    }

}

/// Codes of a column as indices into its labels; a missing value becomes the extra label "(empty)".
fn codes_for(g: &Graph, name: &str, vis: Option<&[bool]>) -> Result<(Vec<u32>, Vec<String>), String> {
    let col = g.node_col(name).ok_or_else(|| format!("There is no attribute called {name}."))?;
    let (codes, mut labels) = col.as_categories();
    if labels.len() > MAX_XTAB_VALUES {
        return Err(format!("{name} has {} different values; a cross-tab shows at most {MAX_XTAB_VALUES}. Pick an attribute with fewer values.", labels.len()));
    }
    let empty = labels.len() as u32;
    labels.push("(empty)".into());
    let out = codes
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if vis.is_some_and(|v| !v[i]) {
                u32::MAX
            } else if c == NULL_CODE {
                empty
            } else {
                c
            }
        })
        .collect();
    Ok((out, labels))
}

pub fn crosstab(g: &Graph, rows_col: &str, cols_col: &str, dir: Dir, vis: Option<&[bool]>) -> Result<XTab, String> {
    let (row_of, mut rows) = codes_for(g, rows_col, vis)?;
    let (col_of, mut cols) = codes_for(g, cols_col, vis)?;
    let mut x = XTab { rows_name: rows_col.into(), cols_name: cols_col.into(), rows: rows.clone(), cols: cols.clone(), counts: vec![0; rows.len() * cols.len()], n_edges: 0, row_of, col_of, dir };
    let nc = cols.len();
    for e in 0..g.edges.len() {
        let cells = x.cells_of(g, e);
        let mut any = false;
        for (r, c) in cells.into_iter().flatten() {
            x.counts[r * nc + c] += 1;
            any = true;
        }
        x.n_edges += any as usize;
    }
    // the "(empty)" row / column only stays if something was counted in it
    let last_r = rows.len() - 1;
    let last_c = cols.len() - 1;
    let row_used = (0..cols.len()).any(|c| x.counts[last_r * nc + c] > 0);
    let col_used = (0..rows.len()).any(|r| x.counts[r * nc + last_c] > 0);
    let keep_r = if row_used { rows.len() } else { rows.len() - 1 };
    let keep_c = if col_used { cols.len() } else { cols.len() - 1 };
    if keep_r != rows.len() || keep_c != cols.len() {
        let mut counts = Vec::with_capacity(keep_r * keep_c);
        for r in 0..keep_r {
            for c in 0..keep_c {
                counts.push(x.counts[r * nc + c]);
            }
        }
        x.counts = counts;
        rows.truncate(keep_r);
        cols.truncate(keep_c);
        x.rows = rows;
        x.cols = cols;
    }
    Ok(x)
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::*;
    use crate::model::Session;

    /// Rooms r1 r2 r3, sensors s1 s2, door d1, loner l (no connections).
    /// r1→s1 (directed), s2→r3 (directed), r2–d1 and r1–r2 (undirected), s1→d1 (directed).
    fn session() -> Session {
        let e = parse_csv_text("source,target,direction\nr1,s1,directed\ns2,r3,directed\nr2,d1,undirected\nr1,r2,undirected\ns1,d1,directed\n").unwrap();
        let n = parse_csv_text("id,Type,Level,Name\nr1,Room,1,Hall\nr2,Room,1,Kitchen\nr3,Room,2,Study\ns1,Sensor,1,\ns2,Sensor,2,Temp\nd1,Door,1,\nl,Room,3,Attic\n").unwrap();
        let mut s = Session::new();
        let em = auto_edge_mapping(&e);
        assert!(em.dir_col.is_some(), "the direction column is found");
        s.set_edges("e".into(), e, em);
        let nm = auto_node_mapping(&n);
        s.set_nodes("n".into(), n, nm);
        s.rebuild();
        s
    }

    fn cond(col: &str, op: Op, value: &str) -> Cond {
        Cond { col: Some(col.into()), op, value: value.into() }
    }

    fn q(a: Cond, b: Cond, dir: Dir) -> QuerySettings {
        QuerySettings { a, b, dir, ..Default::default() }
    }

    fn ids(s: &Session, v: &[usize]) -> Vec<String> {
        let mut r: Vec<String> = v.iter().map(|&i| s.g.ids[i].clone()).collect();
        r.sort();
        r
    }

    #[test]
    fn conditions_become_filter_expressions() {
        assert_eq!(cond("Type", Op::Is, "Room").expr_text().unwrap(), "`Type` == \"Room\"");
        assert_eq!(cond("Level", Op::Ge, "2").expr_text().unwrap(), "`Level` >= \"2\"");
        assert_eq!(cond("Name", Op::Empty, "").expr_text().unwrap(), "`Name` == null");
        assert_eq!(cond("Name", Op::NotEmpty, "").expr_text().unwrap(), "`Name` != null");
        assert_eq!(cond("Name", Op::Contains, "it\"y").expr_text().unwrap(), "`Name` contains 'it\"y'");
        assert!(cond("Name", Op::Is, "").expr_text().is_err(), "a value is needed");
        assert!(cond("Name", Op::Is, "a\"b'c").expr_text().is_err());
        assert_eq!(Cond::default().expr_text().unwrap(), "");
    }

    #[test]
    fn conditions_select_the_right_nodes() {
        let s = session();
        let m = |c: Cond| ids(&s, &(0..s.g.n()).filter(|&i| c.matching(&s.g, None).unwrap()[i]).collect::<Vec<_>>());
        assert_eq!(m(cond("Type", Op::Is, "Room")), vec!["l", "r1", "r2", "r3"]);
        assert_eq!(m(cond("Type", Op::IsNot, "Room")), vec!["d1", "s1", "s2"]);
        assert_eq!(m(cond("Level", Op::Ge, "2")), vec!["l", "r3", "s2"]);
        assert_eq!(m(cond("Name", Op::Empty, "")), vec!["d1", "s1"]);
        assert_eq!(m(cond("Name", Op::Contains, "TEM")), vec!["s2"], "contains ignores case");
        assert_eq!(m(cond("id", Op::StartsWith, "r")), vec!["r1", "r2", "r3"]);
        assert_eq!(m(Cond::default()).len(), 7);
        assert!(cond("nosuch", Op::Is, "x").matching(&s.g, None).is_err());
    }

    #[test]
    fn counts_rooms_that_reach_a_sensor_in_either_direction() {
        let s = session();
        let qs = q(cond("Type", Op::Is, "Room"), cond("Type", Op::Is, "Sensor"), Dir::Either);
        let i = run(&s.g, &qs, &s.vis).unwrap();
        // r1–s1 and s2–r3 connect a room to a sensor; r2 and the loner do not
        assert_eq!((i.n_a, i.n_b, i.n_a_conn, i.n_a_none(), i.n_b_reached, i.n_edges), (4, 2, 2, 2, 2, 2));
        assert_eq!(ids(&s, &i.connected()), vec!["r1", "r3"]);
        assert_eq!(ids(&s, &i.unconnected()), vec!["l", "r2"]);
        assert_eq!(ids(&s, &i.reached()), vec!["s1", "s2"]);
        let (first, second) = sentences(&s.g, &qs, &i);
        assert_eq!(first, "2 of 4 Room nodes connect to Sensor nodes.");
        assert_eq!(second.as_deref(), Some("2 have no connection to Sensor nodes."));
    }

    #[test]
    fn direction_follows_each_edge() {
        let s = session();
        let rooms = cond("Type", Op::Is, "Room");
        let sensors = cond("Type", Op::Is, "Sensor");
        // out of a room into a sensor: only r1→s1 (s2→r3 points the other way)
        let out = run(&s.g, &q(rooms.clone(), sensors.clone(), Dir::Out), &s.vis).unwrap();
        assert_eq!(ids(&s, &out.connected()), vec!["r1"]);
        // into a room from a sensor: only s2→r3
        let inn = run(&s.g, &q(rooms.clone(), sensors.clone(), Dir::In), &s.vis).unwrap();
        assert_eq!(ids(&s, &inn.connected()), vec!["r3"]);
        assert_eq!(ids(&s, &inn.reached()), vec!["s2"]);
        // undirected edges count both ways: r2–d1 is found with out and with in
        let doors = cond("Type", Op::Is, "Door");
        for d in [Dir::Out, Dir::In, Dir::Either] {
            let r = run(&s.g, &q(rooms.clone(), doors.clone(), d), &s.vis).unwrap();
            assert_eq!(ids(&s, &r.connected()), vec!["r2"], "{d:?}");
        }
        // s1→d1 is directed: sensors reach the door going out, not coming in
        let so = run(&s.g, &q(sensors.clone(), doors.clone(), Dir::Out), &s.vis).unwrap();
        let si = run(&s.g, &q(sensors, doors, Dir::In), &s.vis).unwrap();
        assert_eq!((so.n_a_conn, si.n_a_conn), (1, 0));
        let (first, _) = sentences(&s.g, &q(rooms.clone(), cond("Type", Op::Is, "Sensor"), Dir::In), &inn);
        assert_eq!(first, "1 of 4 Room nodes have an incoming connection from Sensor nodes.");
    }

    #[test]
    fn a_group_can_connect_to_itself_and_to_any_node() {
        let s = session();
        // rooms that connect to another room: r1–r2 (undirected)
        let rr = run(&s.g, &q(cond("Type", Op::Is, "Room"), cond("Type", Op::Is, "Room"), Dir::Either), &s.vis).unwrap();
        assert_eq!(ids(&s, &rr.connected()), vec!["r1", "r2"]);
        assert_eq!(rr.n_edges, 1);
        // no second condition: any connection at all; the loner has none
        let any = run(&s.g, &q(cond("Type", Op::Is, "Room"), Cond::default(), Dir::Either), &s.vis).unwrap();
        assert_eq!(ids(&s, &any.unconnected()), vec!["l"]);
        assert_eq!(any.n_a_conn, 3);
        let (first, _) = sentences(&s.g, &q(cond("Type", Op::Is, "Room"), Cond::default(), Dir::Either), &any);
        assert_eq!(first, "3 of 4 Room nodes connect to other nodes.");
        // a node does not count as connected to itself
        let me = run(&s.g, &q(cond("id", Op::Is, "l"), cond("id", Op::Is, "l"), Dir::Either), &s.vis).unwrap();
        assert_eq!(me.n_a_conn, 0);
    }

    #[test]
    fn only_visible_nodes_count_when_asked() {
        let mut s = session();
        s.filter.expr_text = "id != \"s1\"".into();
        s.recompute_visibility();
        let mut qs = q(cond("Type", Op::Is, "Room"), cond("Type", Op::Is, "Sensor"), Dir::Either);
        let seen = run(&s.g, &qs, &s.vis).unwrap();
        assert_eq!(ids(&s, &seen.connected()), vec!["r3"], "s1 is filtered out, so r1 loses its sensor");
        assert_eq!(seen.n_b, 1);
        qs.visible_only = false;
        let all = run(&s.g, &qs, &s.vis).unwrap();
        assert_eq!(ids(&s, &all.connected()), vec!["r1", "r3"]);
    }

    #[test]
    fn mistakes_are_explained_and_empty_groups_are_said_so() {
        let s = session();
        let e = run(&s.g, &q(cond("Type", Op::Is, ""), Cond::default(), Dir::Either), &s.vis).unwrap_err();
        assert!(e.starts_with("First condition"), "{e}");
        let e = run(&s.g, &q(Cond::default(), cond("zzz", Op::Is, "1"), Dir::Either), &s.vis).unwrap_err();
        assert!(e.starts_with("Second condition") && e.contains("unknown column"), "{e}");
        let qs = q(cond("Type", Op::Is, "Window"), Cond::default(), Dir::Either);
        let i = run(&s.g, &qs, &s.vis).unwrap();
        assert_eq!(sentences(&s.g, &qs, &i).0, "No node meets the first condition.");
    }

    #[test]
    fn crosstab_counts_connections_per_pair_of_values() {
        let s = session();
        // Type against Type, either way round: each edge sits in the cell of each reading (once if they coincide)
        let x = crosstab(&s.g, "Type", "Type", Dir::Either, None).unwrap();
        let at = |a: &str, b: &str| x.count(x.rows.iter().position(|l| l == a).unwrap(), x.cols.iter().position(|l| l == b).unwrap());
        assert_eq!((at("Room", "Sensor"), at("Sensor", "Room")), (2, 2));
        assert_eq!(at("Room", "Room"), 1, "r1–r2 is read both ways but is one connection in one cell");
        assert_eq!((at("Room", "Door"), at("Door", "Room")), (1, 1));
        assert_eq!((at("Sensor", "Door"), at("Door", "Sensor")), (1, 1));
        assert_eq!(x.n_edges, 5);
        assert_eq!(x.rows, x.cols);
        assert!(!x.rows.contains(&"(empty)".to_string()), "nothing is counted under an empty value");
        // directed reading: from a row value to a column value
        let out = crosstab(&s.g, "Type", "Type", Dir::Out, None).unwrap();
        let at = |a: &str, b: &str| out.count(out.rows.iter().position(|l| l == a).unwrap(), out.cols.iter().position(|l| l == b).unwrap());
        assert_eq!((at("Room", "Sensor"), at("Sensor", "Room")), (1, 1), "r1→s1 and s2→r3");
        assert_eq!((at("Sensor", "Door"), at("Door", "Sensor")), (1, 0), "s1→d1 is directed; nothing leaves a door that way");
        assert_eq!((at("Room", "Door"), at("Door", "Room")), (1, 1), "r2–d1 is undirected: both ways");
    }

    #[test]
    fn crosstab_between_two_attributes_and_cell_selection() {
        let s = session();
        let x = crosstab(&s.g, "Type", "Level", Dir::Either, None).unwrap();
        let rr = x.rows.iter().position(|l| l == "Room").unwrap();
        let c1 = x.cols.iter().position(|l| l == "1").unwrap();
        let c2 = x.cols.iter().position(|l| l == "2").unwrap();
        // Room–Level 1: r1–s1 (s1 is level 1), r1–r2 (r2 is level 1, read from r1), r2–d1 (d1 is level 1), and r1–r2 read from r2
        assert_eq!(x.count(rr, c1), 3);
        assert_eq!(x.count(rr, c2), 1, "s2 → r3: r3 is the Room end, s2 is level 2");
        assert_eq!(ids(&s, &x.cell_nodes(&s.g, rr, c2)), vec!["r3", "s2"]);
        assert_eq!(ids(&s, &x.cell_nodes(&s.g, rr, c1)), vec!["d1", "r1", "r2", "s1"]);
        // too many values are refused with an explanation
        let mut rows = String::from("id,v\n");
        let mut edges = String::from("source,target\n");
        for k in 0..60 {
            rows += &format!("n{k},{k}\n");
            edges += &format!("n{k},n{}\n", (k + 1) % 60);
        }
        let (et, nt) = (parse_csv_text(&edges).unwrap(), parse_csv_text(&rows).unwrap());
        let mut big = Session::new();
        let em = auto_edge_mapping(&et);
        big.set_edges("e".into(), et, em);
        let nm = auto_node_mapping(&nt);
        big.set_nodes("n".into(), nt, nm);
        big.rebuild();
        let err = crosstab(&big.g, "v", "v", Dir::Either, None).unwrap_err();
        assert!(err.contains("60 different values"), "{err}");
    }

    #[test]
    fn crosstab_keeps_an_empty_row_only_when_something_is_counted_there() {
        let s = session();
        // Name is empty for s1 and d1, which have connections
        let x = crosstab(&s.g, "Name", "Type", Dir::Either, None).unwrap();
        assert_eq!(x.rows.last().map(String::as_str), Some("(empty)"));
        assert!(x.counts[(x.rows.len() - 1) * x.cols.len()..].iter().sum::<usize>() > 0);
    }

    #[test]
    fn the_session_keeps_the_result_up_to_date() {
        let mut s = session();
        assert!(s.query_info.is_none());
        s.query = q(cond("Type", Op::Is, "Room"), cond("Type", Op::Is, "Sensor"), Dir::Either);
        s.recompute_query();
        assert_eq!(s.query_info.as_ref().unwrap().n_a_conn, 2);
        // a filter change refreshes it
        s.filter.expr_text = "id != \"s1\"".into();
        s.recompute_visibility();
        assert_eq!(s.query_info.as_ref().unwrap().n_a_conn, 1);
        // a mistake keeps no stale result and says what is wrong
        s.query.a.value.clear();
        s.recompute_query();
        assert!(s.query_info.is_none() && s.query_error.as_deref().is_some_and(|e| e.starts_with("First condition")));
        // cross-tab mode
        s.query = QuerySettings { mode: QueryMode::CrossTab, rows: Some("Type".into()), cols: Some("Level".into()), ..Default::default() };
        s.recompute_query();
        assert!(s.xtab.is_some() && s.query_info.is_none());
        // selecting nodes from a result
        s.select_nodes([1usize, 2]);
        assert_eq!(s.sel_count, 2);
        s.select_nodes([3usize]);
        assert_eq!((s.sel_count, s.sel[3], s.sel[1]), (1, true, false));
    }

    #[test]
    fn settings_round_trip_and_old_sessions_still_load() {
        let qs = q(cond("Type", Op::Is, "Room"), cond("Level", Op::Gt, "1"), Dir::Out);
        let text = serde_json::to_string(&qs).unwrap();
        let back: QuerySettings = serde_json::from_str(&text).unwrap();
        assert_eq!((back.dir, back.b.op, back.a.col.as_deref()), (Dir::Out, Op::Gt, Some("Type")));
        let empty: QuerySettings = serde_json::from_str("{}").unwrap();
        assert!(empty.visible_only && !empty.is_set());
    }
}
