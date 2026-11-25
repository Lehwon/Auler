use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use auler_domain::template::{SHEETS_V1, XLSX_TEMPLATE_V1};
use rust_xlsxwriter::Workbook;
use serde_json::Value;

struct TestFile(PathBuf);

impl TestFile {
    fn create(enrollment: u32, numeric_section: bool) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let filename = format!(
            "auler-cli-{}-{}.xlsx",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = TestFile(std::env::temp_dir().join(filename));
        let mut book = Workbook::new();
        book.add_worksheet()
            .set_name("Info")
            .unwrap()
            .write_string(1, 1, XLSX_TEMPLATE_V1)
            .unwrap();
        for spec in SHEETS_V1.iter().filter(|spec| !spec.optional) {
            let sheet = book.add_worksheet().set_name(spec.key).unwrap();
            for (index, col) in spec.columns.iter().enumerate() {
                sheet.write_string(0, index as u16, col.key).unwrap();
                let index = index as u16;
                match (spec.key, col.key) {
                    ("term", "year") => {
                        sheet.write_number(1, index, 2027).unwrap();
                    }
                    ("term", "season") => {
                        sheet.write_number(1, index, 1).unwrap();
                    }
                    ("term", "days") => {
                        sheet.write_string(1, index, "mon").unwrap();
                    }
                    ("periods", "label") => {
                        sheet.write_string(1, index, "1").unwrap();
                    }
                    ("periods", "start") => {
                        sheet.write_string(1, index, "09:00").unwrap();
                    }
                    ("periods", "end") => {
                        sheet.write_string(1, index, "09:50").unwrap();
                    }
                    ("rooms", "building") => {
                        sheet.write_string(1, index, "공학관").unwrap();
                    }
                    ("rooms", "number") => {
                        sheet.write_string(1, index, "012").unwrap();
                    }
                    ("rooms", "kind") | ("sections", "kind") => {
                        sheet.write_string(1, index, "lecture").unwrap();
                    }
                    ("rooms", "total_seats") => {
                        sheet.write_number(1, index, 30).unwrap();
                    }
                    ("courses", "code") | ("sections", "course") => {
                        sheet.write_string(1, index, "CS01").unwrap();
                    }
                    ("courses", "name") => {
                        sheet.write_string(1, index, "자료구조").unwrap();
                    }
                    ("sections", "section") if numeric_section => {
                        sheet.write_number(1, index, 1).unwrap();
                    }
                    ("sections", "section") => {
                        sheet.write_string(1, index, "01").unwrap();
                    }
                    ("sections", "enrollment") => {
                        sheet.write_number(1, index, enrollment).unwrap();
                    }
                    ("sections", "weekly_periods") => {
                        sheet.write_number(1, index, 1).unwrap();
                    }
                    _ => {}
                }
            }
        }
        book.save(&path.0).unwrap();
        path
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(path: &Path, flags: &[&str]) -> (Output, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_auler-cli"))
        .arg(path)
        .arg("--json")
        .args(flags)
        .output()
        .unwrap();
    let result = serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
        panic!("{error}, stderr: {}", String::from_utf8_lossy(&out.stderr));
    });
    (out, result)
}

#[test]
fn cli_reads_standard_xlsx_and_outputs_a_named_draft_with_text_codes() {
    let file = TestFile::create(28, false);
    let (out, result) = run(&file.0, &[]);
    assert!(out.status.success());
    assert_eq!(result["status"], "solved");
    assert_eq!(result["total_meetings"], 1);
    assert_eq!(result["placements"][0]["meeting"]["section"], "01");
    assert_eq!(result["placements"][0]["room"], "공학관 012");
}

#[test]
fn cli_distinguishes_workbook_issues_timeouts_and_proven_impossibility() {
    let numeric = TestFile::create(28, true);
    let (out, result) = run(&numeric.0, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(result["status"], "invalid_input");
    let issue = &result["diagnostics"][0];
    assert_eq!(issue["code"], "numeric_code");
    assert_eq!(issue["source"]["sheet"], "sections");
    assert_eq!(issue["source"]["row"], 2);
    assert_eq!(issue["source"]["column"], "B");

    let file = TestFile::create(31, false);
    let (out, result) = run(&file.0, &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["status"], "no_solution");
    assert_eq!(result["unassigned"][0]["cause"], "no_candidate");
    assert_eq!(
        result["unassigned"][0]["reasons"][0]["code"],
        "not_enough_seats"
    );
    assert_eq!(result["unassigned"][0]["reasons"][0]["occurrences"], 1);
    assert_eq!(result["capacity_shortfalls"].as_array().unwrap().len(), 0);

    let (out, result) = run(&file.0, &["--time-limit-ms", "0"]);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(result["status"], "timed_out");
    assert_eq!(result["unassigned"][0]["cause"], "time_limit");
}

#[test]
fn cli_shows_korean_diagnostics_and_marks_partial_placement_in_text() {
    let numeric = TestFile::create(28, true);
    let output = Command::new(env!("CARGO_BIN_EXE_auler-cli"))
        .arg(&numeric.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("[확인 필요]") && error.contains("sections · B2"));
    assert!(error.contains("코드가 숫자로 입력됨 (1)") && error.contains("선행 0"));

    let file = TestFile::create(31, false);
    let output = Command::new(env!("CARGO_BIN_EXE_auler-cli"))
        .arg(&file.0)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let unavailable = String::from_utf8(output.stdout).unwrap();
    assert!(unavailable.contains("현재 조건에서 전체 편성 불가"));
    assert!(unavailable.contains("가용 좌석 30석, 필요 31석 (관찰 1건)"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_auler-cli"))
        .arg(root.join("samples/reference.json"))
        .args(["--time-limit-ms", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let partial = String::from_utf8(output.stdout).unwrap();
    assert!(partial.contains("부분 배치 미리보기") && partial.contains("배치 2/68"));
    assert!(partial.contains("월요일 6~7교시") && partial.contains("[잠김]"));
    assert!(partial.contains("--time-limit-ms 값을 늘려"));
}

#[test]
fn json_distinguishes_invalid_arguments_from_invalid_file_content() {
    let output = Command::new(env!("CARGO_BIN_EXE_auler-cli"))
        .args(["--unsupported", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "invalid_arguments");
}

#[test]
fn cli_schedules_reference_data_and_reports_unsupported_original_workbook() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (out, report) = run(
        &root.join("samples/reference.json"),
        &["--time-limit-ms", "8000"],
    );
    assert!(out.status.success());
    assert_eq!(report["placements"].as_array().unwrap().len(), 68);
    assert_eq!(
        report["placements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["locked"] == true)
            .count(),
        2
    );
    let (out, report) = run(&root.join("samples/messy/2027-1 개설강좌 현황.xlsx"), &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "missing_sheet")
    );
}
