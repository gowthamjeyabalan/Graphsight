//! Graph construction from raw tables, adjacency structures and metrics.

use crate::data::*;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct BuildStats {
    pub duplicate_node_rows: usize,
    pub skipped_edge_rows: usize,
    pub self_loops: usize,
    pub nodes_from_edges_only: usize,
    pub parallel_edges: usize,
    /// Rows whose direction column holds something other than directed/undirected.
    pub unknown_direction: usize,
}

pub struct Graph {
    pub ids: Vec<String>,
    pub index: HashMap<String, u32>,
    pub edges: Vec<(u32, u32)>,
    pub edge_cols: Vec<Column>,
    pub node_cols: Vec<Column>,
    /// True if at least one edge is directed (arrows, in/out degree, directed layouts).
    pub directed: bool,
    /// Per edge: directed (source → target) or undirected.
    pub edge_dir: Vec<bool>,
    /// Index into `edge_cols` of the mapped weight column.
    pub weight_col: Option<usize>,
    /// Undirected, de-duplicated adjacency without self loops (CSR).
    adj_off: Vec<u32>,
    adj: Vec<u32>,
    pub stats: BuildStats,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            ids: vec![],
            index: HashMap::new(),
            edges: vec![],
            edge_cols: vec![],
            node_cols: vec![],
            directed: false,
            edge_dir: vec![],
            weight_col: None,
            adj_off: vec![0],
            adj: vec![],
            stats: BuildStats::default(),
        }
    }
}

impl Graph {
    pub fn n(&self) -> usize {
        self.ids.len()
    }

    #[inline]
    pub fn neighbors(&self, i: usize) -> &[u32] {
        &self.adj[self.adj_off[i] as usize..self.adj_off[i + 1] as usize]
    }

    #[inline]
    pub fn degree(&self, i: usize) -> usize {
        (self.adj_off[i + 1] - self.adj_off[i]) as usize
    }

    #[inline]
    pub fn edge_directed(&self, e: usize) -> bool {
        self.edge_dir.get(e).copied().unwrap_or(self.directed)
    }

    pub fn node_col(&self, name: &str) -> Option<&Column> {
        self.node_cols.iter().find(|c| c.name == name)
    }

    pub fn edge_col(&self, name: &str) -> Option<&Column> {
        self.edge_cols.iter().find(|c| c.name == name)
    }

    /// Edge weights (1.0 where missing or non-numeric); None if no weight column was mapped.
    pub fn weights(&self) -> Option<Vec<f32>> {
        let c = &self.edge_cols[self.weight_col?];
        Some((0..self.edges.len()).map(|e| c.num(e).map(|x| x as f32).unwrap_or(1.0)).collect())
    }

    fn build_csr(&mut self) {
        let n = self.n();
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); n];
        for &(s, t) in &self.edges {
            if s != t {
                lists[s as usize].push(t);
                lists[t as usize].push(s);
            }
        }
        let mut off = Vec::with_capacity(n + 1);
        let mut adj = Vec::new();
        off.push(0u32);
        for l in &mut lists {
            l.sort_unstable();
            l.dedup();
            adj.extend_from_slice(l);
            off.push(adj.len() as u32);
        }
        self.adj_off = off;
        self.adj = adj;
    }
}

