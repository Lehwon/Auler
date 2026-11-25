//! 작업공간 저장소 시험. 기준 학과 가상 데이터를 넣고 고치고 되돌리고 확정한다.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use auler_domain::input::{CodeValue, InputDocument, UnresolvedInput};
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

#[test]
fn copies_previous_term_with_fresh_sections_and_no_schedule_or_requests() {
    let (_path, workspace, source) = seeded("copy-term");
    let target = TermKey {
        season: TermSeason::Second,
        ..source
    };
    let before = workspace.load_term(source).unwrap();
    let copied = workspace
        .copy_term(source, target, ChangeMeta::direct("학기 복사"))
        .unwrap();
    let after = workspace.load_term(target).unwrap();
    assert_eq!(after.setup.periods, before.setup.periods);
    assert_eq!(after.setup.days, before.setup.days);
    assert_eq!(after.setup.start, None);
    assert_eq!(after.setup.end, None);
    assert!(after.placements.is_empty() && after.constraints.is_empty());
    assert_eq!(after.sections.len(), before.sections.len());
    assert!(
        after
            .sections
            .iter()
            .zip(&before.sections)
            .all(|(new, old)| new.id != old.id && new.code == old.code)
    );
    assert_eq!(
        workspace.history_for_term(target, 10).unwrap()[0].revision,
        copied.revision
    );
    assert_eq!(
        workspace.load_term(source).unwrap().sections,
        before.sections
    );
    assert!(
        matches!(workspace.copy_term(source, target, ChangeMeta::direct("중복")), Err(StoreError::TermExists(key)) if key == target)
    );
}

#[test]
fn restores_to_new_file_without_replacing_an_existing_workspace() {
    let (source, workspace, key) = seeded("restore-source");
    let target = TempPath::new("restored");
    let expected = workspace.load_term(key).unwrap();
    let restored = Workspace::restore(&source.0, &target.0).unwrap();
    assert_eq!(
        restored.department().unwrap(),
        workspace.department().unwrap()
    );
    assert_eq!(restored.revision().unwrap(), workspace.revision().unwrap());
    assert_eq!(restored.load_term(key).unwrap(), expected);
    assert!(matches!(
        Workspace::restore(&source.0, &target.0),
        Err(StoreError::AlreadyExists)
    ));
    assert_eq!(restored.load_term(key).unwrap(), expected);

    let bad = TempPath::new("corrupt");
    std::fs::write(&bad.0, b"not a database").unwrap();
    let destination = TempPath::new("invalid-restore");
    assert!(Workspace::restore(&bad.0, &destination.0).is_err());
    assert!(!destination.0.exists());
}

#[test]
fn online_backup_preserves_versions_and_reports_the_restored_counts() {
    let (source, workspace, term) = seeded("live-backup-source");
    let version = workspace
        .finalize(
            term,
            workspace.revision().unwrap(),
            ChangeMeta::direct("Test snapshot"),
        )
        .unwrap();
    let frozen = workspace.load_finalized(term, version.version).unwrap();
    let backup = TempPath::new("protected-backup");
    let summary = Workspace::backup_file(&source.0, &backup.0).unwrap();
    assert_eq!(summary.revision, workspace.revision().unwrap());
    assert_eq!(summary.finalizations, 1);
    assert_eq!(summary.sections, frozen.sections.len());
    assert_eq!(summary.placements, frozen.placements.len());
    assert!(matches!(
        Workspace::backup_file(&source.0, &backup.0),
        Err(StoreError::AlreadyExists)
    ));

    let restored = TempPath::new("restored-backup");
    let checked = Workspace::prepare_restore(&backup.0, &restored.0).unwrap();
    assert_eq!(checked.summary(), &summary);
    assert!(!restored.0.with_extension("auler-wal").exists());
    assert!(!restored.0.with_extension("auler-shm").exists());
    let copy = checked.open().unwrap();
    assert_eq!(copy.load_finalized(term, version.version).unwrap(), frozen);
    assert_eq!(
        workspace.load_finalized(term, version.version).unwrap(),
        frozen
    );
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = backup.0.clone().into_os_string();
        sidecar.push(suffix);
        assert!(!PathBuf::from(sidecar).exists());
    }
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

