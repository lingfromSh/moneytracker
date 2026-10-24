//! Safe writes: snapshots, atomic replace, conflict detection and include
//! scaffolding. Every mutation of a vault file goes through this module.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, SecondsFormat, Utc};

use crate::types::AuditEntryDto;
use crate::vault::{day_path, month_path, year_path, VaultOptions, BAK_DIR, INDEX_FILE};
use crate::{EngineError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    pub mtime_ms: i64,
    pub size: u64,
    pub hash: String,
}

pub fn file_state(abs: &Path) -> Result<FileState> {
    let meta = fs::metadata(abs).map_err(|e| EngineError::Io(e.to_string()))?;
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let bytes = fs::read(abs).map_err(|e| EngineError::Io(e.to_string()))?;
    Ok(FileState {
        mtime_ms,
        size: meta.len(),
        hash: blake3::hash(&bytes).to_hex().to_string(),
    })
}

pub fn hash_of(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

fn io_err(e: std::io::Error) -> EngineError {
    EngineError::Io(e.to_string())
}

/// `temp file -> fsync -> rename -> fsync(dir)` in the same directory.
pub fn atomic_write(abs: &Path, content: &str) -> Result<()> {
    let dir = abs
        .parent()
        .ok_or_else(|| EngineError::Io(format!("{} has no parent directory", abs.display())))?;
    fs::create_dir_all(dir).map_err(io_err)?;
    let tmp = dir.join(format!(".moneytracker.{}.tmp", std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(io_err)?;
        f.write_all(content.as_bytes()).map_err(io_err)?;
        f.sync_all().map_err(io_err)?;
    }
    if let Ok(meta) = fs::metadata(abs) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    fs::rename(&tmp, abs).map_err(io_err)?;
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Copy the current file into `.bak/` before it is rewritten, then prune.
pub fn snapshot(root: &Path, rel: &Path, max_backups: usize) -> Result<Option<PathBuf>> {
    let abs = root.join(rel);
    if !abs.is_file() {
        return Ok(None);
    }
    let stamp = Utc::now()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
        .replace(':', "-");
    let name = format!(
        "{}.{}",
        abs.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into()),
        stamp
    );
    let bak_dir = root.join(BAK_DIR);
    let target_dir = match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => bak_dir.join(p),
        _ => bak_dir.clone(),
    };
    fs::create_dir_all(&target_dir).map_err(io_err)?;
    let target = target_dir.join(name);
    fs::copy(&abs, &target).map_err(io_err)?;

    // prune oldest snapshots for this file
    if max_backups > 0 {
        let prefix = format!(
            "{}.",
            abs.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        let mut snaps: Vec<PathBuf> = fs::read_dir(&target_dir)
            .map_err(io_err)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(&prefix))
                    .unwrap_or(false)
            })
            .collect();
        snaps.sort();
        let excess = snaps.len().saturating_sub(max_backups);
        for old in snaps.into_iter().take(excess) {
            let _ = fs::remove_file(old);
        }
    }
    Ok(Some(target))
}

#[derive(Debug, Clone)]
pub struct WriteResult {
    pub rel: String,
    pub created: bool,
}

/// Snapshot + atomic write of `root/rel`, refusing when the on-disk file no
/// longer matches the state the caller parsed.
pub fn write_rel(
    root: &Path,
    rel: &Path,
    content: &str,
    expected: Option<&FileState>,
    opts: &VaultOptions,
) -> Result<WriteResult> {
    let abs = root.join(rel);
    let created = !abs.exists();
    if let Some(exp) = expected {
        let current = file_state(&abs)?;
        if current.hash != exp.hash {
            return Err(EngineError::Conflict(format!(
                "{} was modified outside MoneyTracker — refresh and try again",
                rel.display()
            )));
        }
    }
    if !created {
        snapshot(root, rel, opts.max_backups)?;
    }
    atomic_write(&abs, content)?;
    Ok(WriteResult {
        rel: rel.to_string_lossy().to_string(),
        created,
    })
}

fn has_include(content: &str, target: &Path) -> bool {
    let want = target.to_string_lossy().replace('\\', "/");
    content.lines().any(|line| {
        let t = line.trim();
        let t = t.strip_prefix('!').map(str::trim_start).unwrap_or(t);
        match t.strip_prefix("include ") {
            Some(rest) => rest.trim().replace('\\', "/") == want,
            None => false,
        }
    })
}

fn append_include(
    root: &Path,
    container: &Path,
    target: &Path,
    opts: &VaultOptions,
) -> Result<bool> {
    let abs = root.join(container);
    let (mut content, state) = if abs.is_file() {
        let text = fs::read_to_string(&abs).map_err(io_err)?;
        let state = file_state(&abs)?;
        (text, Some(state))
    } else {
        (String::new(), None)
    };
    if has_include(&content, target) {
        return Ok(false);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!("include {}\n", target.to_string_lossy()));
    write_rel(root, container, &content, state.as_ref(), opts)?;
    Ok(true)
}

