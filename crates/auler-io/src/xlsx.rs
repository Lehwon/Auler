//! 표준 XLSX 입력을 공통 입력 문서로 변환한다.

mod document;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};
use std::path::Path;
use std::rc::Rc;

use auler_domain::input::InputDocument;
use auler_domain::model::SourceRef;
use auler_domain::template::{
    ColumnKind, ColumnSpec, FALSE_WORDS, INFO_SHEET, LIST_SEPARATOR, SHEETS_V1, SheetSpec,
    TRUE_WORDS, VERSION_CELL, XLSX_TEMPLATE_V1, find_sheet,
};
use auler_domain::time::{ClockTime, Date, Weekday};
use calamine::{DataRef, Reader, Xlsx, XlsxError};
use serde_json::{Map, Number, Value, json};

#[derive(Debug)]
pub enum ImportError {
    Workbook {
        source: SourceRef,
        error: Box<XlsxError>,
    },
    Validation(Vec<Diagnostic>),
    Document(serde_json::Error),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workbook { error, .. } => write!(f, "XLSX 파일을 읽을 수 없음 ({error})"),
            Self::Validation(issues) => write!(f, "표준 XLSX에서 확인할 항목 {}개", issues.len()),
            Self::Document(error) => write!(f, "공통 입력 문서로 변환할 수 없음 ({error})"),
        }
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub source: SourceRef,
    pub kind: Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    MissingSheet {
        key: &'static str,
    },
    UnknownSheet,
    DuplicateSheet {
        key: &'static str,
    },
    UnsupportedVersion {
        found: Option<String>,
    },
    MissingColumn {
        key: &'static str,
    },
    UnknownColumn {
        found: String,
    },
    DuplicateColumn {
        key: &'static str,
    },
    MissingValue {
        key: &'static str,
    },
    InvalidValue {
        key: &'static str,
        expected: ColumnKind,
    },
    Formula,
    TermRowCount {
        found: usize,
    },
    ConflictingConstraint {
        key: String,
        field: String,
    },
}

impl Problem {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingSheet { .. } => "missing_sheet",
            Self::UnknownSheet => "unknown_sheet",
            Self::DuplicateSheet { .. } => "duplicate_sheet",
            Self::UnsupportedVersion { .. } => "unsupported_version",
            Self::MissingColumn { .. } => "missing_column",
            Self::UnknownColumn { .. } => "unknown_column",
            Self::DuplicateColumn { .. } => "duplicate_column",
            Self::MissingValue { .. } => "missing_value",
            Self::InvalidValue { .. } => "invalid_value",
            Self::Formula => "formula",
            Self::TermRowCount { .. } => "term_row_count",
            Self::ConflictingConstraint { .. } => "conflicting_constraint",
        }
    }

    pub fn guidance(&self) -> &'static str {
        match self {
            Self::MissingSheet { .. } | Self::UnsupportedVersion { .. } => {
                "지원되는 최신 Auler 표준 양식에 값을 복사하세요."
            }
            Self::UnknownSheet | Self::DuplicateSheet { .. } => {
                "표준 양식에서 정의한 각 시트를 한 번씩만 사용하세요."
            }
            Self::MissingColumn { .. }
            | Self::UnknownColumn { .. }
            | Self::DuplicateColumn { .. } => "첫 행의 머리글을 표준 양식과 맞추세요.",
            Self::MissingValue { .. } => "필수 셀에 값을 입력하세요.",
            Self::InvalidValue { .. } => "표준 양식 머리글에 있는 값 형식과 예시를 확인하세요.",
            Self::Formula => "수식 결과를 값으로 붙여넣으세요.",
            Self::TermRowCount { .. } => "학기 시트에 학기 한 개를 입력하세요.",
            Self::ConflictingConstraint { .. } => {
                "같은 조건 번호의 대상, 강도 및 적용 기간을 일치시키세요."
            }
        }
    }
}

#[derive(Debug)]
pub struct WorkbookPreview {
    pub document: InputDocument,
    pub worksheets: Vec<WorksheetPreview>,
}