fn row(draft: &ReviewDraft, path: &str) -> Disposition {
    draft
        .items
        .iter()
        .find(|row| row.item.path == path)
        .unwrap()
        .item
        .disposition()
}

fn selected(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn review_applies_only_selected_rows_and_keeps_source_across_reopen() {
    let (path, ws, key) = seeded("review-partial");
    let before = ws.load_term(key).unwrap();
    let mut input = auler_samples::reference();
    input.setup = None;
    input.courses.clear();
    input.rooms.clear();
    input.equipment.clear();
    input.groups.clear();
    input.instructors.clear();
    input.constraints.clear();
    input.placements.clear();
    input.sections.truncate(3);
    input.sections[0].enrollment += 2;
    input.sections[1].section = CodeValue::number(1);
    input.sections[1].source = Some(SourceRef {
        file: Some("study.xlsx".into()),
        sheet: Some("Sections".into()),
        row: Some(3),
        column: Some("B".into()),
    });
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토").origin(Origin::Xlsx))
        .unwrap();
    assert_eq!(row(&draft, "sections[0]"), Disposition::Update);
    assert_eq!(row(&draft, "sections[1]"), Disposition::NeedsReview);
    assert_eq!(row(&draft, "sections[2]"), Disposition::Unchanged);
    let applied = ws
        .approve_review(
            draft.id,
            draft.version,
            selected(&["sections[0]"]),
            "assistant".into(),
            "once".into(),
        )
        .unwrap();
    let after = ws.load_term(key).unwrap();
    assert_eq!(after.sections.len(), before.sections.len());
    assert_eq!(
        after.sections[0].enrollment.count,
        before.sections[0].enrollment.count + 2
    );
    assert_eq!(&after.sections[1..], &before.sections[1..]);
    assert_eq!(ws.history(1).unwrap()[0].actor, "assistant");
    drop(ws);

    let ws = Workspace::open(&path.0).unwrap();
    let duplicate = ws
        .approve_review(
            draft.id,
            draft.version,
            selected(&["sections[0]"]),
            "assistant".into(),
            "once".into(),
        )
        .unwrap();
    assert!(duplicate.duplicate);
    assert_eq!(applied.revision, duplicate.revision);
    let current = ws.review(draft.id).unwrap();
    assert_eq!(
        current.status(ws.revision().unwrap()),
        ReviewStatus::PartiallyApplied
    );
    assert_eq!(
        current
            .items
            .iter()
            .find(|r| r.item.path == "sections[1]")
            .unwrap()
            .item
            .source
            .as_ref()
            .unwrap()
            .row,
        Some(3)
    );
    assert_eq!(row(&current, "sections[1]"), Disposition::NeedsReview);
}

#[test]
fn review_rechecks_selection_dependencies_and_rolls_back_new_term() {
    let path = TempPath::new("review-dependencies");
    let ws = Workspace::create(&path.0, "Department").unwrap();
    let input = auler_samples::reference();
    let key = reference_data().setup.key;
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("제안"))
        .unwrap();
    assert!(matches!(
        ws.approve_review(draft.id, draft.version, selected(&["setup", "sections[0]"]), "assistant".into(), "try".into()),
        Err(StoreError::ReviewBlocked(issues)) if issues.len() == 1 && issues[0].path == "sections[0]"
    ));
    assert!(ws.terms().unwrap().is_empty());
    assert_eq!(ws.revision().unwrap(), 0);
    assert_eq!(ws.review(draft.id).unwrap().version, draft.version);
    let all = draft
        .items
        .iter()
        .filter(|row| row.item.disposition() == Disposition::New)
        .map(|row| row.item.path.clone())
        .collect();
    let approved = ws
        .approve_review(
            draft.id,
            draft.version,
            all,
            "assistant".into(),
            "try".into(),
        )
        .unwrap();
    assert_eq!(approved.revision, 1);
    assert_eq!(ws.load_term(key).unwrap().sections.len(), 42);
    assert_eq!(
        ws.review(draft.id).unwrap().status(approved.revision),
        ReviewStatus::Applied
    );
}

