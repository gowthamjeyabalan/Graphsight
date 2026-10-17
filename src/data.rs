//! Raw tabular data, typed columns, CSV and JSON loading.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::Path;

pub const NULL_CODE: u32 = u32::MAX;

// ───────────────────────────── Raw tables ─────────────────────────────

/// A table of strings exactly as read from disk.
#[derive(Clone, Debug, Default)]
pub struct RawTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn detect_delimiter(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or("");
    let mut best = (b',', 0usize);
    for d in [b',', b';', b'\t', b'|'] {
        let mut in_q = false;
        let mut n = 0;
        for b in first.bytes() {
            if b == b'"' {
                in_q = !in_q;
            } else if b == d && !in_q {
                n += 1;
            }
        }
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// Make header names non-empty and unique.
fn clean_headers(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(raw.len());
    for (i, h) in raw.into_iter().enumerate() {
        let mut h = h.trim().trim_start_matches('\u{feff}').to_string();
        if h.is_empty() {
            h = format!("col{}", i + 1);
        }
        let base = h.clone();
        let mut k = 2;
        while !seen.insert(h.clone()) {
            h = format!("{base}_{k}");
            k += 1;
        }
        out.push(h);
    }
    out
}

pub fn parse_csv_text(text: &str) -> Result<RawTable> {
    let text = text.trim_start_matches('\u{feff}');
    let delim = detect_delimiter(text);
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .has_headers(true)
        .trim(csv::Trim::None)
        .from_reader(text.as_bytes());
    let headers = clean_headers(rdr.headers()?.iter().map(|s| s.to_string()).collect());
    if headers.is_empty() {
        bail!("the file has no header row");
    }
    let ncol = headers.len();
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let mut row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
        row.resize(ncol, String::new());
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        rows.push(row);
    }
    Ok(RawTable { headers, rows })
}

pub fn read_csv(path: &Path) -> Result<RawTable> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    parse_csv_text(&text).with_context(|| format!("parsing {}", path.display()))
}

// ───────────────────────────── Column guessing ─────────────────────────────

const SRC_NAMES: &[&str] = &[
    "source", "src", "from", "start", "u", "node1", "node_1", "source_id", "sourceid", "from_id", "a", "origin",
];
const DST_NAMES: &[&str] = &[
    "target", "dst", "dest", "to", "end", "v", "node2", "node_2", "target_id", "targetid", "to_id", "b", "destination",
];
const WEIGHT_NAMES: &[&str] = &["weight", "w", "value", "strength", "score", "count", "cost"];
const ID_NAMES: &[&str] = &["id", "node", "name", "key", "node_id", "nodeid", "label", "vertex"];

