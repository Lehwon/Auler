//! 작업공간 저장소 시험. 기준 학과 가상 데이터를 넣고 고치고 되돌리고 확정한다.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use auler_domain::model::*;
use auler_domain::resolve::resolve;
use auler_domain::time::Weekday;
use auler_store::*;

/// 테스트마다 다른 임시 파일. 끝나면 지운다
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "auler-store-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.join(format!("{name}.{EXTENSION}")))
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn reference_data() -> TermData {
    resolve(&auler_samples::reference()).expect("기준 학과")
}

/// 기준 학과를 넣은 작업공간
fn seeded(name: &str) -> (TempPath, Workspace, TermKey) {
    let path = TempPath::new(name);
    let ws = Workspace::create(&path.0, "컴퓨터공학과").unwrap();
    let data = reference_data();
    let key = data.setup.key;
    ws.import_term(data, ChangeMeta::direct("예제 학과 넣기"))
        .unwrap();
    (path, ws, key)
}

fn section_id(data: &TermData, course: &str, code: &str) -> SectionId {
    let course = data
        .courses
        .iter()
        .find(|c| c.code.as_str() == course)
        .unwrap();
    data.sections
        .iter()
        .find(|s| s.course == course.id && s.code.as_str() == code)
        .unwrap()
        .id
}

fn room_id(data: &TermData, label: &str) -> RoomId {
    data.rooms
        .iter()
        .find(|r| r.key.to_string() == label)
        .unwrap()
        .id
}

fn place(section: SectionId, part: u8, day: Weekday, start: u8, room: Option<RoomId>) -> Op {
    Op::Put(Record::Placement(Placement {
        meeting: MeetingRef { section, part },
        day,
        start,
        room,
        locked: false,
    }))
}

#[test]
fn create_open_and_lock() {
    let path = TempPath::new("lock");
    {
        let ws = Workspace::create(&path.0, "컴퓨터공학과").unwrap();
        assert_eq!(ws.department().unwrap(), "컴퓨터공학과");
        assert!(matches!(Workspace::open(&path.0), Err(StoreError::Locked)));
        assert!(matches!(
            Workspace::create(&path.0, "x"),
            Err(StoreError::AlreadyExists)
        ));
    }
    // 닫으면 다시 열 수 있다
    let ws = Workspace::open(&path.0).unwrap();
    assert_eq!(ws.department().unwrap(), "컴퓨터공학과");
    assert_eq!(ws.revision().unwrap(), 0);
    let missing = path.0.with_file_name("없음.auler");
    assert!(matches!(Workspace::open(&missing), Err(StoreError::Io(_))));
}

#[test]
fn foreign_files_are_not_opened() {
    let path = TempPath::new("foreign");
    std::fs::write(&path.0, b"not a database at all, just text....................................................................................").unwrap();
    assert!(matches!(
        Workspace::open(&path.0),
        Err(StoreError::NotWorkspace)
    ));

    let other = path.0.with_file_name("other.auler");
    rusqlite_plain_db(&other);
    assert!(matches!(
        Workspace::open(&other),
        Err(StoreError::NotWorkspace)
    ));
}

/// Auler가 아닌 SQLite 파일
fn rusqlite_plain_db(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
}

#[test]
fn newer_workspace_format_is_refused() {
    let path = TempPath::new("newer");
    drop(Workspace::create(&path.0, "컴퓨터공학과").unwrap());
    {
        let conn = rusqlite::Connection::open(&path.0).unwrap();
        conn.pragma_update(None, "user_version", LATEST_VERSION + 1)
            .unwrap();
    }
    assert!(matches!(
        Workspace::open(&path.0),
        Err(StoreError::TooNew { found, supported }) if found == LATEST_VERSION + 1 && supported == LATEST_VERSION
    ));
}