/// Build a graph from an edge table and/or a node table.
pub fn build_graph(
    edges: Option<(&RawTable, &EdgeMapping)>,
    nodes: Option<(&RawTable, &NodeMapping)>,
) -> Graph {
    let mut g = Graph::default();
    let mut stats = BuildStats::default();
    let mut node_row: Vec<Option<usize>> = Vec::new();

    let intern = |g: &mut Graph, id: &str, row: Option<usize>, node_row: &mut Vec<Option<usize>>| -> (u32, bool) {
        if let Some(&i) = g.index.get(id) {
            return (i, false);
        }
        let i = g.ids.len() as u32;
        g.ids.push(id.to_string());
        g.index.insert(id.to_string(), i);
        node_row.push(row);
        (i, true)
    };

    if let Some((t, m)) = nodes {
        for (ri, row) in t.rows.iter().enumerate() {
            let id = row[m.id].trim();
            if id.is_empty() {
                continue;
            }
            let (_, fresh) = intern(&mut g, id, Some(ri), &mut node_row);
            if !fresh {
                stats.duplicate_node_rows += 1;
            }
        }
    }

    let mut kept_edge_rows: Vec<usize> = Vec::new();
    if let Some((t, m)) = edges {
        for (ri, row) in t.rows.iter().enumerate() {
            let (s, d) = (row[m.src].trim(), row[m.dst].trim());
            if s.is_empty() || d.is_empty() {
                stats.skipped_edge_rows += 1;
                continue;
            }
            let (si, f1) = intern(&mut g, s, None, &mut node_row);
            let (di, f2) = intern(&mut g, d, None, &mut node_row);
            if nodes.is_some() {
                stats.nodes_from_edges_only += f1 as usize + f2 as usize;
            }
            if si == di {
                stats.self_loops += 1;
            }
            let dir = match m.dir_col.and_then(|c| row.get(c)) {
                Some(v) => match parse_direction(v) {
                    Some(d) => d,
                    None => {
                        if !v.trim().is_empty() {
                            stats.unknown_direction += 1;
                        }
                        m.directed
                    }
                },
                None => m.directed,
            };
            g.edges.push((si, di));
            g.edge_dir.push(dir);
            kept_edge_rows.push(ri);
        }
        g.directed = g.edge_dir.iter().any(|d| *d);
        // parallel edges
        let mut seen = std::collections::HashSet::with_capacity(g.edges.len());
        for (e, &(s, t)) in g.edges.iter().enumerate() {
            let key = if g.edge_dir[e] { (s, t) } else { (s.min(t), s.max(t)) };
            if !seen.insert(key) {
                stats.parallel_edges += 1;
            }
        }
        // edge attribute columns
        for (ci, name) in t.headers.iter().enumerate() {
            if ci == m.src || ci == m.dst {
                continue;
            }
            let vals: Vec<&str> = kept_edge_rows.iter().map(|&r| t.rows.cell(r, ci)).collect();
            let mut col = Column::from_strings(name, &vals);
            if Some(ci) == m.weight && !col.is_numeric() {
                // force numeric for the weight column
                let nums = vals.iter().map(|s| s.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
                col = Column::numeric(name, nums);
            }
            if Some(ci) == m.weight {
                g.weight_col = Some(g.edge_cols.len());
            }
            g.edge_cols.push(col);
        }
    }

    // node attribute columns
    if let Some((t, m)) = nodes {
        for (ci, name) in t.headers.iter().enumerate() {
            if ci == m.id {
                continue;
            }
            let vals: Vec<&str> = node_row
                .iter()
                .map(|r| r.map(|r| t.rows.cell(r, ci)).unwrap_or(""))
                .collect();
            g.node_cols.push(Column::from_strings(name, &vals));
        }
    }

    g.stats = stats;
    g.build_csr();
    compute_metrics(&mut g);
    g
}

// ───────────────────────────── metrics ─────────────────────────────

fn relabel_by_size(labels: &[u32]) -> Vec<f64> {
    let mut count: HashMap<u32, usize> = HashMap::new();
    for &l in labels {
        *count.entry(l).or_default() += 1;
    }
    let mut order: Vec<(u32, usize)> = count.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let rank: HashMap<u32, usize> = order.iter().enumerate().map(|(i, (l, _))| (*l, i)).collect();
    labels.iter().map(|l| rank[l] as f64).collect()
}

pub fn components(g: &Graph) -> Vec<u32> {
    let n = g.n();
    let mut comp = vec![u32::MAX; n];
    let mut stack = Vec::new();
    let mut next = 0;
    for s in 0..n {
        if comp[s] != u32::MAX {
            continue;
        }
        comp[s] = next;
        stack.push(s as u32);
        while let Some(u) = stack.pop() {
            for &v in g.neighbors(u as usize) {
                if comp[v as usize] == u32::MAX {
                    comp[v as usize] = next;
                    stack.push(v);
                }
            }
        }
        next += 1;
    }
    comp
}

fn pagerank(g: &Graph) -> Vec<f64> {
    let n = g.n();
    if n == 0 {
        return vec![];
    }
    // directed: follow edge direction; undirected: both ways (counts parallel edges)
    let mut out_off = vec![0usize; n + 1];
    let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(g.edges.len() * 2);
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s == t {
            continue;
        }
        pairs.push((s, t));
        if !g.edge_directed(e) {
            pairs.push((t, s));
        }
    }
    for &(s, _) in &pairs {
        out_off[s as usize + 1] += 1;
    }
    for i in 0..n {
        out_off[i + 1] += out_off[i];
    }
    let mut fill = out_off.clone();
    let mut targets = vec![0u32; pairs.len()];
    for &(s, t) in &pairs {
        targets[fill[s as usize]] = t;
        fill[s as usize] += 1;
    }
    let d = 0.85;
    let mut pr = vec![1.0 / n as f64; n];
    let mut next = vec![0.0; n];
    for _ in 0..100 {
        let mut dangling = 0.0;
        next.iter_mut().for_each(|x| *x = 0.0);
        for u in 0..n {
            let (a, b) = (out_off[u], out_off[u + 1]);
            if a == b {
                dangling += pr[u];
            } else {
                let share = pr[u] / (b - a) as f64;
                for &v in &targets[a..b] {
                    next[v as usize] += share;
                }
            }
        }
        let base = (1.0 - d) / n as f64 + d * dangling / n as f64;
        let mut diff = 0.0;
        for u in 0..n {
            let v = base + d * next[u];
            diff += (v - pr[u]).abs();
            pr[u] = v;
        }
        if diff < 1e-10 {
            break;
        }
    }
    pr
}