fn find_alias(headers: &[String], names: &[&str]) -> Option<usize> {
    for n in names {
        if let Some(i) = headers.iter().position(|h| h.trim().eq_ignore_ascii_case(n)) {
            return Some(i);
        }
    }
    None
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeMapping {
    pub src: usize,
    pub dst: usize,
    pub weight: Option<usize>,
    pub directed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeMapping {
    pub id: usize,
}

pub fn auto_edge_mapping(t: &RawTable) -> EdgeMapping {
    let src = find_alias(&t.headers, SRC_NAMES).unwrap_or(0);
    let mut dst = find_alias(&t.headers, DST_NAMES).unwrap_or(if t.headers.len() > 1 { 1 } else { 0 });
    if dst == src && t.headers.len() > 1 {
        dst = if src == 0 { 1 } else { 0 };
    }
    let weight = find_alias(&t.headers, WEIGHT_NAMES).filter(|&w| w != src && w != dst);
    EdgeMapping { src, dst, weight, directed: false }
}

pub fn auto_node_mapping(t: &RawTable) -> NodeMapping {
    NodeMapping { id: find_alias(&t.headers, ID_NAMES).unwrap_or(0) }
}

/// Does this table look like an edge list (has recognisable source and target columns)?
pub fn looks_like_edges(t: &RawTable) -> bool {
    find_alias(&t.headers, SRC_NAMES).is_some() && find_alias(&t.headers, DST_NAMES).is_some()
}

// ───────────────────────────── Typed columns ─────────────────────────────

#[derive(Clone, Debug)]
pub enum ColData {
    Num(Vec<f64>), // NaN = missing
    Cat { codes: Vec<u32>, labels: Vec<String> },
}

#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub data: ColData,
    /// Computed by Grapht (degree, pagerank …) rather than loaded from a file.
    pub is_metric: bool,
    /// Numeric column that is better treated as categories (component id, community id).
    pub hint_cat: bool,
}

pub fn is_null_token(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || t.eq_ignore_ascii_case("na")
        || t.eq_ignore_ascii_case("n/a")
        || t.eq_ignore_ascii_case("nan")
        || t.eq_ignore_ascii_case("null")
        || t.eq_ignore_ascii_case("none")
}

/// Total order: numbers (by value) before text (lexicographic).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let pa = a.trim().parse::<f64>().ok().filter(|x| x.is_finite());
    let pb = b.trim().parse::<f64>().ok().filter(|x| x.is_finite());
    match (pa, pb) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal).then_with(|| a.cmp(b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}

pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{:.0}", v);
    }
    let a = v.abs();
    if (1e-3..1e6).contains(&a) {
        let s = format!("{:.4}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        format!("{:.3e}", v)
    }
}

impl Column {
    pub fn numeric(name: &str, vals: Vec<f64>) -> Self {
        Column { name: name.to_string(), data: ColData::Num(vals), is_metric: false, hint_cat: false }
    }

    pub fn len(&self) -> usize {
        match &self.data {
            ColData::Num(v) => v.len(),
            ColData::Cat { codes, .. } => codes.len(),
        }
    }

    /// Infer the type from raw strings: all-numeric → Num, otherwise Cat.
    pub fn from_strings(name: &str, values: &[&str]) -> Self {
        let mut nums = Vec::with_capacity(values.len());
        let mut all_num = true;
        let mut any_val = false;
        for s in values {
            if is_null_token(s) {
                nums.push(f64::NAN);
            } else if let Ok(x) = s.trim().parse::<f64>() {
                if x.is_finite() {
                    nums.push(x);
                    any_val = true;
                } else {
                    all_num = false;
                    break;
                }
            } else {
                all_num = false;
                break;
            }
        }
        if all_num && any_val {
            return Column::numeric(name, nums);
        }
        // categorical
        let mut uniq: Vec<&str> = values.iter().filter(|s| !is_null_token(s)).map(|s| s.trim()).collect();
        uniq.sort_unstable();
        uniq.dedup();
        uniq.sort_by(|a, b| natural_cmp(a, b));
        let map: std::collections::HashMap<&str, u32> =
            uniq.iter().enumerate().map(|(i, s)| (*s, i as u32)).collect();
        let codes = values
            .iter()
            .map(|s| if is_null_token(s) { NULL_CODE } else { map[s.trim()] })
            .collect();
        Column {
            name: name.to_string(),
            data: ColData::Cat { codes, labels: uniq.iter().map(|s| s.to_string()).collect() },
            is_metric: false,
            hint_cat: false,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self.data, ColData::Num(_))
    }

    /// Numeric value (None for missing or categorical).
    #[inline]
    pub fn num(&self, i: usize) -> Option<f64> {
        match &self.data {
            ColData::Num(v) => {
                let x = v[i];
                if x.is_nan() { None } else { Some(x) }
            }
            _ => None,
        }
    }

    /// Category code (None if missing or numeric).
    #[inline]
    pub fn code(&self, i: usize) -> Option<u32> {
        match &self.data {
            ColData::Cat { codes, .. } => {
                let c = codes[i];
                if c == NULL_CODE { None } else { Some(c) }
            }
            _ => None,
        }
    }

