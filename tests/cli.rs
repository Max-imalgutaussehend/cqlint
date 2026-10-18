use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cqlint"))
}

fn write(dir: &std::path::Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn check_fails_on_warnings_and_passes_when_clean() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "bad.cql", "select * from t;\n");
    let out = bin().args(["check"]).arg(d.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("CQLINT002") && s.contains("CQLINT003"), "{s}");
    write(d.path(), "bad.cql", "SELECT a FROM t LIMIT 1;\n");
    assert_eq!(
        bin().args(["check"]).arg(d.path()).status().unwrap().code(),
        Some(0)
    );
}

#[test]
fn notes_only_fail_in_strict_mode() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "a.cql",
        "INSERT INTO t (a) VALUES (1) IF NOT EXISTS;\n",
    );
    assert_eq!(
        bin().arg("check").arg(d.path()).status().unwrap().code(),
        Some(0)
    );
    assert_eq!(
        bin()
            .args(["check", "--strict"])
            .arg(d.path())
            .status()
            .unwrap()
            .code(),
        Some(1)
    );
}

#[test]
fn json_and_github_output() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "a.cql", "select * from t limit 1;\n");
    let j = bin()
        .args(["check", "--output-format", "json", "--exit-zero"])
        .arg(d.path())
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert_eq!(v[0]["finding"]["code"], "CQLINT002");
    let g = bin()
        .args(["check", "--output-format", "github", "--exit-zero"])
        .arg(d.path())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&g.stdout).starts_with("::warning file="));
}

#[test]
fn fix_adds_if_not_exists() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "a.cql", "CREATE TABLE t (a int PRIMARY KEY);\n");
    assert_eq!(
        bin()
            .args(["check", "--select", "CQLINT010", "--fix"])
            .arg(d.path())
            .status()
            .unwrap()
            .code(),
        Some(0)
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("a.cql")).unwrap(),
        "CREATE TABLE IF NOT EXISTS t (a int PRIMARY KEY);\n"
    );
}

#[test]
fn format_check_and_write() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "a.cql", "select a from t where k=1;\n");
    assert_eq!(
        bin()
            .args(["format", "--check"])
            .arg(d.path())
            .status()
            .unwrap()
            .code(),
        Some(1)
    );
    assert_eq!(
        bin().arg("format").arg(d.path()).status().unwrap().code(),
        Some(0)
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("a.cql")).unwrap(),
        "SELECT a\nFROM t\nWHERE k = 1;\n"
    );
    assert_eq!(
        bin()
            .args(["format", "--check"])
            .arg(d.path())
            .status()
            .unwrap()
            .code(),
        Some(0)
    );
}

#[test]
fn format_stdin() {
    use std::io::Write;
    let mut c = bin()
        .args(["format", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(b"select 1 from t;")
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "SELECT 1\nFROM t;\n");
}

#[test]
fn config_file_is_honoured() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "cqlint.toml", "ignore = [\"select-star\"]\n");
    write(d.path(), "a.cql", "select * from t limit 1;\n");
    let out = bin()
        .arg("check")
        .arg(d.path())
        .current_dir(d.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn schema_aware_checks() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("schema")).unwrap();
    write(
        &d.path().join("schema"),
        "s.cql",
        "CREATE TABLE t (id uuid, name text, PRIMARY KEY (id));\n",
    );
    write(d.path(), "q.cql", "SELECT nope FROM t WHERE id = ?;\n");
    let out = bin()
        .args(["check", "--schema"])
        .arg(d.path().join("schema"))
        .arg(d.path().join("q.cql"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("CQL"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn rules_listing() {
    let out = bin().arg("rules").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("CQLINT013"));
    assert!(
        bin()
            .args(["rules", "no-such-rule"])
            .status()
            .unwrap()
            .code()
            != Some(0)
    );
}
