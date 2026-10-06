//! Path resolution + legacy db migration + git-root walk-up.
//!
//! `resolve_db_path` is the public API — every Palace open routes through
//! it. `find_git_root` is the standalone walk-up used by [`PalaceRegistry`]
//! and any caller that wants to canonicalize a `cwd` before invoking the
//! registry.
//!
//! Other helpers (`sanitize_slug`, `repo_storage_key`, `git_origin_url`,
//! `crabcc_home_dir`) are `pub(super)` so the parent module's tests can
//! exercise them directly.

use anyhow::{Context, Result};
use crabcc_core::hash::sha256_hex;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve the on-disk path for a repo's `memory.db`. Layout:
///
/// ```text
/// $CRABCC_HOME/repos/<slug>-<hash6>/memory.db
/// ```
///
/// * `$CRABCC_HOME` defaults to `$HOME/.crabcc` (matches the convention
///   already used by `crabcc-cli/backup.rs`, `model_info`, agents/, etc.).
/// * `<slug>` is the repo dir's basename, ascii-lowered, with non-
///   `[a-z0-9_-]` collapsed to `-`. Falls back to `unknown-repo` when
///   the path has no terminal component.
/// * `<hash6>` is the first 6 hex chars of `sha256(origin_url)`. When
///   no `remote.origin.url` is set (fresh `git init`, non-git dir), the
///   slug stands alone — at the cost of basename collisions across
///   unrelated repos sharing a name.
///
/// Centralising memory.db in `$CRABCC_HOME` means worktrees of the same
/// repo share one db (they share `.git/config`), and `git clean -fdx`
/// in the working tree doesn't blow it away.
pub fn resolve_db_path(repo_root: &Path) -> Result<PathBuf> {
    let home = crabcc_home_dir()?;
    let key = repo_storage_key(repo_root);
    Ok(home.join("repos").join(key).join("memory.db"))
}

pub(super) fn crabcc_home_dir() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CRABCC_HOME") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let home = std::env::var("HOME").context("$HOME is not set")?;
    Ok(PathBuf::from(home).join(".crabcc"))
}

/// Delegates to `crabcc-core` so memory.db and index.db land in the *same*
/// `$CRABCC_HOME/repos/<key>/` directory. They used to disagree: this
/// function returned a bare `slug` when a repo had no `remote.origin.url`,
/// while the index added a `-<hash6>` derived from the git common dir. Repos
/// with an origin were unaffected (both hash the origin URL); origin-less
/// repos — `git init` with no remote, and the CI fixtures — split across two
/// directories. See `migrate_key_change_if_needed` for the carry-forward.
pub(super) fn repo_storage_key(repo_root: &Path) -> String {
    crabcc_core::index_layout::repo_storage_key(repo_root)
}

/// The pre-alignment key rule, kept only so the migration below can find
/// drawers written under it. Do not use for new writes.
fn legacy_repo_storage_key(repo_root: &Path) -> String {
    let raw_basename = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown-repo".into());
    let slug = sanitize_slug(&raw_basename);
    match git_origin_url(repo_root) {
        Some(url) => {
            let hash6: String = sha256_hex(url.as_bytes()).chars().take(6).collect();
            format!("{slug}-{hash6}")
        }
        None => slug,
    }
}

pub(super) fn sanitize_slug(raw: &str) -> String {
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

pub(super) fn git_origin_url(repo_root: &Path) -> Option<String> {
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

/// One-shot migration: if `<repo_root>/.crabcc/memory.db` exists and
/// the new global path doesn't, copy it over so the user's existing
/// drawers carry forward. Idempotent — once the new path exists,
/// subsequent opens skip the check.
pub(super) fn migrate_legacy_if_needed(repo_root: &Path, new_path: &Path) -> Result<()> {
    let legacy = repo_root.join(".crabcc").join("memory.db");
    if !legacy.exists() || new_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    snapshot_if_absent(&legacy, new_path)?;
    eprintln!(
        "crabcc-memory: migrated drawers from {} -> {}",
        legacy.display(),
        new_path.display()
    );
    Ok(())
}

/// One-shot migration for the storage-key alignment: if drawers exist under
/// the old key and nothing exists under the new one, copy them forward.
/// Idempotent and a no-op for every repo with a `remote.origin.url` (old and
/// new keys are identical there).
///
/// Copies rather than renames, matching `migrate_legacy_if_needed`: the old
/// directory stays put, so a downgrade to a pre-alignment build still finds
/// its data instead of silently opening an empty database.
pub(super) fn migrate_key_change_if_needed(repo_root: &Path, new_path: &Path) -> Result<()> {
    if new_path.exists() {
        return Ok(());
    }
    let old_key = legacy_repo_storage_key(repo_root);
    if old_key == repo_storage_key(repo_root) {
        return Ok(());
    }
    let old_path = crabcc_home_dir()?
        .join("repos")
        .join(old_key)
        .join("memory.db");
    if !old_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    snapshot_if_absent(&old_path, new_path)?;
    eprintln!(
        "crabcc-memory: storage key aligned with the symbol index; copied drawers {} -> {}",
        old_path.display(),
        new_path.display()
    );
    Ok(())
}

/// Walk up from `start` looking for `.git/`. Returns the first ancestor
/// containing one, or `None` if not in a git repo.
///
/// Hot paths (the MCP tool dispatch loop) should prefer routing through
/// [`super::PalaceRegistry::open_for`] / [`super::PalaceRegistry::resolve_git_root`],
/// which memoize this walk so a flurry of calls with the same `cwd` only
/// pays the canonicalize + ancestor scan once per minute.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut p = start.canonicalize().ok()?;
    loop {
        if p.join(".git").exists() {
            return Some(p);
        }
        p = p.parent()?.to_path_buf();
    }
}

// A main-file copy misses committed WAL pages. Build a SQLite snapshot beside
// the destination, then publish without replacing another opener's database.
fn snapshot_if_absent(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("migration destination has no parent")?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let connection =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let path = temporary
        .path()
        .to_str()
        .context("snapshot path is not UTF-8")?;
    connection.execute("VACUUM INTO ?1", rusqlite::params![path])?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error.into()),
    }
}

#[cfg(test)]
mod migration_snapshot_tests {
    use super::*;

    #[test]
    fn corrupt_source_does_not_publish_or_leave_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.db");
        let destination = dir.path().join("destination.db");
        std::fs::write(&source, b"not a database").unwrap();
        assert!(snapshot_if_absent(&source, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(&source).unwrap(), b"not a database");
    }

    #[test]
    fn snapshot_preserves_live_wal_and_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.db");
        let destination = dir.path().join("destination.db");
        let conn = rusqlite::Connection::open(&source).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE probe(value TEXT); PRAGMA wal_checkpoint(TRUNCATE); INSERT INTO probe VALUES ('committed');").unwrap();
        snapshot_if_absent(&source, &destination).unwrap();
        let copy = rusqlite::Connection::open(&destination).unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM probe", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "committed"
        );
        copy.execute("UPDATE probe SET value='preserved'", [])
            .unwrap();
        snapshot_if_absent(&source, &destination).unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM probe", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserved"
        );
        assert_eq!(
            conn.query_row("SELECT value FROM probe", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "committed"
        );
    }
}
