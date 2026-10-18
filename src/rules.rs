//! Lint rules. Each rule has a stable `CQLINT` code and a kebab-case name; both work in `select`, `ignore` and
//! suppression comments (`-- cqlint: ignore CQLINT001`).

use cqlc_core::lexer::{Token, TokenKind as K, lex};
use cqlc_core::span::FileId;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Note,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub severity: Severity,
    pub default_on: bool,
    pub summary: &'static str,
    pub why: &'static str,
}

pub const RULES: &[Rule] = &[
    Rule {
        code: "CQLINT001",
        name: "allow-filtering",
        severity: Severity::Warning,
        default_on: true,
        summary: "`ALLOW FILTERING` scans partitions and filters on the server",
        why: "The query reads data it then discards, so latency grows with table size. Model a table (or an SAI index) for the query instead.",
    },
    Rule {
        code: "CQLINT002",
        name: "select-star",
        severity: Severity::Warning,
        default_on: true,
        summary: "`SELECT *` reads every column",
        why: "It moves unneeded bytes, breaks when columns are added and defeats column pruning. List the columns you use.",
    },
    Rule {
        code: "CQLINT003",
        name: "unbounded-select",
        severity: Severity::Warning,
        default_on: true,
        summary: "`SELECT` without `WHERE` or `LIMIT` scans the whole table",
        why: "A full table scan touches every partition on every node; its cost grows with the data, not with the result you need.",
    },
    Rule {
        code: "CQLINT004",
        name: "count-scan",
        severity: Severity::Warning,
        default_on: true,
        summary: "`COUNT` without `WHERE` reads the whole table",
        why: "Cassandra has no row counter; COUNT(*) over a table is a distributed scan that can time out. Keep a counter table or bound the count to a partition.",
    },
    Rule {
        code: "CQLINT005",
        name: "large-in",
        severity: Severity::Warning,
        default_on: true,
        summary: "`IN` with many values fans out to many partitions",
        why: "On a partition key each value is a separate read coordinated by one node. Issue parallel single-partition queries instead.",
    },
    Rule {
        code: "CQLINT006",
        name: "large-batch",
        severity: Severity::Warning,
        default_on: true,
        summary: "a batch with many statements",
        why: "Batches are for atomicity, not throughput. Large batches overload the coordinator and trigger batch-size warnings and failures.",
    },
    Rule {
        code: "CQLINT007",
        name: "native-secondary-index",
        severity: Severity::Warning,
        default_on: true,
        summary: "native secondary index (no `USING`)",
        why: "Native 2i queries fan out to every node and work badly for high- or low-cardinality columns. Use an SAI index or a query table.",
    },
    Rule {
        code: "CQLINT008",
        name: "lightweight-transaction",
        severity: Severity::Note,
        default_on: true,
        summary: "lightweight transaction (`IF ...`)",
        why: "LWTs run a Paxos round (several round trips) and serialise per partition; they are 4 to 10 times slower than a plain write.",
    },
    Rule {
        code: "CQLINT009",
        name: "destructive",
        severity: Severity::Warning,
        default_on: true,
        summary: "destructive statement (`DROP`, `TRUNCATE`)",
        why: "Removes data or schema irreversibly. Make sure it is not in a script that runs against production.",
    },
    Rule {
        code: "CQLINT010",
        name: "non-idempotent-ddl",
        severity: Severity::Warning,
        default_on: false,
        summary: "`CREATE` without `IF NOT EXISTS` / `DROP` without `IF EXISTS`",
        why: "Migrations that can be re-run safely do not fail half way. Fixable automatically.",
    },
    Rule {
        code: "CQLINT011",
        name: "list-column",
        severity: Severity::Note,
        default_on: true,
        summary: "non-frozen `list<...>` column",
        why: "Lists need a read before positional updates and leave tombstones when overwritten. Prefer `set`, a clustering column, or `frozen<list<...>>`.",
    },
    Rule {
        code: "CQLINT012",
        name: "null-write",
        severity: Severity::Warning,
        default_on: true,
        summary: "writing `NULL` creates a tombstone",
        why: "Inserting or setting a column to null writes a tombstone that every later read has to scan. Omit the column, or leave it unset.",
    },
    Rule {
        code: "CQLINT013",
        name: "wide-table",
        severity: Severity::Warning,
        default_on: true,
        summary: "table with very many columns",
        why: "Wide tables make rows large, slow compaction and inflate reads that project few columns. Split it by access pattern.",
    },
];

