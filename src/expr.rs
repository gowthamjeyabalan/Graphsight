//! A small filter expression language:
//!   age > 30 and (group == "A" or group in ["B", "C"]) and not name contains "test"
//! Column names with spaces/symbols can be written in `backticks`. `id` is the node id.

use crate::graph::Graph;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Cmp(CmpOp),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    And,
    Or,
    Not,
    Contains,
    StartsWith,
    In,
    True,
    False,
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => { out.push(Tok::LParen); i += 1 }
            ')' => { out.push(Tok::RParen); i += 1 }
            '[' => { out.push(Tok::LBracket); i += 1 }
            ']' => { out.push(Tok::RBracket); i += 1 }
            ',' => { out.push(Tok::Comma); i += 1 }
            '=' => {
                i += 1;
                if cs.get(i) == Some(&'=') { i += 1; }
                out.push(Tok::Cmp(CmpOp::Eq));
            }
            '!' => {
                if cs.get(i + 1) == Some(&'=') {
                    out.push(Tok::Cmp(CmpOp::Ne));
                    i += 2;
                } else {
                    out.push(Tok::Not);
                    i += 1;
                }
            }
            '<' => {
                if cs.get(i + 1) == Some(&'=') { out.push(Tok::Cmp(CmpOp::Le)); i += 2 }
                else if cs.get(i + 1) == Some(&'>') { out.push(Tok::Cmp(CmpOp::Ne)); i += 2 }
                else { out.push(Tok::Cmp(CmpOp::Lt)); i += 1 }
            }
            '>' => {
                if cs.get(i + 1) == Some(&'=') { out.push(Tok::Cmp(CmpOp::Ge)); i += 2 }
                else { out.push(Tok::Cmp(CmpOp::Gt)); i += 1 }
            }
            '&' if cs.get(i + 1) == Some(&'&') => { out.push(Tok::And); i += 2 }
            '|' if cs.get(i + 1) == Some(&'|') => { out.push(Tok::Or); i += 2 }
            '"' | '\'' | '`' => {
                let q = c;
                let mut s = String::new();
                i += 1;
                loop {
                    match cs.get(i) {
                        None => return Err(format!("unterminated {q}…{q}")),
                        Some(&x) if x == q => { i += 1; break }
                        Some(&x) => { s.push(x); i += 1 }
                    }
                }
                out.push(if q == '`' { Tok::Ident(s) } else { Tok::Str(s) });
            }
            c if c.is_ascii_digit() || (c == '-' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '.')) || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let st = i;
                i += 1;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.' || cs[i] == 'e' || cs[i] == 'E'
                    || ((cs[i] == '-' || cs[i] == '+') && matches!(cs[i - 1], 'e' | 'E'))) {
                    i += 1;
                }
                let s: String = cs[st..i].iter().collect();
                out.push(Tok::Num(s.parse().map_err(|_| format!("bad number `{s}`"))?));
            }
            c if c.is_alphabetic() || c == '_' => {
                let st = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || matches!(cs[i], '_' | '.' | '#' | ':')) {
                    i += 1;
                }
                let s: String = cs[st..i].iter().collect();
                out.push(match s.to_ascii_lowercase().as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "contains" => Tok::Contains,
                    "startswith" => Tok::StartsWith,
                    "in" => Tok::In,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "null" | "none" | "na" => Tok::Null,
                    _ => Tok::Ident(s),
                });
            }
            other => return Err(format!("unexpected character `{other}`")),
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum ColRef {
    Id,
    Node(usize),
}

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Col(ColRef),
    Cmp(Box<Expr>, CmpOp, Box<Expr>),
    Contains(Box<Expr>, Box<Expr>),
    StartsWith(Box<Expr>, Box<Expr>),
    In(Box<Expr>, Vec<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

struct Parser<'g> {
    toks: Vec<Tok>,
    pos: usize,
    g: &'g Graph,
}

