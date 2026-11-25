//! Auler 작업공간 저장소.
//!
//! 작업공간은 SQLite 파일 하나(`.auler`)다. 쓰기는 작업공간마다 스레드 하나가 맡고, 읽기는
//! 부르는 쪽 스레드의 읽기 전용 연결로 한다(WAL). 옆에 둔 잠금 파일로 두 번째 프로세스가
//! 같은 작업공간을 여는 것을 막는다.
//!
//! - 변경은 작업(`Op`) 목록 하나가 한 트랜잭션이다. 변경 번호(revision), 실행자, 시각,
//!   경로, 요청 ID, 기준 번호를 이력에 남기고, 같은 요청 ID는 두 번 적용하지 않는다.
//! - 변경마다 되돌리는 작업을 만들어 두어 실행 취소·다시 실행을 한다.
//! - 다른 기록이 쓰는 기록은 지우지 않는다. 쓰는 곳 목록을 돌려준다.
//! - 확정본은 바꿀 수 없는 스냅숏이다. 확정은 기준 번호를 다시 확인하고 한 트랜잭션으로
//!   저장한다. 확정 뒤 고치려면 수정용 초안을 연다.

mod backup;
mod backup_permissions;
mod error;
mod finalize;
mod lock;
mod read;
mod review;
mod schema;
mod writer;

use std::path::{Path, PathBuf};

use auler_domain::model::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub use backup::{BackupSummary, VerifiedRestore};
pub use error::{Result, StoreError};
pub use finalize::FinalizationChecks;
pub use review::*;
pub use schema::LATEST_VERSION;
pub use writer::UNDO_LIMIT;

/// 작업공간 파일 확장자
pub const EXTENSION: &str = "auler";

/// 넣거나 고칠 기록
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Record {
    Course(Course),
    Instructor(Instructor),
    Group(StudentGroup),
    Equipment(EquipmentKind),
    Room(Room),
    /// 아래 셋은 학기를 지정한 변경에서만 쓴다
    Section(Section),
    Constraint(Constraint),
    Placement(Placement),
}

/// 기록을 가리킨다
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum RecordRef {
    Course(CourseId),
    Instructor(InstructorId),
    Group(GroupId),
    Equipment(EquipmentId),
    Room(RoomId),
    Section(SectionId),
    Constraint(ConstraintId),
    Placement(MeetingRef),
}

/// 변경 작업 하나
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "value", rename_all = "snake_case")]
pub enum Op {
    /// 같은 ID가 있으면 고치고 없으면 넣는다
    Put(Record),
    Remove(RecordRef),
    /// 학기 설정(기간, 요일, 교시표, 학교 규칙)을 바꾼다
    SetSetup(TermSetup),
}

/// 변경이 들어온 경로
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// 조교가 앱에서 직접
    Direct,
    Xlsx,
    ChatGpt,
    Mcp,
    Solver,
    Undo,
    Redo,
    System,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Xlsx => "xlsx",
            Self::ChatGpt => "chatgpt",
            Self::Mcp => "mcp",
            Self::Solver => "solver",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::System => "system",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Direct,
            Self::Xlsx,
            Self::ChatGpt,
            Self::Mcp,
            Self::Solver,
            Self::Undo,
            Self::Redo,
            Self::System,
        ]
        .into_iter()
        .find(|o| o.as_str() == text)
    }
}

/// 변경에 붙이는 정보
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeMeta {
    /// 실행자 (조교, 외부 클라이언트 이름 등)
    pub actor: String,
    pub origin: Origin,
    /// 같은 요청을 다시 보내도 한 번만 적용한다
    pub request_id: Option<String>,
    /// 이 변경을 만들 때 본 변경 번호. 그 뒤에 다른 변경이 있으면 거절한다
    pub base_revision: Option<u64>,
    /// 이력에 보일 한 줄 설명
    pub summary: String,
}

impl ChangeMeta {
    /// 앱에서 조교가 직접 한 변경
    pub fn direct(summary: impl Into<String>) -> Self {
        Self {
            actor: "local".into(),
            origin: Origin::Direct,
            request_id: None,
            base_revision: None,
            summary: summary.into(),
        }
    }

    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    pub fn actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = actor.into();
        self
    }

    pub fn request(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    pub fn base(mut self, revision: u64) -> Self {
        self.base_revision = Some(revision);
        self
    }
}