#[derive(Debug, Default)]
pub struct WorksheetPreview {
    pub name: String,
    pub last_row: u32,
    pub columns: u32,
    pub original_rows: BTreeMap<u32, Vec<OriginalCell>>,
}

#[derive(Debug)]
pub struct OriginalCell {
    pub column: String,
    pub text: String,
}

pub fn import(path: impl AsRef<Path>) -> Result<InputDocument, ImportError> {
    let (file, name) = open(path.as_ref())?;
    import_reader(file, &name)
}

pub fn import_with_preview(path: impl AsRef<Path>) -> Result<WorkbookPreview, ImportError> {
    let (file, name) = open(path.as_ref())?;
    let mut worksheets = Vec::new();
    let document = read_workbook(file, &name, Some(&mut worksheets))?;
    Ok(WorkbookPreview {
        document,
        worksheets,
    })
}

fn open(path: &Path) -> Result<(std::io::BufReader<std::fs::File>, String), ImportError> {
    let file = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    let data = std::fs::File::open(path).map_err(|error| ImportError::Workbook {
        source: location(&file, None, None, None),
        error: Box::new(error.into()),
    })?;
    Ok((std::io::BufReader::new(data), file.into_owned()))
}

pub fn import_reader<R: Read + Seek>(input: R, file: &str) -> Result<InputDocument, ImportError> {
    read_workbook(input, file, None)
}

fn read_workbook<R: Read + Seek>(
    input: R,
    file: &str,
    mut preview: Option<&mut Vec<WorksheetPreview>>,
) -> Result<InputDocument, ImportError> {
    let workbook_error = |sheet, error| ImportError::Workbook {
        source: location(file, sheet, None, None),
        error: Box::new(error),
    };
    let mut workbook = Xlsx::new(input).map_err(|error| workbook_error(None, error))?;
    let mut diagnostics = Vec::new();
    let mut names = BTreeMap::new();
    for name in workbook.sheet_names() {
        if let Some(spec) = if INFO_SHEET.matches(&name) {
            Some(&INFO_SHEET)
        } else {
            find_sheet(&name)
        } {
            match names.entry(spec.key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(name);
                }
                std::collections::btree_map::Entry::Occupied(_) => diagnostics.push(Diagnostic {
                    source: location(file, Some(&name), None, None),
                    kind: Problem::DuplicateSheet { key: spec.key },
                }),
            }
        } else {
            diagnostics.push(Diagnostic {
                source: location(file, Some(&name), None, None),
                kind: Problem::UnknownSheet,
            });
        }
    }
    for spec in std::iter::once(&INFO_SHEET).chain(SHEETS_V1) {
        if !spec.optional && !names.contains_key(spec.key) {
            diagnostics.push(Diagnostic {
                source: location(file, None, None, None),
                kind: Problem::MissingSheet { key: spec.key },
            });
        }
    }
    if let Some(name) = names.get(INFO_SHEET.key) {
        let mut cells = workbook
            .worksheet_cells_reader(name)
            .map_err(|error| workbook_error(Some(name), error))?;
        let mut version = None;
        let mut formula = false;
        while let Some(cell) = cells
            .next_cell_with_formula_metadata()
            .map_err(|error| workbook_error(Some(name), error))?
        {
            if let Some(worksheets) = preview.as_deref_mut() {
                if worksheets.is_empty() {
                    worksheets.push(WorksheetPreview {
                        name: name.clone(),
                        ..WorksheetPreview::default()
                    });
                }
                record_original(&mut worksheets[0], cell.pos, &cell.value);
            }
            if cell.pos == (VERSION_CELL.0, u32::from(VERSION_CELL.1)) {
                formula = cell.formula.is_some();
                if !formula {
                    version = text(&cell.value).map(str::to_owned);
                }
                if preview.is_none() {
                    break;
                }
            }
        }
        if version.as_deref() != Some(XLSX_TEMPLATE_V1) {
            diagnostics.push(Diagnostic {
                source: location(
                    file,
                    Some(name),
                    Some(VERSION_CELL.0 + 1),
                    Some(VERSION_CELL.1.into()),
                ),
                kind: if formula {
                    Problem::Formula
                } else {
                    Problem::UnsupportedVersion { found: version }
                },
            });
        }
    }
    if !diagnostics.is_empty() {
        return Err(ImportError::Validation(diagnostics));
    }
    let mut sheets = BTreeMap::new();
    for spec in SHEETS_V1 {
        if let Some(name) = names.get(spec.key) {
            let worksheet = preview.as_deref_mut().map(|worksheets| {
                worksheets.push(WorksheetPreview {
                    name: name.clone(),
                    ..WorksheetPreview::default()
                });
                worksheets.last_mut().expect("worksheet was just added")
            });
            let rows = read_sheet(&mut workbook, file, name, spec, &mut diagnostics, worksheet)
                .map_err(|error| workbook_error(Some(name), error))?;
            sheets.insert(spec.key, rows);
        }
    }
    if let Some(rows) = sheets.get("term")
        && rows.len() != 1
    {
        diagnostics.push(Diagnostic {
            source: rows
                .get(1)
                .map(|row| row.source.clone())
                .unwrap_or_else(|| {
                    location(file, names.get("term").map(String::as_str), Some(2), None)
                }),
            kind: Problem::TermRowCount { found: rows.len() },
        });
    }
    if !diagnostics.is_empty() {
        return Err(ImportError::Validation(diagnostics));
    }
    document::assemble(sheets)
}