impl<'g> Parser<'g> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.bump();
            l = Expr::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.not()?;
        while self.peek() == Some(&Tok::And) {
            self.bump();
            l = Expr::And(Box::new(l), Box::new(self.not()?));
        }
        Ok(l)
    }
    fn not(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Tok::Not) {
            self.bump();
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let l = self.primary()?;
        match self.peek().cloned() {
            Some(Tok::Cmp(op)) => {
                self.bump();
                Ok(Expr::Cmp(Box::new(l), op, Box::new(self.primary()?)))
            }
            Some(Tok::Contains) => {
                self.bump();
                Ok(Expr::Contains(Box::new(l), Box::new(self.primary()?)))
            }
            Some(Tok::StartsWith) => {
                self.bump();
                Ok(Expr::StartsWith(Box::new(l), Box::new(self.primary()?)))
            }
            Some(Tok::In) => {
                self.bump();
                if self.bump() != Some(Tok::LBracket) {
                    return Err("expected `[` after `in`".into());
                }
                let mut items = Vec::new();
                loop {
                    items.push(self.primary()?);
                    match self.bump() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RBracket) => break,
                        _ => return Err("expected `,` or `]` in list".into()),
                    }
                }
                Ok(Expr::In(Box::new(l), items))
            }
            _ => Ok(l),
        }
    }
    fn primary(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Some(Tok::Num(x)) => Ok(Expr::Num(x)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::True) => Ok(Expr::Bool(true)),
            Some(Tok::False) => Ok(Expr::Bool(false)),
            Some(Tok::Null) => Ok(Expr::Null),
            Some(Tok::Ident(name)) => {
                if name.eq_ignore_ascii_case("id") && self.g.node_col(&name).is_none() {
                    return Ok(Expr::Col(ColRef::Id));
                }
                if let Some(i) = self.g.node_cols.iter().position(|c| c.name == name) {
                    return Ok(Expr::Col(ColRef::Node(i)));
                }
                if let Some(i) = self.g.node_cols.iter().position(|c| c.name.eq_ignore_ascii_case(&name)) {
                    return Ok(Expr::Col(ColRef::Node(i)));
                }
                // allow `degree` for `_degree` etc.
                let with_us = format!("_{name}");
                if let Some(i) = self.g.node_cols.iter().position(|c| c.name.eq_ignore_ascii_case(&with_us)) {
                    return Ok(Expr::Col(ColRef::Node(i)));
                }
                let names: Vec<&str> = self.g.node_cols.iter().map(|c| c.name.as_str()).collect();
                Err(format!("unknown column `{name}` (available: id, {})", names.join(", ")))
            }
            Some(Tok::LParen) => {
                let e = self.or()?;
                if self.bump() != Some(Tok::RParen) {
                    return Err("missing `)`".into());
                }
                Ok(e)
            }
            Some(t) => Err(format!("unexpected {}", describe(&t))),
            None => Err("expression ended unexpectedly".into()),
        }
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(x) => format!("number {x}"),
        Tok::Str(s) => format!("string \"{s}\""),
        Tok::Ident(s) => format!("name `{s}`"),
        other => format!("{other:?}").to_lowercase(),
    }
}

#[derive(Clone, Debug)]
enum Val<'a> {
    Num(f64),
    Str(&'a str),
    Bool(bool),
    Null,
}

fn as_num(v: &Val) -> Option<f64> {
    match v {
        Val::Num(x) => Some(*x),
        Val::Str(s) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()),
        _ => None,
    }
}

fn as_text(v: &Val, buf: &mut String) {
    buf.clear();
    match v {
        Val::Str(s) => buf.push_str(s),
        Val::Num(x) => buf.push_str(&crate::data::fmt_num(*x)),
        Val::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        Val::Null => {}
    }
}

fn compare(a: &Val, op: CmpOp, b: &Val) -> bool {
    use CmpOp::*;
    match (a, b) {
        (Val::Null, Val::Null) => return matches!(op, Eq | Le | Ge),
        (Val::Null, _) | (_, Val::Null) => return op == Ne,
        _ => {}
    }
    if let (Some(x), Some(y)) = (as_num(a), as_num(b)) {
        return match op {
            Eq => x == y,
            Ne => x != y,
            Lt => x < y,
            Le => x <= y,
            Gt => x > y,
            Ge => x >= y,
        };
    }
    let (mut sa, mut sb) = (String::new(), String::new());
    as_text(a, &mut sa);
    as_text(b, &mut sb);
    let ord = sa.cmp(&sb);
    match op {
        Eq => ord.is_eq(),
        Ne => ord.is_ne(),
        Lt => ord.is_lt(),
        Le => ord.is_le(),
        Gt => ord.is_gt(),
        Ge => ord.is_ge(),
    }
}

pub struct Compiled {
    expr: Expr,
}

/// Parse and bind column names against `g`. Empty input yields Ok(None).
pub fn compile(src: &str, g: &Graph) -> Result<Option<Compiled>, String> {
    if src.trim().is_empty() {
        return Ok(None);
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, g };
    let expr = p.or()?;
    if p.pos < p.toks.len() {
        return Err(format!("unexpected {} after the expression", describe(&p.toks[p.pos])));
    }
    Ok(Some(Compiled { expr }))
}

