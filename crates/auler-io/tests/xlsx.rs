use std::io::Cursor;

use auler_domain::input::InputDocument;
use auler_domain::model::{TermKey, TermSeason};
use auler_domain::resolve::{Severity, resolve};
use auler_domain::template::{INFO_SHEET, SHEETS_V1, SheetSpec, XLSX_TEMPLATE_V1};
use auler_io::xlsx::{Diagnostic, ImportError, Problem, import_reader};
use auler_store::{ChangeMeta, Disposition, Origin, ReviewStatus, Workspace};
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};

struct Fixture {
    book: Workbook,
    korean: bool,
}

impl Fixture {
    fn new(korean: bool) -> Self {
        let mut book = Workbook::new();
        let name = if korean { INFO_SHEET.ko } else { INFO_SHEET.en };
        book.add_worksheet()
            .set_name(name)
            .unwrap()
            .write_string(1, 1, XLSX_TEMPLATE_V1)
            .unwrap();
        for spec in SHEETS_V1 {
            let name = if korean { spec.ko } else { spec.en };
            let sheet = book.add_worksheet().set_name(name).unwrap();
            for column in spec.columns {
                let col = column_index(spec, column.key, korean);
                sheet
                    .write_string(0, col, if korean { column.ko } else { column.en })
                    .unwrap();
            }
        }
        Self { book, korean }
    }

    fn cell(&mut self, sheet: &str, key: &str) -> (&mut Worksheet, u16) {
        let spec = SHEETS_V1.iter().find(|spec| spec.key == sheet).unwrap();
        let name = if self.korean { spec.ko } else { spec.en };
        let column = column_index(spec, key, self.korean);
        (self.book.worksheet_from_name(name).unwrap(), column)
    }

    fn row(&mut self, sheet: &str, row: u32, values: &[(&str, &str)]) {
        for (key, value) in values {
            let (sheet, column) = self.cell(sheet, key);
            sheet.write_string(row, column, *value).unwrap();
        }
    }

    fn import(mut self) -> Result<InputDocument, ImportError> {
        let bytes = self.book.save_to_buffer().unwrap();
        import_reader(Cursor::new(bytes), "fixture.xlsx")
    }
}

fn column_index(spec: &SheetSpec, key: &str, korean: bool) -> u16 {
    let index = spec
        .columns
        .iter()
        .position(|column| column.key == key)
        .unwrap();
    if korean {
        index as u16
    } else {
        (spec.columns.len() - 1 - index) as u16
    }
}

