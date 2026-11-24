//! 표준 XLSX 양식 v1의 시트·열 규격.
//!
//! 양식 파일을 만드는 쪽과 읽는 쪽이 이 정의 하나를 함께 쓴다. 시트 이름과 열 머리글은
//! 한국어와 영어 중 어느 쪽이어도 같은 열로 읽는다. 데이터는 머리글 다음 행부터다.
//! 가져오기는 이 규격에 맞는 파일만 받는다: 열을 임의로 잇거나 값을 추측하지 않는다.

/// 양식 버전. 안내 시트의 [`VERSION_CELL`]에 이 값이 있어야 한다.
pub const XLSX_TEMPLATE_V1: &str = "auler.xlsx.v1";

/// 안내 시트에서 양식 버전이 있는 칸 (0부터 센 행, 열) = B2
pub const VERSION_CELL: (u32, u16) = (1, 1);

/// 목록 칸의 구분자 (`월;화;수`, `T01;T02`)
pub const LIST_SEPARATOR: char = ';';

/// 정해진 값 중 하나. 칸에는 한국어나 영어 표기, 또는 입력 규격의 키를 쓴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumValue {
    /// 입력 규격([`crate::input`])의 값
    pub key: &'static str,
    pub ko: &'static str,
    pub en: &'static str,
}

