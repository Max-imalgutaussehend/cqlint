use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use cqlint::config::{self, Config};
use cqlint::format::format;
use cqlint::rules::{Finding, RULES, Severity, apply_fixes, lint};
use cqlint::schema::SchemaCheck;
use rayon::prelude::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use walkdir::WalkDir;

/// A very fast linter and formatter for CQL (Cassandra and ScyllaDB).
#[derive(Parser)]
#[command(version, about, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// Path to a `cqlint.toml` (default: search from the current directory upwards).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lint `.cql` files (performance and safety rules, optionally type-aware).
    Check {
        /// Files or directories (default: the current directory).
        paths: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t = Output::Text)]
        output_format: Output,
        /// Apply automatic fixes (rules marked fixable).
        #[arg(long)]
        fix: bool,
        /// Enable more rules (codes, names or `ALL`).
        #[arg(long, value_delimiter = ',')]
        select: Vec<String>,
        /// Disable rules (codes, names or `ALL`).
        #[arg(long, value_delimiter = ',')]
        ignore: Vec<String>,
        /// Directory with the schema `.cql` files; adds type-aware checks from cqlc-core.
        #[arg(long)]
        schema: Option<PathBuf>,
        /// Fail on notes too (default: only warnings and errors fail).
        #[arg(long)]
        strict: bool,
        /// Always exit with code 0.
        #[arg(long)]
        exit_zero: bool,
        /// Print the reason behind each finding.
        #[arg(long)]
        explain: bool,
    },
    /// Format `.cql` files in place (`-` reads stdin and writes stdout).
    Format {
        paths: Vec<PathBuf>,
        /// Do not write; exit with 1 if a file would change.
        #[arg(long)]
        check: bool,
        /// Print a diff instead of writing.
        #[arg(long)]
        diff: bool,
        #[arg(long)]
        line_width: Option<usize>,
    },
    /// List all rules, or explain one.
    Rules { rule: Option<String> },
}

