//! Edge attributes made from the nodes at both ends of each edge: the pair of values ("A → B"), whether they are the
//! same, either end's value, or for numbers their difference, sum, mean … They are kept as recipes (saved in sessions)
//! and made again whenever the graph is rebuilt, and can be used like any edge column, e.g. to colour the edges.

use crate::data::{Column, natural_cmp};
use crate::graph::Graph;
use serde::{Deserialize, Serialize};

/// How the two ends' values make the edge's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Combine {
    /// "source → target"
    Pair,
    /// The two values in a fixed order, so A–B and B–A are one category.
    PairEitherWay,
    /// "same" or "different"
    Same,
    Source,
    Target,
    /// target − source
    Diff,
    AbsDiff,
    Sum,
    Mean,
    Min,
    Max,
}

impl Combine {
    /// For any column.
    pub const ANY: [Combine; 5] = [Combine::Pair, Combine::PairEitherWay, Combine::Same, Combine::Source, Combine::Target];
    /// Only for numeric columns.
    pub const NUMBERS: [Combine; 6] = [Combine::Diff, Combine::AbsDiff, Combine::Sum, Combine::Mean, Combine::Min, Combine::Max];

    pub fn numbers_only(self) -> bool {
        Combine::NUMBERS.contains(&self)
    }

    /// The text in the menu.
    pub fn label(self) -> &'static str {
        match self {
            Combine::Pair => "Pair: source → target",
            Combine::PairEitherWay => "Pair, either way round",
            Combine::Same => "Same or different",
            Combine::Source => "Source node's value",
            Combine::Target => "Target node's value",
            Combine::Diff => "Difference (target − source)",
            Combine::AbsDiff => "Difference, ignoring the sign",
            Combine::Sum => "Sum",
            Combine::Mean => "Mean",
            Combine::Min => "Smaller value",
            Combine::Max => "Larger value",
        }
    }

    /// Short words for the default name and the description.
    pub fn short(self) -> &'static str {
        match self {
            Combine::Pair => "source → target",
            Combine::PairEitherWay => "pair",
            Combine::Same => "same?",
            Combine::Source => "source",
            Combine::Target => "target",
            Combine::Diff => "target − source",
            Combine::AbsDiff => "|difference|",
            Combine::Sum => "sum",
            Combine::Mean => "mean",
            Combine::Min => "min",
            Combine::Max => "max",
        }
    }
}

/// One edge attribute: its name, the node column it is made from, and how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeAttr {
    pub name: String,
    pub col: String,
    pub how: Combine,
}

impl EdgeAttr {
    pub fn default_name(col: &str, how: Combine) -> String {
        format!("{col} ({})", how.short())
    }

    /// The column for every edge of `g`; None when the node column is missing, or is not numeric for a number recipe.
    pub fn compute(&self, g: &Graph) -> Option<Column> {
        let col = g.node_col(&self.col)?;
        if self.how.numbers_only() && !col.is_numeric() {
            return None;
        }
        let mut out = if self.how.numbers_only() {
            let vals = g
                .edges
                .iter()
                .map(|&(s, t)| match (col.num(s as usize), col.num(t as usize)) {
                    (Some(a), Some(b)) => match self.how {
                        Combine::Diff => b - a,
                        Combine::AbsDiff => (b - a).abs(),
                        Combine::Sum => a + b,
                        Combine::Mean => (a + b) / 2.0,
                        Combine::Min => a.min(b),
                        _ => a.max(b),
                    },
                    _ => f64::NAN,
                })
                .collect();
            Column::numeric(&self.name, vals)
        } else {
            // each node's value as text, once
            let text: Vec<Option<String>> = (0..g.n()).map(|i| col.label_at(i)).collect();
            let vals: Vec<String> = g
                .edges
                .iter()
                .map(|&(s, t)| {
                    let (a, b) = (text[s as usize].as_deref(), text[t as usize].as_deref());
                    match self.how {
                        Combine::Source => a.unwrap_or("").to_string(),
                        Combine::Target => b.unwrap_or("").to_string(),
                        _ => match (a, b) {
                            (Some(a), Some(b)) => match self.how {
                                Combine::Pair => format!("{a} → {b}"),
                                Combine::PairEitherWay => {
                                    let (x, y) = if natural_cmp(a, b).is_gt() { (b, a) } else { (a, b) };
                                    format!("{x} – {y}")
                                }
                                _ => (if a == b { "same" } else { "different" }).to_string(),
                            },
                            _ => String::new(),
                        },
                    }
                })
                .collect();
            let refs: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
            let mut c = Column::from_strings(&self.name, &refs);
            // a copied community or component id stays a category
            if matches!(self.how, Combine::Source | Combine::Target) {
                c.hint_cat = col.hint_cat;
            }
            c
        };
        out.is_metric = true;
        Some(out)
    }
}

