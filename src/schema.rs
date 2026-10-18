//! Type-aware findings from `cqlc-core`: unknown tables and columns, type mismatches, missing partition keys,
//! broad scans. Every DML statement of a file is checked against the schema files.

use crate::rules::{Finding, Severity, line_col};
use cqlc_core::lexer::{TokenKind as K, lex};
use cqlc_core::span::FileId;
use cqlc_core::{Input, compile};

pub struct SchemaCheck {
    schema: Vec<Input>,
}

impl SchemaCheck {
    pub fn new(files: Vec<(String, String)>) -> Self {
        SchemaCheck {
            schema: files.into_iter().map(|(n, t)| Input::new(n, t)).collect(),
        }
    }

    /// Findings for the statements of `src` (offsets refer to `src`).
    pub fn check(&self, src: &str) -> Vec<Finding> {
        let (lexed, _) = lex(FileId(0), src);
        let mut stmts: Vec<(usize, usize, bool)> = Vec::new(); // start, end, is_select
        let mut start: Option<usize> = None;
        let mut first = String::new();
        for t in lexed.tokens.iter().filter(|t| t.kind != K::Eof) {
            let r = t.span.range();
            if let K::Semi = t.kind {
                if let Some(s) = start.take() {
                    stmts.push((s, r.start, first.eq_ignore_ascii_case("select")));
                }
                continue;
            }
            if start.is_none() {
                start = Some(r.start);
                first = if let K::Ident(i) = &t.kind {
                    i.clone()
                } else {
                    String::new()
                };
            }
        }
        let dml = |s: &str| {
            matches!(
                s.to_ascii_lowercase().as_str(),
                "select" | "insert" | "update" | "delete"
            )
        };
        let mut text = String::new();
        let mut map: Vec<(usize, usize, usize)> = Vec::new(); // new_start, new_end, orig_start
        for (i, (s, e, sel)) in stmts.iter().enumerate() {
            let body = &src[*s..*e];
            let head = body.split_whitespace().next().unwrap_or("");
            if !dml(head) {
                continue;
            }
            text.push_str(&format!(
                "-- name: Q{i} :{}\n",
                if *sel { "many" } else { "exec" }
            ));
            let ns = text.len();
            text.push_str(body);
            map.push((ns, text.len(), *s));
            text.push_str(";\n");
        }
        if map.is_empty() {
            return vec![];
        }
        let c = compile(&self.schema, &[Input::new("q.cql", text)]);
        let nschema = self.schema.len();
        let mut out = Vec::new();
        for d in &c.diagnostics {
            if matches!(d.code.as_str(), "CQL111" | "CQL112") {
                continue;
            }
            let r = d.span.range();
            if (d.span.file.0 as usize) < nschema {
                continue; // a problem in the schema files themselves; reported by `cqlc`
            }
            let Some(&(ns, ne, orig)) = map
                .iter()
                .find(|(ns, ne, _)| r.start >= *ns && r.start <= *ne)
            else {
                continue;
            };
            let start = orig + (r.start - ns);
            let end = (orig + (r.end.min(ne) - ns)).max(start);
            let (line, column) = line_col(src, start);
            out.push(Finding {
                code: d.code.as_str().to_string(),
                name: "schema".into(),
                severity: if d.is_error() {
                    Severity::Error
                } else {
                    Severity::Warning
                },
                message: d.message.clone(),
                help: d.help.clone(),
                start,
                end,
                line,
                column,
                fix: None,
            });
        }
        out
    }
}