fn kcore(g: &Graph) -> Vec<f64> {
    let n = g.n();
    let mut deg: Vec<usize> = (0..n).map(|i| g.degree(i)).collect();
    let maxd = deg.iter().copied().max().unwrap_or(0);
    let mut bins = vec![0usize; maxd + 2];
    for &d in &deg {
        bins[d] += 1;
    }
    let mut start = 0;
    for b in bins.iter_mut() {
        let c = *b;
        *b = start;
        start += c;
    }
    let mut pos = vec![0usize; n];
    let mut vert = vec![0usize; n];
    for v in 0..n {
        pos[v] = bins[deg[v]];
        vert[pos[v]] = v;
        bins[deg[v]] += 1;
    }
    for d in (1..=maxd + 1).rev() {
        bins[d] = bins[d - 1];
    }
    bins[0] = 0;
    for i in 0..n {
        let v = vert[i];
        for &u in g.neighbors(v) {
            let u = u as usize;
            if deg[u] > deg[v] {
                let du = deg[u];
                let pu = pos[u];
                let pw = bins[du];
                let w = vert[pw];
                if u != w {
                    pos[u] = pw;
                    pos[w] = pu;
                    vert[pu] = w;
                    vert[pw] = u;
                }
                bins[du] += 1;
                deg[u] -= 1;
            }
        }
    }
    deg.iter().map(|&d| d as f64).collect()
}

/// One Louvain level: greedy local moving of nodes between communities. Returns (community per node, moved?).
fn louvain_level(adj: &[Vec<(u32, f64)>], self_w: &[f64]) -> (Vec<u32>, bool) {
    let n = adj.len();
    let k: Vec<f64> = (0..n).map(|i| adj[i].iter().map(|e| e.1).sum::<f64>() + 2.0 * self_w[i]).collect();
    let total: f64 = k.iter().sum();
    let mut comm: Vec<u32> = (0..n as u32).collect();
    if total <= 0.0 {
        return (comm, false);
    }
    let mut tot = k.clone();
    let mut neigh = vec![0.0f64; n];
    let mut touched: Vec<u32> = Vec::new();
    let mut moved_any = false;
    for _pass in 0..30 {
        let mut moved = 0;
        for i in 0..n {
            let ci = comm[i] as usize;
            touched.clear();
            for &(j, w) in &adj[i] {
                let cj = comm[j as usize] as usize;
                if neigh[cj] == 0.0 {
                    touched.push(cj as u32);
                }
                neigh[cj] += w;
            }
            tot[ci] -= k[i];
            let mut best = ci;
            let mut best_gain = neigh[ci] - tot[ci] * k[i] / total;
            for &c in &touched {
                let c = c as usize;
                let gain = neigh[c] - tot[c] * k[i] / total;
                if gain > best_gain + 1e-12 {
                    best_gain = gain;
                    best = c;
                }
            }
            tot[best] += k[i];
            for &c in &touched {
                neigh[c as usize] = 0.0;
            }
            neigh[ci] = 0.0;
            if best != ci {
                comm[i] = best as u32;
                moved += 1;
            }
        }
        if moved == 0 {
            break;
        }
        moved_any = true;
    }
    (comm, moved_any)
}

