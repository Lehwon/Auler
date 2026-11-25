use std::fmt;

use auler_domain::{model::TermKey, resolve::Issue};

use crate::{FinalizationChecks, RecordRef, SelectionIssue};

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    /// 다른 Auler 창이 이 작업공간을 쓰고 있다
    Locked,
    /// 이미 있는 파일에 새 작업공간을 만들려 했다
    AlreadyExists,
    /// Auler 작업공간 파일이 아니다
    NotWorkspace,
    BackupPermissions(std::io::Error),
    BackupIntegrity,
    /// 이 앱보다 새 버전이 만든 작업공간이다
    TooNew {
        found: i64,
        supported: i64,
    },
    UnknownTerm(TermKey),
    UnknownReview(i64),
    StaleReview,
    ReviewUnavailable,
    ReviewRequestConflict,
    ReviewDocument(Vec<Issue>),
    ReviewBlocked(Vec<SelectionIssue>),
    ReviewSelection(Vec<String>),
    TermExists(TermKey),
    /// 확정된 학기다. 수정용 초안을 만들어야 고칠 수 있다
    Finalized(TermKey),
    FinalizationBlocked(FinalizationChecks),
    FinalizationRequestConflict,
    /// 수정용 초안을 만들 확정본이 없거나 이미 초안이 있다
    NotFinalized(TermKey),
    /// 기준 버전 뒤에 다른 변경이 있었다
    StaleRevision {
        expected: u64,
        current: u64,
    },
    /// 지우려는 기록을 다른 기록이 쓰고 있다. 자동으로 함께 지우지 않는다
    InUse {
        what: RecordRef,
        used_by: Vec<RecordRef>,
    },
    /// 없는 기록을 가리킨다
    Missing(RecordRef),
    /// 같은 키(코드, 건물+호실 등)가 이미 있다
    Duplicate(String),
    /// 규칙에 맞지 않는 값
    Invalid(String),
    NothingToUndo,
    NothingToRedo,
    /// 쓰기 스레드가 멈췄다
    WriterGone,
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "SQLite: {e}"),
            Self::Io(e) => write!(f, "파일: {e}"),
            Self::Json(e) => write!(f, "저장된 자료를 읽지 못함: {e}"),
            Self::Locked => write!(f, "다른 창이 이 작업공간을 열고 있음"),
            Self::AlreadyExists => write!(f, "같은 이름의 파일이 이미 있음"),
            Self::NotWorkspace => write!(f, "Auler 작업공간 파일이 아님"),
            Self::BackupPermissions(error) => {
                write!(f, "백업 파일 접근 권한을 설정할 수 없음 ({error})")
            }
            Self::BackupIntegrity => write!(f, "백업 파일의 무결성 또는 건수 검사 실패"),
            Self::TooNew { found, supported } => {
                write!(f, "더 새 버전의 작업공간 (형식 {found}, 지원 {supported})")
            }
            Self::UnknownTerm(t) => write!(f, "학기 {t} 없음"),
            Self::UnknownReview(id) => write!(f, "검토 초안 {id} 없음"),
            Self::StaleReview => write!(f, "검토 초안이 변경됨. 최신 초안을 다시 확인해야 함"),
            Self::ReviewUnavailable => write!(f, "현재 상태에서 검토 초안을 변경할 수 없음"),
            Self::ReviewRequestConflict => write!(f, "같은 요청 ID의 검토 내용이 다름"),
            Self::ReviewDocument(_) => write!(f, "검토 초안 문서에 오류가 있음"),
            Self::ReviewBlocked(rows) => write!(
                f,
                "선택한 행을 적용할 수 없음 ({})",
                rows.iter()
                    .map(|row| row.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::ReviewSelection(rows) => {
                write!(f, "선택한 행을 적용할 수 없음 ({})", rows.join(", "))
            }
            Self::TermExists(t) => write!(f, "학기 {t} 이미 있음"),
            Self::Finalized(t) => write!(f, "학기 {t}는 확정됨. 수정용 초안이 필요함"),
            Self::FinalizationBlocked(checks) if !checks.required_passed() => {
                write!(f, "확정 전에 필수 배치 검사를 해결해야 함")
            }
            Self::FinalizationBlocked(_) => write!(f, "확정 전에 남은 경고를 확인해야 함"),
            Self::FinalizationRequestConflict => {
                write!(f, "같은 확정 요청 ID가 다른 학기나 기준 버전에 사용됨")
            }
            Self::NotFinalized(t) => write!(f, "학기 {t}에 수정용 초안을 만들 확정본이 없음"),
            Self::StaleRevision { expected, current } => {
                write!(f, "기준 버전 {expected} 뒤에 변경이 있음 (현재 {current})")
            }
            Self::InUse { what, used_by } => write!(f, "{what:?}를 {used_by:?}가 쓰고 있음"),
            Self::Missing(r) => write!(f, "{r:?} 없음"),
            Self::Duplicate(k) => write!(f, "{k} 이미 있음"),
            Self::Invalid(m) => write!(f, "{m}"),
            Self::NothingToUndo => write!(f, "되돌릴 변경 없음"),
            Self::NothingToRedo => write!(f, "다시 할 변경 없음"),
            Self::WriterGone => write!(f, "저장 스레드가 멈춤"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