impl Compiled {
    pub fn matches(&self, g: &Graph, node: usize) -> bool {
        truthy(&eval(&self.expr, g, node))
    }
}

fn truthy(v: &Val) -> bool {
    match v {
        Val::Bool(b) => *b,
        Val::Num(x) => *x != 0.0,
        Val::Str(s) => !s.is_empty(),
        Val::Null => false,
    }
}

fn eval<'a>(e: &'a Expr, g: &'a Graph, node: usize) -> Val<'a> {
    match e {
        Expr::Num(x) => Val::Num(*x),
        Expr::Str(s) => Val::Str(s),
        Expr::Bool(b) => Val::Bool(*b),
        Expr::Null => Val::Null,
        Expr::Col(ColRef::Id) => Val::Str(&g.ids[node]),
        Expr::Col(ColRef::Node(ci)) => {
            let c = &g.node_cols[*ci];
            match &c.data {
                crate::data::ColData::Num(v) => {
                    let x = v[node];
                    if x.is_nan() { Val::Null } else { Val::Num(x) }
                }
                _ => c.cat_str(node).map(Val::Str).unwrap_or(Val::Null),
            }
        }
        Expr::Cmp(a, op, b) => Val::Bool(compare(&eval(a, g, node), *op, &eval(b, g, node))),
        Expr::Contains(a, b) => {
            let (va, vb) = (eval(a, g, node), eval(b, g, node));
            let (mut sa, mut sb) = (String::new(), String::new());
            as_text(&va, &mut sa);
            as_text(&vb, &mut sb);
            Val::Bool(!matches!(va, Val::Null) && sa.to_lowercase().contains(&sb.to_lowercase()))
        }
        Expr::StartsWith(a, b) => {
            let (va, vb) = (eval(a, g, node), eval(b, g, node));
            let (mut sa, mut sb) = (String::new(), String::new());
            as_text(&va, &mut sa);
            as_text(&vb, &mut sb);
            Val::Bool(!matches!(va, Val::Null) && sa.to_lowercase().starts_with(&sb.to_lowercase()))
        }
        Expr::In(a, items) => {
            let va = eval(a, g, node);
            Val::Bool(items.iter().any(|it| compare(&va, CmpOp::Eq, &eval(it, g, node))))
        }
        Expr::And(a, b) => Val::Bool(truthy(&eval(a, g, node)) && truthy(&eval(b, g, node))),
        Expr::Or(a, b) => Val::Bool(truthy(&eval(a, g, node)) || truthy(&eval(b, g, node))),
        Expr::Not(a) => Val::Bool(!truthy(&eval(a, g, node))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::tests::tiny;

    fn run(src: &str) -> Vec<String> {
        let g = tiny(false);
        let c = compile(src, &g).unwrap().unwrap();
        let mut v: Vec<String> = (0..g.n()).filter(|&i| c.matches(&g, i)).map(|i| g.ids[i].clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn numeric_comparison() {
        assert_eq!(run("score > 1.5"), vec!["b", "d", "loner"]);
        assert_eq!(run("score >= 1.5 and score <= 2"), vec!["a", "b"]);
    }

    #[test]
    fn category_logic() {
        assert_eq!(run("group == \"g1\" or group == 'g3'"), vec!["a", "b", "loner"]);
        assert_eq!(run("group in [\"g2\", \"g3\"]"), vec!["c", "d", "loner"]);
        assert_eq!(run("not group == \"g1\" and group != null"), vec!["c", "d", "loner"]);
    }

    #[test]
    fn null_handling_and_text() {
        assert_eq!(run("score == null"), vec!["c", "q", "x", "y"]);
        assert_eq!(run("id contains \"LON\""), vec!["loner"]);
        assert_eq!(run("id startswith \"l\""), vec!["loner"]);
    }

    #[test]
    fn metric_alias_and_precedence() {
        // `degree` resolves to `_degree`; and binds tighter than or
        assert_eq!(run("degree >= 3 or id == \"x\" and score > 100"), vec!["a", "b", "c"]);
        assert_eq!(run("(degree >= 3 or id == \"x\") and score > 100"), Vec::<String>::new());
    }

    #[test]
    fn errors_are_readable() {
        let g = tiny(false);
        let e = compile("nosuch > 3", &g).err().unwrap();
        assert!(e.contains("unknown column"), "{e}");
        assert!(compile("score >", &g).is_err());
        assert!(compile("(score > 1", &g).is_err());
        assert!(compile("score > 1 )", &g).is_err());
        assert!(compile("", &g).unwrap().is_none());
    }
}