    pub fn label_at(&self, i: usize) -> Option<String> {
        match &self.data {
            ColData::Num(v) => {
                let x = v[i];
                if x.is_nan() { None } else { Some(fmt_num(x)) }
            }
            ColData::Cat { codes, labels } => {
                let c = codes[i];
                if c == NULL_CODE { None } else { Some(labels[c as usize].clone()) }
            }
        }
    }

    pub fn cat_str(&self, i: usize) -> Option<&str> {
        match &self.data {
            ColData::Cat { codes, labels } => {
                let c = codes[i];
                if c == NULL_CODE { None } else { Some(labels[c as usize].as_str()) }
            }
            _ => None,
        }
    }

    pub fn num_range(&self) -> Option<(f64, f64)> {
        match &self.data {
            ColData::Num(v) => {
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for &x in v {
                    if !x.is_nan() {
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
                if lo <= hi { Some((lo, hi)) } else { None }
            }
            _ => None,
        }
    }

    /// Value at percentile p (0..1) over non-missing values.
    pub fn percentile(&self, p: f64) -> Option<f64> {
        match &self.data {
            ColData::Num(v) => {
                let mut s: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
                if s.is_empty() {
                    return None;
                }
                s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let idx = ((s.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
                Some(s[idx])
            }
            _ => None,
        }
    }

    /// View any column as categories. Numeric columns use their sorted distinct values.
    pub fn as_categories(&self) -> (Vec<u32>, Vec<String>) {
        match &self.data {
            ColData::Cat { codes, labels } => (codes.clone(), labels.clone()),
            ColData::Num(v) => {
                let mut uniq: Vec<f64> = v.iter().copied().filter(|x| !x.is_nan()).collect();
                uniq.sort_by(|a, b| a.partial_cmp(b).unwrap());
                uniq.dedup();
                let labels: Vec<String> = uniq.iter().map(|&x| fmt_num(x)).collect();
                let codes = v
                    .iter()
                    .map(|&x| {
                        if x.is_nan() {
                            NULL_CODE
                        } else {
                            uniq.binary_search_by(|p| p.partial_cmp(&x).unwrap()).unwrap() as u32
                        }
                    })
                    .collect();
                (codes, labels)
            }
        }
    }

    /// Compare two rows for sorting (missing values always last).
    pub fn cmp_rows(&self, a: usize, b: usize) -> Ordering {
        match &self.data {
            ColData::Num(v) => {
                let (x, y) = (v[a], v[b]);
                match (x.is_nan(), y.is_nan()) {
                    (true, true) => Ordering::Equal,
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    _ => x.partial_cmp(&y).unwrap(),
                }
            }
            ColData::Cat { codes, .. } => {
                // codes are already in natural label order
                let (x, y) = (codes[a], codes[b]);
                x.cmp(&y) // NULL_CODE is u32::MAX so it sorts last
            }
        }
    }
}

// ───────────────────────────── JSON ─────────────────────────────

pub struct JsonGraph {
    pub edges: RawTable,
    pub nodes: Option<RawTable>,
    pub directed: Option<bool>,
    /// Node id → (x, y) when the file carries positions (Cytoscape).
    pub positions: Vec<(String, f32, f32)>,
}

use serde_json::Value;

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn flatten(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&key, x, out);
            }
        }
        Value::Array(a) => {
            if a.iter().all(|x| !x.is_object() && !x.is_array()) {
                out.push((prefix.to_string(), a.iter().map(scalar_to_string).collect::<Vec<_>>().join("; ")));
            } else {
                out.push((prefix.to_string(), v.to_string()));
            }
        }
        other => out.push((prefix.to_string(), scalar_to_string(other))),
    }
}

