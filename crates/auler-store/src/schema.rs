//! SQLite 스키마와 마이그레이션.
//!
//! 기록마다 키 열(유일성과 외래 키 검사용)과 JSON 본문을 둔다. 본문은 도메인 모델을
//! 그대로 직렬화한 것이다. 마이그레이션은 `user_version`으로 차례를 센다.

use rusqlite::{Connection, OpenFlags};

use crate::error::{Result, StoreError};

/// 파일 머리의 `application_id`: "AULR"
pub const APPLICATION_ID: i32 = 0x4155_4C52;

/// 순서대로 적용한다. 이미 배포한 항목은 고치지 않고 뒤에 덧붙인다.
const MIGRATIONS: &[&str] = &[
    // 1: 첫 스키마
    "
    CREATE TABLE meta (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE courses (
        id   INTEGER PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        body TEXT NOT NULL
    );
    CREATE TABLE instructors (
        id   INTEGER PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        body TEXT NOT NULL
    );
    CREATE TABLE student_groups (
        id   INTEGER PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        body TEXT NOT NULL
    );
    CREATE TABLE equipment (
        id   INTEGER PRIMARY KEY,
        code TEXT NOT NULL UNIQUE,
        body TEXT NOT NULL
    );
    CREATE TABLE rooms (
        id       INTEGER PRIMARY KEY,
        building TEXT NOT NULL,
        number   TEXT NOT NULL,
        body     TEXT NOT NULL,
        UNIQUE (building, number)
    );
    CREATE TABLE terms (
        id      INTEGER PRIMARY KEY,
        year    INTEGER NOT NULL,
        season  TEXT NOT NULL,
        setup   TEXT NOT NULL,
        -- 1이면 초안을 고칠 수 있다. 확정하면 0, 수정용 초안을 만들면 다시 1
        editing INTEGER NOT NULL DEFAULT 1,
        UNIQUE (year, season)
    );
    CREATE TABLE sections (
        id        INTEGER PRIMARY KEY,
        term_id   INTEGER NOT NULL REFERENCES terms(id),
        course_id INTEGER NOT NULL REFERENCES courses(id),
        code      TEXT NOT NULL,
        body      TEXT NOT NULL,
        UNIQUE (term_id, course_id, code)
    );
    CREATE TABLE constraints (
        id      INTEGER PRIMARY KEY,
        term_id INTEGER NOT NULL REFERENCES terms(id),
        body    TEXT NOT NULL
    );
    -- 현재 초안의 배치
    CREATE TABLE placements (
        section_id INTEGER NOT NULL REFERENCES sections(id),
        part       INTEGER NOT NULL,
        day        INTEGER NOT NULL,
        start      INTEGER NOT NULL,
        room_id    INTEGER REFERENCES rooms(id),
        locked     INTEGER NOT NULL,
        PRIMARY KEY (section_id, part)
    );
    -- 확정본: 바꿀 수 없는 스냅숏
    CREATE TABLE finalized (
        id            INTEGER PRIMARY KEY,
        term_id       INTEGER NOT NULL REFERENCES terms(id),
        version       INTEGER NOT NULL,
        revision      INTEGER NOT NULL,
        at            INTEGER NOT NULL,
        actor         TEXT NOT NULL,
        request_id    TEXT UNIQUE,
        snapshot      TEXT NOT NULL,
        UNIQUE (term_id, version)
    );
    -- 변경 이력: 실행자, 시각, 요청 ID, 기준 버전, 내용과 되돌리는 내용
    CREATE TABLE changes (
        revision      INTEGER PRIMARY KEY,
        term_id       INTEGER REFERENCES terms(id),
        at            INTEGER NOT NULL,
        actor         TEXT NOT NULL,
        origin        TEXT NOT NULL,
        request_id    TEXT UNIQUE,
        base_revision INTEGER,
        summary       TEXT NOT NULL,
        ops           TEXT NOT NULL,
        inverse       TEXT NOT NULL
    );
    CREATE INDEX sections_term ON sections(term_id);
    CREATE INDEX constraints_term ON constraints(term_id);
    CREATE INDEX changes_term ON changes(term_id);
    ",
];

/// 이 앱이 아는 마지막 스키마 버전
pub const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

/// 쓰기 연결. 새 파일이면 스키마를 만들고, 오래된 파일이면 마이그레이션한다.
pub fn open_writer(path: &std::path::Path, create: bool) -> Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let mut conn = Connection::open_with_flags(path, flags)?;
    configure(&conn)?;
    // SQLite 파일이 아니면 첫 읽기에서 "not a database"가 난다
    let app_id: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(|e| match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::NotADatabase) => StoreError::NotWorkspace,
            _ => e.into(),
        })?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    match (create, app_id, version) {
        (true, 0, 0) => {
            conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        (false, APPLICATION_ID, _) => {}
        _ => return Err(StoreError::NotWorkspace),
    }
    if version > LATEST_VERSION {
        return Err(StoreError::TooNew {
            found: version,
            supported: LATEST_VERSION,
        });
    }
    // WAL: 읽기 연결이 쓰기를 기다리지 않는다. 같은 PC의 프로세스만 쓸 수 있다.
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "wal", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Invalid(format!("WAL을 켤 수 없음: {mode}")));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let tx = conn.transaction()?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
    }
    tx.commit()?;
    Ok(conn)
}

/// 읽기 전용 연결
pub fn open_reader(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&conn)?;
    Ok(conn)
}
