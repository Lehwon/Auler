//! 공통 입력 규격 v1 (`auler.input.v1`).
//!
//! 표준 XLSX, 내장 ChatGPT의 구조화 출력, MCP 도구 입력이 모두 이 형식으로 들어와
//! 같은 검사([`crate::resolve`])를 거친다. 그래서 값은 느슨하게 받는다: 요일·시각·종류는
//! 문자열로 받아 검사에서 위치와 함께 알리고, 코드는 숫자로 와도 받아서 "확인 필요"로 표시한다.
//!
//! 기록은 내부 ID가 아니라 자연 키(학수번호, 분반 코드, 교원 코드, 건물+호실)로 서로를 가리킨다.
//! 모든 기록에 원본 위치(`source`)를 붙일 수 있다.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::SourceRef;

/// 이 규격의 식별자. 문서의 `schema` 값과 같아야 한다.
pub const SCHEMA_V1: &str = "auler.input.v1";

/// 코드 값. JSON 문자열과 숫자를 모두 받는다. 숫자로 온 코드는 앞자리 0이 사라졌을 수
/// 있으므로 `from_number`를 켜 두고 검사에서 "확인 필요"로 알린다.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CodeValue {
    pub text: String,
    pub from_number: bool,
}

impl CodeValue {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            from_number: false,
        }
    }

    pub fn number(n: i64) -> Self {
        Self {
            text: n.to_string(),
            from_number: true,
        }
    }
}

impl From<&str> for CodeValue {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<String> for CodeValue {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl fmt::Display for CodeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for CodeValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.text.parse::<i64>() {
            Ok(n) if self.from_number => s.serialize_i64(n),
            _ => s.serialize_str(&self.text),
        }
    }
}

