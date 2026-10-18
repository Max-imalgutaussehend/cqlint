//! CQL formatter: upper-cases keywords, puts one clause per line, wraps long lists, keeps comments.
//!
//! The formatter works on the token stream of `cqlc-core`'s lexer, so it also formats statements
//! that `cqlc` cannot analyse. After formatting, the token stream is compared with the input
//! (keyword case ignored); if it differs the input is returned as an error instead of being rewritten.

use crate::doc::{Doc, Doc::*, render, text};
use cqlc_core::lexer::{Token, TokenKind as K, lex};
use cqlc_core::span::FileId;
use std::fmt;

#[derive(Clone, Debug)]
pub struct FormatOptions {
    pub indent: usize,
    pub line_width: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            indent: 4,
            line_width: 88,
        }
    }
}

#[derive(Debug)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

const RESERVED: &[&str] = &[
    "ADD",
    "ALLOW",
    "ALTER",
    "AND",
    "APPLY",
    "ASC",
    "AUTHORIZE",
    "BATCH",
    "BEGIN",
    "BY",
    "COLUMNFAMILY",
    "CREATE",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DROP",
    "ENTRIES",
    "EXECUTE",
    "FROM",
    "FULL",
    "GRANT",
    "IF",
    "IN",
    "INDEX",
    "INFINITY",
    "INSERT",
    "INTO",
    "KEYSPACE",
    "LIMIT",
    "MODIFY",
    "NAN",
    "NORECURSIVE",
    "NOT",
    "NULL",
    "OF",
    "ON",
    "OR",
    "ORDER",
    "PRIMARY",
    "RENAME",
    "REPLACE",
    "REVOKE",
    "SCHEMA",
    "SELECT",
    "SET",
    "TABLE",
    "TO",
    "TRUNCATE",
    "UNLOGGED",
    "UPDATE",
    "USE",
    "USING",
    "VIEW",
    "WHERE",
    "WITH",
    // not reserved in Cassandra, but structural enough to read better in capitals
    "VALUES",
    "EXISTS",
    "FILTERING",
    "DISTINCT",
    "AS",
    "JSON",
    "CLUSTERING",
    "COMPACT",
    "STORAGE",
    "MATERIALIZED",
    "CONTAINS",
    "IS",
    "COUNTER",
    "PER",
    "PARTITION",
    "GROUP",
    "TYPE",
    "CUSTOM",
    "UNSET",
    "TRUE",
    "FALSE",
    "TTL_",
    "KEYS",
    "VALUES_",
];

const TYPE_CTORS: &[&str] = &["map", "list", "set", "frozen", "tuple", "vector"];
const NAME_PREV: &[&str] = &[
    "INTO",
    "TABLE",
    "ON",
    "TYPE",
    "INDEX",
    "EXISTS",
    "VIEW",
    "KEYSPACE",
    "COLUMNFAMILY",
    "SCHEMA",
];

fn up(s: &str) -> String {
    s.to_ascii_uppercase()
}

#[derive(Clone, Debug)]
struct E {
    kind: Kind,
    raw: String,
    gen_open: bool,
    gen_close: bool,
    /// Ident text in upper case (empty for others).
    uc: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Tok(K),
    /// Comment: `line` comments end at the end of the line; `own` means it started a line.
    Cmt {
        line: bool,
        own: bool,
    },
}

impl E {
    fn is(&self, k: &K) -> bool {
        matches!(&self.kind, Kind::Tok(t) if std::mem::discriminant(t) == std::mem::discriminant(k))
    }
    fn kw(&self, s: &str) -> bool {
        self.uc == s && matches!(self.kind, Kind::Tok(K::Ident(_)))
    }
    fn is_ident(&self) -> bool {
        matches!(self.kind, Kind::Tok(K::Ident(_)))
    }
    fn open(&self) -> bool {
        self.is(&K::LParen) || self.is(&K::LBracket) || self.is(&K::LBrace)
    }
    fn close(&self) -> bool {
        self.is(&K::RParen) || self.is(&K::RBracket) || self.is(&K::RBrace)
    }
    fn comment(&self) -> bool {
        matches!(self.kind, Kind::Cmt { .. })
    }
}