/// Louvain modularity optimisation (multi-level, deterministic).
fn communities(g: &Graph) -> Vec<u32> {
    let n = g.n();
    let mut adj: Vec<Vec<(u32, f64)>> = {
        let mut m: Vec<HashMap<u32, f64>> = vec![HashMap::new(); n];
        for &(s, t) in &g.edges {
            if s != t {
                *m[s as usize].entry(t).or_default() += 1.0;
                *m[t as usize].entry(s).or_default() += 1.0;
            }
        }
        m.into_iter()
            .map(|h| {
                let mut v: Vec<(u32, f64)> = h.into_iter().collect();
                v.sort_by_key(|e| e.0);
                v
            })
            .collect()
    };
    let mut self_w = vec![0.0f64; n];
    let mut assign: Vec<u32> = (0..n as u32).collect();
    for _level in 0..12 {
        let (comm, moved) = louvain_level(&adj, &self_w);
        if !moved {
            break;
        }
        // compact community ids
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let comm: Vec<u32> = comm
            .iter()
            .map(|&c| {
                let next = remap.len() as u32;
                *remap.entry(c).or_insert(next)
            })
            .collect();
        let kc = remap.len();
        for a in assign.iter_mut() {
            *a = comm[*a as usize];
        }
        // aggregate
        let mut new_self = vec![0.0f64; kc];
        let mut maps: Vec<HashMap<u32, f64>> = vec![HashMap::new(); kc];
        for i in 0..adj.len() {
            let ci = comm[i] as usize;
            new_self[ci] += self_w[i];
            for &(j, w) in &adj[i] {
                let cj = comm[j as usize] as usize;
                if ci == cj {
                    new_self[ci] += w * 0.5;
                } else {
                    *maps[ci].entry(cj as u32).or_default() += w;
                }
            }
        }
        adj = maps
            .into_iter()
            .map(|h| {
                let mut v: Vec<(u32, f64)> = h.into_iter().collect();
                v.sort_by_key(|e| e.0);
                v
            })
            .collect();
        self_w = new_self;
        if adj.len() <= 1 {
            break;
        }
    }
    assign
}