/// Collect objects (flattened to key/value pairs) into a table with fixed leading columns.
fn objects_to_table(items: &[Vec<(String, String)>], lead: &[&str]) -> RawTable {
    let mut headers: Vec<String> = lead.iter().map(|s| s.to_string()).collect();
    let mut index: std::collections::HashMap<String, usize> =
        headers.iter().enumerate().map(|(i, h)| (h.clone(), i)).collect();
    for it in items {
        for (k, _) in it {
            if !index.contains_key(k) {
                index.insert(k.clone(), headers.len());
                headers.push(k.clone());
            }
        }
    }
    let mut rows = Vec::with_capacity(items.len());
    for it in items {
        let mut row = vec![String::new(); headers.len()];
        for (k, v) in it {
            row[index[k]] = v.clone();
        }
        rows.push(row);
    }
    RawTable { headers, rows }
}

fn endpoint_to_string(v: &Value) -> String {
    match v {
        Value::Object(m) => m.get("id").map(scalar_to_string).unwrap_or_default(),
        other => scalar_to_string(other),
    }
}

fn first_key<'a>(m: &'a serde_json::Map<String, Value>, names: &[&str]) -> Option<(&'a String, &'a Value)> {
    for n in names {
        for (k, v) in m {
            if k.eq_ignore_ascii_case(n) {
                return Some((k, v));
            }
        }
    }
    None
}

const JSON_SRC: &[&str] = &["source", "src", "from", "u", "start"];
const JSON_DST: &[&str] = &["target", "dst", "to", "v", "end"];

/// Turn a list of link objects into an edge table (source, target, + attributes).
fn links_to_edges(links: &[Value]) -> Result<RawTable> {
    let mut items = Vec::with_capacity(links.len());
    for l in links {
        match l {
            Value::Object(m) => {
                let s = first_key(m, JSON_SRC);
                let t = first_key(m, JSON_DST);
                let (Some((sk, sv)), Some((tk, tv))) = (s, t) else { continue };
                let mut kv = vec![("source".to_string(), endpoint_to_string(sv)), ("target".to_string(), endpoint_to_string(tv))];
                for (k, v) in m {
                    if k == sk || k == tk {
                        continue;
                    }
                    flatten(k, v, &mut kv);
                }
                items.push(kv);
            }
            Value::Array(a) if a.len() >= 2 => {
                let mut kv = vec![
                    ("source".to_string(), scalar_to_string(&a[0])),
                    ("target".to_string(), scalar_to_string(&a[1])),
                ];
                if a.len() >= 3 {
                    kv.push(("weight".to_string(), scalar_to_string(&a[2])));
                }
                items.push(kv);
            }
            _ => {}
        }
    }
    if items.is_empty() {
        bail!("no edges with source/target found in the JSON");
    }
    Ok(objects_to_table(&items, &["source", "target"]))
}

fn nodes_to_table(nodes: &[Value]) -> RawTable {
    let mut items = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Value::Object(m) => {
                let mut kv = Vec::new();
                // Cytoscape wraps attributes in "data"
                let data = m.get("data").and_then(|d| d.as_object()).unwrap_or(m);
                let id = data.get("id").or_else(|| data.get("name")).map(scalar_to_string).unwrap_or_default();
                kv.push(("id".to_string(), id));
                for (k, v) in data {
                    if k == "id" {
                        continue;
                    }
                    flatten(k, v, &mut kv);
                }
                items.push(kv);
            }
            other => items.push(vec![("id".to_string(), scalar_to_string(other))]),
        }
    }
    objects_to_table(&items, &["id"])
}

fn cytoscape_positions(nodes: &[Value]) -> Vec<(String, f32, f32)> {
    let mut out = Vec::new();
    for n in nodes {
        let Some(m) = n.as_object() else { continue };
        let Some(p) = m.get("position").and_then(|p| p.as_object()) else { continue };
        let (Some(x), Some(y)) = (p.get("x").and_then(|v| v.as_f64()), p.get("y").and_then(|v| v.as_f64())) else {
            continue;
        };
        let data = m.get("data").and_then(|d| d.as_object()).unwrap_or(m);
        let id = data.get("id").map(scalar_to_string).unwrap_or_default();
        out.push((id, x as f32, y as f32));
    }
    out
}