/// 적용 결과
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applied {
    pub revision: u64,
    /// 같은 요청 ID로 이미 적용한 변경이라 다시 적용하지 않았다
    pub duplicate: bool,
}

/// 변경 이력 한 줄
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRecord {
    pub revision: u64,
    pub term: Option<TermKey>,
    /// 유닉스 시각(밀리초)
    pub at_ms: i64,
    pub actor: String,
    pub origin: Origin,
    pub request_id: Option<String>,
    pub base_revision: Option<u64>,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermSummary {
    pub key: TermKey,
    /// 초안을 고칠 수 있는지 (확정 뒤 수정용 초안을 열기 전에는 아니다)
    pub editing: bool,
    pub last_finalized: Option<u32>,
    pub sections: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalizedInfo {
    pub term: TermKey,
    pub version: u32,
    pub revision: u64,
    pub at_ms: i64,
    pub actor: String,
}

/// 열린 작업공간
pub struct Workspace {
    path: PathBuf,
    reader: Connection,
    writer: writer::WriterHandle,
    // 연결을 모두 닫은 뒤 잠금을 푼다 (필드는 선언 순서대로 놓인다)
    _lock: lock::WorkspaceLock,
}

impl Workspace {
    /// 새 작업공간 파일을 만든다. 같은 경로에 파일이 있으면 만들지 않는다
    pub fn create(path: impl AsRef<Path>, department: &str) -> Result<Self> {
        Self::start(path.as_ref(), Some(department))
    }

    /// 있는 작업공간을 연다. 오래된 형식이면 마이그레이션한다
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::start(path.as_ref(), None)
    }

    pub fn restore(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<Self> {
        Self::prepare_restore(source, destination)?.open()
    }

    pub fn prepare_restore(
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<VerifiedRestore> {
        backup::restore(source.as_ref(), destination.as_ref())
    }

    pub fn backup_file(
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<BackupSummary> {
        backup::create(source.as_ref(), destination.as_ref())
    }

    fn start(path: &Path, create: Option<&str>) -> Result<Self> {
        match (create.is_some(), path.exists()) {
            (true, true) => return Err(StoreError::AlreadyExists),
            (false, false) => {
                return Err(StoreError::Io(std::io::Error::from(
                    std::io::ErrorKind::NotFound,
                )));
            }
            _ => {}
        }
        let lock = lock::WorkspaceLock::acquire(path)?;
        if create.is_some() {
            std::fs::File::create_new(path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    StoreError::AlreadyExists
                } else {
                    error.into()
                }
            })?;
        }
        Self::start_locked(path, create, lock)
    }

    fn start_locked(path: &Path, create: Option<&str>, lock: lock::WorkspaceLock) -> Result<Self> {
        let conn = match schema::open_writer(path, create.is_some()) {
            Ok(c) => c,
            Err(e) => {
                if create.is_some() {
                    let _ = std::fs::remove_file(path);
                }
                return Err(e);
            }
        };
        let mut w = writer::Writer::new(conn);
        if let Some(department) = create {
            w.init_meta(department)?;
        }
        let reader = schema::open_reader(path)?;
        Ok(Self {
            path: path.to_owned(),
            reader,
            writer: writer::WriterHandle::spawn(w)?,
            _lock: lock,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn department(&self) -> Result<String> {
        Ok(read::meta(&self.reader, "department")?.unwrap_or_default())
    }

    /// 현재 변경 번호
    pub fn revision(&self) -> Result<u64> {
        read::revision(&self.reader)
    }

    pub fn terms(&self) -> Result<Vec<TermSummary>> {
        read::terms(&self.reader)
    }

    /// 학기의 현재 초안
    pub fn load_term(&self, key: TermKey) -> Result<TermData> {
        read::load_term(&self.reader, key)
    }

    pub fn next_section_id(&self) -> Result<SectionId> {
        let last: i64 =
            self.reader
                .query_row("SELECT COALESCE(MAX(id), 0) FROM sections", [], |row| {
                    row.get(0)
                })?;
        let next = last
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("분반 ID 범위를 초과함".into()))?;
        Ok(SectionId(next))
    }

    pub fn next_constraint_id(&self) -> Result<ConstraintId> {
        let last: i64 =
            self.reader
                .query_row("SELECT COALESCE(MAX(id), 0) FROM constraints", [], |row| {
                    row.get(0)
                })?;
        last.checked_add(1)
            .map(ConstraintId)
            .ok_or_else(|| StoreError::Invalid("편성 조건 ID 범위를 초과함".into()))
    }

    /// 최근 변경부터
    pub fn history(&self, limit: usize) -> Result<Vec<ChangeRecord>> {
        read::history(&self.reader, None, limit)
    }

    pub fn history_for_term(&self, key: TermKey, limit: usize) -> Result<Vec<ChangeRecord>> {
        read::history(&self.reader, Some(key), limit)
    }

    pub fn finalized(&self, key: TermKey) -> Result<Vec<FinalizedInfo>> {
        read::finalized_list(&self.reader, key)
    }

    pub fn load_finalized(&self, key: TermKey, version: u32) -> Result<TermData> {
        read::load_finalized(&self.reader, key, version)
    }

    pub fn create_term(&self, setup: TermSetup, meta: ChangeMeta) -> Result<Applied> {
        self.writer.call(move |w| w.create_term(&setup, &meta))
    }

    /// Copy a term's calendar and sections with new section IDs. Year-specific requests and placements start empty.
    pub fn copy_term(&self, source: TermKey, target: TermKey, meta: ChangeMeta) -> Result<Applied> {
        self.writer
            .call(move |w| w.copy_term(source, target, &meta))
    }

    /// 작업 목록을 한 변경으로 적용한다. 학기 기록(분반, 조건, 배치, 학기 설정)을 바꾸려면
    /// `term`을 지정한다
    pub fn apply(&self, term: Option<TermKey>, ops: Vec<Op>, meta: ChangeMeta) -> Result<Applied> {
        self.writer.call(move |w| w.apply(term, ops, &meta))
    }

    pub fn preview_changes(
        &self,
        term: Option<TermKey>,
        ops: Vec<Op>,
        meta: ChangeMeta,
    ) -> Result<()> {
        self.writer
            .call(move |writer| writer.preview_changes(term, &ops, &meta))
    }

    /// 검사를 마친 학기 자료를 넣는다 (예제 학과, 표준 양식에서 바로 만든 작업공간)
    pub fn import_term(&self, data: TermData, meta: ChangeMeta) -> Result<Applied> {
        self.writer.call(move |w| w.import_term(&data, &meta))
    }

    pub fn undo(&self) -> Result<Applied> {
        self.writer.call(|w| w.undo("local"))
    }

    pub fn redo(&self) -> Result<Applied> {
        self.writer.call(|w| w.redo("local"))
    }

    pub fn can_undo(&self) -> Result<bool> {
        self.writer.call(|w| Ok(w.can_undo()))
    }

    pub fn can_redo(&self) -> Result<bool> {
        self.writer.call(|w| Ok(w.can_redo()))
    }

    /// 학기를 확정한다. `expected_revision`은 확정 전 점검을 마친 변경 번호다. 그 뒤에
    /// 변경이 있었으면 거절한다
    pub fn finalize(
        &self,
        key: TermKey,
        expected_revision: u64,
        meta: ChangeMeta,
    ) -> Result<FinalizedInfo> {
        self.writer
            .call(move |w| w.finalize(key, expected_revision, None, &meta))
    }

    /// Validate the current schedule inside the same transaction that writes its immutable snapshot.
    pub fn finalize_checked(
        &self,
        key: TermKey,
        expected_revision: u64,
        acknowledge_warnings: bool,
        meta: ChangeMeta,
    ) -> Result<FinalizedInfo> {
        self.writer
            .call(move |w| w.finalize(key, expected_revision, Some(acknowledge_warnings), &meta))
    }

    /// 확정된 학기에 수정용 초안을 연다
    pub fn start_correction(&self, key: TermKey, meta: ChangeMeta) -> Result<Applied> {
        self.writer.call(move |w| w.start_correction(key, &meta))
    }
}
