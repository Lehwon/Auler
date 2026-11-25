use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::atomic::AtomicBool;

use auler_domain::input::InputDocument;
use auler_domain::resolve::resolve;
use auler_engine::{LocalSolver, SolveOptions, Solver};
use auler_io::export::{Fields, Grouping, Language, Layout, Snapshot, View};
use auler_store::{ChangeMeta, Op, Record, Workspace};
use calamine::{Data, Reader, Xlsx};

#[test]
fn outputs_keep_the_selected_finalization_and_only_include_shared_fields() {
    let fixture = include_str!("../../../samples/reference.json");
    let mut data = resolve(&InputDocument::from_json(fixture).unwrap()).unwrap();
    let term = data.setup.key;
    let selected = data
        .constraints
        .iter_mut()
        .find(|item| item.is_active())
        .unwrap();
    selected.note = Some("private-note-sentinel".into());
    selected.text = Some("private-request-sentinel".into());
    selected.confirmed_by = Some("private-actor-sentinel".into());
    let condition = selected.id;
    data.rooms[0].info_source = Some("private-source-sentinel".into());
    let solved = LocalSolver.solve(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(solved.unassigned.is_empty());
    data.placements = solved.placements;

    let directory = tempfile::tempdir().unwrap();
    let workspace =
        Workspace::create(directory.path().join("export.auler"), "Export fixture").unwrap();
    let meta = || ChangeMeta::direct("Export fixture");
    workspace.import_term(data, meta()).unwrap();
    assert!(Snapshot::load(&workspace, term, 1).is_err());
    workspace
        .finalize_checked(term, workspace.revision().unwrap(), true, meta())
        .unwrap();
    let original = Snapshot::load(&workspace, term, 1).unwrap();
    let meeting = original.meetings(View::All)[0];

    let workbook = original
        .xlsx(View::All, Language::En, Layout::List, Fields::default())
        .unwrap();
    let mut xlsx = Xlsx::new(Cursor::new(workbook)).unwrap();
    let sheet = xlsx.worksheet_range("Timetable").unwrap();
    let mut rows = sheet.rows().skip(4);
    let first = rows.next().unwrap();
    let value = |i: usize| match &first[i] {
        Data::String(text) => text.as_str(),
        cell => panic!("Unexpected cell {cell:?}"),
    };
    assert_eq!(value(0), Language::En.day(meeting.placement.day));
    assert_eq!(value(1), meeting.first.start.to_string());
    assert_eq!(value(2), meeting.last.end.to_string());
    assert_eq!(value(3), meeting.course.code.as_str());
    assert_eq!(value(4), meeting.section.code.as_str());
    assert_eq!(value(7), meeting.room.key.to_string());
    assert_eq!(rows.count() + 1, 68);
    let grid = original.table(
        View::Room(meeting.room.id),
        Language::Ko,
        Layout::Grid,
        Fields::default(),
    );
    let column = grid
        .headings
        .iter()
        .position(|title| title == meeting.placement.day.ko())
        .unwrap();
    assert!(
        grid.rows[usize::from(meeting.placement.start)][column]
            .contains(meeting.section.code.as_str())
    );
    assert!(
        original
            .views(Grouping::Year)
            .iter()
            .any(|view| !original.meetings(*view).is_empty())
    );
    assert!(
        original
            .views(Grouping::Instructor)
            .iter()
            .any(|view| !original.meetings(*view).is_empty())
    );

    let handover = original.handover(&BTreeSet::from([condition])).unwrap();
    assert!(!String::from_utf8_lossy(&handover).contains("-sentinel"));
    let json: serde_json::Value = serde_json::from_slice(&handover).unwrap();
    assert_eq!(json["constraints"].as_array().unwrap().len(), 1);
    assert_eq!(json["placements"].as_array().unwrap().len(), 68);
    assert_eq!(json["finalized_version"], 1);
    assert!(json.get("token").is_none());
    compare_submission(&original);

    workspace.start_correction(term, meta()).unwrap();
    let mut course = meeting.course.clone();
    course.name = "Changed in the second publication".into();
    workspace
        .apply(None, vec![Op::Put(Record::Course(course))], meta())
        .unwrap();
    workspace
        .finalize_checked(term, workspace.revision().unwrap(), true, meta())
        .unwrap();
    let old = Snapshot::load(&workspace, term, 1).unwrap();
    let new = Snapshot::load(&workspace, term, 2).unwrap();
    assert_eq!(old.meetings(View::All)[0].course.name, meeting.course.name);
    assert_eq!(
        new.meetings(View::All)[0].course.name,
        "Changed in the second publication"
    );

    #[cfg(all(feature = "pdf", windows))]
    save_comparison_files(&old, &new);
}

fn compare_submission(snapshot: &Snapshot) {
    use auler_io::submission::{EXAMPLE, Error, Profile};
    let report = Profile::default().prepare(snapshot).unwrap();
    assert_eq!(report.table.rows.len(), 68);
    let (text, invalid) = encoding_rs::EUC_KR.decode_without_bom_handling(&report.bytes);
    assert!(!invalid);
    assert!(text.starts_with(
        "학수번호,분반,수업차수,요일코드,시작교시,종료교시,시작시각,종료시각,건물,호실\r\n"
    ));
    let mut parsed = csv::Reader::from_reader(text.as_bytes());
    for (record, meeting) in parsed.records().zip(snapshot.meetings(View::All)) {
        let record = record.unwrap();
        assert_eq!(&record[0], meeting.course.code.as_str());
        assert_eq!(&record[1], meeting.section.code.as_str());
        assert_eq!(
            &record[3],
            format!("{:02}", meeting.placement.day.index() + 1)
        );
        let start = u16::from(meeting.placement.start) + 1;
        assert_eq!(&record[4], format!("{start:02}"));
        assert_eq!(
            &record[5],
            format!("{:02}", start + u16::from(meeting.span) - 1)
        );
        assert_eq!(&record[6], meeting.first.start.to_string());
        assert_eq!(&record[7], meeting.last.end.to_string());
        assert_eq!(&record[8], meeting.room.key.building);
        assert_eq!(&record[9], meeting.room.key.number);
    }

    let mut custom: serde_json::Value = serde_json::from_str(EXAMPLE).unwrap();
    custom["encoding"] = "utf-8".into();
    custom["include_headers"] = false.into();
    custom["columns"].as_array_mut().unwrap().reverse();
    let utf8 = Profile::from_json(&serde_json::to_vec(&custom).unwrap())
        .unwrap()
        .prepare(snapshot)
        .unwrap();
    assert_eq!(utf8.table.headings[0], "호실");
    let first = snapshot.meetings(View::All)[0];
    assert!(
        std::str::from_utf8(&utf8.bytes)
            .unwrap()
            .starts_with(&format!(
                "{},{}",
                first.room.key.number, first.room.key.building
            ))
    );

    custom["period_codes"] = serde_json::json!({});
    assert!(matches!(
        Profile::from_json(&serde_json::to_vec(&custom).unwrap())
            .unwrap()
            .prepare(snapshot),
        Err(Error::MissingPeriod(_))
    ));
    let bad = EXAMPLE.replace("학수번호", "학수번호 🧪");
    assert!(matches!(
        Profile::from_json(bad.as_bytes())
            .unwrap()
            .prepare(snapshot),
        Err(Error::Unrepresentable { row: 1, column: 1 })
    ));
    let duplicate = EXAMPLE.replace("\"mon\": \"01\"", "\"mon\": \"01\", \"mon\": \"02\"");
    assert!(matches!(
        Profile::from_json(duplicate.as_bytes()),
        Err(Error::InvalidJson(_))
    ));

    let directory =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-snapshots/export-files");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("v1-school-cp949.csv"), report.bytes).unwrap();
    std::fs::write(directory.join("v1-school-utf8.csv"), utf8.bytes).unwrap();
}

#[cfg(all(feature = "pdf", windows))]
fn save_comparison_files(old: &Snapshot, new: &Snapshot) {
    use auler_io::pdf::{Preview, TimetableOptions, timetable};

    let directory =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-snapshots/export-files");
    std::fs::create_dir_all(&directory).unwrap();
    let list = TimetableOptions {
        locale: Language::En,
        target: View::All,
        layout: Layout::List,
        fields: Fields::default(),
        landscape: true,
    };
    let preview = Some(Preview {
        index: 0,
        width: None,
    });
    let first = timetable(old, &list, preview).unwrap();
    assert!(first.pages > 1 && first.pdf.starts_with(b"%PDF"));
    std::fs::write(directory.join("v1-en-list.pdf"), first.pdf).unwrap();
    std::fs::write(
        directory.join("v1-en-list.xlsx"),
        old.xlsx(list.target, list.locale, list.layout, list.fields)
            .unwrap(),
    )
    .unwrap();
    let grid = TimetableOptions {
        locale: Language::Ko,
        layout: Layout::Grid,
        ..list
    };
    let latest = timetable(new, &grid, preview).unwrap();
    std::fs::write(directory.join("v2-ko-grid.pdf"), latest.pdf).unwrap();
    std::fs::write(
        directory.join("v2-ko-grid.xlsx"),
        new.xlsx(grid.target, grid.locale, grid.layout, grid.fields)
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.join("v2-handover.json"),
        new.handover(&BTreeSet::new()).unwrap(),
    )
    .unwrap();
}