impl EnumValue {
    pub fn matches(&self, text: &str) -> bool {
        let t = text.trim();
        t == self.key || t == self.ko || t.eq_ignore_ascii_case(self.en)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    /// 코드. 문자열 칸이어야 한다. 숫자 칸이면 앞자리 0이 사라졌을 수 있어 확인 필요로 표시한다
    Code,
    Text,
    /// 0 이상의 정수
    Integer,
    /// 예/아니오
    Bool,
    /// 요일 (`월`, `mon`)
    Day,
    /// 시각 `HH:MM` (엑셀 시각 칸도 받는다)
    Time,
    /// 날짜 `YYYY-MM-DD` (엑셀 날짜 칸도 받는다)
    Date,
    /// `;`로 구분한 코드 목록
    CodeList,
    /// `;`로 구분한 요일 목록
    DayList,
    /// 분할: `+`로 구분한 교시 수 (`2+1`)
    Split,
    Enum(&'static [EnumValue]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    /// 입력 규격의 필드 이름
    pub key: &'static str,
    pub ko: &'static str,
    pub en: &'static str,
    pub required: bool,
    pub kind: ColumnKind,
    /// 양식에 넣는 예시 값
    pub example: &'static str,
}

impl ColumnSpec {
    /// 머리글이 이 열인지 (한국어, 영어, 키 모두 받는다)
    pub fn matches(&self, header: &str) -> bool {
        let h = header.trim();
        h == self.key || h == self.ko || h.eq_ignore_ascii_case(self.en)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetSpec {
    pub key: &'static str,
    pub ko: &'static str,
    pub en: &'static str,
    /// 이 시트가 없어도 되는지
    pub optional: bool,
    pub columns: &'static [ColumnSpec],
}

impl SheetSpec {
    pub fn matches(&self, name: &str) -> bool {
        let n = name.trim();
        n == self.key || n == self.ko || n.eq_ignore_ascii_case(self.en)
    }

    pub fn column(&self, key: &str) -> Option<&'static ColumnSpec> {
        self.columns.iter().find(|c| c.key == key)
    }
}

const fn col(
    key: &'static str,
    ko: &'static str,
    en: &'static str,
    required: bool,
    kind: ColumnKind,
    example: &'static str,
) -> ColumnSpec {
    ColumnSpec {
        key,
        ko,
        en,
        required,
        kind,
        example,
    }
}

const fn val(key: &'static str, ko: &'static str, en: &'static str) -> EnumValue {
    EnumValue { key, ko, en }
}

pub const SEASONS: &[EnumValue] = &[
    val("1", "1학기", "1"),
    val("summer", "여름학기", "summer"),
    val("2", "2학기", "2"),
    val("winter", "겨울학기", "winter"),
];

pub const STRENGTHS: &[EnumValue] = &[val("hard", "필수", "hard"), val("soft", "선호", "soft")];

pub const STATUSES: &[EnumValue] = &[
    val("confirmed", "확인됨", "confirmed"),
    val("pending", "확인 대기", "pending"),
];

pub const EQUIPMENT_SCOPES: &[EnumValue] = &[
    val("per_person", "1인당", "per person"),
    val("per_room", "강의실당", "per room"),
];

pub const ROOM_KINDS: &[EnumValue] = &[
    val("lecture", "강의실", "lecture"),
    val("lab", "실습실", "lab"),
    val("seminar", "세미나실", "seminar"),
    val("other", "기타", "other"),
];

pub const CLASS_KINDS: &[EnumValue] = &[
    val("lecture", "이론", "lecture"),
    val("lab", "실습", "lab"),
    val("mixed", "이론+실습", "mixed"),
];

pub const TARGET_KINDS: &[EnumValue] = &[
    val("instructor", "교원", "instructor"),
    val("section", "분반", "section"),
    val("group", "수강 집단", "group"),
    val("room", "강의실", "room"),
];

/// 예/아니오 칸이 받는 값
pub const TRUE_WORDS: &[&str] = &["예", "yes", "y", "true", "o"];
pub const FALSE_WORDS: &[&str] = &["아니오", "no", "n", "false", "x"];

use ColumnKind::*;

/// 안내 시트 (양식 버전과 사용법)
pub const INFO_SHEET: SheetSpec = SheetSpec {
    key: "info",
    ko: "안내",
    en: "Info",
    optional: false,
    columns: &[],
};

/// 표준 XLSX 양식 v1의 데이터 시트. 순서대로 양식에 들어간다.
pub const SHEETS_V1: &[SheetSpec] = &[
    SheetSpec {
        key: "term",
        ko: "학기",
        en: "Term",
        optional: false,
        columns: &[
            col("year", "연도", "Year", true, Integer, "2027"),
            col("season", "학기", "Term", true, Enum(SEASONS), "1학기"),
            col("start", "시작일", "Start date", false, Date, "2027-03-02"),
            col("end", "종료일", "End date", false, Date, "2027-06-15"),
            col(
                "days",
                "수업 요일",
                "Class days",
                true,
                DayList,
                "월;화;수;목;금",
            ),
            col(
                "required_course_overlap",
                "필수과목 중복",
                "Required-course overlap",
                false,
                Enum(STRENGTHS),
                "필수",
            ),
            col(
                "block_may_cross_break",
                "연강의 휴게시간 통과",
                "Blocks may cross breaks",
                false,
                Bool,
                "아니오",
            ),
            col(
                "room_turnaround_minutes",
                "강의실 준비·정리(분)",
                "Room turnaround (min)",
                false,
                Integer,
                "0",
            ),
        ],
    },
    SheetSpec {
        key: "periods",
        ko: "교시표",
        en: "Periods",
        optional: false,
        columns: &[
            col("label", "교시", "Period", true, Code, "1"),
            col("start", "시작", "Start", true, Time, "09:00"),
            col("end", "종료", "End", true, Time, "09:50"),
        ],
    },
    SheetSpec {
        key: "breaks",
        ko: "휴게시간",
        en: "Breaks",
        optional: true,
        columns: &[
            col("label", "이름", "Name", true, Text, "점심"),
            col("start", "시작", "Start", true, Time, "12:00"),
            col("end", "종료", "End", true, Time, "13:00"),
        ],
    },
    SheetSpec {
        key: "equipment",
        ko: "장비",
        en: "Equipment",
        optional: true,
        columns: &[
            col("code", "장비 코드", "Equipment code", true, Code, "PC"),
            col("name", "장비 이름", "Name", true, Text, "실습용 PC"),
            col(
                "scope",
                "필요량 기준",
                "Counted",
                true,
                Enum(EQUIPMENT_SCOPES),
                "1인당",
            ),
        ],
    },
    SheetSpec {
        key: "rooms",
        ko: "강의실",
        en: "Rooms",
        optional: false,
        columns: &[
            col("building", "건물", "Building", true, Text, "공학관"),
            col("number", "호실", "Room", true, Code, "302"),
            col("kind", "유형", "Type", true, Enum(ROOM_KINDS), "실습실"),
            col(
                "department",
                "관리 부서",
                "Managed by",
                false,
                Text,
                "공과대학 행정실",
            ),
            col("total_seats", "총좌석", "Seats", true, Integer, "32"),
            col(
                "available_seats",
                "가용 좌석",
                "Usable seats",
                false,
                Integer,
                "32",
            ),
            col("shared", "공용실", "Shared", false, Bool, "아니오"),
            col(
                "schedule_checked",
                "일정 확인일",
                "Schedule checked on",
                false,
                Date,
                "",
            ),
            col(
                "info_source",
                "정보 출처",
                "Source",
                false,
                Text,
                "시설팀 목록",
            ),
        ],
    },
    SheetSpec {
        key: "room_equipment",
        ko: "강의실 장비",
        en: "Room equipment",
        optional: true,
        columns: &[
            col("building", "건물", "Building", true, Text, "공학관"),
            col("number", "호실", "Room", true, Code, "302"),
            col("equipment", "장비 코드", "Equipment code", true, Code, "PC"),
            col("owned", "보유", "Owned", true, Integer, "32"),
            col("available", "정상", "Working", false, Integer, "30"),
            col(
                "inspection_from",
                "점검 시작",
                "Inspection from",
                false,
                Date,
                "",
            ),
            col(
                "inspection_until",
                "점검 종료",
                "Inspection until",
                false,
                Date,
                "",
            ),
        ],
    },
    SheetSpec {
        key: "instructors",
        ko: "교원",
        en: "Instructors",
        optional: false,
        columns: &[
            col("code", "교원 코드", "Instructor code", true, Code, "T03"),
            col("name", "이름", "Name", true, Text, "이서연"),
            col(
                "department",
                "소속",
                "Department",
                false,
                Text,
                "컴퓨터공학과",
            ),
        ],
    },
    SheetSpec {
        key: "groups",
        ko: "수강 집단",
        en: "Student groups",
        optional: true,
        columns: &[
            col("code", "집단 코드", "Group code", true, Code, "Y2"),
            col("name", "이름", "Name", true, Text, "2학년"),
            col("grade", "학년", "Year level", false, Integer, "2"),
            col("track", "트랙", "Track", false, Text, ""),
        ],
    },
    SheetSpec {
        key: "courses",
        ko: "과목",
        en: "Courses",
        optional: false,
        columns: &[
            col("code", "학수번호", "Course code", true, Code, "CSE2010"),
            col("name", "과목명", "Course name", true, Text, "자료구조"),
        ],
    },
    SheetSpec {
        key: "sections",
        ko: "분반",
        en: "Sections",
        optional: false,
        columns: &[
            col("course", "학수번호", "Course code", true, Code, "CSE2010"),
            col("section", "분반", "Section", true, Code, "01"),
            col(
                "instructors",
                "교원 코드",
                "Instructor codes",
                false,
                CodeList,
                "T03",
            ),
            col("enrollment", "인원", "Enrollment", true, Integer, "28"),
            col(
                "provisional",
                "인원 잠정",
                "Provisional",
                false,
                Bool,
                "아니오",
            ),
            col(
                "kind",
                "수업 형태",
                "Class type",
                true,
                Enum(CLASS_KINDS),
                "실습",
            ),
            col(
                "room_kind",
                "강의실 유형",
                "Room type",
                false,
                Enum(ROOM_KINDS),
                "실습실",
            ),
            col(
                "weekly_periods",
                "주당 시수",
                "Periods per week",
                true,
                Integer,
                "3",
            ),
            col("split", "분할", "Split", false, Split, "2+1"),
        ],
    },
    SheetSpec {
        key: "section_groups",
        ko: "분반 수강 집단",
        en: "Section groups",
        optional: true,
        columns: &[
            col("course", "학수번호", "Course code", true, Code, "CSE2010"),
            col("section", "분반", "Section", true, Code, "01"),
            col("group", "집단 코드", "Group code", true, Code, "Y2"),
            col("required", "필수과목", "Required", false, Bool, "예"),
        ],
    },
    SheetSpec {
        key: "section_equipment",
        ko: "분반 장비",
        en: "Section equipment",
        optional: true,
        columns: &[
            col("course", "학수번호", "Course code", true, Code, "CSE2010"),
            col("section", "분반", "Section", true, Code, "01"),
            col("equipment", "장비 코드", "Equipment code", true, Code, "PC"),
            col("quantity", "수량", "Quantity", true, Integer, "1"),
        ],
    },
    SheetSpec {
        key: "constraints",
        ko: "편성 조건",
        en: "Constraints",
        optional: true,
        columns: &[
            col("key", "조건 번호", "Constraint no.", true, Code, "C1"),
            col(
                "target_kind",
                "대상 유형",
                "Target type",
                true,
                Enum(TARGET_KINDS),
                "교원",
            ),
            col("target", "대상", "Target", true, Text, "T03"),
            col(
                "target_section",
                "대상 분반",
                "Target section",
                false,
                Code,
                "",
            ),
            col("day", "요일", "Day", false, Day, "수"),
            col("first_period", "시작 교시", "First period", false, Code, ""),
            col("last_period", "끝 교시", "Last period", false, Code, ""),
            col("start", "시작 시각", "Start time", false, Time, "13:00"),
            col("end", "종료 시각", "End time", false, Time, "16:00"),
            col(
                "strength",
                "강도",
                "Strength",
                false,
                Enum(STRENGTHS),
                "선호",
            ),
            col("weight", "가중치", "Weight", false, Integer, "1"),
            col("status", "상태", "Status", true, Enum(STATUSES), "확인됨"),
            col("from", "적용 시작일", "From", false, Date, ""),
            col("until", "적용 종료일", "Until", false, Date, ""),
            col(
                "text",
                "요청 원문",
                "Original request",
                false,
                Text,
                "수요일 13~16시는 가능하면 비워 주세요.",
            ),
            col("confirmed_by", "확인자", "Confirmed by", false, Text, ""),
            col("note", "내부 메모", "Internal note", false, Text, ""),
        ],
    },
    SheetSpec {
        key: "placements",
        ko: "배치",
        en: "Placements",
        optional: true,
        columns: &[
            col("course", "학수번호", "Course code", true, Code, "CSE2010"),
            col("section", "분반", "Section", true, Code, "01"),
            col("part", "순번", "Part", false, Integer, "1"),
            col("day", "요일", "Day", true, Day, "목"),
            col("start_period", "시작 교시", "First period", true, Code, "3"),
            col("building", "건물", "Building", false, Text, "공학관"),
            col("number", "호실", "Room", false, Code, "302"),
            col("locked", "잠금", "Locked", false, Bool, "아니오"),
        ],
    },
];

/// 시트 이름(한국어·영어·키)으로 규격을 찾는다
pub fn find_sheet(name: &str) -> Option<&'static SheetSpec> {
    SHEETS_V1.iter().find(|s| s.matches(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_unique_in_both_languages() {
        let sheets: Vec<&SheetSpec> = std::iter::once(&INFO_SHEET).chain(SHEETS_V1).collect();
        for s in &sheets {
            for name in [s.key, s.ko, s.en] {
                let hits = sheets.iter().filter(|o| o.matches(name)).count();
                assert_eq!(hits, 1, "시트 이름 {name:?}이 여러 시트에 맞음");
            }
            assert!(
                s.ko.chars().count() <= 31 && s.en.len() <= 31,
                "엑셀 시트 이름은 31자까지"
            );
            let mut keys = HashSet::new();
            for c in s.columns {
                assert!(keys.insert(c.key), "{}.{} 키 중복", s.key, c.key);
                for header in [c.key, c.ko, c.en] {
                    let hits = s.columns.iter().filter(|o| o.matches(header)).count();
                    assert_eq!(hits, 1, "{}의 머리글 {header:?}이 여러 열에 맞음", s.key);
                }
            }
        }
    }

    #[test]
    fn sheets_resolve_by_any_name() {
        assert_eq!(find_sheet("분반").unwrap().key, "sections");
        assert_eq!(find_sheet("sections").unwrap().key, "sections");
        assert_eq!(find_sheet("Room Equipment").unwrap().key, "room_equipment");
        assert!(find_sheet("Sheet1").is_none());
        let sections = find_sheet("Sections").unwrap();
        assert!(sections.column("section").unwrap().matches(" 분반 "));
        assert_eq!(sections.column("section").unwrap().kind, Code);
    }

    #[test]
    fn enum_values_match_ko_en_and_key() {
        let soft = STRENGTHS[1];
        assert!(soft.matches("선호") && soft.matches("Soft") && soft.matches("soft"));
        assert!(!soft.matches("필수"));
        assert!(SEASONS[0].matches("1학기") && SEASONS[0].matches("1"));
    }

    #[test]
    fn examples_fit_their_kinds() {
        use crate::code::normalize_code;
        use crate::time::{ClockTime, Date, Weekday};
        for s in SHEETS_V1 {
            for c in s.columns {
                let e = c.example;
                if e.is_empty() {
                    assert!(!c.required, "{}.{} 필수 열에 예시가 없음", s.key, c.key);
                    continue;
                }
                let ok = match c.kind {
                    Code => normalize_code(e).is_ok(),
                    Text => true,
                    Integer => e.parse::<u32>().is_ok(),
                    Bool => TRUE_WORDS.contains(&e) || FALSE_WORDS.contains(&e),
                    Day => Weekday::parse(e).is_some(),
                    Time => ClockTime::parse(e).is_some(),
                    Date => Date::parse(e).is_some(),
                    CodeList => e.split(LIST_SEPARATOR).all(|p| normalize_code(p).is_ok()),
                    DayList => e.split(LIST_SEPARATOR).all(|p| Weekday::parse(p).is_some()),
                    Split => e.split('+').all(|p| p.trim().parse::<u8>().is_ok()),
                    Enum(values) => values.iter().any(|v| v.matches(e)),
                };
                assert!(ok, "{}.{} 예시 {e:?}", s.key, c.key);
            }
        }
    }
}