pub fn compute_metrics(g: &mut Graph) {
    g.node_cols.retain(|c| !c.is_metric);
    let n = g.n();
    if n == 0 {
        return;
    }
    let mut deg = vec![0f64; n];
    let mut indeg = vec![0f64; n];
    let mut outdeg = vec![0f64; n];
    for (e, &(s, t)) in g.edges.iter().enumerate() {
        if s == t {
            continue;
        }
        deg[s as usize] += 1.0;
        deg[t as usize] += 1.0;
        outdeg[s as usize] += 1.0;
        indeg[t as usize] += 1.0;
        if !g.edge_directed(e) {
            // an undirected edge leaves and enters both of its ends
            outdeg[t as usize] += 1.0;
            indeg[s as usize] += 1.0;
        }
    }
    let add = |g: &mut Graph, name: &str, v: Vec<f64>, hint_cat: bool| {
        let mut c = Column::numeric(name, v);
        c.is_metric = true;
        c.hint_cat = hint_cat;
        g.node_cols.push(c);
    };
    add(g, "_degree", deg, false);
    if g.directed {
        add(g, "_in_degree", indeg, false);
        add(g, "_out_degree", outdeg, false);
    }
    let pr = pagerank(g);
    add(g, "_pagerank", pr, false);
    let kc = kcore(g);
    add(g, "_kcore", kc, false);
    let comp = components(g);
    add(g, "_component", relabel_by_size(&comp), true);
    let com = communities(g);
    add(g, "_community", relabel_by_size(&com), true);
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn tiny(directed: bool) -> Graph {
        let e = parse_csv_text("source,target,weight\na,b,1\nb,c,2\nc,a,1\nc,d,5\nx,y,1\na,b,1\nq,q,1\n,z,1\n").unwrap();
        let n = parse_csv_text("id,group,score\na,g1,1.5\nb,g1,2\nc,g2,\nd,g2,4\nloner,g3,9\n").unwrap();
        let mut em = auto_edge_mapping(&e);
        em.directed = directed;
        build_graph(Some((&e, &em)), Some((&n, &auto_node_mapping(&n))))
    }

    #[test]
    fn builds_union_of_nodes() {
        let g = tiny(false);
        // a b c d loner x y q  (empty-id edge skipped)
        assert_eq!(g.n(), 8);
        assert_eq!(g.stats.skipped_edge_rows, 1);
        assert_eq!(g.stats.self_loops, 1);
        assert_eq!(g.stats.parallel_edges, 1);
        let score = g.node_col("score").unwrap();
        assert_eq!(score.num(g.index["d"] as usize), Some(4.0));
        assert_eq!(score.num(g.index["x"] as usize), None);
        assert!(g.weights().is_some());
    }

    #[test]
    fn metrics_sane() {
        let g = tiny(false);
        let pr = g.node_col("_pagerank").unwrap();
        let sum: f64 = (0..g.n()).map(|i| pr.num(i).unwrap()).sum();
        assert!((sum - 1.0).abs() < 1e-6, "pagerank sums to {sum}");
        let comp = g.node_col("_component").unwrap();
        // main component {a,b,c,d} is largest → id 0
        assert_eq!(comp.num(g.index["a"] as usize), Some(0.0));
        assert_eq!(comp.num(g.index["b"] as usize), Some(0.0));
        assert_ne!(comp.num(g.index["x"] as usize), Some(0.0));
        let deg = g.node_col("_degree").unwrap();
        assert_eq!(deg.num(g.index["c"] as usize), Some(3.0)); // b, a, d
        let kc = g.node_col("_kcore").unwrap();
        assert_eq!(kc.num(g.index["a"] as usize), Some(2.0)); // triangle a-b-c
        assert_eq!(kc.num(g.index["d"] as usize), Some(1.0));
    }

    #[test]
    fn directed_metrics() {
        let g = tiny(true);
        assert!(g.node_col("_in_degree").is_some());
        let ind = g.node_col("_in_degree").unwrap();
        assert_eq!(ind.num(g.index["b"] as usize), Some(2.0)); // a->b twice
    }

    #[test]
    fn communities_find_two_cliques() {
        // two 5-cliques joined by one bridge
        let mut s = String::from("source,target\n");
        for off in [0, 5] {
            for i in 0..5 {
                for j in (i + 1)..5 {
                    s += &format!("{},{}\n", off + i, off + j);
                }
            }
        }
        s += "0,5\n";
        let t = parse_csv_text(&s).unwrap();
        let g = build_graph(Some((&t, &auto_edge_mapping(&t))), None);
        let c = g.node_col("_community").unwrap();
        let a = c.num(g.index["1"] as usize);
        let b = c.num(g.index["6"] as usize);
        assert_eq!(c.num(g.index["2"] as usize), a);
        assert_eq!(c.num(g.index["7"] as usize), b);
        assert_ne!(a, b);
    }

    #[test]
    fn per_edge_direction_column() {
        let e = parse_csv_text("source,target,direction\na,b,directed\nb,c,undirected\nc,d,Directed\nd,a,\ne,a,sideways\nf,g,directed\ng,h,undirected\n").unwrap();
        let m = auto_edge_mapping(&e);
        assert_eq!(m.dir_col, Some(2), "found by its name although one value is a typo");
        let g = build_graph(Some((&e, &m)), None);
        assert_eq!(g.edge_dir, vec![true, false, true, false, false, true, false]);
        assert!(g.directed);
        assert_eq!(g.stats.unknown_direction, 1, "empty is silently the default, 'sideways' is reported");
        // degrees: b has in 1 (a→b) + undirected b-c counts both ways
        let ind = g.node_col("_in_degree").unwrap();
        let outd = g.node_col("_out_degree").unwrap();
        let (b, c) = (g.index["b"] as usize, g.index["c"] as usize);
        assert_eq!((ind.num(b), outd.num(b)), (Some(2.0), Some(1.0)));
        assert_eq!((ind.num(c), outd.num(c)), (Some(1.0), Some(2.0)));
        let mut all_undirected = m.clone();
        all_undirected.dir_col = None;
        assert!(!build_graph(Some((&e, &all_undirected)), None).directed);
    }

    #[test]
    fn direction_column_is_found_by_its_words_even_with_another_name() {
        let e = parse_csv_text("source,target,kind\na,b,directed\nb,c,undirected\n").unwrap();
        assert_eq!(auto_edge_mapping(&e).dir_col, Some(2));
        let plain = parse_csv_text("source,target,kind\na,b,family\nb,c,work\n").unwrap();
        assert_eq!(auto_edge_mapping(&plain).dir_col, None);
        let flags = parse_csv_text("source,target,mark\na,b,1\nb,c,0\n").unwrap();
        assert_eq!(auto_edge_mapping(&flags).dir_col, None, "a bare 0/1 column is not guessed");
    }
}