fn example(korean: bool) -> Fixture {
    let mut book = Fixture::new(korean);
    book.row(
        "term",
        1,
        &[
            ("year", "2027"),
            ("season", if korean { "1학기" } else { "1" }),
            ("days", if korean { "월;화" } else { "Monday;tue" }),
        ],
    );
    let (term, start) = book.cell("term", "start");
    term.write_datetime_with_format(
        1,
        start,
        ExcelDateTime::from_ymd(2027, 3, 2).unwrap(),
        &Format::new().set_num_format("yyyy-mm-dd"),
    )
    .unwrap();
    book.row(
        "periods",
        1,
        &[("label", "1"), ("start", "09:00"), ("end", "09:50")],
    );
    book.row(
        "periods",
        2,
        &[("label", "2"), ("start", "10:00"), ("end", "10:50")],
    );
    let (periods, end) = book.cell("periods", "end");
    periods
        .write_datetime_with_format(
            2,
            end,
            ExcelDateTime::from_hms(10, 50, 0).unwrap(),
            &Format::new().set_num_format("hh:mm"),
        )
        .unwrap();
    book.row(
        "equipment",
        1,
        &[("code", "PC"), ("name", "Lab PC"), ("scope", "per_person")],
    );
    book.row(
        "rooms",
        1,
        &[
            ("building", "공학관"),
            ("number", "012"),
            ("kind", "lab"),
            ("total_seats", "32"),
        ],
    );
    book.row(
        "room_equipment",
        3,
        &[
            ("building", "공학관"),
            ("number", "012호"),
            ("equipment", "PC"),
            ("owned", "32"),
            ("available", "30"),
        ],
    );
    book.row("instructors", 1, &[("code", "I01"), ("name", "김민수")]);
    book.row("groups", 1, &[("code", "G1"), ("name", "1학년")]);
    book.row("courses", 1, &[("code", "CSE01"), ("name", "자료구조")]);
    book.row(
        "sections",
        3,
        &[
            ("course", "CSE01"),
            ("section", "01"),
            ("instructors", "I01"),
            ("enrollment", "28"),
            ("kind", if korean { "실습" } else { "Lab" }),
            ("weekly_periods", "2"),
            ("split", "1+1"),
        ],
    );
    book.row(
        "section_groups",
        2,
        &[
            ("course", "CSE01"),
            ("section", "01"),
            ("group", "G1"),
            ("required", "yes"),
        ],
    );
    book.row(
        "section_equipment",
        4,
        &[
            ("course", "CSE01"),
            ("section", "01"),
            ("equipment", "PC"),
            ("quantity", "1"),
        ],
    );
    for (index, day) in [(1, "mon"), (3, "tue")] {
        book.row(
            "constraints",
            index,
            &[
                ("key", "C1"),
                ("target_kind", "instructor"),
                ("target", "I01"),
                ("day", day),
                ("first_period", "2"),
                ("strength", "soft"),
                ("status", "confirmed"),
            ],
        );
    }
    book.row(
        "placements",
        1,
        &[
            ("course", "CSE01"),
            ("section", "01"),
            ("part", "1"),
            ("day", "mon"),
            ("start_period", "1"),
            ("building", "공학관"),
            ("number", "012"),
            ("locked", "true"),
        ],
    );
    book
}

fn invalid(result: Result<InputDocument, ImportError>) -> Vec<Diagnostic> {
    match result.unwrap_err() {
        ImportError::Validation(diagnostics) => diagnostics,
        error => panic!("expected XLSX diagnostics, got {error}"),
    }
}

#[test]
fn standard_workbooks_keep_codes_and_locations_in_both_languages() {
    for korean in [true, false] {
        let doc = example(korean).import().unwrap();
        assert_eq!(
            doc.setup.as_ref().unwrap().start.as_deref(),
            Some("2027-03-02")
        );
        assert_eq!(doc.setup.as_ref().unwrap().periods[1].end, "10:50");
        assert_eq!(doc.sections[0].section.text, "01");
        assert_eq!(doc.rooms[0].number.text, "012");
        assert_eq!(doc.sections[0].source.as_ref().unwrap().row, Some(4));
        assert_eq!(doc.constraints[0].rule.as_ref().unwrap().windows.len(), 2);
        assert_eq!(
            doc.locations["constraints[0].rule.windows[1].first_period"].row,
            Some(4)
        );
        assert_eq!(
            doc.locations["sections[0].equipment[0].quantity"].row,
            Some(5)
        );
        assert_eq!(
            doc.locations["rooms[0].equipment[0].available"].row,
            Some(4)
        );
        let data = resolve(&doc).unwrap();
        assert_eq!(data.sections.len(), 1);
        assert_eq!(data.rooms[0].equipment[0].available, 30);
        assert!(data.placements[0].locked);
    }
}