pub fn parse_json_text(text: &str) -> Result<JsonGraph> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).context("invalid JSON")?;
    parse_json_value(&v)
}

pub fn parse_json_value(v: &Value) -> Result<JsonGraph> {
    match v {
        Value::Object(m) => {
            // Cytoscape: {"elements": {"nodes": [...], "edges": [...]}} or elements as a flat array
            if let Some(el) = m.get("elements") {
                return parse_cytoscape_elements(el);
            }
            let nodes = m.get("nodes").and_then(|x| x.as_array());
            let links = ["links", "edges", "relationships", "connections"]
                .iter()
                .find_map(|k| m.get(*k).and_then(|x| x.as_array()));
            if let Some(links) = links {
                let directed = m.get("directed").and_then(|d| d.as_bool());
                let edges = links_to_edges(links)?;
                let node_table = nodes.map(|n| nodes_to_table(n));
                let positions = nodes.map(|n| cytoscape_positions(n)).unwrap_or_default();
                return Ok(JsonGraph { edges, nodes: node_table, directed, positions });
            }
            // adjacency dictionary {"a": ["b", "c"]}
            if !m.is_empty() && m.values().all(|x| x.as_array().is_some_and(|a| a.iter().all(|e| !e.is_object() && !e.is_array()))) {
                let mut links = Vec::new();
                for (k, arr) in m {
                    for t in arr.as_array().unwrap() {
                        links.push(Value::Array(vec![Value::String(k.clone()), t.clone()]));
                    }
                }
                return Ok(JsonGraph { edges: links_to_edges(&links)?, nodes: None, directed: None, positions: vec![] });
            }
            bail!("unrecognised JSON layout: expected {{nodes, links|edges}}, Cytoscape elements, an edge array or an adjacency map")
        }
        Value::Array(a) => {
            if a.iter().any(|x| x.as_object().is_some_and(|o| o.contains_key("data"))) {
                return parse_cytoscape_elements(v);
            }
            Ok(JsonGraph { edges: links_to_edges(a)?, nodes: None, directed: None, positions: vec![] })
        }
        _ => bail!("unrecognised JSON layout"),
    }
}

fn parse_cytoscape_elements(el: &Value) -> Result<JsonGraph> {
    let (nodes, edges): (Vec<Value>, Vec<Value>) = match el {
        Value::Object(m) => (
            m.get("nodes").and_then(|x| x.as_array()).cloned().unwrap_or_default(),
            m.get("edges").and_then(|x| x.as_array()).cloned().unwrap_or_default(),
        ),
        Value::Array(a) => {
            let mut n = Vec::new();
            let mut e = Vec::new();
            for x in a {
                let is_edge = x.get("group").and_then(|g| g.as_str()) == Some("edges")
                    || x.get("data").and_then(|d| d.get("source")).is_some();
                if is_edge { e.push(x.clone()) } else { n.push(x.clone()) }
            }
            (n, e)
        }
        _ => bail!("bad Cytoscape elements"),
    };
    // edges: unwrap "data"
    let unwrapped: Vec<Value> = edges.iter().map(|e| e.get("data").cloned().unwrap_or_else(|| e.clone())).collect();
    let edge_table = links_to_edges(&unwrapped)?;
    let positions = cytoscape_positions(&nodes);
    let node_table = if nodes.is_empty() { None } else { Some(nodes_to_table(&nodes)) };
    Ok(JsonGraph { edges: edge_table, nodes: node_table, directed: None, positions })
}

