use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};

use crate::{Result, StoreError, Workspace, backup_permissions, lock::WorkspaceLock, read, schema};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSummary {
    pub department: String,
    pub revision: u64,
    pub terms: usize,
    pub courses: usize,
    pub instructors: usize,
    pub rooms: usize,
    pub sections: usize,
    pub placements: usize,
    pub finalizations: usize,
}

/// A verified copy with its workspace lock held. SQLite opens it for writing on request.
pub struct VerifiedRestore {
    path: PathBuf,
    summary: BackupSummary,
    lock: WorkspaceLock,
}

impl VerifiedRestore {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn summary(&self) -> &BackupSummary {
        &self.summary
    }

    pub fn open(self) -> Result<Workspace> {
        Workspace::start_locked(&self.path, None, self.lock)
    }
}

pub(crate) fn create(source: &Path, destination: &Path) -> Result<BackupSummary> {
    copy_new(&open_source(source)?, destination)
}

pub(crate) fn restore(source: &Path, destination: &Path) -> Result<VerifiedRestore> {
    let source = open_source(source)?;
    ensure_new(destination)?;
    let lock = WorkspaceLock::acquire(destination)?;
    let summary = copy_new(&source, destination)?;
    Ok(VerifiedRestore {
        path: destination.to_owned(),
        summary,
        lock,
    })
}

fn open_source(source: &Path) -> Result<Connection> {
    let source = schema::open_reader(source)?;
    let application_id: i32 = source
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(|error| match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::NotADatabase) => StoreError::NotWorkspace,
            _ => error.into(),
        })?;
    if application_id != schema::APPLICATION_ID {
        return Err(StoreError::NotWorkspace);
    }
    let version: i64 = source.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > schema::LATEST_VERSION {
        return Err(StoreError::TooNew {
            found: version,
            supported: schema::LATEST_VERSION,
        });
    }
    Ok(source)
}

fn copy_new(source: &Connection, destination: &Path) -> Result<BackupSummary> {
    ensure_new(destination)?;
    let directory = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let private_stage = tempfile::Builder::new()
        .prefix(".auler-backup-")
        .tempdir_in(directory)?;
    backup_permissions::protect(private_stage.path(), true)
        .map_err(StoreError::BackupPermissions)?;
    let stage = tempfile::NamedTempFile::new_in(private_stage.path())?.into_temp_path();
    backup_permissions::protect(&stage, false).map_err(StoreError::BackupPermissions)?;

    // Hold a read snapshot while counting and copying so concurrent writes cannot change the source.
    let source = source.unchecked_transaction()?;
    let expected = counts(&source)?;
    let copied = copy_and_verify(&source, &stage)?;
    if expected != copied {
        return Err(StoreError::BackupIntegrity);
    }
    source.commit()?;
    stage.persist_noclobber(destination).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            StoreError::AlreadyExists
        } else {
            error.error.into()
        }
    })?;
    Ok(copied)
}

fn ensure_new(destination: &Path) -> Result<()> {
    match destination.symlink_metadata() {
        Ok(_) => Err(StoreError::AlreadyExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn copy_and_verify(source: &Connection, destination: &Path) -> Result<BackupSummary> {
    let mut target = Connection::open_with_flags(
        destination,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    {
        let backup = Backup::new(source, &mut target)?;
        backup.run_to_completion(128, Duration::from_millis(20), None)?;
    }
    // Persist a standalone copy even when the source workspace uses WAL.
    let journal_mode: String =
        target.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))?;
    if journal_mode != "delete" {
        return Err(StoreError::BackupIntegrity);
    }
    let consistency: String =
        target.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
    let foreign_keys: i64 =
        target.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if consistency != "ok" || foreign_keys != 0 {
        return Err(StoreError::BackupIntegrity);
    }
    for term in read::terms(&target)? {
        let data = read::load_term(&target, term.key)?;
        if data.sections.len() != term.sections {
            return Err(StoreError::BackupIntegrity);
        }
        for finalization in read::finalized_list(&target, term.key)? {
            let _ = read::load_finalized(&target, term.key, finalization.version)?;
        }
    }
    counts(&target)
}

fn counts(connection: &Connection) -> Result<BackupSummary> {
    let department = read::meta(connection, "department")?.unwrap_or_default();
    let revision = read::revision(connection)?;
    Ok(connection.query_row(
        "SELECT (SELECT count(*) FROM terms), (SELECT count(*) FROM courses),
                (SELECT count(*) FROM instructors), (SELECT count(*) FROM rooms),
                (SELECT count(*) FROM sections), (SELECT count(*) FROM placements),
                (SELECT count(*) FROM finalized)",
        [],
        |row| {
            let count = |column| {
                let value: i64 = row.get(column)?;
                usize::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
            };
            Ok(BackupSummary {
                department,
                revision,
                terms: count(0)?,
                courses: count(1)?,
                instructors: count(2)?,
                rooms: count(3)?,
                sections: count(4)?,
                placements: count(5)?,
                finalizations: count(6)?,
            })
        },
    )?)
}