/// Formats a whole file.
pub fn format(src: &str, opts: &FormatOptions) -> Result<String, FormatError> {
    let (lexed, diags) = lex(FileId(0), src);
    if let Some(d) = diags.iter().find(|d| d.is_error()) {
        return Err(FormatError(format!("cannot format: {}", d.message)));
    }
    let items = interleave(src, &lexed.tokens, &lexed.comments);
    let out = layout_file(src, items, opts);
    verify(src, &out)?;
    Ok(out)
}

/// Elements of a file in source order with the blank-line information the layout needs.
struct Item {
    e: E,
    /// Number of line breaks between the previous item and this one.
    breaks_before: usize,
}

fn interleave(src: &str, toks: &[Token], cmts: &[cqlc_core::lexer::Comment]) -> Vec<Item> {
    let mut all: Vec<(usize, usize, Option<&Token>, bool)> = Vec::new();
    for t in toks {
        if t.kind != K::Eof {
            all.push((t.span.range().start, t.span.range().end, Some(t), false));
        }
    }
    for c in cmts {
        let r = c.span.range();
        all.push((r.start, r.end, None, src[r.start..].starts_with("/*")));
    }
    all.sort_by_key(|a| a.0);
    let mut items = Vec::with_capacity(all.len());
    let mut prev_end = 0usize;
    for (s, e, tok, block) in all {
        let gap = &src[prev_end..s];
        let breaks = gap.bytes().filter(|b| *b == b'\n').count();
        let raw = src[s..e].trim_end().to_string();
        let first = items.is_empty();
        let e_ = match tok {
            Some(t) => E {
                uc: if let K::Ident(i) = &t.kind {
                    up(i)
                } else {
                    String::new()
                },
                kind: Kind::Tok(t.kind.clone()),
                raw,
                gen_open: false,
                gen_close: false,
            },
            None => E {
                kind: Kind::Cmt {
                    line: !block,
                    own: breaks > 0 || first,
                },
                raw,
                gen_open: false,
                gen_close: false,
                uc: String::new(),
            },
        };
        items.push(Item {
            e: e_,
            breaks_before: breaks,
        });
        prev_end = e;
    }
    items
}

fn mark_generics(els: &mut [E]) {
    let mut depth = 0usize;
    for i in 0..els.len() {
        if els[i].is(&K::Lt)
            && i > 0
            && els[i - 1].is_ident()
            && TYPE_CTORS.contains(&els[i - 1].raw.to_ascii_lowercase().as_str())
        {
            els[i].gen_open = true;
            depth += 1;
        } else if els[i].is(&K::Gt) && depth > 0 {
            els[i].gen_close = true;
            depth -= 1;
        }
    }
}

/// Index of the token closing the group opened at `i`.
fn matching(els: &[E], i: usize) -> usize {
    let mut d = 0i32;
    for (j, e) in els.iter().enumerate().skip(i) {
        if e.open() {
            d += 1;
        } else if e.close() {
            d -= 1;
            if d == 0 {
                return j;
            }
        }
    }
    els.len() - 1
}

/// Splits at commas that are outside any parentheses and generic brackets.
fn split_commas(els: &[E]) -> Vec<&[E]> {
    let mut out = Vec::new();
    let (mut d, mut g, mut start) = (0i32, 0i32, 0usize);
    for (i, e) in els.iter().enumerate() {
        if e.open() {
            d += 1;
        } else if e.close() {
            d -= 1;
        } else if e.gen_open {
            g += 1;
        } else if e.gen_close {
            g -= 1;
        } else if e.is(&K::Comma) && d == 0 && g == 0 {
            out.push(&els[start..i]);
            start = i + 1;
        }
    }
    out.push(&els[start..]);
    out
}

