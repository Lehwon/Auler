//! 가상 데이터 검사: 기준 학과가 깨끗하게 풀리는지, 원본 엑셀의 정답이 정해 둔 확인 항목만
//! 남기는지, 저장소의 생성 파일이 최신인지.

use auler_domain::input::InputDocument;
use auler_domain::model::{ConstraintStatus, EquipmentScope, Rule, Strength, Target, TimeWindow};
use auler_domain::resolve::{IssueKind, Severity, resolve};
use auler_domain::time::Weekday;
use auler_samples::{messy, outputs, reference, repo_root, stress};

#[test]
fn reference_department_resolves_with_expected_shape() {
    let data = resolve(&reference()).expect("기준 학과는 문제없이 풀려야 한다");
    assert_eq!(data.sections.len(), 42);
    assert_eq!(data.instructors.len(), 28);
    assert_eq!(data.rooms.len(), 12);
    assert_eq!(data.setup.periods.len(), 9);
    assert_eq!(data.setup.days.len(), 5);
    assert_eq!(data.placements.len(), 2);
    assert!(data.placements.iter().all(|p| p.locked));

    // 모든 교원이 한 분반 이상 맡는다
    for t in &data.instructors {
        assert!(
            data.sections.iter().any(|s| s.instructors.contains(&t.id)),
            "{} 담당 분반 없음",
            t.code
        );
    }

    // 자료구조 01분반: 28명, 1인당 PC 1대, 2+1 분할
    let course = data.courses.iter().find(|c| c.name == "자료구조").unwrap();
    let ds = data
        .sections
        .iter()
        .find(|s| s.course == course.id && s.code.as_str() == "01")
        .unwrap();
    assert_eq!(ds.enrollment.count, 28);
    assert_eq!(ds.split, [2, 1]);
    let pc = data
        .equipment
        .iter()
        .find(|e| e.code.as_str() == "PC")
        .unwrap();
    assert_eq!(pc.scope, EquipmentScope::PerPerson);
    assert_eq!(ds.equipment[0].equipment, pc.id);

    // 목요일 실습실 후보의 재료: 301호 정상 PC 26대, 302호 30대, 305호 다른 학과 사용, 본관 201호 미확인
    let pcs = |b: &str, n: &str| {
        let room = data
            .rooms
            .iter()
            .find(|r| r.key.building == b && r.key.number == n)
            .unwrap();
        room.equipment
            .iter()
            .find(|e| e.equipment == pc.id)
            .unwrap()
            .available
    };
    assert_eq!(pcs("공학관", "301"), 26);
    assert_eq!(pcs("공학관", "302"), 30);
    let main201 = data
        .rooms
        .iter()
        .find(|r| r.key.to_string() == "본관 201")
        .unwrap();
    assert!(main201.needs_external_check());
    let eng305 = data
        .rooms
        .iter()
        .find(|r| r.key.to_string() == "공학관 305")
        .unwrap();
    let thu_afternoon = data
        .constraints
        .iter()
        .find(|c| c.target == Target::Room(eng305.id))
        .unwrap();
    assert!(thu_afternoon.is_active());
    assert_eq!(thu_afternoon.strength, Some(Strength::Hard));
    let Some(Rule::AvoidTimes { windows }) = &thu_afternoon.rule else {
        panic!()
    };
    // 14:00~14:50(6교시)이 막힌다
    assert!(windows[0].period_range(&data.setup.periods).contains(&5));

    // 자료구조 01 담당 교원의 요청 세 건: 선호, 필수, 확인 대기
    let teacher = ds.instructors[0];
    let requests: Vec<_> = data
        .constraints
        .iter()
        .filter(|c| c.target == Target::Instructor(teacher))
        .collect();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].strength, Some(Strength::Soft));
    assert_eq!(
        requests[0].rule,
        Some(Rule::AvoidTimes {
            windows: vec![TimeWindow::Clock {
                day: Weekday::Wed,
                start: auler_domain::time::ClockTime::from_hm(13, 0).unwrap(),
                end: auler_domain::time::ClockTime::from_hm(16, 0).unwrap(),
            }]
        })
    );
    assert_eq!(requests[1].strength, Some(Strength::Hard));
    assert_eq!(requests[2].status, ConstraintStatus::Pending);
    assert!(!requests[2].is_active());

    // 동명이인, 같은 호실 번호
    let kims = data
        .instructors
        .iter()
        .filter(|t| t.name == "김민수")
        .count();
    assert_eq!(kims, 2);
    let room201 = data.rooms.iter().filter(|r| r.key.number == "201").count();
    assert_eq!(room201, 2);
}

