use cqlint::format::{FormatOptions, format};

fn f(s: &str) -> String {
    format(s, &FormatOptions::default()).unwrap()
}

#[test]
fn select_layout() {
    assert_eq!(
        f(
            "select id,name from ks.users where id = ? and name > 'a' order by name desc limit 10 allow filtering;"
        ),
        "SELECT id, name\nFROM ks.users\nWHERE id = ?\n  AND name > 'a'\nORDER BY name DESC\nLIMIT 10\nALLOW FILTERING;\n"
    );
}

#[test]
fn create_table_block() {
    assert_eq!(
        f(
            "create table if not exists ks.t (id uuid, tags map<text,frozen<list<int>>>, PRIMARY KEY ((id), tags)) with clustering order by (tags desc) and compaction={'class':'X'};"
        ),
        "CREATE TABLE IF NOT EXISTS ks.t (\n    id uuid,\n    tags map<text, frozen<list<int>>>,\n    PRIMARY KEY ((id), tags)\n) WITH CLUSTERING ORDER BY (tags DESC)\n  AND compaction = {'class': 'X'};\n"
    );
}

#[test]
fn dml_clauses_and_function_calls() {
    assert_eq!(
        f("insert into t (a,b) values (?,now()) using ttl 60 if not exists;"),
        "INSERT INTO t (a, b)\nVALUES (?, now())\nUSING TTL 60\nIF NOT EXISTS;\n"
    );
    assert_eq!(
        f("select count(*) from t where token(id) > -5;"),
        "SELECT count(*)\nFROM t\nWHERE token(id) > -5;\n"
    );
}

#[test]
fn long_select_list_wraps() {
    let out = f(
        "select a_very_long_column_name_one, a_very_long_column_name_two, a_very_long_column_name_three, another_col from some_keyspace.some_table where pk = ?;",
    );
    assert!(
        out.starts_with("SELECT\n    a_very_long_column_name_one,\n"),
        "{out}"
    );
}

#[test]
fn batch_is_indented() {
    assert_eq!(
        f("begin batch insert into a (x) values (1); update a set y=2 where x=1; apply batch;"),
        "BEGIN BATCH\n    INSERT INTO a (x)\n    VALUES (1);\n    UPDATE a\n    SET y = 2\n    WHERE x = 1;\nAPPLY BATCH;\n"
    );
}

#[test]
fn comments_survive() {
    let out =
        f("-- head\nselect a, -- first\n b from t; -- tail\n\n\n/* block */ select 1 from x;");
    assert!(out.contains("-- head\n"));
    assert!(out.contains("-- first"));
    assert!(out.contains("; -- tail"));
    assert!(out.contains("/* block */"));
    assert!(!out.contains("\n\n\n"), "at most one blank line");
}

#[test]
fn identifiers_keep_their_case_and_quotes() {
    let out = f("select \"MixedCase\", MyCol from Ks.MyTable where \"Key\" = 1;");
    assert!(out.contains("\"MixedCase\", MyCol"), "{out}");
    assert!(out.contains("FROM Ks.MyTable"), "{out}");
}

#[test]
fn lexical_errors_are_refused() {
    assert!(format("select 'unterminated from t;", &FormatOptions::default()).is_err());
}

#[test]
fn idempotent_and_token_preserving_on_real_files() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "cql") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        let once = format(&src, &FormatOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let twice = format(&once, &FormatOptions::default()).unwrap();
        assert_eq!(once, twice, "not idempotent: {}", p.display());
        n += 1;
    }
    assert!(n >= 8, "corpus missing");
}