#[test]
fn imported_reference_reads_back_with_workspace_ids() {
    let (path, ws, key) = seeded("import");
    let source = reference_data();
    let data = ws.load_term(key).unwrap();
    assert_eq!(data.setup, source.setup);
    assert_eq!(data.sections.len(), 42);
    assert_eq!(data.rooms.len(), 12);
    assert_eq!(data.instructors.len(), 28);
    assert_eq!(data.constraints.len(), source.constraints.len());
    assert_eq!(data.placements.len(), 2);

    // 자연 키로 비교: 분반의 교원·수강 집단·장비 연결이 그대로다
    for s in &source.sections {
        let course = &source.course(s.course).unwrap().code;
        let got = data.find_section(course, &s.code).unwrap();
        let names = |d: &TermData, ids: &[InstructorId]| -> Vec<String> {
            ids.iter()
                .map(|i| d.instructor(*i).unwrap().code.to_string())
                .collect()
        };
        assert_eq!(
            names(&data, &got.instructors),
            names(&source, &s.instructors)
        );
        assert_eq!(got.split, s.split);
        assert_eq!(got.enrollment, s.enrollment);
    }
    let summary = ws.terms().unwrap();
    assert_eq!(summary.len(), 1);
    assert!(summary[0].editing);
    assert_eq!(summary[0].sections, 42);

    // 학기 만들기 + 가져오기
    let history = ws.history(10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].revision, 2);
    assert_eq!(history[0].term, Some(key));

    // 다시 열어도 같다
    drop(ws);
    let ws = Workspace::open(&path.0).unwrap();
    assert_eq!(ws.load_term(key).unwrap(), data);

    // 같은 자료를 다시 넣어도 기준자료와 분반은 늘지 않는다 (자연 키로 맞춘다)
    let mut again = reference_data();
    again.constraints.clear();
    again.placements.clear();
    ws.import_term(again, ChangeMeta::direct("다시 넣기"))
        .unwrap();
    let after = ws.load_term(key).unwrap();
    assert_eq!(after.sections.len(), 42);
    assert_eq!(after.rooms.len(), 12);
    assert_eq!(after.sections, data.sections);
}

#[test]
fn edits_undo_and_redo() {
    let (_path, ws, key) = seeded("undo");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");
    let r302 = room_id(&data, "공학관 302");
    let before = ws.revision().unwrap();

    // 자료구조 01 두 번째 수업을 목요일 6교시 302호에
    ws.apply(
        Some(key),
        vec![place(ds, 1, Weekday::Thu, 5, Some(r302))],
        ChangeMeta::direct("배치"),
    )
    .unwrap();
    let moved = ws.load_term(key).unwrap();
    let p = moved
        .placement(MeetingRef {
            section: ds,
            part: 1,
        })
        .unwrap();
    assert_eq!((p.day, p.start, p.room), (Weekday::Thu, 5, Some(r302)));

    // 옮기기
    ws.apply(
        Some(key),
        vec![place(ds, 1, Weekday::Fri, 2, Some(r302))],
        ChangeMeta::direct("이동"),
    )
    .unwrap();
    assert!(ws.can_undo().unwrap());
    ws.undo().unwrap();
    let p = ws.load_term(key).unwrap();
    let p = p
        .placement(MeetingRef {
            section: ds,
            part: 1,
        })
        .unwrap()
        .clone();
    assert_eq!((p.day, p.start), (Weekday::Thu, 5));
    ws.undo().unwrap();
    assert!(
        ws.load_term(key)
            .unwrap()
            .placement(MeetingRef {
                section: ds,
                part: 1
            })
            .is_none()
    );
    // 가져오기까지 되돌리면 비지만, 여기서는 다시 실행으로 돌아온다
    ws.redo().unwrap();
    ws.redo().unwrap();
    assert!(matches!(ws.redo(), Err(StoreError::NothingToRedo)));
    let p = ws.load_term(key).unwrap();
    let p = p
        .placement(MeetingRef {
            section: ds,
            part: 1,
        })
        .unwrap()
        .clone();
    assert_eq!((p.day, p.start), (Weekday::Fri, 2));

    // 새 변경을 하면 다시 실행 기록은 사라진다
    ws.undo().unwrap();
    ws.apply(
        Some(key),
        vec![Op::Remove(RecordRef::Placement(MeetingRef {
            section: ds,
            part: 1,
        }))],
        ChangeMeta::direct("배치 빼기"),
    )
    .unwrap();
    assert!(!ws.can_redo().unwrap());

    let history = ws.history(20).unwrap();
    let origins: Vec<Origin> = history.iter().map(|h| h.origin).collect();
    assert_eq!(
        &origins[..7],
        [
            Origin::Direct,
            Origin::Undo,
            Origin::Redo,
            Origin::Redo,
            Origin::Undo,
            Origin::Undo,
            Origin::Direct
        ]
    );
    // 배치, 이동, 되돌리기 2, 다시 실행 2, 되돌리기, 배치 빼기
    assert_eq!(ws.revision().unwrap(), before + 8);
}