fn is_name_chain_start(els: &[E], i: usize) -> bool {
    // els[i] is an identifier; walk back over `ident .` pairs to the token before the qualified name
    let mut j = i;
    while j >= 2 && els[j - 1].is(&K::Dot) {
        j -= 2;
    }
    if j == 0 {
        return false;
    }
    let p = &els[j - 1];
    p.is_ident() && NAME_PREV.contains(&p.uc.as_str())
        || (p.is_ident() && p.uc == "NOT" && false)
        || (j >= 3 && p.kw("EXISTS") && els[j - 2].kw("NOT"))
}

fn colon_binary(prev: Option<&E>) -> bool {
    match prev {
        Some(p) => {
            matches!(
                p.kind,
                Kind::Tok(K::String(_))
                    | Kind::Tok(K::Number(_))
                    | Kind::Tok(K::Ident(_))
                    | Kind::Tok(K::QuotedIdent(_))
                    | Kind::Tok(K::Uuid(_))
                    | Kind::Tok(K::RParen)
                    | Kind::Tok(K::RBracket)
                    | Kind::Tok(K::Bind)
            ) && !p.is(&K::Comma)
        }
        None => false,
    }
}

fn unary_minus(els: &[E], i: usize) -> bool {
    if !els[i].is(&K::Minus) {
        return false;
    }
    if i == 0 {
        return true;
    }
    let p = &els[i - 1];
    matches!(
        p.kind,
        Kind::Tok(K::Eq)
            | Kind::Tok(K::Ne)
            | Kind::Tok(K::Lt)
            | Kind::Tok(K::Le)
            | Kind::Tok(K::Gt)
            | Kind::Tok(K::Ge)
            | Kind::Tok(K::Plus)
            | Kind::Tok(K::Minus)
            | Kind::Tok(K::Star)
            | Kind::Tok(K::Slash)
            | Kind::Tok(K::Percent)
            | Kind::Tok(K::LParen)
            | Kind::Tok(K::LBracket)
            | Kind::Tok(K::LBrace)
            | Kind::Tok(K::Comma)
            | Kind::Tok(K::Colon)
    ) || (p.is_ident() && RESERVED.contains(&p.uc.as_str()))
}

fn space_between(els: &[E], i: usize) -> bool {
    let (p, c) = (&els[i - 1], &els[i]);
    if p.comment() || c.comment() {
        return true;
    }
    if c.is(&K::Comma) || c.is(&K::Semi) || c.is(&K::Dot) || c.close() || c.gen_open || c.gen_close
    {
        return false;
    }
    if p.open() || p.is(&K::Dot) || p.gen_open {
        return false;
    }
    if p.gen_close && (c.is(&K::Gt) || c.is(&K::Comma)) {
        return false;
    }
    if c.is(&K::Colon) {
        return !colon_binary(Some(p));
    }
    if p.is(&K::Colon) {
        return colon_binary(i.checked_sub(2).map(|k| &els[k]));
    }
    if p.is(&K::Minus) && unary_minus(els, i - 1) {
        return false;
    }
    if c.is(&K::LParen) {
        if p.is_ident() {
            if p.uc == "TOKEN" {
                return false;
            }
            if p.uc == "KEY" && i >= 2 && els[i - 2].kw("PRIMARY") {
                return true;
            }
            return RESERVED.contains(&p.uc.as_str()) || is_name_chain_start(els, i - 1);
        }
        if matches!(p.kind, Kind::Tok(K::QuotedIdent(_))) {
            return is_name_chain_start(els, i - 1);
        }
        return true;
    }
    if p.is(&K::Star) && c.close() {
        return false;
    }
    true
}