#[test]
fn workbook_changes_are_applied_only_after_review_approval() {
    let root = std::env::temp_dir().join(format!("auler-xlsx-review-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    struct RemoveOnDrop(std::path::PathBuf);
    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let temp = RemoveOnDrop(root);
    let ws = Workspace::create(temp.0.join("example.auler"), "Department").unwrap();
    let key = TermKey {
        year: 2027,
        season: TermSeason::First,
    };
    let draft = ws
        .create_review(
            key,
            example(false).import().unwrap(),
            ChangeMeta::direct("가져오기").origin(Origin::Xlsx),
        )
        .unwrap();
    assert_eq!(
        draft.status(ws.revision().unwrap()),
        ReviewStatus::Applicable
    );
    assert!(ws.terms().unwrap().is_empty());
    let chosen = draft
        .items
        .iter()
        .filter(|row| row.item.disposition() == Disposition::New)
        .map(|row| row.item.path.clone())
        .collect();
    ws.approve_review(
        draft.id,
        draft.version,
        chosen,
        "local".into(),
        "apply-xlsx".into(),
    )
    .unwrap();
    let term = ws.load_term(key).unwrap();
    assert_eq!(term.sections[0].code.as_str(), "01");
    assert_eq!(term.rooms[0].key.number, "012");
    assert!(term.placements[0].locked);
}

#[test]
fn numeric_codes_need_review_and_are_not_used_for_child_joins() {
    let mut book = example(true);
    let (sheet, number) = book.cell("sections", "section");
    sheet.write_number(3, number, 1).unwrap();
    let doc = book.import().unwrap();
    assert!(doc.sections[0].section.from_number);
    assert!(doc.sections[0].equipment.is_empty());
    assert_eq!(doc.unresolved.len(), 2);
    let issues = resolve(&doc).unwrap_err();
    let code = issues
        .iter()
        .find(|issue| issue.path == "sections[0].section")
        .unwrap();
    assert_eq!(code.severity(), Severity::NeedsReview);
    let source = code.source.as_ref().unwrap();
    assert_eq!(source.column.as_deref(), Some("B"));
    assert_eq!(source.row, Some(4));
}

#[test]
fn a_single_numeric_split_and_an_english_term_number_are_accepted() {
    let mut book = example(false);
    let (sheet, season) = book.cell("term", "season");
    sheet.write_number(1, season, 1).unwrap();
    let (sheet, split) = book.cell("sections", "split");
    sheet.write_number(3, split, 2).unwrap();
    let input = book.import().unwrap();
    assert_eq!(input.term.season, "1");
    assert_eq!(input.sections[0].split, Some(vec![2]));
    assert!(resolve(&input).is_ok());
}

#[test]
fn unsupported_version_and_unknown_sheets_report_original_positions() {
    let mut book = example(false);
    book.book
        .worksheet_from_name("Info")
        .unwrap()
        .write_string(1, 1, "auler.xlsx.v9")
        .unwrap();
    book.book.add_worksheet().set_name("Original").unwrap();
    let errors = invalid(book.import());
    assert!(errors.iter().any(
        |issue| matches!(issue.kind, Problem::UnsupportedVersion { .. })
            && issue.source.column.as_deref() == Some("B")
            && issue.source.row == Some(2)
    ));
    assert!(
        errors
            .iter()
            .any(|issue| issue.kind == Problem::UnknownSheet
                && issue.source.sheet.as_deref() == Some("Original"))
    );
}

#[test]
fn malformed_sheets_report_unknown_headers_formulas_and_wrong_value_types() {
    let mut book = example(true);
    let (sheet, name) = book.cell("courses", "name");
    sheet.write_string(0, name, "다른 머리글").unwrap();
    let (sheet, seats) = book.cell("rooms", "total_seats");
    sheet.write_string(1, seats, "thirty").unwrap();
    let (sheet, count) = book.cell("sections", "enrollment");
    sheet.write_formula(3, count, "=28").unwrap();
    let errors = invalid(book.import());
    assert!(
        errors
            .iter()
            .any(|issue| matches!(issue.kind, Problem::UnknownColumn { .. })
                && issue.source.row == Some(1)
                && issue.source.column.as_deref() == Some("B"))
    );
    assert!(errors.iter().any(|issue| matches!(
        issue.kind,
        Problem::InvalidValue {
            key: "total_seats",
            ..
        }
    ) && issue.source.row == Some(2)));
    assert!(
        errors
            .iter()
            .any(|issue| issue.kind == Problem::Formula && issue.source.row == Some(4))
    );
}

#[test]
fn contradictory_and_incomplete_constraint_rows_have_actionable_errors() {
    let mut book = example(false);
    book.row("constraints", 3, &[("strength", "hard")]);
    book.row(
        "constraints",
        5,
        &[
            ("key", "wrong"),
            ("target_kind", "section"),
            ("target", "CSE01"),
            ("status", "pending"),
        ],
    );
    book.row(
        "constraints",
        6,
        &[
            ("key", "next"),
            ("target_kind", "group"),
            ("target", "G1"),
            ("status", "pending"),
        ],
    );
    let errors = invalid(book.import());
    assert!(errors.iter().any(|issue| matches!(&issue.kind, Problem::ConflictingConstraint { field, .. } if field == "strength")
        && issue.source.row == Some(4)));
    assert!(errors.iter().any(|issue| issue.kind
        == Problem::MissingValue {
            key: "target_section"
        }
        && issue.source.row == Some(6)));
}

#[test]
fn distributed_empty_templates_have_valid_versions_and_headers() {
    for bytes in [
        include_bytes!("../../../templates/auler-standard-v1.ko.xlsx").as_slice(),
        include_bytes!("../../../templates/auler-standard-v1.en.xlsx").as_slice(),
    ] {
        let errors = invalid(import_reader(Cursor::new(bytes), "template.xlsx"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, Problem::TermRowCount { found: 0 });
    }
}

#[test]
fn numeric_condition_keys_and_join_errors_keep_distinct_recovery_details() {
    let mut book = example(true);
    for row in [1, 3] {
        let (sheet, key) = book.cell("constraints", "key");
        sheet.write_number(row, key, 1).unwrap();
    }
    let (sheet, number) = book.cell("rooms", "number");
    sheet.write_number(1, number, 12).unwrap();
    let (sheet, number) = book.cell("room_equipment", "number");
    sheet.write_string(3, number, "12").unwrap();
    let (sheet, code) = book.cell("section_groups", "section");
    sheet.write_number(2, code, 1).unwrap();
    book.row("section_equipment", 4, &[("section", "98")]);
    let doc = book.import().unwrap();
    assert_eq!(doc.constraints.len(), 1);
    assert!(doc.constraints[0].key.as_ref().unwrap().from_number);
    assert_eq!(doc.constraints[0].rule.as_ref().unwrap().windows.len(), 2);
    assert!(
        doc.unresolved
            .iter()
            .any(|row| row.reason == "numeric_parent_key"
                && row.source.as_ref().unwrap().column.as_deref() == Some("B"))
    );
    assert!(doc.unresolved.iter().any(
        |row| row.reason == "numeric_child_key" && row.source.as_ref().unwrap().row == Some(3)
    ));
    assert!(
        doc.unresolved.iter().any(
            |row| row.reason == "missing_parent" && row.source.as_ref().unwrap().row == Some(5)
        )
    );
    assert!(
        resolve(&doc)
            .unwrap_err()
            .iter()
            .any(|issue| issue.path == "constraints[0].key" && issue.kind.code() == "numeric_code")
    );
}

#[test]
fn version_formula_and_missing_sheet_have_actionable_locations() {
    let mut book = example(true);
    book.book
        .worksheet_from_name(INFO_SHEET.ko)
        .unwrap()
        .write_formula(1, 1, "=\"auler.xlsx.v1\"")
        .unwrap();
    book.book
        .worksheet_from_name("분반")
        .unwrap()
        .set_name("기타")
        .unwrap();
    let issues = invalid(book.import());
    assert!(
        issues
            .iter()
            .any(|issue| issue.kind == Problem::MissingSheet { key: "sections" })
    );
    assert!(issues.iter().any(|issue| issue.kind == Problem::Formula
        && issue.source.column.as_deref() == Some("B")
        && issue.source.row == Some(2)));
    assert!(
        !issues
            .iter()
            .any(|issue| matches!(issue.kind, Problem::UnsupportedVersion { .. }))
    );
}

#[test]
fn a_required_formula_cell_is_reported_once() {
    let mut book = example(false);
    let (sheet, enrollment) = book.cell("sections", "enrollment");
    sheet.write_formula(3, enrollment, "=28").unwrap();
    let issues = invalid(book.import());
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, Problem::Formula);
    assert_eq!(issues[0].source.row, Some(4));
}