#[test]
fn import_can_be_undone() {
    let (_path, ws, key) = seeded("undo-import");
    ws.undo().unwrap();
    let data = ws.load_term(key).unwrap();
    assert!(data.sections.is_empty());
    assert!(data.rooms.is_empty());
    // 학기는 남는다 (학기 만들기는 되돌리지 않는다)
    assert_eq!(ws.terms().unwrap().len(), 1);
    ws.redo().unwrap();
    assert_eq!(ws.load_term(key).unwrap().sections.len(), 42);
}

#[test]
fn same_request_id_applies_once() {
    let (_path, ws, key) = seeded("request");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");
    let op = || vec![place(ds, 0, Weekday::Mon, 0, None)];
    let meta = || {
        ChangeMeta::direct("MCP 초안 적용")
            .origin(Origin::Mcp)
            .actor("claude-code")
            .request("req-001")
    };
    let first = ws.apply(Some(key), op(), meta()).unwrap();
    let second = ws.apply(Some(key), op(), meta()).unwrap();
    assert!(!first.duplicate);
    assert!(second.duplicate);
    assert_eq!(first.revision, second.revision);
    assert_eq!(ws.revision().unwrap(), first.revision);
    let h = &ws.history(1).unwrap()[0];
    assert_eq!(h.request_id.as_deref(), Some("req-001"));
    assert_eq!(h.actor, "claude-code");
}

#[test]
fn stale_base_revision_is_rejected() {
    let (_path, ws, key) = seeded("stale");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");
    let seen = ws.revision().unwrap();
    ws.apply(
        Some(key),
        vec![place(ds, 0, Weekday::Mon, 0, None)],
        ChangeMeta::direct("먼저"),
    )
    .unwrap();
    let late = ws.apply(
        Some(key),
        vec![place(ds, 0, Weekday::Tue, 0, None)],
        ChangeMeta::direct("늦게").base(seen),
    );
    assert!(
        matches!(late, Err(StoreError::StaleRevision { expected, current }) if expected == seen && current == seen + 1)
    );
}

#[test]
fn records_in_use_are_not_removed() {
    let (_path, ws, key) = seeded("in-use");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");
    let teacher = data.section(ds).unwrap().instructors[0];
    let err = ws
        .apply(
            None,
            vec![Op::Remove(RecordRef::Instructor(teacher))],
            ChangeMeta::direct("교원 삭제"),
        )
        .unwrap_err();
    let StoreError::InUse { used_by, .. } = err else {
        panic!("{err}")
    };
    // 자료구조 01분반과 그 교원의 요청 세 건
    assert!(used_by.contains(&RecordRef::Section(ds)));
    assert_eq!(
        used_by
            .iter()
            .filter(|r| matches!(r, RecordRef::Constraint(_)))
            .count(),
        3
    );

    let r301 = room_id(&data, "공학관 301");
    let err = ws
        .apply(
            None,
            vec![Op::Remove(RecordRef::Room(r301))],
            ChangeMeta::direct("강의실 삭제"),
        )
        .unwrap_err();
    assert!(
        matches!(err, StoreError::InUse { .. }),
        "잠근 캡스톤 배치가 쓰는 강의실"
    );

    // 배치가 있는 분반도 지우지 않는다
    let capstone = section_id(&data, "CSE4010", "01");
    let err = ws
        .apply(
            Some(key),
            vec![Op::Remove(RecordRef::Section(capstone))],
            ChangeMeta::direct("분반 삭제"),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::InUse { .. }));

    // 실패한 변경은 아무것도 남기지 않는다
    assert_eq!(ws.load_term(key).unwrap(), data);
    assert_eq!(ws.history(1).unwrap()[0].revision, 2);
}