fn render_el(els: &[E], i: usize) -> String {
    let e = &els[i];
    if e.is_ident() {
        let after_dot = i > 0 && els[i - 1].is(&K::Dot);
        let key_ctx =
            e.uc == "KEY" && i > 0 && (els[i - 1].kw("PRIMARY") || els[i - 1].kw("CONTAINS"));
        let ttl_ctx = (e.uc == "TTL" || e.uc == "TIMESTAMP")
            && els.get(i + 1).is_some_and(|n| {
                n.is(&K::Number(String::new())) || n.is(&K::Bind) || n.is(&K::Colon)
            });
        let storage_ctx = (e.uc == "KEYS" || e.uc == "ENTRIES" || e.uc == "FULL")
            && i > 0
            && els[i - 1].is(&K::LParen);
        let upper = !after_dot
            && (RESERVED.contains(&e.uc.as_str())
                && !(e.uc == "TYPE"
                    || e.uc == "COUNTER"
                    || e.uc == "FULL"
                    || e.uc == "ENTRIES"
                    || e.uc == "KEYS"
                    || e.uc == "IS"
                    || e.uc == "AS" && false)
                || key_ctx
                || ttl_ctx);
        let _ = storage_ctx;
        let ty_ctx = e.uc == "TYPE" && i > 0 && els[i - 1].kw("CREATE")
            || e.uc == "TYPE" && i > 0 && (els[i - 1].kw("ALTER") || els[i - 1].kw("DROP"));
        let counter_ctx = e.uc == "COUNTER" && i > 0 && els[i - 1].kw("BEGIN");
        if upper || ty_ctx || counter_ctx {
            return e.uc.clone();
        }
    }
    e.raw.clone()
}

/// Layout of a run of elements without clause splitting (spacing, nested groups, comments).
fn body(els: &[E]) -> Vec<Doc> {
    let mut out: Vec<Doc> = Vec::new();
    let mut i = 0usize;
    let mut pending_break = false;
    let mut first = true;
    while i < els.len() {
        let e = &els[i];
        if !first {
            if pending_break || matches!(e.kind, Kind::Cmt { own: true, .. }) {
                out.push(Hard);
            } else if space_between(els, i) {
                out.push(Line_flat());
            }
        }
        pending_break = false;
        first = false;
        match &e.kind {
            Kind::Cmt { line, .. } => {
                out.push(text(e.raw.clone()));
                pending_break = *line;
                i += 1;
            }
            _ if e.open() => {
                let j = matching(els, i);
                let inner = &els[i + 1..j];
                let l = render_el(els, i);
                let r = render_el(els, j);
                if inner.is_empty() {
                    out.push(text(format!("{l}{r}")));
                } else {
                    let mut items = Vec::new();
                    for (n, part) in split_commas(inner).into_iter().enumerate() {
                        if n > 0 {
                            items.push(text(","));
                            items.push(Line);
                        }
                        items.extend(body(part));
                    }
                    out.push(Group(vec![
                        text(l),
                        Indent(vec![Soft, Group(items)]),
                        Soft,
                        text(r),
                    ]));
                }
                i = j + 1;
            }
            _ => {
                out.push(text(render_el(els, i)));
                i += 1;
            }
        }
    }
    out
}

#[allow(non_snake_case)]
fn Line_flat() -> Doc {
    // a space that never breaks: plain text keeps clauses compact; breaking happens at list level
    text(" ")
}

fn clause_starter(kind: &str, els: &[E], i: usize) -> bool {
    let e = &els[i];
    if !e.is_ident() || (i > 0 && els[i - 1].is(&K::Dot)) {
        return false;
    }
    let prev_kw = |s: &str| i > 0 && els[i - 1].kw(s);
    match kind {
        "SELECT" => match e.uc.as_str() {
            "SELECT" | "FROM" | "WHERE" | "ORDER" | "GROUP" | "PER" | "ALLOW" => true,
            "LIMIT" => !prev_kw("PARTITION"),
            _ => false,
        },
        "INSERT" => matches!(e.uc.as_str(), "INSERT" | "VALUES" | "USING" | "IF"),
        "UPDATE" => matches!(e.uc.as_str(), "UPDATE" | "USING" | "SET" | "WHERE" | "IF"),
        "DELETE" => matches!(e.uc.as_str(), "DELETE" | "FROM" | "USING" | "WHERE" | "IF"),
        "VIEW" => matches!(
            e.uc.as_str(),
            "CREATE" | "SELECT" | "FROM" | "WHERE" | "PRIMARY" | "WITH" | "AS"
        ),
        _ => false,
    }
}