#[test]
fn review_requires_fresh_baseline_and_leaves_finalized_snapshots_intact() {
    let (_path, ws, key) = seeded("review-baseline");
    let mut input = InputDocument::new(key.year, key.season.code());
    let mut course = auler_samples::reference().courses.remove(0);
    course.name.push_str(" (revised)");
    input.courses.push(course);
    let meta = ChangeMeta::direct("제안")
        .origin(Origin::Mcp)
        .request("request-1");
    let draft = ws.create_review(key, input.clone(), meta.clone()).unwrap();
    assert_eq!(ws.create_review(key, input, meta).unwrap().id, draft.id);
    let finalized = ws
        .finalize(key, ws.revision().unwrap(), ChangeMeta::direct("확정"))
        .unwrap();
    let snapshot = ws.load_finalized(key, finalized.version).unwrap();
    assert_eq!(
        ws.review(draft.id).unwrap().status(finalized.revision),
        ReviewStatus::BaseChanged
    );
    assert!(matches!(
        ws.approve_review(
            draft.id,
            draft.version,
            selected(&["courses[0]"]),
            "assistant".into(),
            "try".into()
        ),
        Err(StoreError::StaleRevision { .. })
    ));
    let fresh = ws.refresh_review(draft.id, draft.version, None).unwrap();
    assert!(matches!(
        ws.approve_review(draft.id, fresh.version, selected(&["courses[0]"]), "assistant".into(), "try".into()),
        Err(StoreError::Finalized(t)) if t == key
    ));
    let closed = ws
        .close_review(
            draft.id,
            fresh.version,
            ReviewDecision::Cancelled,
            "assistant".into(),
        )
        .unwrap();
    assert_eq!(
        closed.status(ws.revision().unwrap()),
        ReviewStatus::Cancelled
    );
    assert_eq!(ws.load_finalized(key, finalized.version).unwrap(), snapshot);
}