#[test]
fn invalid_changes_roll_back_whole_batch() {
    let (_path, ws, key) = seeded("invalid");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");

    // 첫 작업은 맞지만 둘째가 틀리면 둘 다 적용하지 않는다
    let err = ws
        .apply(
            Some(key),
            vec![
                place(ds, 0, Weekday::Mon, 0, None),
                place(ds, 1, Weekday::Mon, 9, None),
            ],
            ChangeMeta::direct("마지막 교시 넘김"),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "{err}");
    assert!(
        ws.load_term(key)
            .unwrap()
            .placement(MeetingRef {
                section: ds,
                part: 0
            })
            .is_none()
    );

    let sat = ws.apply(
        Some(key),
        vec![place(ds, 0, Weekday::Sat, 0, None)],
        ChangeMeta::direct("토요일"),
    );
    assert!(matches!(sat, Err(StoreError::Invalid(_))));
    let no_part = ws.apply(
        Some(key),
        vec![place(ds, 2, Weekday::Mon, 0, None)],
        ChangeMeta::direct("없는 단위"),
    );
    assert!(matches!(no_part, Err(StoreError::Invalid(_))));

    let mut dup = data.courses[1].clone();
    dup.code = data.courses[0].code.clone();
    let err = ws
        .apply(
            None,
            vec![Op::Put(Record::Course(dup))],
            ChangeMeta::direct("학수번호 중복"),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::Duplicate(_)), "{err}");

    let mut orphan = data.section(ds).unwrap().clone();
    orphan.id = SectionId(9999);
    orphan.code = auler_domain::code::SectionCode::new("09").unwrap();
    orphan.instructors = vec![InstructorId(9999)];
    let err = ws
        .apply(
            Some(key),
            vec![Op::Put(Record::Section(orphan))],
            ChangeMeta::direct("없는 교원"),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::Missing(RecordRef::Instructor(InstructorId(9999)))
        ),
        "{err}"
    );

    // 학기 없이 학기 기록을 바꾸려 하면 거절
    let err = ws
        .apply(
            None,
            vec![place(ds, 0, Weekday::Mon, 0, None)],
            ChangeMeta::direct("학기 없음"),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)));

    // 배치가 있는데 교시표를 줄이면 거절
    let mut setup = data.setup.clone();
    setup.periods = auler_domain::time::PeriodTable::uniform(
        auler_domain::time::ClockTime::from_hm(9, 0).unwrap(),
        6,
        50,
        10,
    );
    let err = ws
        .apply(
            Some(key),
            vec![Op::SetSetup(setup)],
            ChangeMeta::direct("교시 줄이기"),
        )
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Invalid(_)),
        "금요일 7교시 졸업세미나 배치가 있다: {err}"
    );
    assert_eq!(ws.load_term(key).unwrap(), data);
}