fn stmt_kind(els: &[E]) -> &'static str {
    let w: Vec<&str> = els
        .iter()
        .filter(|e| e.is_ident())
        .take(4)
        .map(|e| e.uc.as_str())
        .collect();
    match w.as_slice() {
        ["SELECT", ..] => "SELECT",
        ["INSERT", ..] => "INSERT",
        ["UPDATE", ..] => "UPDATE",
        ["DELETE", ..] => "DELETE",
        ["CREATE", "MATERIALIZED", "VIEW", ..] => "VIEW",
        ["CREATE", "TABLE", ..] | ["CREATE", "COLUMNFAMILY", ..] | ["CREATE", "TYPE", ..] => {
            "BLOCK"
        }
        _ => "OTHER",
    }
}

fn statement(els: &[E]) -> Doc {
    match stmt_kind(els) {
        "BLOCK" => block_statement(els),
        k @ ("SELECT" | "INSERT" | "UPDATE" | "DELETE" | "VIEW") => clause_statement(k, els),
        _ => other_statement(els),
    }
}

/// DDL and everything else: one line when it fits, otherwise `USING`/`WITH` tails move to a new indented line.
fn other_statement(els: &[E]) -> Doc {
    let mut d = 0i32;
    let cut = els.iter().enumerate().position(|(i, e)| {
        if e.open() {
            d += 1;
        } else if e.close() {
            d -= 1;
        }
        d == 0 && i > 0 && (e.kw("USING") || e.kw("WITH")) && !els[i - 1].is(&K::Dot)
    });
    match cut {
        Some(c) => {
            let mut tail = body(&els[c..]);
            tail.insert(0, Line);
            Group(vec![Group(body(&els[..c])), Indent(tail)])
        }
        None => Group(body(els)),
    }
}

fn clause_statement(kind: &str, els: &[E]) -> Doc {
    // split into clauses at depth-0 starters
    let mut starts = Vec::new();
    let mut d = 0i32;
    for (i, e) in els.iter().enumerate() {
        if e.open() {
            d += 1;
        } else if e.close() {
            d -= 1;
        }
        if d == 0 && !e.open() && clause_starter(kind, els, i) {
            starts.push(i);
        }
    }
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    let mut docs: Vec<Doc> = Vec::new();
    for (n, &s) in starts.iter().enumerate() {
        let e = starts.get(n + 1).copied().unwrap_or(els.len());
        if n > 0 {
            docs.push(Hard);
        }
        docs.push(clause(&els[s..e]));
    }
    Group(docs)
}

fn clause(els: &[E]) -> Doc {
    // keyword part: leading identifiers up to the first non-keyword-ish element
    let mut k = 1;
    let head = els[0].uc.as_str();
    if matches!(head, "ORDER" | "GROUP" | "ALLOW") || (head == "PER") {
        while k < els.len()
            && els[k].is_ident()
            && matches!(
                els[k].uc.as_str(),
                "BY" | "FILTERING" | "PARTITION" | "LIMIT"
            )
        {
            k += 1;
        }
    }
    if head == "DELETE" || head == "SELECT" {
        k = 1;
    }
    if head == "IF"
        && els.get(1).is_some_and(|e| e.kw("NOT"))
        && els.get(2).is_some_and(|e| e.kw("EXISTS"))
    {
        return Group(body(els));
    }
    let and_split = matches!(head, "WHERE" | "IF");
    let rest = &els[k.min(els.len())..];
    let mut head_docs = body(&els[..k.min(els.len())]);
    if rest.is_empty() {
        return Group(head_docs);
    }
    let comma_list = matches!(head, "SELECT" | "SET" | "ORDER" | "GROUP" | "DELETE");
    if and_split {
        // conditions separated by AND (depth 0), continuation lines indented by two levels' worth
        let mut conds: Vec<&[E]> = Vec::new();
        let (mut d, mut start) = (0i32, 0usize);
        for (i, e) in rest.iter().enumerate() {
            if e.open() {
                d += 1;
            } else if e.close() {
                d -= 1;
            } else if d == 0 && e.kw("AND") && i > start {
                conds.push(&rest[start..i]);
                start = i + 1;
            }
        }
        conds.push(&rest[start..]);
        head_docs.push(text(" "));
        for (n, c) in conds.iter().enumerate() {
            if n > 0 {
                head_docs.push(Hard);
                head_docs.push(text("  AND "));
            }
            head_docs.extend(body(c));
        }
        return Group(head_docs);
    }
    if comma_list {
        let mut items = Vec::new();
        for (n, part) in split_commas(rest).into_iter().enumerate() {
            if n > 0 {
                items.push(text(","));
                items.push(Line);
            }
            items.extend(body(part));
        }
        return Group(vec![Group(head_docs), Indent(vec![Line, Group(items)])]);
    }
    head_docs.push(text(" "));
    head_docs.extend(body(rest));
    Group(head_docs)
}

