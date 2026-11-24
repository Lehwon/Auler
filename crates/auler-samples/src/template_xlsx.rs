//! 표준 XLSX 양식 v1 파일을 만든다 (한국어판, 영어판).
//!
//! 시트와 열은 [`auler_domain::template`] 규격 그대로다. 코드 열은 텍스트 서식이라
//! `01`을 적어도 숫자로 바뀌지 않는다. 정해진 값 중 하나를 고르는 열과 예/아니오 열에는
//! 목록 선택을 붙이고, 머리글 메모에 필수 여부와 예시를 적는다. 예시 행은 넣지 않는다
//! (가져올 때 자료로 읽히지 않게).

use auler_domain::template::{
    ColumnKind, INFO_SHEET, SHEETS_V1, SheetSpec, VERSION_CELL, XLSX_TEMPLATE_V1,
};
use rust_xlsxwriter::{
    DataValidation, DocProperties, ExcelDateTime, Format, FormatBorder, Note, Workbook, XlsxError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ko,
    En,
}

impl Lang {
    fn pick(self, ko: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Ko => ko,
            Lang::En => en,
        }
    }

    pub fn file_name(self) -> &'static str {
        self.pick("auler-standard-v1.ko.xlsx", "auler-standard-v1.en.xlsx")
    }
}

/// 양식이 목록 선택과 서식을 거는 행 수 (머리글 제외)
const FORMATTED_ROWS: u32 = 2000;

const GUIDE_KO: &[&str] = &[
    "각 시트의 첫 행은 머리글이다. 머리글은 한국어나 영어로 적을 수 있고 열 순서는 바꿔도 된다. 모르는 머리글이 있으면 가져오지 않는다.",
    "자료는 둘째 행부터 적는다. 빈 행은 건너뛴다. 필요 없는 선택 시트는 지워도 된다.",
    "코드 열(학수번호, 분반, 교원 코드 등)은 텍스트 서식이다. 숫자 칸으로 바뀌면 앞자리 0이 사라질 수 있어 '확인 필요'로 표시된다.",
    "여러 값은 ;로 구분한다 (예: 월;화;수, P01;P05). 분할은 +로 적는다 (예: 2+1).",
    "날짜는 YYYY-MM-DD, 시각은 HH:MM으로 적는다. 엑셀 날짜·시각 칸도 받는다.",
    "편성 조건 시트는 한 행이 시간 범위 하나다. 조건 번호가 같은 행은 한 조건으로 묶는다. 시간은 교시(시작·끝 교시)나 시각(시작·종료 시각) 중 하나로 적는다.",
    "확인 대기 조건은 편성에 적용하지 않는다. 가져온 자료는 검토 화면에서 확인한 뒤 적용된다.",
];

const GUIDE_EN: &[&str] = &[
    "The first row of each sheet is the header. Headers may be in Korean or English and columns may be in any order. Files with unknown headers are not imported.",
    "Enter data from the second row. Blank rows are skipped. Optional sheets you don't need can be deleted.",
    "Code columns (course code, section, instructor code, ...) are formatted as text. If a code becomes a number cell, leading zeros may be lost, so it is flagged as 'Needs review'.",
    "Separate multiple values with ; (e.g. mon;tue;wed, P01;P05). Write splits with + (e.g. 2+1).",
    "Dates are YYYY-MM-DD and times are HH:MM. Excel date and time cells are also accepted.",
    "In the Constraints sheet, each row is one time range. Rows with the same constraint no. form one constraint. Give times either as periods (first/last period) or clock times (start/end time).",
    "Pending constraints are not applied when scheduling. Imported data is applied only after you review it.",
];

fn note_text(lang: Lang, required: bool, kind: ColumnKind, example: &str) -> String {
    let need = match (lang, required) {
        (Lang::Ko, true) => "필수",
        (Lang::Ko, false) => "선택",
        (Lang::En, true) => "Required",
        (Lang::En, false) => "Optional",
    };
    let hint = match (lang, kind) {
        (Lang::Ko, ColumnKind::Code) => " · 텍스트 코드",
        (Lang::En, ColumnKind::Code) => " · text code",
        (Lang::Ko, ColumnKind::CodeList | ColumnKind::DayList) => " · ;로 구분",
        (Lang::En, ColumnKind::CodeList | ColumnKind::DayList) => " · separated by ;",
        _ => "",
    };
    match (lang, example.is_empty()) {
        (_, true) => format!("{need}{hint}"),
        (Lang::Ko, false) => format!("{need}{hint}\n예: {example}"),
        (Lang::En, false) => format!("{need}{hint}\ne.g. {}", english_example(example)),
    }
}

