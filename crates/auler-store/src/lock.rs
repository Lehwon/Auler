//! 작업공간 잠금: 한 작업공간은 한 프로세스만 쓴다.
//!
//! 작업공간 파일 옆에 `<이름>.auler-lock`을 두고 OS 파일 잠금을 건다. 프로세스가 비정상으로
//! 끝나도 OS가 잠금을 풀기 때문에 남은 잠금 파일 때문에 열지 못하는 일이 없다. SQLite 파일
//! 자체에는 잠금을 걸지 않는다(SQLite가 쓰는 바이트 범위 잠금과 부딪히지 않게).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::{Result, StoreError};

pub(crate) struct WorkspaceLock {
    _file: File,
}

pub(crate) fn lock_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push("-lock");
    PathBuf::from(name)
}

impl WorkspaceLock {
    pub fn acquire(db: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path(db))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked),
            Err(TryLockError::Error(e)) => Err(StoreError::Io(e)),
        }
    }
}