/// `CREATE TABLE ... ( one definition per line ) WITH ... AND ...`
fn block_statement(els: &[E]) -> Doc {
    let Some(p) = els.iter().position(|e| e.is(&K::LParen)) else {
        return Group(body(els));
    };
    let q = matching(els, p);
    let mut docs = body(&els[..p]);
    docs.push(text(" ("));
    let inner = &els[p + 1..q];
    let mut items = Vec::new();
    let parts = split_commas(inner);
    for (n, part) in parts.iter().enumerate() {
        items.push(Hard);
        items.push(Group(body(part)));
        if n + 1 < parts.len() {
            items.push(text(","));
        }
    }
    docs.push(Indent(items));
    docs.push(Hard);
    docs.push(text(")"));
    let tail = &els[q + 1..];
    if !tail.is_empty() {
        // WITH a = b AND c = d: options after AND on their own lines
        let mut parts: Vec<&[E]> = Vec::new();
        let (mut d, mut start) = (0i32, 0usize);
        for (i, e) in tail.iter().enumerate() {
            if e.open() {
                d += 1;
            } else if e.close() {
                d -= 1;
            } else if d == 0 && e.kw("AND") && i > start {
                parts.push(&tail[start..i]);
                start = i + 1;
            }
        }
        parts.push(&tail[start..]);
        docs.push(text(" "));
        for (n, part) in parts.iter().enumerate() {
            if n > 0 {
                docs.push(Hard);
                docs.push(text("  AND "));
            }
            docs.extend(body(part));
        }
    }
    Group(docs)
}

/// One top-level unit: a statement, a batch, or a lone comment.
enum Unit {
    Stmt { els: Vec<E>, trailing: Vec<E> },
    Comment(E),
}