#[test]
fn finalize_snapshots_and_correction_drafts() {
    let (_path, ws, key) = seeded("finalize");
    let data = ws.load_term(key).unwrap();
    let ds = section_id(&data, "CSE2010", "01");
    let checked = ws.revision().unwrap();

    // 점검 뒤에 변경이 있으면 확정하지 않는다
    ws.apply(
        Some(key),
        vec![place(ds, 0, Weekday::Mon, 2, None)],
        ChangeMeta::direct("배치"),
    )
    .unwrap();
    let stale = ws.finalize(key, checked, ChangeMeta::direct("확정"));
    assert!(matches!(stale, Err(StoreError::StaleRevision { .. })));

    let checked = ws.revision().unwrap();
    let v1 = ws
        .finalize(key, checked, ChangeMeta::direct("확정").request("fin-1"))
        .unwrap();
    assert_eq!(v1.version, 1);
    // 같은 요청 ID로 다시 확정해도 같은 결과
    let again = ws
        .finalize(key, checked, ChangeMeta::direct("확정").request("fin-1"))
        .unwrap();
    assert_eq!(again, v1);
    assert!(!ws.can_undo().unwrap(), "확정 전 편집은 되돌릴 수 없다");

    // 확정된 학기는 고칠 수 없다
    let err = ws
        .apply(
            Some(key),
            vec![place(ds, 0, Weekday::Tue, 2, None)],
            ChangeMeta::direct("확정 뒤 수정"),
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::Finalized(k) if k == key));
    // 작업공간 기준자료는 고칠 수 있다 (확정본 스냅숏은 그대로)
    let mut teacher = data.instructors[0].clone();
    teacher.department = Some("소속 변경".into());
    ws.apply(
        None,
        vec![Op::Put(Record::Instructor(teacher))],
        ChangeMeta::direct("소속 수정"),
    )
    .unwrap();

    // 수정용 초안 → 고치기 → 2판 확정
    assert!(matches!(
        ws.finalize(key, ws.revision().unwrap(), ChangeMeta::direct("다시 확정")),
        Err(StoreError::Finalized(_))
    ));
    ws.start_correction(key, ChangeMeta::direct("수정용 초안"))
        .unwrap();
    assert!(matches!(
        ws.start_correction(key, ChangeMeta::direct("또")),
        Err(StoreError::NotFinalized(_))
    ));
    ws.apply(
        Some(key),
        vec![place(ds, 0, Weekday::Tue, 2, None)],
        ChangeMeta::direct("정정"),
    )
    .unwrap();
    let v2 = ws
        .finalize(key, ws.revision().unwrap(), ChangeMeta::direct("2판 확정"))
        .unwrap();
    assert_eq!(v2.version, 2);

    let first = ws.load_finalized(key, 1).unwrap();
    let second = ws.load_finalized(key, 2).unwrap();
    let day = |d: &TermData| {
        d.placement(MeetingRef {
            section: ds,
            part: 0,
        })
        .unwrap()
        .day
    };
    assert_eq!(day(&first), Weekday::Mon);
    assert_eq!(day(&second), Weekday::Tue);
    assert_eq!(
        first.instructors[0].department.as_deref(),
        Some("컴퓨터공학과"),
        "1판은 확정 당시 그대로"
    );
    assert_eq!(
        second.instructors[0].department.as_deref(),
        Some("소속 변경")
    );
    let list = ws.finalized(key).unwrap();
    assert_eq!(list.iter().map(|f| f.version).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(ws.terms().unwrap()[0].last_finalized, Some(2));
    assert!(!ws.terms().unwrap()[0].editing);
}

#[test]
fn second_term_and_pending_constraints_are_kept() {
    let (_path, ws, key) = seeded("terms");
    let mut setup = ws.load_term(key).unwrap().setup;
    setup.key.season = TermSeason::Second;
    ws.create_term(setup.clone(), ChangeMeta::direct("2학기 만들기"))
        .unwrap();
    assert!(matches!(
        ws.create_term(setup.clone(), ChangeMeta::direct("또")),
        Err(StoreError::TermExists(_))
    ));
    let second = ws.load_term(setup.key).unwrap();
    // 기준자료는 작업공간에 있어 두 학기가 함께 본다. 분반·조건·배치는 학기마다 따로다
    assert_eq!(second.rooms.len(), 12);
    assert!(second.sections.is_empty());
    assert!(second.constraints.is_empty());

    let first = ws.load_term(key).unwrap();
    let pending: Vec<&Constraint> = first
        .constraints
        .iter()
        .filter(|c| !c.is_active())
        .collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].text.as_deref(),
        Some("오전에 수업하기 어렵습니다.")
    );
}