#[derive(Default)]
struct Row {
    source: SourceRef,
    values: Map<String, Value>,
    cells: BTreeMap<String, SourceRef>,
    columns: Rc<BTreeMap<&'static str, u32>>,
}

impl Row {
    fn field(&self, key: &str) -> SourceRef {
        self.cells.get(key).cloned().unwrap_or_else(|| {
            let mut source = self.source.clone();
            source.column = self.columns.get(key).copied().map(column_name);
            source
        })
    }
}

fn read_sheet<R: Read + Seek>(
    workbook: &mut Xlsx<R>,
    file: &str,
    name: &str,
    spec: &SheetSpec,
    diagnostics: &mut Vec<Diagnostic>,
    mut preview: Option<&mut WorksheetPreview>,
) -> Result<Vec<Row>, XlsxError> {
    let mut reader = workbook.worksheet_cells_reader(name)?;
    let mut columns: BTreeMap<u32, &ColumnSpec> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut unheaded = BTreeSet::new();
    let mut rows: BTreeMap<u32, Row> = BTreeMap::new();
    while let Some(cell) = reader.next_cell_with_formula_metadata()? {
        let (row, column) = cell.pos;
        if let Some(preview) = preview.as_deref_mut() {
            record_original(preview, cell.pos, &cell.value);
        }
        let source = || location(file, Some(name), Some(row + 1), Some(column));
        if cell.formula.is_some() {
            diagnostics.push(Diagnostic {
                source: source(),
                kind: Problem::Formula,
            });
            if row != 0
                && let Some(spec) = columns.get(&column)
            {
                let record = rows.entry(row).or_insert_with(|| Row {
                    source: location(file, Some(name), Some(row + 1), None),
                    ..Row::default()
                });
                record.cells.insert(spec.key.into(), source());
            }
            continue;
        }
        if matches!(cell.value, DataRef::Empty) || text(&cell.value).is_some_and(str::is_empty) {
            continue;
        }
        if row == 0 {
            let heading = text(&cell.value);
            let found =
                heading.and_then(|heading| spec.columns.iter().find(|spec| spec.matches(heading)));
            match found {
                Some(spec) if seen.insert(spec.key) => {
                    columns.insert(column, spec);
                }
                Some(spec) => diagnostics.push(Diagnostic {
                    source: source(),
                    kind: Problem::DuplicateColumn { key: spec.key },
                }),
                None => diagnostics.push(Diagnostic {
                    source: source(),
                    kind: Problem::UnknownColumn {
                        found: heading.map_or_else(|| format!("{:?}", cell.value), str::to_owned),
                    },
                }),
            }
            continue;
        }
        let record = rows.entry(row).or_insert_with(|| Row {
            source: location(file, Some(name), Some(row + 1), None),
            ..Row::default()
        });
        if let Some(spec) = columns.get(&column) {
            record.cells.insert(spec.key.into(), source());
            if let Some(value) = cell_value(&cell.value, spec) {
                record.values.insert(spec.key.into(), value);
            } else {
                diagnostics.push(Diagnostic {
                    source: source(),
                    kind: Problem::InvalidValue {
                        key: spec.key,
                        expected: spec.kind,
                    },
                });
            }
        } else if unheaded.insert(column) {
            diagnostics.push(Diagnostic {
                source: source(),
                kind: Problem::UnknownColumn {
                    found: String::new(),
                },
            });
        }
    }
    for col in spec
        .columns
        .iter()
        .filter(|col| col.required && !seen.contains(col.key))
    {
        diagnostics.push(Diagnostic {
            source: location(file, Some(name), Some(1), None),
            kind: Problem::MissingColumn { key: col.key },
        });
    }
    let field_columns = Rc::new(
        columns
            .iter()
            .map(|(&index, spec)| (spec.key, index))
            .collect(),
    );
    for (row_index, row) in &mut rows {
        row.columns = Rc::clone(&field_columns);
        for (&column, spec) in &columns {
            if spec.required && !row.cells.contains_key(spec.key) {
                diagnostics.push(Diagnostic {
                    source: location(file, Some(name), Some(row_index + 1), Some(column)),
                    kind: Problem::MissingValue { key: spec.key },
                });
            }
        }
    }
    Ok(rows.into_values().collect())
}