pub fn find_rule(sel: &str) -> Option<&'static Rule> {
    RULES
        .iter()
        .find(|r| r.code.eq_ignore_ascii_case(sel) || r.name.eq_ignore_ascii_case(sel))
}

#[derive(Clone, Debug)]
pub struct LintOptions {
    /// Codes or names to enable on top of the defaults (`ALL` enables everything).
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub in_list_limit: usize,
    pub batch_limit: usize,
    pub wide_table_columns: usize,
}

impl Default for LintOptions {
    fn default() -> Self {
        LintOptions {
            select: vec![],
            ignore: vec![],
            in_list_limit: 20,
            batch_limit: 10,
            wide_table_columns: 64,
        }
    }
}

impl LintOptions {
    pub fn enabled(&self, r: &Rule) -> bool {
        let hit = |l: &[String]| {
            l.iter().any(|s| {
                s.eq_ignore_ascii_case("ALL")
                    || s.eq_ignore_ascii_case(r.code)
                    || s.eq_ignore_ascii_case(r.name)
            })
        };
        if hit(&self.ignore) {
            return false;
        }
        r.default_on || hit(&self.select)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Fix {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub code: String,
    pub name: String,
    pub severity: Severity,
    pub message: String,
    pub help: Option<String>,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

struct T<'a> {
    k: &'a K,
    uc: String,
    start: usize,
    end: usize,
}

impl T<'_> {
    fn kw(&self, s: &str) -> bool {
        self.uc == s
    }
    fn is(&self, k: K) -> bool {
        std::mem::discriminant(self.k) == std::mem::discriminant(&k)
    }
}

pub fn line_col(src: &str, off: usize) -> (usize, usize) {
    let off = off.min(src.len());
    let before = &src[..off];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn mk(src: &str, r: &Rule, msg: String, start: usize, end: usize, fix: Option<Fix>) -> Finding {
    let (line, column) = line_col(src, start);
    Finding {
        code: r.code.into(),
        name: r.name.into(),
        severity: r.severity,
        message: msg,
        help: Some(r.why.into()),
        start,
        end,
        line,
        column,
        fix,
    }
}

fn statements<'a>(toks: &'a [T<'a>]) -> Vec<&'a [T<'a>]> {
    let mut out = Vec::new();
    let mut s = 0;
    for (i, t) in toks.iter().enumerate() {
        if t.is(K::Semi) {
            if i > s {
                out.push(&toks[s..i]);
            }
            s = i + 1;
        }
    }
    if s < toks.len() {
        out.push(&toks[s..]);
    }
    out
}

fn first_kind(st: &[T]) -> &'static str {
    for t in st {
        if matches!(
            t.uc.as_str(),
            "SELECT"
                | "INSERT"
                | "UPDATE"
                | "DELETE"
                | "CREATE"
                | "ALTER"
                | "DROP"
                | "TRUNCATE"
                | "BEGIN"
                | "APPLY"
        ) {
            return match t.uc.as_str() {
                "SELECT" => "SELECT",
                "INSERT" => "INSERT",
                "UPDATE" => "UPDATE",
                "DELETE" => "DELETE",
                "CREATE" => "CREATE",
                "ALTER" => "ALTER",
                "DROP" => "DROP",
                "TRUNCATE" => "TRUNCATE",
                _ => "BATCH",
            };
        }
    }
    ""
}

/// Lints one file. Returns findings in source order, with suppression comments applied.
pub fn lint(src: &str, opts: &LintOptions) -> Vec<Finding> {
    let (lexed, _) = lex(FileId(0), src);
    let toks_raw: Vec<&Token> = lexed.tokens.iter().filter(|t| t.kind != K::Eof).collect();
    let toks: Vec<T> = toks_raw
        .iter()
        .map(|t| T {
            k: &t.kind,
            uc: if let K::Ident(i) = &t.kind {
                i.to_ascii_uppercase()
            } else {
                String::new()
            },
            start: t.span.range().start,
            end: t.span.range().end,
        })
        .collect();
    let mut out: Vec<Finding> = Vec::new();
    let on = |code: &str| find_rule(code).is_some_and(|r| opts.enabled(r));
    let rule = |code: &str| find_rule(code).unwrap();

    // batch size spans several statements
    if on("CQLINT006") {
        let mut i = 0;
        while i < toks.len() {
            if toks[i].kw("BEGIN") {
                let mut n = 0;
                let mut j = i;
                while j < toks.len() && !toks[j].kw("APPLY") {
                    if matches!(toks[j].uc.as_str(), "INSERT" | "UPDATE" | "DELETE")
                        && (j == 0 || !toks[j - 1].is(K::Dot))
                    {
                        n += 1;
                    }
                    j += 1;
                }
                if n > opts.batch_limit {
                    out.push(mk(
                        src,
                        rule("CQLINT006"),
                        format!("batch of {n} statements (limit {})", opts.batch_limit),
                        toks[i].start,
                        toks[(i + 2).min(toks.len() - 1)].end,
                        None,
                    ));
                }
                i = j;
            }
            i += 1;
        }
    }

    for st in statements(&toks) {
        let kind = first_kind(st);
        let depth0 = |f: &dyn Fn(&T) -> bool| -> Option<usize> {
            let mut d = 0i32;
            for (i, t) in st.iter().enumerate() {
                if t.is(K::LParen) {
                    d += 1;
                } else if t.is(K::RParen) {
                    d -= 1;
                } else if d == 0 && f(t) {
                    return Some(i);
                }
            }
            None
        };
        let system = |st: &[T]| {
            st.iter()
                .position(|t| t.kw("FROM"))
                .and_then(|i| st.get(i + 1))
                .is_some_and(|t| t.uc.starts_with("SYSTEM"))
                || st
                    .iter()
                    .position(|t| t.kw("FROM"))
                    .and_then(|i| st.get(i + 1))
                    .is_some_and(|t| {
                        matches!(t.k, K::QuotedIdent(_))
                            && src[t.start..t.end]
                                .to_ascii_lowercase()
                                .starts_with("\"system")
                    })
        };
        if kind == "SELECT" {
            if on("CQLINT001")
                && let Some(i) = st
                    .windows(2)
                    .position(|w| w[0].kw("ALLOW") && w[1].kw("FILTERING"))
            {
                out.push(mk(
                    src,
                    rule("CQLINT001"),
                    "ALLOW FILTERING".into(),
                    st[i].start,
                    st[i + 1].end,
                    None,
                ));
            }
            if on("CQLINT002") {
                let sel = st.iter().position(|t| t.kw("SELECT")).unwrap_or(0);
                let from = st.iter().position(|t| t.kw("FROM")).unwrap_or(st.len());
                if let Some(t) = st[sel..from].iter().find(|t| t.is(K::Star)) {
                    // `count(*)` is not a column list
                    let inside_call = st[sel..from]
                        .windows(2)
                        .any(|w| w[0].is(K::LParen) && std::ptr::eq(w[1].k, t.k));
                    if !inside_call {
                        out.push(mk(
                            src,
                            rule("CQLINT002"),
                            "`SELECT *`".into(),
                            t.start,
                            t.end,
                            None,
                        ));
                    }
                }
            }
            let has_where = depth0(&|t| t.kw("WHERE")).is_some();
            let has_limit = depth0(&|t| t.kw("LIMIT")).is_some();
            let count_at = st
                .windows(2)
                .position(|w| w[0].kw("COUNT") && w[1].is(K::LParen));
            if !has_where && !system(st) {
                if let Some(c) = count_at {
                    if on("CQLINT004") {
                        out.push(mk(
                            src,
                            rule("CQLINT004"),
                            "COUNT over the whole table".into(),
                            st[c].start,
                            st[c].end,
                            None,
                        ));
                    }
                } else if !has_limit && on("CQLINT003") {
                    let from = st.iter().position(|t| t.kw("FROM")).unwrap_or(0);
                    let end = st.get(from + 1).map_or(st[from].end, |t| t.end);
                    out.push(mk(
                        src,
                        rule("CQLINT003"),
                        "SELECT without WHERE or LIMIT".into(),
                        st[from].start,
                        end,
                        None,
                    ));
                }
            }
        }
        if on("CQLINT005") {
            for i in 0..st.len().saturating_sub(1) {
                if st[i].kw("IN") && st[i + 1].is(K::LParen) {
                    let (mut d, mut commas, mut j) = (0i32, 0usize, i + 1);
                    while j < st.len() {
                        if st[j].is(K::LParen) {
                            d += 1;
                        } else if st[j].is(K::RParen) {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        } else if d == 1 && st[j].is(K::Comma) {
                            commas += 1;
                        }
                        j += 1;
                    }
                    if commas + 1 > opts.in_list_limit {
                        out.push(mk(
                            src,
                            rule("CQLINT005"),
                            format!(
                                "IN list with {} values (limit {})",
                                commas + 1,
                                opts.in_list_limit
                            ),
                            st[i].start,
                            st[j.min(st.len() - 1)].end,
                            None,
                        ));
                    }
                }
            }
        }
        if kind == "CREATE" && on("CQLINT007") {
            let idx = st.iter().position(|t| t.kw("INDEX"));
            if let Some(i) = idx
                && !st.iter().any(|t| t.kw("USING"))
                && !st.iter().any(|t| t.kw("CUSTOM"))
            {
                out.push(mk(
                    src,
                    rule("CQLINT007"),
                    "CREATE INDEX without USING".into(),
                    st[0].start,
                    st[i].end,
                    None,
                ));
            }
        }
        if matches!(kind, "INSERT" | "UPDATE" | "DELETE")
            && on("CQLINT008")
            && let Some(i) = depth0(&|t| t.kw("IF"))
        {
            let end = st.get(i + 1).map_or(st[i].end, |t| t.end);
            out.push(mk(
                src,
                rule("CQLINT008"),
                "lightweight transaction".into(),
                st[i].start,
                end,
                None,
            ));
        }
        if ((kind == "DROP"
            && matches!(
                st.get(1).map(|t| t.uc.as_str()),
                Some(
                    "KEYSPACE"
                        | "TABLE"
                        | "COLUMNFAMILY"
                        | "SCHEMA"
                        | "TYPE"
                        | "INDEX"
                        | "MATERIALIZED"
                )
            ))
            || kind == "TRUNCATE")
            && on("CQLINT009")
        {
            let end = st.get(2).map_or(st[0].end, |t| t.end);
            out.push(mk(
                src,
                rule("CQLINT009"),
                format!("{} statement", st[0].uc),
                st[0].start,
                end,
                None,
            ));
        }
        if on("CQLINT010") && matches!(kind, "CREATE" | "DROP") {
            let obj = st.iter().skip(1).take(4).position(|t| {
                matches!(
                    t.uc.as_str(),
                    "KEYSPACE"
                        | "TABLE"
                        | "COLUMNFAMILY"
                        | "SCHEMA"
                        | "TYPE"
                        | "INDEX"
                        | "VIEW"
                        | "ROLE"
                        | "USER"
                        | "FUNCTION"
                        | "AGGREGATE"
                )
            });
            if let Some(o) = obj {
                let o = o + 1;
                let has_if = st.get(o + 1).is_some_and(|t| t.kw("IF"));
                if !has_if {
                    let (msg, ins) = if kind == "CREATE" {
                        ("CREATE without IF NOT EXISTS", " IF NOT EXISTS")
                    } else {
                        ("DROP without IF EXISTS", " IF EXISTS")
                    };
                    out.push(mk(
                        src,
                        rule("CQLINT010"),
                        msg.into(),
                        st[0].start,
                        st[o].end,
                        Some(Fix {
                            start: st[o].end,
                            end: st[o].end,
                            replacement: ins.into(),
                        }),
                    ));
                }
            }
        }
        if matches!(kind, "CREATE" | "ALTER") && on("CQLINT011") {
            for i in 0..st.len().saturating_sub(1) {
                if st[i].kw("LIST") && st[i + 1].is(K::Lt) {
                    let frozen = i >= 2 && st[i - 1].is(K::Lt) && st[i - 2].kw("FROZEN");
                    if !frozen {
                        out.push(mk(
                            src,
                            rule("CQLINT011"),
                            "non-frozen list column".into(),
                            st[i].start,
                            st[i].end,
                            None,
                        ));
                    }
                }
            }
        }
        if matches!(kind, "INSERT" | "UPDATE") && on("CQLINT012") {
            for (i, t) in st.iter().enumerate() {
                if t.kw("NULL") && !(i > 0 && st[i - 1].kw("NOT")) {
                    out.push(mk(
                        src,
                        rule("CQLINT012"),
                        "NULL write".into(),
                        t.start,
                        t.end,
                        None,
                    ));
                }
            }
        }
        if kind == "CREATE"
            && st.get(1).is_some_and(|t| t.kw("TABLE"))
            && on("CQLINT013")
            && let Some(p) = st.iter().position(|t| t.is(K::LParen))
        {
            let (mut d, mut cols, mut item_start) = (0i32, 0usize, p + 1);
            for j in p..st.len() {
                if st[j].is(K::LParen) {
                    d += 1;
                } else if st[j].is(K::RParen) {
                    d -= 1;
                    if d == 0 {
                        if !st[item_start].kw("PRIMARY") && j > item_start {
                            cols += 1;
                        }
                        break;
                    }
                } else if d == 1 && st[j].is(K::Comma) {
                    if !st[item_start].kw("PRIMARY") {
                        cols += 1;
                    }
                    item_start = j + 1;
                }
            }
            if cols > opts.wide_table_columns {
                out.push(mk(
                    src,
                    rule("CQLINT013"),
                    format!(
                        "table with {cols} columns (limit {})",
                        opts.wide_table_columns
                    ),
                    st[0].start,
                    st[p.saturating_sub(1)].end,
                    None,
                ));
            }
        }
    }
    out.sort_by_key(|f| (f.start, f.code.clone()));
    apply_suppressions(src, &lexed.comments, out)
}

fn apply_suppressions(
    src: &str,
    comments: &[cqlc_core::lexer::Comment],
    findings: Vec<Finding>,
) -> Vec<Finding> {
    let mut file_off = false;
    let mut by_line: Vec<(usize, bool, Vec<String>)> = Vec::new(); // (line, own_line, ids)
    for c in comments {
        let t = c.text.trim();
        let Some(rest) = t.strip_prefix("cqlint:") else {
            continue;
        };
        let rest = rest.trim();
        let r = c.span.range();
        let (line, _) = line_col(src, r.start);
        let own = src[..r.start]
            .rsplit('\n')
            .next()
            .is_some_and(|l| l.trim().is_empty());
        if rest.starts_with("ignore-file") {
            file_off = true;
        } else if let Some(ids) = rest.strip_prefix("ignore") {
            let ids: Vec<String> = ids
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            by_line.push((line, own, ids));
        }
    }
    if file_off {
        return vec![];
    }
    findings
        .into_iter()
        .filter(|f| {
            !by_line.iter().any(|(line, own, ids)| {
                let applies = if *own {
                    f.line == line + 1 || stmt_line(src, f.start) == line + 1
                } else {
                    f.line == *line
                };
                applies
                    && (ids.is_empty()
                        || ids.iter().any(|i| {
                            i.eq_ignore_ascii_case(&f.code) || i.eq_ignore_ascii_case(&f.name)
                        }))
            })
        })
        .collect()
}

/// Line of the first non-blank, non-comment text of the statement containing `off`.
fn stmt_line(src: &str, off: usize) -> usize {
    let before = &src[..off.min(src.len())];
    let start = before.rfind(';').map_or(0, |p| p + 1);
    let mut pos = start;
    for l in src[start..].split_inclusive('\n') {
        let t = l.trim();
        if !t.is_empty() && !t.starts_with("--") && !t.starts_with("//") {
            return line_col(src, pos).0;
        }
        pos += l.len();
    }
    line_col(src, start).0
}

/// Applies the fixes of `findings` (non-overlapping, back to front) and returns the new text.
pub fn apply_fixes(src: &str, findings: &[Finding]) -> (String, usize) {
    let mut fixes: Vec<&Fix> = findings.iter().filter_map(|f| f.fix.as_ref()).collect();
    fixes.sort_by_key(|f| std::cmp::Reverse(f.start));
    let mut out = src.to_string();
    let mut n = 0;
    let mut last = usize::MAX;
    for f in fixes {
        if f.end <= last {
            out.replace_range(f.start..f.end, &f.replacement);
            last = f.start;
            n += 1;
        }
    }
    (out, n)
}