#[test]
fn stress_data_is_deterministic_and_valid() {
    let doc = stress();
    assert_eq!(doc, stress(), "같은 시드는 같은 문서");
    let data = resolve(&doc).unwrap_or_else(|issues| {
        panic!(
            "{}",
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    assert_eq!(data.sections.len(), stress::SECTION_COUNT);
    assert_eq!(data.instructors.len(), stress::INSTRUCTOR_COUNT);
    assert!(data.rooms.len() >= 30);
    let most = data
        .instructors
        .iter()
        .map(|t| {
            data.sections
                .iter()
                .filter(|s| s.instructors.contains(&t.id))
                .count()
        })
        .max()
        .unwrap();
    assert!(most <= 3, "교원당 최대 3분반: {most}");
}

/// 원본 엑셀의 정답을 작업공간 자료(학기 설정, 교원, 수강 집단, 장비)와 합친다
fn with_workspace(expected: &InputDocument) -> InputDocument {
    let mut doc = reference();
    doc.rooms = expected.rooms.clone();
    doc.courses = expected.courses.clone();
    doc.sections = expected.sections.clone();
    doc.constraints = expected.constraints.clone();
    doc.placements = expected.placements.clone();
    doc.unresolved = expected.unresolved.clone();
    doc
}

#[test]
fn messy_conversion_leaves_only_the_planned_review_items() {
    let messy = messy::build().unwrap();
    let issues = resolve(&with_workspace(&messy.expected)).unwrap_err();
    let got: Vec<(String, &str)> = issues
        .iter()
        .map(|i| (i.path.clone(), i.kind.code()))
        .collect();
    assert!(
        issues.iter().all(|i| i.severity() == Severity::NeedsReview),
        "오류 없이 확인 항목만 남아야 한다: {got:?}"
    );
    let section = |course: &str, sec: &str| {
        messy
            .expected
            .sections
            .iter()
            .position(|s| s.course.text == course && s.section.text == sec)
            .unwrap()
    };
    let request = messy
        .expected
        .constraints
        .iter()
        .position(|c| c.key.as_deref() == Some("R04"))
        .unwrap();
    let want = vec![
        (
            format!("sections[{}].instructors[0].name", section("CSE1001", "01")),
            "ambiguous_instructor",
        ),
        (
            format!("sections[{}].instructors[0].name", section("MTH1001", "01")),
            "ambiguous_instructor",
        ),
        (
            format!("sections[{}].instructors[0].name", section("CSE4010", "01")),
            "ambiguous_instructor",
        ),
        (
            format!("constraints[{request}].target.instructor.name"),
            "ambiguous_instructor",
        ),
        ("unresolved[0]".to_owned(), "unresolved"),
        ("unresolved[1]".to_owned(), "unresolved"),
    ];
    let want: Vec<(String, &str)> = want.into_iter().collect();
    assert_eq!(got, want);

    // 동명이인 후보는 두 김민수
    let IssueKind::AmbiguousInstructor { candidates, .. } = &issues[0].kind else {
        unreachable!()
    };
    let codes: Vec<&str> = candidates.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes, ["P01", "P21"]);
    // 원본 위치가 붙는다: 개설강좌 5행부터
    let src = issues[0].source.as_ref().unwrap();
    assert_eq!(src.file.as_deref(), Some(messy::OFFERINGS_FILE));
    assert_eq!(src.row, Some(5));
}

#[test]
fn messy_sections_match_the_reference_department() {
    let messy = messy::build().unwrap();
    let reference = reference();
    assert_eq!(messy.expected.sections.len(), reference.sections.len());
    for (got, want) in messy.expected.sections.iter().zip(&reference.sections) {
        let mut got = got.clone();
        assert!(got.source.is_some());
        got.source = None;
        // 원본에는 교원 이름만 있다
        assert!(
            got.instructors
                .iter()
                .all(|t| t.code.is_none() && t.name.is_some())
        );
        got.instructors = want.instructors.clone();
        assert_eq!(&got, want);
    }
    // 강의실: 건물 없는 201호 한 곳만 빠진다
    assert_eq!(messy.expected.rooms.len(), reference.rooms.len() - 1);
    assert!(messy.expected.rooms.iter().any(|r| r.number.text == "302"));
    assert_eq!(messy.expected.unresolved[0].what, "room");
}

#[test]
fn generated_files_are_deterministic_and_committed() {
    let first = outputs().unwrap();
    let second = outputs().unwrap();
    assert_eq!(first, second, "같은 코드는 같은 파일을 만든다");
    let root = repo_root();
    let mut stale = Vec::new();
    for (path, bytes) in first {
        match std::fs::read(root.join(&path)) {
            Ok(on_disk) if on_disk == bytes => {}
            _ => stale.push(path.display().to_string()),
        }
    }
    assert!(
        stale.is_empty(),
        "생성 파일이 최신이 아니다. `cargo run -p auler-samples`로 다시 만든다: {stale:?}"
    );
}

#[test]
fn committed_json_reads_back() {
    let text = std::fs::read_to_string(repo_root().join("samples/reference.json")).unwrap();
    let doc = InputDocument::from_json(&text).unwrap();
    assert_eq!(doc, reference());
}