/// Add the edge attributes to a freshly built graph (those whose node column is missing are skipped).
pub fn apply(g: &mut Graph, list: &[EdgeAttr]) {
    for a in list {
        g.edge_cols.retain(|c| !(c.is_metric && c.name == a.name));
        if g.edge_col(&a.name).is_some() {
            continue; // a column from the file has this name
        }
        if let Some(c) = a.compute(g) {
            g.edge_cols.push(c);
        }
    }
}

/// The half-made edge attribute in the Appearance panel.
pub struct Draft {
    pub col: Option<String>,
    pub how: Combine,
    pub name: String,
    pub error: Option<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Draft { col: None, how: Combine::Pair, name: String::new(), error: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{auto_edge_mapping, auto_node_mapping, parse_csv_text};
    use crate::graph::build_graph;

    fn graph() -> Graph {
        let e = parse_csv_text("source,target\na,b\nb,c\nc,a\na,d\n").unwrap();
        let n = parse_csv_text("id,team,score\na,red,1\nb,red,4\nc,blue,10\nd,,2\n").unwrap();
        build_graph(Some((&e, &auto_edge_mapping(&e))), Some((&n, &auto_node_mapping(&n))))
    }

    fn values(g: &Graph, col: &str, how: Combine) -> Vec<Option<String>> {
        let c = EdgeAttr { name: "x".into(), col: col.into(), how }.compute(g).unwrap();
        (0..c.len()).map(|i| c.label_at(i)).collect()
    }

    #[test]
    fn text_recipes_follow_both_ends() {
        let g = graph();
        let s = |v: &str| Some(v.to_string());
        assert_eq!(values(&g, "team", Combine::Pair), [s("red → red"), s("red → blue"), s("blue → red"), None]);
        assert_eq!(values(&g, "team", Combine::PairEitherWay), [s("red – red"), s("blue – red"), s("blue – red"), None]);
        assert_eq!(values(&g, "team", Combine::Same), [s("same"), s("different"), s("different"), None]);
        assert_eq!(values(&g, "team", Combine::Source), [s("red"), s("red"), s("blue"), s("red")]);
        assert_eq!(values(&g, "team", Combine::Target), [s("red"), s("blue"), s("red"), None]);
    }

    #[test]
    fn number_recipes_and_their_limits() {
        let g = graph();
        let num = |how| {
            let c = EdgeAttr { name: "x".into(), col: "score".into(), how }.compute(&g).unwrap();
            (0..c.len()).map(|i| c.num(i).unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(num(Combine::Diff), [3.0, 6.0, -9.0, 1.0]);
        assert_eq!(num(Combine::AbsDiff), [3.0, 6.0, 9.0, 1.0]);
        assert_eq!(num(Combine::Mean), [2.5, 7.0, 5.5, 1.5]);
        assert_eq!(num(Combine::Max), [4.0, 10.0, 10.0, 2.0]);
        // copying a number keeps it a number
        assert!(EdgeAttr { name: "x".into(), col: "score".into(), how: Combine::Source }.compute(&g).unwrap().is_numeric());
        // numbers only for numeric columns, and nothing for a missing column
        assert!(EdgeAttr { name: "x".into(), col: "team".into(), how: Combine::Sum }.compute(&g).is_none());
        assert!(EdgeAttr { name: "x".into(), col: "nope".into(), how: Combine::Pair }.compute(&g).is_none());
    }

    #[test]
    fn apply_adds_once_and_never_replaces_a_file_column() {
        let mut g = graph();
        let list = [EdgeAttr { name: "teams".into(), col: "team".into(), how: Combine::Pair }];
        apply(&mut g, &list);
        apply(&mut g, &list);
        assert_eq!(g.edge_cols.iter().filter(|c| c.name == "teams").count(), 1);
        let e = parse_csv_text("source,target,teams\na,b,x\n").unwrap();
        let mut g2 = build_graph(Some((&e, &auto_edge_mapping(&e))), None);
        apply(&mut g2, &list);
        assert_eq!(g2.edge_col("teams").unwrap().label_at(0).as_deref(), Some("x"));
    }
}