fn layout_file(_src: &str, items: Vec<Item>, opts: &FormatOptions) -> String {
    // marking generics per statement happens after splitting
    let mut units: Vec<(Unit, usize)> = Vec::new(); // (unit, blank lines before: 0 or 1)
    let mut cur: Vec<E> = Vec::new();
    let mut cur_blank = 0usize;
    let mut in_batch = false;
    let mut idx = 0usize;
    let n = items.len();
    while idx < n {
        let it = &items[idx];
        let e = it.e.clone();
        let blank = it.breaks_before.saturating_sub(1).min(1);
        if cur.is_empty() {
            // between statements
            if let Kind::Cmt { own, .. } = e.kind {
                if !own && !units.is_empty() {
                    // trailing comment of the previous statement
                    if let Some((Unit::Stmt { trailing, .. }, _)) = units.last_mut() {
                        trailing.push(e);
                        idx += 1;
                        continue;
                    }
                }
                units.push((Unit::Comment(e), blank));
                idx += 1;
                continue;
            }
            cur_blank = blank;
        }
        if cur.is_empty() && e.kw("BEGIN") {
            in_batch = true;
        }
        let is_semi = e.is(&K::Semi);
        let is_apply_batch = e.kw("BATCH") && cur.last().is_some_and(|p| p.kw("APPLY"));
        cur.push(e);
        if is_apply_batch {
            in_batch = false;
        }
        if is_semi && !in_batch {
            units.push((
                Unit::Stmt {
                    els: std::mem::take(&mut cur),
                    trailing: Vec::new(),
                },
                cur_blank,
            ));
        }
        idx += 1;
    }
    if !cur.is_empty() {
        units.push((
            Unit::Stmt {
                els: cur,
                trailing: Vec::new(),
            },
            cur_blank,
        ));
    }

    let mut out = String::new();
    for (n, (u, blank)) in units.into_iter().enumerate() {
        if n > 0 {
            out.push('\n');
            if blank > 0 {
                out.push('\n');
            }
        }
        match u {
            Unit::Comment(c) => out.push_str(&c.raw),
            Unit::Stmt { mut els, trailing } => {
                mark_generics(&mut els);
                let semi = els.last().is_some_and(|e| e.is(&K::Semi));
                if semi {
                    els.pop();
                }
                let mut doc = if els.first().is_some_and(|e| e.kw("BEGIN")) {
                    batch(&els, opts)
                } else {
                    statement(&els)
                };
                if semi {
                    doc = Group(vec![doc, text(";")]);
                }
                out.push_str(&render(&doc, opts.indent, opts.line_width));
                for t in trailing {
                    out.push(' ');
                    out.push_str(&t.raw);
                }
            }
        }
    }
    out.push('\n');
    out
}

fn batch(els: &[E], _opts: &FormatOptions) -> Doc {
    // BEGIN [UNLOGGED|COUNTER] BATCH [USING ...] stmt; stmt; APPLY BATCH
    let apply = els.iter().rposition(|e| e.kw("APPLY")).unwrap_or(els.len());
    let first_dml = els
        .iter()
        .position(|e| matches!(e.uc.as_str(), "INSERT" | "UPDATE" | "DELETE"))
        .unwrap_or(apply);
    let mut docs = body(&els[..first_dml]);
    let mut inner: Vec<Doc> = Vec::new();
    let mut cur: Vec<E> = Vec::new();
    let flush = |cur: &mut Vec<E>, inner: &mut Vec<Doc>| {
        if cur.is_empty() {
            return;
        }
        let semi = cur.last().is_some_and(|e| e.is(&K::Semi));
        if semi {
            cur.pop();
        }
        let mut s = cur.clone();
        mark_generics(&mut s);
        let mut d = statement(&s);
        if semi {
            d = Group(vec![d, text(";")]);
        }
        inner.push(Hard);
        inner.push(d);
        cur.clear();
    };
    for e in &els[first_dml..apply] {
        let semi = e.is(&K::Semi);
        cur.push(e.clone());
        if semi {
            flush(&mut cur, &mut inner);
        }
    }
    flush(&mut cur, &mut inner);
    docs.push(Indent(inner));
    docs.push(Hard);
    docs.extend(body(&els[apply.min(els.len())..]));
    Group(docs)
}

/// The formatted text must lex to the same tokens (keyword case aside) and the same comments.
pub fn verify(before: &str, after: &str) -> Result<(), FormatError> {
    let (a, _) = lex(FileId(0), before);
    let (b, _) = lex(FileId(0), after);
    let norm = |k: &K| match k {
        K::Ident(s) => K::Ident(s.to_ascii_uppercase()),
        o => o.clone(),
    };
    let ta: Vec<K> = a.tokens.iter().map(|t| norm(&t.kind)).collect();
    let tb: Vec<K> = b.tokens.iter().map(|t| norm(&t.kind)).collect();
    if ta != tb {
        return Err(FormatError(
            "internal error: formatting would change the token stream; input left untouched".into(),
        ));
    }
    let ca: Vec<&str> = a.comments.iter().map(|c| c.text.trim_end()).collect();
    let cb: Vec<&str> = b.comments.iter().map(|c| c.text.trim_end()).collect();
    if ca != cb {
        return Err(FormatError(
            "internal error: formatting would change comments; input left untouched".into(),
        ));
    }
    Ok(())
}