#[derive(Debug, Clone)]
pub struct DayFile {
    pub rel: PathBuf,
    pub created: bool,
}

/// Make sure `index -> year -> month -> day` contains the file for `date`,
/// creating missing levels. Returns the day file (relative to the vault root).
pub fn ensure_day_file(root: &Path, date: NaiveDate, opts: &VaultOptions) -> Result<DayFile> {
    let year = year_path(date);
    let month = month_path(date);
    let day = day_path(date);

    append_include(root, Path::new(INDEX_FILE), &year, opts)?;
    let year_created = append_include(
        root,
        &year,
        &PathBuf::from(month.file_name().unwrap()),
        opts,
    )?;
    if year_created && !root.join(&year).exists() {
        return Err(EngineError::Vault(format!(
            "could not create {}",
            year.display()
        )));
    }
    // ensure the year file exists even when it already had the include
    if !root.join(&year).is_file() {
        write_rel(
            root,
            &year,
            &format!("; {}\n", date.format("%Y")),
            None,
            opts,
        )?;
    }
    append_include(root, &month, &PathBuf::from(day.file_name().unwrap()), opts)?;
    if !root.join(&month).is_file() {
        write_rel(
            root,
            &month,
            &format!("; {}\n", date.format("%Y-%m")),
            None,
            opts,
        )?;
    }

    let day_abs = root.join(&day);
    let created = !day_abs.is_file();
    if created {
        write_rel(
            root,
            &day,
            &format!("; {}\n", date.format("%Y-%m-%d")),
            None,
            opts,
        )?;
    }
    Ok(DayFile { rel: day, created })
}

pub fn read_rel(root: &Path, rel: &Path) -> Result<(String, FileState)> {
    let abs = root.join(rel);
    let content = fs::read_to_string(&abs).map_err(io_err)?;
    let state = file_state(&abs)?;
    Ok((content, state))
}

pub fn append_audit(path: &Path, entry: &AuditEntryDto) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let line = serde_json::to_string(entry).map_err(|e| EngineError::Io(e.to_string()))?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io_err)?;
    writeln!(f, "{line}").map_err(io_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{create_vault, VaultCreateOptions};
    use tempfile::tempdir;

    fn opts() -> VaultOptions {
        VaultOptions::default()
    }

    #[test]
    fn atomic_write_creates_dirs_and_content() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("2026/2026-10/2026-10-25.ledger");
        atomic_write(&target, "hello\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello\n");
    }

    #[test]
    fn ensure_day_file_builds_the_include_chain() {
        let dir = tempdir().unwrap();
        create_vault(
            dir.path(),
            &VaultCreateOptions {
                base_commodity: None,
                default_funding: None,
                default_unclassified: None,
                template: Some(true),
            },
        )
        .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let day = ensure_day_file(dir.path(), date, &opts()).unwrap();
        assert!(day.created);
        let day_src = fs::read_to_string(dir.path().join(&day.rel)).unwrap();
        assert!(day_src.contains("2026-10-25"));

        let year_src = fs::read_to_string(dir.path().join("2026/2026.ledger")).unwrap();
        assert!(year_src.contains("include 2026-10.ledger"));
        let month_src = fs::read_to_string(dir.path().join("2026/2026-10.ledger")).unwrap();
        assert!(month_src.contains("include 2026-10-25.ledger"));
        let index = fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap();
        assert_eq!(index.matches("2026/2026.ledger").count(), 1);

        // idempotent: no duplicate include lines, no new files
        let again = ensure_day_file(dir.path(), date, &opts()).unwrap();
        assert!(!again.created);
        let index = fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap();
        assert_eq!(index.matches("2026/2026.ledger").count(), 1);
    }

    #[test]
    fn write_rel_refuses_on_external_change() {
        let dir = tempdir().unwrap();
        let rel = Path::new("a.ledger");
        atomic_write(&dir.path().join(rel), "one\n").unwrap();
        let state = file_state(&dir.path().join(rel)).unwrap();
        atomic_write(&dir.path().join(rel), "externally changed\n").unwrap();
        let err = write_rel(dir.path(), rel, "two\n", Some(&state), &opts()).unwrap_err();
        assert!(matches!(err, EngineError::Conflict(_)));
    }

    #[test]
    fn snapshot_is_written_before_overwrite() {
        let dir = tempdir().unwrap();
        let rel = Path::new("2026/2026-10-25.ledger");
        atomic_write(&dir.path().join(rel), "old\n").unwrap();
        write_rel(dir.path(), rel, "new\n", None, &opts()).unwrap();
        let baks: Vec<_> = fs::read_dir(dir.path().join(".bak/2026"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(baks.len(), 1);
        let content = fs::read_to_string(baks[0].path()).unwrap();
        assert_eq!(content, "old\n");
    }
}
