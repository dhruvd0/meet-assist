//! One-time import into CASE's application directory. Originals are retained.

use anyhow::{bail, Context, Result};
use std::path::Path;
use tauri::Manager;

/// Import before any settings, database, or model managers open their files.
pub fn migrate(app: &tauri::AppHandle) -> Result<()> {
    if crate::portable::is_portable() {
        return Ok(());
    }
    let destination = app.path().app_data_dir()?;
    let Some(parent) = destination.parent() else {
        return Ok(());
    };
    for identifier in ["com.meetassist.app", "com.pais.handy"] {
        let source = parent.join(identifier);
        if source.join("meet.db").is_file() || source.join("settings_store.json").is_file() {
            let log_source = std::path::PathBuf::from(
                app.path()
                    .app_log_dir()?
                    .to_string_lossy()
                    .replace("com.case.app", identifier),
            );
            migrate_directory(&source, &destination, Some(&log_source))?;
            break;
        }
    }
    Ok(())
}

/// Stage a complete import, then move complete entries into place. A ready
/// staging folder resumes an interrupted install; existing CASE data wins.
pub(crate) fn migrate_directory(
    source: &Path,
    destination: &Path,
    logs: Option<&Path>,
) -> Result<()> {
    let staging = destination.with_extension("case-import");
    let ready = staging.join(".ready");
    let completed = destination.join(".case-migration-complete");
    if completed.exists() {
        return Ok(());
    }
    if !ready.exists() {
        if destination.exists()
            && std::fs::read_dir(destination)?
                .any(|entry| entry.map(|e| e.file_name() != "logs").unwrap_or(true))
        {
            return Ok(());
        }
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with("-wal")
                || name_str.ends_with("-shm")
                || name_str.ends_with("-journal")
            {
                continue;
            }
            if name_str.ends_with(".db") && entry.file_type()?.is_file() {
                // SQLite backup includes committed WAL pages even if the old
                // process is still open; copying only the .db would lose them.
                let conn = rusqlite::Connection::open_with_flags(
                    entry.path(),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                conn.backup(rusqlite::MAIN_DB, staging.join(&name), None)?;
            } else {
                let target_name = if name == "logs" {
                    std::ffi::OsStr::new("previous-data-logs")
                } else {
                    &name
                };
                copy_entry(&entry.path(), &staging.join(target_name))?;
            }
        }
        rebase_source_paths(&staging, source, destination)?;
        if let Some(logs) = logs.filter(|p| p.is_dir()) {
            copy_entry(logs, &staging.join("previous-logs"))?;
        }
        std::fs::write(&ready, b"CASE import ready\n")?;
    }
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(&staging)? {
        let entry = entry?;
        if entry.file_name() == ".ready" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if target.exists() {
            bail!("CASE import refuses to overwrite {}", target.display());
        }
        std::fs::rename(entry.path(), &target)
            .with_context(|| format!("install {}", target.display()))?;
    }
    std::fs::write(
        &completed,
        b"CASE data import complete; original files retained.\n",
    )?;
    std::fs::remove_dir_all(staging)?;
    Ok(())
}

fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
    let kind = std::fs::symlink_metadata(source)?.file_type();
    if kind.is_symlink() {
        let target = std::fs::read_link(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)?;
        #[cfg(windows)]
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(target, destination)?;
        } else {
            std::os::windows::fs::symlink_file(target, destination)?;
        }
    } else if kind.is_dir() {
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if kind.is_file() {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

fn rebase_source_paths(staging: &Path, source: &Path, destination: &Path) -> Result<()> {
    let database = staging.join("meet.db");
    if !database.exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(database)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'sources')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let old = source.to_string_lossy();
        let new = destination.to_string_lossy();
        // External project directories are untouched; only copied app-owned
        // source files should point at the new app directory.
        conn.execute("UPDATE sources SET locator = ?2 || substr(locator, length(?1) + 1)
            WHERE substr(locator, 1, length(?1) + 1) = ?1 || '/' OR substr(locator, 1, length(?1) + 1) = ?1 || char(92)",
            rusqlite::params![old.as_ref(), new.as_ref()])?;
    }
    Ok(())
}
