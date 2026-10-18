use cqlint::rules::{LintOptions, apply_fixes, lint};

fn codes(src: &str) -> Vec<String> {
    lint(src, &LintOptions::default())
        .into_iter()
        .map(|f| f.code)
        .collect()
}
fn all(src: &str) -> Vec<String> {
    lint(
        src,
        &LintOptions {
            select: vec!["ALL".into()],
            ..Default::default()
        },
    )
    .into_iter()
    .map(|f| f.code)
    .collect()
}

#[test]
fn allow_filtering_and_select_star() {
    assert_eq!(
        codes("SELECT * FROM t WHERE a = 1 ALLOW FILTERING;"),
        ["CQLINT002", "CQLINT001"]
    );
}

#[test]
fn count_star_is_not_select_star() {
    assert_eq!(
        codes("SELECT count(*) FROM t WHERE pk = 1;"),
        Vec::<String>::new()
    );
    assert_eq!(codes("SELECT count(*) FROM t;"), ["CQLINT004"]);
}

#[test]
fn unbounded_select() {
    assert_eq!(codes("SELECT a FROM t;"), ["CQLINT003"]);
    assert!(codes("SELECT a FROM t LIMIT 10;").is_empty());
    assert!(codes("SELECT a FROM system.local;").is_empty());
}

#[test]
fn large_in_and_batch() {
    let list = (1..=25)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        codes(&format!("SELECT a FROM t WHERE pk IN ({list});")),
        ["CQLINT005"]
    );
    let mut b = String::from("BEGIN BATCH\n");
    for i in 0..12 {
        b.push_str(&format!("INSERT INTO t (a) VALUES ({i});\n"));
    }
    b.push_str("APPLY BATCH;");
    assert_eq!(codes(&b), ["CQLINT006"]);
}

#[test]
fn indexes_lwt_null_lists() {
    assert_eq!(codes("CREATE INDEX i ON t (c);"), ["CQLINT007"]);
    assert!(codes("CREATE INDEX i ON t (c) USING 'sai';").is_empty());
    assert_eq!(
        codes("INSERT INTO t (a) VALUES (1) IF NOT EXISTS;"),
        ["CQLINT008"]
    );
    assert_eq!(
        codes("INSERT INTO t (a, b) VALUES (1, null);"),
        ["CQLINT012"]
    );
    assert_eq!(
        codes("CREATE TABLE t (a int PRIMARY KEY, l list<int>);"),
        ["CQLINT011"]
    );
    assert!(codes("CREATE TABLE t (a int PRIMARY KEY, l frozen<list<int>>);").is_empty());
    assert_eq!(codes("TRUNCATE t;"), ["CQLINT009"]);
}

#[test]
fn wide_table() {
    let cols: String = (0..70).map(|i| format!("c{i} int, ")).collect();
    assert_eq!(
        codes(&format!("CREATE TABLE t ({cols}PRIMARY KEY (c0));")),
        ["CQLINT013"]
    );
}

#[test]
fn idempotent_ddl_is_opt_in_and_fixable() {
    let src = "CREATE TABLE t (a int PRIMARY KEY);\nDROP TABLE u;";
    assert!(!codes(src).contains(&"CQLINT010".to_string()));
    let f = lint(
        src,
        &LintOptions {
            select: vec!["non-idempotent-ddl".into()],
            ..Default::default()
        },
    );
    let (fixed, n) = apply_fixes(src, &f);
    assert_eq!(n, 2);
    assert_eq!(
        fixed,
        "CREATE TABLE IF NOT EXISTS t (a int PRIMARY KEY);\nDROP TABLE IF EXISTS u;"
    );
    assert!(all(&fixed).iter().all(|c| c != "CQLINT010"));
}

#[test]
fn suppression_comments() {
    assert!(codes("-- cqlint: ignore CQLINT002\nSELECT * FROM t LIMIT 1;").is_empty());
    assert!(codes("SELECT * FROM t LIMIT 1; -- cqlint: ignore select-star").is_empty());
    assert!(codes("-- cqlint: ignore-file\nSELECT * FROM t;").is_empty());
    assert_eq!(
        codes("-- cqlint: ignore CQLINT002\nSELECT * FROM t LIMIT 1;\nSELECT * FROM t LIMIT 1;")
            .len(),
        1
    );
}

#[test]
fn ignore_option() {
    let o = LintOptions {
        ignore: vec!["CQLINT002".into()],
        ..Default::default()
    };
    assert!(lint("SELECT * FROM t LIMIT 1;", &o).is_empty());
}