/// 영어판 메모의 예시: 정해진 값과 요일은 영어로 바꾼다
fn english_example(example: &str) -> String {
    use auler_domain::template::{
        CLASS_KINDS, EQUIPMENT_SCOPES, ROOM_KINDS, SEASONS, STATUSES, STRENGTHS, TARGET_KINDS,
    };
    use auler_domain::time::Weekday;
    for values in [
        CLASS_KINDS,
        EQUIPMENT_SCOPES,
        ROOM_KINDS,
        SEASONS,
        STATUSES,
        STRENGTHS,
        TARGET_KINDS,
    ] {
        if let Some(v) = values.iter().find(|v| v.ko == example) {
            return v.en.to_owned();
        }
    }
    match example {
        "예" => return "yes".into(),
        "아니오" => return "no".into(),
        _ => {}
    }
    let days: Option<Vec<&str>> = example
        .split(';')
        .map(|d| Weekday::parse(d).map(Weekday::code))
        .collect();
    match days {
        Some(days) => days.join(";"),
        None => example.to_owned(),
    }
}

fn add_sheet(wb: &mut Workbook, spec: &SheetSpec, lang: Lang) -> Result<(), XlsxError> {
    let ws = wb.add_worksheet();
    ws.set_name(lang.pick(spec.ko, spec.en))?;
    let base = Format::new()
        .set_bold()
        .set_border(FormatBorder::Thin)
        .set_text_wrap();
    let required = base.clone().set_background_color(0xD6E4F5);
    let optional = base.set_background_color(0xF1F3F6);
    let text = Format::new().set_num_format("@");
    for (c, col) in spec.columns.iter().enumerate() {
        let c = c as u16;
        let header = lang.pick(col.ko, col.en);
        let format = if col.required { &required } else { &optional };
        ws.write_string_with_format(0, c, header, format)?;
        let width = header.chars().count().max(col.example.chars().count()) as f64 * 1.6 + 4.0;
        ws.set_column_width(c, width.clamp(10.0, 48.0))?;
        ws.insert_note(
            0,
            c,
            &Note::new(note_text(lang, col.required, col.kind, col.example))
                .add_author_prefix(false),
        )?;
        match col.kind {
            ColumnKind::Code | ColumnKind::CodeList | ColumnKind::Split => {
                ws.set_column_format(c, &text)?;
            }
            ColumnKind::Enum(values) => {
                let list: Vec<&str> = values.iter().map(|v| lang.pick(v.ko, v.en)).collect();
                let dv = DataValidation::new().allow_list_strings(&list)?;
                ws.add_data_validation(1, c, FORMATTED_ROWS, c, &dv)?;
            }
            ColumnKind::Bool => {
                let list = [lang.pick("예", "yes"), lang.pick("아니오", "no")];
                let dv = DataValidation::new().allow_list_strings(&list)?;
                ws.add_data_validation(1, c, FORMATTED_ROWS, c, &dv)?;
            }
            _ => {}
        }
    }
    ws.set_freeze_panes(1, 0)?;
    Ok(())
}

/// 표준 양식 한 언어판
pub fn build(lang: Lang) -> Result<Vec<u8>, XlsxError> {
    let mut wb = Workbook::new();
    let date = ExcelDateTime::from_ymd(2027, 1, 1)?;
    wb.set_properties(
        &DocProperties::new()
            .set_title(lang.pick("Auler 표준 양식 v1", "Auler standard template v1"))
            .set_creation_datetime(&date),
    );

    let info = wb.add_worksheet();
    info.set_name(lang.pick(INFO_SHEET.ko, INFO_SHEET.en))?;
    info.write_string_with_format(
        0,
        0,
        lang.pick("Auler 표준 양식", "Auler standard template"),
        &Format::new().set_bold().set_font_size(14),
    )?;
    info.write_string(
        VERSION_CELL.0,
        VERSION_CELL.1 - 1,
        lang.pick("양식 버전", "Template version"),
    )?;
    info.write_string(VERSION_CELL.0, VERSION_CELL.1, XLSX_TEMPLATE_V1)?;
    info.set_column_width(0, 18)?;
    info.set_column_width(1, 110)?;
    let guide = match lang {
        Lang::Ko => GUIDE_KO,
        Lang::En => GUIDE_EN,
    };
    let wrap = Format::new().set_text_wrap();
    for (i, line) in guide.iter().enumerate() {
        info.write_string_with_format(3 + i as u32, 1, *line, &wrap)?;
    }

    for spec in SHEETS_V1 {
        add_sheet(&mut wb, spec, lang)?;
    }
    wb.save_to_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_examples_translate_fixed_values() {
        assert_eq!(english_example("실습실"), "lab");
        assert_eq!(english_example("월;화;수;목;금"), "mon;tue;wed;thu;fri");
        assert_eq!(english_example("아니오"), "no");
        assert_eq!(english_example("CSE2010"), "CSE2010");
        assert_eq!(english_example("2+1"), "2+1");
    }
}
