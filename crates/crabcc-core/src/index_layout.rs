//! Where a repo's index artifacts live on disk.
//!
//! This module owns **one** rule, so every entry point agrees on it:
//!
//! * `InRepo` — legacy `<repo>/.crabcc/`. Selected when that directory
//!   already exists and the layout has not been forced centralised.
//! * `Centralised` — `$CRABCC_HOME/repos/<key>/`. Everything else.
//!   `$CRABCC_HOME` defaults to `~/.crabcc`.
//!
//! It lives in `crabcc-core` because both the CLI (`root_resolver`, which
//! additionally handles git-URL inputs and cloning) and the MCP server
//! (`dispatch`) must resolve the same repo to the same `index.db`. They
//! previously disagreed: the CLI honoured the centralised layout while the
//! MCP server hardcoded `<root>/.crabcc/index.db`. Because `Store::open`
//! creates the file it is pointed at, an MCP client on any centrally-indexed
//! repo silently opened a fresh empty database and every symbol lookup
//! returned `[]` — no error, just no results.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::hash::sha256_hex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    InRepo,
    Centralised,
}

/// Resolved artifact location for a local repo path.
#[derive(Debug, Clone)]
pub struct IndexLayout {
    /// Directory holding index artifacts (db / fsst / graph.json).
    pub data_dir: PathBuf,
    pub layout: Layout,
    /// Cache key under `$CRABCC_HOME/repos/`. `None` for `InRepo`.
    pub key: Option<String>,
}

impl IndexLayout {
    pub fn db(&self) -> PathBuf {
        self.data_dir.join("index.db")
    }
    pub fn graph_json(&self) -> PathBuf {
        self.data_dir.join("graph.json")
    }
}

/// Resolve artifact layout for an already-canonicalised local repo root.
pub fn resolve(repo_root: &Path) -> Result<IndexLayout> {
    resolve_with_home(repo_root, None)
}

/// Path to the repo's `index.db`. The convenience form used by callers
/// that only need the database (e.g. the MCP dispatch loop).
pub fn index_db(repo_root: &Path) -> Result<PathBuf> {
    Ok(resolve(repo_root)?.db())
}

/// Same as [`resolve`] with an explicit `$CRABCC_HOME/repos/` override.
/// Tests use this instead of mutating process-global env vars, which is
/// racy under the parallel test harness.
pub fn resolve_with_home(repo_root: &Path, home_override: Option<&Path>) -> Result<IndexLayout> {
    if use_in_repo_layout(repo_root) {
        return Ok(IndexLayout {
            data_dir: repo_root.join(".crabcc"),
            layout: Layout::InRepo,
            key: None,
        });
    }
    let key = repo_storage_key(repo_root);
    let data_dir = repos_dir(home_override)?.join(&key);
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("failed to create {}", data_dir.display()))?;
    Ok(IndexLayout {
        data_dir,
        layout: Layout::Centralised,
        key: Some(key),
    })
}

/// Stable per-repo key under `$CRABCC_HOME/repos/`.
///
/// Keyed off `remote.origin.url` first so every worktree and clone of a
/// repo shares one index; then the git common dir (covers `git init` with
/// no remote, where worktrees still share `.git`); then the canonical path.
pub fn repo_storage_key(repo_root: &Path) -> String {
    let slug = sanitize_slug(
        &repo_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown-repo".into()),
    );
    if let Some(url) = git_origin_url(repo_root) {
        return format!("{slug}-{}", hash6(url.as_bytes()));
    }
    if let Some(common) = git_common_dir(repo_root) {
        return format!("{slug}-{}", hash6(common.as_bytes()));
    }
    let canon = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    path_key(&canon.to_string_lossy())
}

pub fn sanitize_slug(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_alphanumeric() || lower == '-' || lower == '_' {
                lower
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() {
        "unknown-repo".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 16 hex chars of sha256 — the fallback key for paths outside any git repo.
pub fn path_key(input: &str) -> String {
    sha256_hex(input.as_bytes()).chars().take(16).collect()
}

fn hash6(bytes: &[u8]) -> String {
    sha256_hex(bytes).chars().take(6).collect()
}

/// When true, artifacts live under `<repo>/.crabcc/` (legacy). Opt out on
/// Mac / worktrees with `CRABCC_LAYOUT=centralised`.
fn use_in_repo_layout(abs: &Path) -> bool {
    if layout_forced_centralised() {
        return false;
    }
    let in_repo = abs.join(".crabcc");
    in_repo.exists() && in_repo.is_dir()
}

fn layout_forced_centralised() -> bool {
    match std::env::var("CRABCC_LAYOUT")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("centralised" | "centralized") => true,
        Some("in-repo" | "in_repo") => false,
        _ => std::env::var("CRABCC_IN_REPO_INDEX").ok().as_deref() == Some("0"),
    }
}

fn repos_dir(override_: Option<&Path>) -> Result<PathBuf> {
    let base = match override_ {
        Some(p) => p.to_path_buf(),
        None => match std::env::var_os("CRABCC_HOME") {
            Some(p) => PathBuf::from(p),
            None => {
                let home = std::env::var_os("HOME")
                    .ok_or_else(|| anyhow!("$HOME is unset; set CRABCC_HOME explicitly"))?;
                PathBuf::from(home).join(".crabcc")
            }
        },
    };
    let dir = base.join("repos");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    Ok(dir)
}

fn git_origin_url(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn git_common_dir(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(&raw);
    let abs = if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    };
    abs.canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_repo_layout_wins_when_dot_crabcc_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".crabcc")).unwrap();
        let r = resolve_with_home(dir.path(), Some(dir.path())).unwrap();
        assert_eq!(r.layout, Layout::InRepo);
        assert_eq!(r.db(), dir.path().join(".crabcc").join("index.db"));
    }

    #[test]
    fn centralised_layout_without_dot_crabcc() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let r = resolve_with_home(dir.path(), Some(home.path())).unwrap();
        assert_eq!(r.layout, Layout::Centralised);
        assert!(r.db().starts_with(home.path().join("repos")));
    }

    // The bug this module exists to prevent: the CLI and the MCP server
    // must land on the same file for the same repo.
    #[test]
    fn same_root_resolves_to_same_db() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let a = resolve_with_home(dir.path(), Some(home.path())).unwrap();
        let b = resolve_with_home(dir.path(), Some(home.path())).unwrap();
        assert_eq!(a.db(), b.db());
    }

    #[test]
    fn slug_collapses_non_alphanumerics() {
        assert_eq!(sanitize_slug("My Repo.v2"), "my-repo-v2");
        assert_eq!(sanitize_slug("---"), "unknown-repo");
    }
}