pub fn read_json(path: &Path) -> Result<JsonGraph> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_json_text(&text).with_context(|| format!("parsing {}", path.display()))
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_basic_and_types() {
        let t = parse_csv_text("id,age,group\na,10,x\nb,,y\nc,30.5,x\n").unwrap();
        assert_eq!(t.rows.len(), 3);
        let age: Vec<&str> = t.rows.iter().map(|r| r[1].as_str()).collect();
        let c = Column::from_strings("age", &age);
        assert!(c.is_numeric());
        assert_eq!(c.num(0), Some(10.0));
        assert_eq!(c.num(1), None);
        let g: Vec<&str> = t.rows.iter().map(|r| r[2].as_str()).collect();
        let c = Column::from_strings("group", &g);
        assert!(!c.is_numeric());
        assert_eq!(c.cat_str(2), Some("x"));
    }

    #[test]
    fn csv_delimiters_and_bom() {
        let t = parse_csv_text("\u{feff}source;target;weight\n1;2;0.5\n").unwrap();
        assert_eq!(t.headers, vec!["source", "target", "weight"]);
        let m = auto_edge_mapping(&t);
        assert_eq!((m.src, m.dst, m.weight), (0, 1, Some(2)));
        let t = parse_csv_text("a\tb\n1\t2\n").unwrap();
        assert_eq!(t.headers.len(), 2);
    }

    #[test]
    fn duplicate_and_empty_headers() {
        let t = parse_csv_text("x,,x\n1,2,3\n").unwrap();
        assert_eq!(t.headers, vec!["x", "col2", "x_2"]);
    }

    #[test]
    fn natural_order_of_categories() {
        let c = Column::from_strings("k", &["10", "2", "x", "1"]);
        // mixed → categorical, labels sorted naturally (numbers first numerically when both parse)
        assert!(!c.is_numeric());
        let (_, labels) = c.as_categories();
        assert_eq!(labels.len(), 4);
    }

    #[test]
    fn json_d3_style() {
        let g = parse_json_text(
            r#"{"directed":true,"nodes":[{"id":"a","age":3,"tags":["x","y"]},{"id":"b"}],
               "links":[{"source":"a","target":"b","weight":2}]}"#,
        )
        .unwrap();
        assert_eq!(g.directed, Some(true));
        assert_eq!(g.edges.headers, vec!["source", "target", "weight"]);
        let n = g.nodes.unwrap();
        assert_eq!(n.headers[0], "id");
        assert!(n.headers.contains(&"tags".to_string()));
    }

    #[test]
    fn json_flat_edges_and_pairs() {
        let g = parse_json_text(r#"[{"from":1,"to":2},{"from":2,"to":3}]"#).unwrap();
        assert_eq!(g.edges.rows.len(), 2);
        let g = parse_json_text(r#"[[1,2,0.5],[2,3,1.5]]"#).unwrap();
        assert_eq!(g.edges.headers, vec!["source", "target", "weight"]);
    }

    #[test]
    fn json_cytoscape_with_positions() {
        let g = parse_json_text(
            r#"{"elements":{"nodes":[{"data":{"id":"a","w":1},"position":{"x":1,"y":2}},{"data":{"id":"b"},"position":{"x":3,"y":4}}],
               "edges":[{"data":{"id":"e1","source":"a","target":"b"}}]}}"#,
        )
        .unwrap();
        assert_eq!(g.positions.len(), 2);
        assert_eq!(g.edges.rows.len(), 1);
        assert!(g.edges.headers.contains(&"id".to_string()));
    }

    #[test]
    fn json_adjacency_map() {
        let g = parse_json_text(r#"{"a":["b","c"],"b":["c"]}"#).unwrap();
        assert_eq!(g.edges.rows.len(), 3);
    }

    #[test]
    fn fmt_num_works() {
        assert_eq!(fmt_num(3.0), "3");
        assert_eq!(fmt_num(0.5), "0.5");
        assert_eq!(fmt_num(0.00001234), "1.234e-5");
    }
}