#[test]
fn version_one_workspace_gains_reviews_without_changing_saved_data() {
    let (path, ws, key) = seeded("review-migration");
    let expected = ws.load_term(key).unwrap();
    let revision = ws.revision().unwrap();
    drop(ws);
    {
        let connection = rusqlite::Connection::open(&path.0).unwrap();
        connection
            .execute_batch(
                "DROP TABLE review_applications;
                 DROP TABLE review_drafts;
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    }
    let ws = Workspace::open(&path.0).unwrap();
    assert_eq!(ws.load_term(key).unwrap(), expected);
    assert_eq!(ws.revision().unwrap(), revision);
    assert!(ws.reviews().unwrap().is_empty());
}

#[test]
fn review_undo_and_redo_update_the_saved_review_with_the_data() {
    let (_path, ws, key) = seeded("review-undo");
    let mut input = InputDocument::new(key.year, key.season.code());
    let mut proposed = auler_samples::reference().sections.remove(0);
    let original = proposed.enrollment;
    proposed.enrollment += 5;
    input.sections.push(proposed);
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토"))
        .unwrap();
    ws.approve_review(
        draft.id,
        draft.version,
        selected(&["sections[0]"]),
        "assistant".into(),
        "apply".into(),
    )
    .unwrap();
    let undone = ws.undo().unwrap();
    let review = ws.review(draft.id).unwrap();
    assert_eq!(review.status(undone.revision), ReviewStatus::Applicable);
    assert!(review.items[0].decision.is_none());
    assert_eq!(
        ws.load_term(key).unwrap().sections[0].enrollment.count,
        original
    );
    let redone = ws.redo().unwrap();
    let review = ws.review(draft.id).unwrap();
    assert_eq!(review.status(redone.revision), ReviewStatus::Applied);
    assert_eq!(
        review.items[0].decision,
        Some(RowDecision::Applied {
            revision: redone.revision
        })
    );
    assert_eq!(
        ws.load_term(key).unwrap().sections[0].enrollment.count,
        original + 5
    );
}

#[test]
fn review_redo_restores_an_approval_removed_from_the_revised_input() {
    let (_path, ws, key) = seeded("review-redo-revised");
    let data = ws.load_term(key).unwrap();
    let original_name = &data.courses[0].name;
    let mut input = InputDocument::new(key.year, key.season.code());
    input.courses = auler_samples::reference()
        .courses
        .into_iter()
        .take(2)
        .collect();
    for course in &mut input.courses {
        course.name.push_str(" revised");
    }
    let applied_name = input.courses[0].name.clone();
    let draft = ws
        .create_review(key, input.clone(), ChangeMeta::direct("검토"))
        .unwrap();
    let applied = ws
        .approve_review(
            draft.id,
            draft.version,
            selected(&["courses[0]"]),
            "assistant".into(),
            "apply".into(),
        )
        .unwrap();
    input.courses.remove(0);
    let partial = ws.review(draft.id).unwrap();
    ws.refresh_review(draft.id, partial.version, Some(input))
        .unwrap();

    ws.undo().unwrap();
    let review = ws.review(draft.id).unwrap();
    assert_eq!(&ws.load_term(key).unwrap().courses[0].name, original_name);
    assert!(review.items.iter().any(|row| matches!(row.decision,
        Some(RowDecision::Undone { applied_revision, .. }) if applied_revision == applied.revision)));
    let redone = ws.redo().unwrap();
    assert_eq!(ws.load_term(key).unwrap().courses[0].name, applied_name);
    let review = ws.review(draft.id).unwrap();
    assert!(review.items.iter().any(|row| row.active_path.is_none()
        && row.decision
            == Some(RowDecision::Applied {
                revision: redone.revision
            })));
    assert_eq!(review.counts().changed, 1);
}

#[test]
fn review_request_ids_reject_different_payloads_and_selections() {
    let (_path, ws, key) = seeded("review-retry");
    let mut input = InputDocument::new(key.year, key.season.code());
    input.courses = auler_samples::reference()
        .courses
        .into_iter()
        .take(2)
        .collect();
    for course in &mut input.courses {
        course.name.push_str(" (new)");
    }
    let meta = ChangeMeta::direct("검토")
        .origin(Origin::Mcp)
        .request("replay");
    let draft = ws.create_review(key, input.clone(), meta.clone()).unwrap();
    input.courses[0].name.push_str(" (different)");
    assert!(matches!(
        ws.create_review(key, input, meta),
        Err(StoreError::ReviewRequestConflict)
    ));
    let done = ws
        .approve_review(
            draft.id,
            draft.version,
            selected(&["courses[0]"]),
            "assistant".into(),
            "apply".into(),
        )
        .unwrap();
    assert!(matches!(
        ws.approve_review(
            draft.id,
            draft.version,
            selected(&["courses[1]"]),
            "assistant".into(),
            "apply".into()
        ),
        Err(StoreError::ReviewRequestConflict)
    ));
    assert_eq!(ws.revision().unwrap(), done.revision);
}

#[test]
fn review_rejection_revalidates_dependents_and_preserves_partial_completion() {
    let path = TempPath::new("review-reject");
    let ws = Workspace::create(&path.0, "Department").unwrap();
    let key = reference_data().setup.key;
    let mut input = auler_samples::reference();
    input.sections.truncate(1);
    let course = input
        .courses
        .iter()
        .position(|course| course.code == input.sections[0].course)
        .unwrap();
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토"))
        .unwrap();
    assert_eq!(row(&draft, "sections[0]"), Disposition::New);
    let rejected = ws
        .reject_review_items(
            draft.id,
            draft.version,
            vec![format!("courses[{course}]")],
            "assistant".into(),
        )
        .unwrap();
    assert_eq!(row(&rejected, "sections[0]"), Disposition::Error);
    let applied = ws
        .approve_review(
            draft.id,
            rejected.version,
            selected(&["setup"]),
            "assistant".into(),
            "apply".into(),
        )
        .unwrap();
    let current = ws.review(draft.id).unwrap();
    let closed = ws
        .close_review(
            draft.id,
            current.version,
            ReviewDecision::Rejected,
            "assistant".into(),
        )
        .unwrap();
    assert_eq!(
        closed.status(applied.revision),
        ReviewStatus::PartiallyAppliedClosed
    );
}

#[test]
fn review_matches_an_existing_constraint_before_changing_its_strength() {
    let (_path, ws, key) = seeded("review-constraint");
    let before = ws.load_term(key).unwrap();
    let mut input = InputDocument::new(key.year, key.season.code());
    let mut proposed = auler_samples::reference()
        .constraints
        .into_iter()
        .find(|c| c.strength.as_deref() == Some("soft") && c.status == "confirmed")
        .unwrap();
    proposed.strength = Some("hard".into());
    input.constraints.push(proposed);
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토"))
        .unwrap();
    assert_eq!(row(&draft, "constraints[0]"), Disposition::Update);
    ws.approve_review(
        draft.id,
        draft.version,
        selected(&["constraints[0]"]),
        "assistant".into(),
        "apply".into(),
    )
    .unwrap();
    let after = ws.load_term(key).unwrap();
    assert_eq!(after.constraints.len(), before.constraints.len());
    assert_eq!(
        before
            .constraints
            .iter()
            .zip(&after.constraints)
            .filter(|(a, b)| a.strength != b.strength)
            .count(),
        1
    );
}

#[test]
fn review_keeps_original_input_and_decisions_when_corrections_shift_rows() {
    let (_path, ws, key) = seeded("review-correction");
    let mut input = InputDocument::new(key.year, key.season.code());
    for text in ["needs a room", "needs an instructor"] {
        input.unresolved.push(UnresolvedInput {
            what: "request".into(),
            text: text.into(),
            reason: "unclear".into(),
            source: None,
        });
    }
    let draft = ws
        .create_review(key, input.clone(), ChangeMeta::direct("검토"))
        .unwrap();
    let rejected = ws
        .reject_review_items(
            draft.id,
            draft.version,
            selected(&["unresolved[1]"]),
            "assistant".into(),
        )
        .unwrap();
    let original = input.clone();
    input.unresolved.remove(0);
    let mut course = auler_samples::reference().courses.remove(0);
    course.name.push_str(" (corrected)");
    input.courses.push(course);
    let revised = ws
        .refresh_review(draft.id, rejected.version, Some(input))
        .unwrap();
    assert_eq!(ws.original_review_input(draft.id).unwrap(), original);
    let historical = revised
        .items
        .iter()
        .find(|r| matches!(r.decision, Some(RowDecision::Rejected { .. })))
        .unwrap();
    assert_eq!(historical.item.path, "unresolved[1]");
    assert_eq!(historical.active_path.as_deref(), Some("unresolved[0]"));
    assert_eq!(
        revised
            .items
            .iter()
            .filter(|r| r.decision.is_none())
            .count(),
        1
    );
    assert_eq!(row(&revised, "courses[0]"), Disposition::Update);
}

#[test]
fn review_explains_no_changes_and_finalized_approval_before_a_write() {
    let (_path, ws, key) = seeded("review-blockers");
    let unchanged = ws
        .create_review(key, auler_samples::reference(), ChangeMeta::direct("자료"))
        .unwrap();
    let current = ws.revision().unwrap();
    assert_eq!(unchanged.status(current), ReviewStatus::NoChanges);
    assert_eq!(
        unchanged.approval_blocker(current),
        Some(ApprovalBlocker::NoApplicableRows)
    );
    let mut input = InputDocument::new(key.year, key.season.code());
    let mut course = auler_samples::reference().courses.remove(0);
    course.name.push_str(" (change)");
    input.courses.push(course);
    let final_version = ws
        .finalize(key, current, ChangeMeta::direct("확정"))
        .unwrap();
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토"))
        .unwrap();
    assert_eq!(
        draft.approval_blocker(final_version.revision),
        Some(ApprovalBlocker::Finalized(key))
    );
}

#[test]
fn new_term_reviews_explain_missing_setup_for_catalog_rows() {
    let path = TempPath::new("review-missing-setup");
    let ws = Workspace::create(&path.0, "Department").unwrap();
    let key = reference_data().setup.key;
    let mut input = InputDocument::new(key.year, key.season.code());
    input
        .courses
        .push(auler_samples::reference().courses.remove(0));
    let draft = ws
        .create_review(key, input, ChangeMeta::direct("검토"))
        .unwrap();
    assert!(
        matches!(draft.approval_blocker(0), Some(ApprovalBlocker::MissingSetup(issues)) if issues[0].path == "setup")
    );
    let course = draft
        .items
        .iter()
        .find(|row| row.item.path == "courses[0]")
        .unwrap();
    assert_eq!(course.item.prerequisites, ["setup"]);
    assert_eq!(course.item.disposition(), Disposition::Error);
    assert!(ws.terms().unwrap().is_empty());
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