fn record_original(preview: &mut WorksheetPreview, (row, column): (u32, u32), value: &DataRef<'_>) {
    if matches!(value, DataRef::Empty) {
        return;
    }
    preview.last_row = preview.last_row.max(row + 1);
    preview.columns = preview.columns.max(column + 1);
    preview
        .original_rows
        .entry(row + 1)
        .or_default()
        .push(OriginalCell {
            column: column_name(column),
            text: calamine::Data::from(value.clone()).to_string(),
        });
}

fn text<'a>(data: &'a DataRef<'_>) -> Option<&'a str> {
    match data {
        DataRef::String(text) => Some(text.trim()),
        DataRef::SharedString(text) => Some(text.trim()),
        _ => None,
    }
}

fn cell_value(data: &DataRef<'_>, column: &ColumnSpec) -> Option<Value> {
    let text = text(data);
    let code = || match data {
        DataRef::Int(value) => Some(json!(value)),
        DataRef::Float(value) => Number::from_f64(*value).map(Value::Number),
        _ => text.map(|value| json!(value)),
    };
    match column.kind {
        ColumnKind::Code => code(),
        ColumnKind::Text if column.key == "target" => code(),
        ColumnKind::Text => Some(json!(text?)),
        ColumnKind::Integer => {
            let max = match column.key {
                "grade" | "weekly_periods" | "part" | "weight" => u8::MAX.into(),
                "year" | "room_turnaround_minutes" => u16::MAX.into(),
                _ => u32::MAX,
            };
            unsigned_integer(data, max).map(|value| json!(value))
        }
        ColumnKind::Bool => {
            let value = match data {
                DataRef::Bool(value) => *value,
                _ => {
                    let txt = text?;
                    if TRUE_WORDS.iter().any(|word| word.eq_ignore_ascii_case(txt)) {
                        true
                    } else if FALSE_WORDS
                        .iter()
                        .any(|word| word.eq_ignore_ascii_case(txt))
                    {
                        false
                    } else {
                        return None;
                    }
                }
            };
            Some(json!(value))
        }
        ColumnKind::Day => Some(json!(Weekday::parse(text?)?.code())),
        ColumnKind::DayList => {
            let days: Option<Vec<_>> = text?
                .split(LIST_SEPARATOR)
                .map(|d| Weekday::parse(d).map(Weekday::code))
                .collect();
            Some(json!(days?))
        }
        ColumnKind::CodeList => {
            let values = if let Some(text) = text {
                text.split(LIST_SEPARATOR)
                    .map(str::trim)
                    .map(|value| (!value.is_empty()).then(|| json!(value)))
                    .collect::<Option<Vec<_>>>()?
            } else {
                vec![code()?]
            };
            Some(Value::Array(values))
        }
        ColumnKind::Split => {
            let parts = if let Some(text) = text {
                text.split('+')
                    .map(|value| value.trim().parse::<u8>().ok())
                    .collect::<Option<Vec<_>>>()?
            } else {
                vec![u8::try_from(unsigned_integer(data, u8::MAX.into())?).ok()?]
            };
            Some(json!(parts))
        }
        ColumnKind::Enum(values) => {
            let choice = match (text, column.key) {
                (Some(text), _) => text,
                (None, "season") => match unsigned_integer(data, 2)? {
                    1 => "1",
                    2 => "2",
                    _ => return None,
                },
                _ => return None,
            };
            Some(json!(
                values.iter().find(|value| value.matches(choice))?.key
            ))
        }
        ColumnKind::Date | ColumnKind::Time => {
            let formatted = match data {
                DataRef::DateTime(value) if value.is_datetime() && value.as_f64().is_finite() => {
                    let (year, month, day, hour, minute, second, millis) = value.to_ymd_hms_milli();
                    match column.kind {
                        ColumnKind::Date
                            if value.as_f64() >= 0.0
                                && (hour, minute, second, millis) == (0, 0, 0, 0) =>
                        {
                            format!("{year:04}-{month:02}-{day:02}")
                        }
                        ColumnKind::Time
                            if (0.0..1.0).contains(&value.as_f64())
                                && (second, millis) == (0, 0) =>
                        {
                            format!("{hour:02}:{minute:02}")
                        }
                        _ => return None,
                    }
                }
                DataRef::DateTimeIso(value) => {
                    let (date, time) = value
                        .split_once('T')
                        .map_or((None, value.as_str()), |(date, time)| (Some(date), time));
                    if column.kind == ColumnKind::Date {
                        match date {
                            Some(date) if iso_minutes(time)? == "00:00" => date.to_owned(),
                            None => value.clone(),
                            _ => return None,
                        }
                    } else if date.is_none() {
                        iso_minutes(time)?.to_owned()
                    } else {
                        return None;
                    }
                }
                _ => text?.to_owned(),
            };
            if column.kind == ColumnKind::Date {
                Some(json!(Date::parse(&formatted)?.to_string()))
            } else {
                Some(json!(ClockTime::parse(&formatted)?.to_string()))
            }
        }
    }
}

fn unsigned_integer(data: &DataRef<'_>, max: u32) -> Option<u32> {
    let value = match data {
        DataRef::Int(value) => u32::try_from(*value).ok(),
        DataRef::Float(value)
            if *value >= 0.0 && *value <= f64::from(max) && value.fract() == 0.0 =>
        {
            Some(*value as u32)
        }
        _ => text(data)?.parse().ok(),
    }?;
    (value <= max).then_some(value)
}

fn iso_minutes(time: &str) -> Option<&str> {
    let suffix = time.get(5..)?;
    if suffix.is_empty()
        || suffix == ":00"
        || suffix
            .strip_prefix(":00.")
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|digit| digit == b'0'))
    {
        time.get(..5)
    } else {
        None
    }
}

fn location(file: &str, sheet: Option<&str>, row: Option<u32>, column: Option<u32>) -> SourceRef {
    SourceRef {
        file: Some(file.to_owned()),
        sheet: sheet.map(str::to_owned),
        row,
        column: column.map(column_name),
    }
}

fn column_name(mut column: u32) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(char::from(b'A' + (column % 26) as u8));
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    letters.into_iter().rev().collect()
}