impl<'de> Deserialize<'de> for CodeValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct CodeVisitor;

        impl Visitor<'_> for CodeVisitor {
            type Value = CodeValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("코드 문자열 또는 숫자")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<CodeValue, E> {
                Ok(CodeValue::text(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<CodeValue, E> {
                Ok(CodeValue::number(v))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<CodeValue, E> {
                Ok(CodeValue {
                    text: v.to_string(),
                    from_number: true,
                })
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<CodeValue, E> {
                let text = if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", v as i64)
                } else {
                    v.to_string()
                };
                Ok(CodeValue {
                    text,
                    from_number: true,
                })
            }
        }

        d.deserialize_any(CodeVisitor)
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 입력 문서 하나. 한 학기의 자료 전체나 일부를 담는다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDocument {
    /// 항상 [`SCHEMA_V1`]
    pub schema: String,
    pub term: TermInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<SetupInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<EquipmentInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rooms: Vec<RoomInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructors: Vec<InstructorInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courses: Vec<CourseInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<SectionInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<ConstraintInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<PlacementInput>,
    /// 변환하지 못한 원본 행. 추측해서 채우지 않고 여기에 남겨 사람이 확인하게 한다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedInput>,
}

impl InputDocument {
    /// 빈 문서
    pub fn new(year: u16, season: &str) -> Self {
        Self {
            schema: SCHEMA_V1.to_owned(),
            term: TermInput {
                year,
                season: season.to_owned(),
            },
            setup: None,
            equipment: Vec::new(),
            rooms: Vec::new(),
            instructors: Vec::new(),
            groups: Vec::new(),
            courses: Vec::new(),
            sections: Vec::new(),
            constraints: Vec::new(),
            placements: Vec::new(),
            unresolved: Vec::new(),
        }
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("입력 문서는 항상 JSON으로 바꿀 수 있다")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermInput {
    pub year: u16,
    /// `1`, `summer`, `2`, `winter`
    pub season: String,
}

/// 학기 설정
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupInput {
    /// 학기 시작일 `YYYY-MM-DD`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// 수업 요일 (`mon`…`sun`)
    pub days: Vec<String>,
    pub periods: Vec<PeriodInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<PeriodInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<RulesInput>,
}

/// 교시 또는 휴게시간. 시각은 `HH:MM`
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodInput {
    pub label: CodeValue,
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesInput {
    /// `hard` 또는 `soft`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_course_overlap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_may_cross_break: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_turnaround_minutes: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentInput {
    pub code: CodeValue,
    pub name: String,
    /// `per_person`(1인당) 또는 `per_room`(강의실당)
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomInput {
    pub building: String,
    pub number: CodeValue,
    /// `lecture`, `lab`, `seminar`, `other`
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    pub total_seats: u32,
    /// 없으면 총좌석과 같다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_seats: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<RoomEquipmentInput>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shared: bool,
    /// 공용실 외부 일정을 확인한 날 `YYYY-MM-DD`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_checked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEquipmentInput {
    pub code: CodeValue,
    pub owned: u32,
    /// 정상 수량. 없으면 보유 수량과 같다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection_until: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstructorInput {
    pub code: CodeValue,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupInput {
    pub code: CodeValue,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseInput {
    pub code: CodeValue,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

/// 교원 참조: 코드가 있으면 코드로, 없으면 이름으로 찾는다. 이름이 같은 교원이 여럿이면 확인 필요.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstructorRefInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupLinkInput {
    pub group: CodeValue,
    /// 그 수강 집단의 필수과목인지
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentNeedInput {
    pub code: CodeValue,
    /// 1인당 장비면 수강생 한 명당 수량, 강의실당 장비면 강의실에 필요한 수량
    pub quantity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionInput {
    /// 학수번호
    pub course: CodeValue,
    /// 분반 코드
    pub section: CodeValue,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instructors: Vec<InstructorRefInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GroupLinkInput>,
    pub enrollment: u32,
    /// 인원이 잠정치인지
    #[serde(default, skip_serializing_if = "is_false")]
    pub provisional: bool,
    /// `lecture`, `lab`, `mixed`
    pub kind: String,
    /// 필요한 강의실 유형 (`lecture`, `lab`, `seminar`, `other`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_kind: Option<String>,
    /// 주당 시수 (교시 수)
    pub weekly_periods: u8,
    /// 배치 단위별 연속 교시 수. 없으면 주당 시수를 한 번에 배치한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<EquipmentNeedInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

/// 분반 참조
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionRefInput {
    pub course: CodeValue,
    pub section: CodeValue,
}

/// 강의실 참조: 건물과 호실을 따로 주거나, 원본 표기(`공학관 302호`, `201호`)를 그대로 준다.
/// 건물 없는 표기는 추측해서 연결하지 않는다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomRefInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub building: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<CodeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// 조건 대상. 네 항목 중 하나만 쓴다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructor: Option<InstructorRefInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<SectionRefInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<CodeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomRefInput>,
}

/// 시간 범위. 교시(`first_period`~`last_period`, 양 끝 포함)나
/// 시각(`start`~`end`, 시작 포함·종료 제외) 중 하나로 적는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowInput {
    pub day: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_period: Option<CodeValue>,
    /// 없으면 `first_period`와 같다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_period: Option<CodeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    /// 지금은 `avoid_times`(이 시간에 배치하지 않음)만 있다
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<WindowInput>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintInput {
    /// 문서 안에서 조건을 구분하는 키 (선택)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub target: TargetInput,
    /// 해석하지 못한 요청은 비워 두고 `status`를 `pending`으로 한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<RuleInput>,
    /// `hard` 또는 `soft`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<String>,
    /// 선호 조건의 상대 중요도 1~10 (기본 1)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u8>,
    /// `confirmed` 또는 `pending`
    pub status: String,
    /// 적용 기간 `YYYY-MM-DD`. 둘 다 없으면 학기 전체
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// 요청 원문
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_by: Option<String>,
    /// 내부 메모 (출력·인계에 넣지 않음)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementInput {
    pub course: CodeValue,
    pub section: CodeValue,
    /// 분반의 몇 번째 배치 단위인지 (1부터). 없으면 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u8>,
    pub day: String,
    /// 시작 교시 이름
    pub start_period: CodeValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomRefInput>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

/// 변환하지 못한 원본 행 (예: 건물 없이 적힌 호실, 요청이 아닌 지시문)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnresolvedInput {
    /// 무엇의 행인지 (`room`, `section`, `constraint`, `other` 등)
    pub what: String,
    /// 원본 내용
    pub text: String,
    /// 변환하지 못한 이유
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_value_accepts_strings_and_numbers() {
        let v: Vec<CodeValue> = serde_json::from_str(r#"["01", 1, 2.0, 302]"#).unwrap();
        assert_eq!(v[0], CodeValue::text("01"));
        assert_eq!(v[1], CodeValue::number(1));
        assert_eq!(v[2], CodeValue::number(2));
        assert_eq!(v[3].text, "302");
        assert_eq!(serde_json::to_string(&v).unwrap(), r#"["01",1,2,302]"#);
        assert!(serde_json::from_str::<CodeValue>("true").is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = r#"{"schema":"auler.input.v1","term":{"year":2027,"season":"1"},"memo":"x"}"#;
        let err = InputDocument::from_json(text).unwrap_err();
        assert!(err.to_string().contains("memo"), "{err}");
    }

    #[test]
    fn minimal_document_round_trips() {
        let doc = InputDocument::new(2027, "1");
        let json = doc.to_json_pretty();
        assert_eq!(InputDocument::from_json(&json).unwrap(), doc);
        assert!(!json.contains("sections"), "빈 목록은 쓰지 않는다");
    }
}