#[derive(Clone, Copy, ValueEnum)]
enum Output {
    Text,
    Json,
    Github,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("cqlint: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn load_config(cli: &Cli) -> Result<(Config, PathBuf)> {
    if let Some(p) = &cli.config {
        return Ok((
            config::load(p)?,
            p.parent().map_or(PathBuf::from("."), |d| d.to_path_buf()),
        ));
    }
    Ok(match config::discover(Path::new("."))? {
        Some((dir, c)) => (c, dir),
        None => (Config::default(), PathBuf::from(".")),
    })
}

fn collect(paths: &[PathBuf], exclude: &[String]) -> Result<Vec<PathBuf>> {
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let mut out = Vec::new();
    for r in roots {
        if r.is_file() {
            out.push(r);
            continue;
        }
        if !r.exists() {
            bail!("{} does not exist", r.display());
        }
        let walker = WalkDir::new(&r).into_iter().filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            e.depth() == 0
                || !(n.starts_with('.')
                    || n == "target"
                    || n == "node_modules"
                    || exclude.iter().any(|x| x == &*n))
        });
        for e in walker.filter_map(|e| e.ok()) {
            if e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("cql"))
            {
                out.push(e.into_path());
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let (cfg, cfg_dir) = load_config(&cli)?;
    match &cli.cmd {
        Cmd::Rules { rule } => {
            match rule {
                None => {
                    for r in RULES {
                        println!(
                            "{}  {:<24} {:<8} {}{}",
                            r.code,
                            r.name,
                            r.severity.as_str(),
                            r.summary,
                            if r.default_on {
                                ""
                            } else {
                                "  [off by default]"
                            }
                        );
                    }
                }
                Some(s) => {
                    let r = cqlint::rules::find_rule(s)
                        .with_context(|| format!("unknown rule `{s}`"))?;
                    println!("{} {}\n\n{}\n\n{}", r.code, r.name, r.summary, r.why);
                    println!(
                        "\nSuppress with `-- cqlint: ignore {}` on or above the statement.",
                        r.code
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Format {
            paths,
            check,
            diff,
            line_width,
        } => {
            let mut opts = cfg.format_options();
            if let Some(w) = line_width {
                opts.line_width = *w;
            }
            if paths.len() == 1 && paths[0] == Path::new("-") {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                print!("{}", format(&s, &opts)?);
                return Ok(ExitCode::SUCCESS);
            }
            let files = collect(paths, &cfg.exclude)?;
            let results: Vec<(PathBuf, Result<(String, String)>)> = files
                .par_iter()
                .map(|p| {
                    let r = std::fs::read_to_string(p)
                        .with_context(|| format!("reading {}", p.display()))
                        .and_then(|s| {
                            let f = format(&s, &opts)
                                .map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                            Ok((s, f))
                        });
                    (p.clone(), r)
                })
                .collect();
            let (mut changed, mut same, mut failed) = (0, 0, 0);
            for (p, r) in results {
                match r {
                    Err(e) => {
                        eprintln!("error: {e:#}");
                        failed += 1;
                    }
                    Ok((old, new)) if old == new => same += 1,
                    Ok((old, new)) => {
                        changed += 1;
                        if *diff {
                            print_diff(&p, &old, &new);
                        } else if *check {
                            println!("Would reformat: {}", p.display());
                        } else {
                            std::fs::write(&p, new)
                                .with_context(|| format!("writing {}", p.display()))?;
                            println!("Reformatted: {}", p.display());
                        }
                    }
                }
            }
            let verb = if *check || *diff {
                "would be reformatted"
            } else {
                "reformatted"
            };
            println!(
                "{changed} file(s) {verb}, {same} file(s) already formatted{}",
                if failed > 0 {
                    format!(", {failed} could not be formatted")
                } else {
                    String::new()
                }
            );
            Ok(if failed > 0 {
                ExitCode::from(2)
            } else if (*check || *diff) && changed > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Cmd::Check {
            paths,
            output_format,
            fix,
            select,
            ignore,
            schema,
            strict,
            exit_zero,
            explain,
        } => {
            let mut lo = cfg.lint_options();
            lo.select.extend(select.iter().cloned());
            lo.ignore.extend(ignore.iter().cloned());
            let files = collect(paths, &cfg.exclude)?;
            let schema_dir = schema
                .clone()
                .or_else(|| cfg.schema.as_ref().map(|s| cfg_dir.join(s)));
            let sc = match &schema_dir {
                Some(d) => {
                    let mut v = Vec::new();
                    for f in collect(std::slice::from_ref(d), &[])? {
                        v.push((f.display().to_string(), std::fs::read_to_string(&f)?));
                    }
                    if v.is_empty() {
                        bail!("no .cql files in schema directory {}", d.display());
                    }
                    Some(SchemaCheck::new(v))
                }
                None => None,
            };
            let schema_files: Vec<PathBuf> = schema_dir
                .as_ref()
                .map(|d| collect(std::slice::from_ref(d), &[]).unwrap_or_default())
                .unwrap_or_default();
            let reports: Vec<(PathBuf, String, Vec<Finding>, usize)> = files
                .par_iter()
                .filter(|p| !schema_files.iter().any(|s| same_file(s, p)))
                .filter_map(|p| {
                    let src = std::fs::read_to_string(p).ok()?;
                    let mut f = lint(&src, &lo);
                    if let Some(sc) = &sc {
                        f.extend(sc.check(&src));
                        f.sort_by_key(|x| x.start);
                    }
                    let mut fixed = 0;
                    if *fix {
                        let (new, n) = apply_fixes(&src, &f);
                        if n > 0 && std::fs::write(p, &new).is_ok() {
                            fixed = n;
                            f = lint(&new, &lo);
                            return Some((p.clone(), new, f, fixed));
                        }
                    }
                    Some((p.clone(), src, f, fixed))
                })
                .collect();
            let total: usize = reports.iter().map(|r| r.2.len()).sum();
            let fixed: usize = reports.iter().map(|r| r.3).sum();
            let fixable: usize = reports
                .iter()
                .map(|r| r.2.iter().filter(|f| f.fix.is_some()).count())
                .sum();
            let failing = reports
                .iter()
                .flat_map(|r| &r.2)
                .filter(|f| {
                    f.severity
                        >= if *strict {
                            Severity::Note
                        } else {
                            Severity::Warning
                        }
                })
                .count();
            match output_format {
                Output::Json => {
                    let v: Vec<_> = reports.iter().flat_map(|(p, _, fs, _)| fs.iter().map(move |f| serde_json::json!({"file": p.display().to_string(), "finding": f}))).collect();
                    println!("{}", serde_json::to_string_pretty(&v)?);
                }
                Output::Github => {
                    for (p, _, fs, _) in &reports {
                        for f in fs {
                            let lvl = match f.severity {
                                Severity::Note => "notice",
                                Severity::Warning => "warning",
                                Severity::Error => "error",
                            };
                            println!(
                                "::{lvl} file={},line={},col={},title={}::{}",
                                p.display(),
                                f.line,
                                f.column,
                                f.code,
                                f.message
                            );
                        }
                    }
                }
                Output::Text => {
                    for (p, src, fs, _) in &reports {
                        for f in fs {
                            print_finding(p, src, f, *explain);
                        }
                    }
                    if fixed > 0 {
                        println!("Fixed {fixed} finding(s).");
                    }
                    if total == 0 {
                        println!("All checks passed ({} file(s)).", reports.len());
                    } else {
                        println!(
                            "Found {total} finding(s) in {} file(s){}.",
                            reports.iter().filter(|r| !r.2.is_empty()).count(),
                            if fixable > 0 {
                                format!(" ({fixable} fixable with `--fix`)")
                            } else {
                                String::new()
                            }
                        );
                    }
                }
            }
            Ok(if *exit_zero || failing == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn print_finding(p: &Path, src: &str, f: &Finding, explain: bool) {
    println!(
        "{}:{}:{}: {} [{}] {}",
        p.display(),
        f.line,
        f.column,
        f.severity.as_str(),
        f.code,
        f.message
    );
    if let Some(line) = src.lines().nth(f.line.saturating_sub(1)) {
        let width = f
            .end
            .saturating_sub(f.start)
            .max(1)
            .min(line.chars().count().saturating_sub(f.column - 1).max(1));
        println!("  {:>4} | {}", f.line, line);
        println!("       | {}{}", " ".repeat(f.column - 1), "^".repeat(width));
    }
    if let (true, Some(h)) = (explain, &f.help) {
        println!("       = {h}");
    }
    if f.fix.is_some() {
        println!("       = fixable with `--fix`");
    }
}

/// A plain line diff (longest common subsequence) for `format --diff`.
fn print_diff(p: &Path, old: &str, new: &str) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    println!("--- {}\n+++ {}", p.display(), p.display());
    if a.len().saturating_mul(b.len()) > 16_000_000 {
        for l in &a {
            println!("-{l}");
        }
        for l in &b {
            println!("+{l}");
        }
        return;
    }
    let mut t = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || t[i][j + 1] >= t[i + 1][j]) {
            println!("+{}", b[j]);
            j += 1;
        } else {
            println!("-{}", a[i]);
            i += 1;
        }
    }
}
