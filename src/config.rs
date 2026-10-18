use crate::format::FormatOptions;
use crate::rules::LintOptions;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub line_width: Option<usize>,
    pub indent: Option<usize>,
    #[serde(default)]
    pub select: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    pub in_list_limit: Option<usize>,
    pub batch_limit: Option<usize>,
    pub wide_table_columns: Option<usize>,
    /// Directory with the schema `.cql` files; enables type-aware checks from `cqlc-core`.
    pub schema: Option<PathBuf>,
}

impl Config {
    pub fn format_options(&self) -> FormatOptions {
        let d = FormatOptions::default();
        FormatOptions {
            indent: self.indent.unwrap_or(d.indent),
            line_width: self.line_width.unwrap_or(d.line_width),
        }
    }
    pub fn lint_options(&self) -> LintOptions {
        let d = LintOptions::default();
        LintOptions {
            select: self.select.clone(),
            ignore: self.ignore.clone(),
            in_list_limit: self.in_list_limit.unwrap_or(d.in_list_limit),
            batch_limit: self.batch_limit.unwrap_or(d.batch_limit),
            wide_table_columns: self.wide_table_columns.unwrap_or(d.wide_table_columns),
        }
    }
}

/// Finds `cqlint.toml` in `start` or a parent directory.
pub fn discover(start: &Path) -> Result<Option<(PathBuf, Config)>> {
    let mut dir = Some(start.canonicalize().unwrap_or_else(|_| start.to_path_buf()));
    while let Some(d) = dir {
        let f = d.join("cqlint.toml");
        if f.is_file() {
            return Ok(Some((d.clone(), load(&f)?)));
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    Ok(None)
}

pub fn load(path: &Path) -> Result<Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}
