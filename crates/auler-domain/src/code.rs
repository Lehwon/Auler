//! 식별 코드: 학수번호, 분반, 교원·수강 집단·장비 코드, 강의실(건물 + 호실).
//!
//! 코드는 언제나 문자열이다. 분반 `01`의 앞자리 0처럼 숫자로 바꾸면 사라지는 정보를
//! 지키기 위해서다. 앞뒤 공백만 지우고 대소문자나 모양은 바꾸지 않는다.

use std::fmt;

use serde::{Deserialize, Serialize};

/// 코드 길이 상한 (글자 수)
pub const MAX_CODE_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeError {
    Empty,
    TooLong,
    /// 공백이나 제어 문자가 들어 있다
    InvalidChar(char),
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "코드가 비었음"),
            Self::TooLong => write!(f, "코드가 {MAX_CODE_LEN}자보다 김"),
            Self::InvalidChar(c) => write!(f, "코드에 쓸 수 없는 문자 {c:?}"),
        }
    }
}

impl std::error::Error for CodeError {}

/// 코드 규칙: 앞뒤 공백을 지운 뒤 1~32자, 공백·제어 문자 없음.
pub fn normalize_code(text: &str) -> Result<String, CodeError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(CodeError::Empty);
    }
    if t.chars().count() > MAX_CODE_LEN {
        return Err(CodeError::TooLong);
    }
    if let Some(c) = t.chars().find(|c| c.is_whitespace() || c.is_control()) {
        return Err(CodeError::InvalidChar(c));
    }
    Ok(t.to_owned())
}

macro_rules! code_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Result<Self, CodeError> {
                normalize_code(text).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = CodeError;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                Self::new(&text)
            }
        }

        impl From<$name> for String {
            fn from(code: $name) -> String {
                code.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

code_type!(
    /// 학수번호 (과목 코드)
    CourseCode
);
code_type!(
    /// 분반 코드. `01`과 `1`은 다른 분반이다.
    SectionCode
);
code_type!(
    /// 교원 코드. 동명이인을 구분하려고 이름과 따로 둔다.
    InstructorCode
);
code_type!(
    /// 수강 집단 코드 (예: 2학년, 3학년 AI 트랙)
    GroupCode
);
code_type!(
    /// 장비 종류 코드 (예: PC, 빔프로젝터)
    EquipmentCode
);

/// 강의실 식별: 건물과 호실. 호실 끝의 `호`는 떼고 저장한다.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoomKey {
    pub building: String,
    pub number: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomKeyError {
    MissingBuilding,
    MissingNumber,
    InvalidChar(char),
}

impl fmt::Display for RoomKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBuilding => write!(f, "건물이 없음"),
            Self::MissingNumber => write!(f, "호실이 없음"),
            Self::InvalidChar(c) => write!(f, "호실에 쓸 수 없는 문자 {c:?}"),
        }
    }
}

impl std::error::Error for RoomKeyError {}

/// 호실 표기를 정리한다: 앞뒤 공백과 끝의 `호`를 지운다.
pub fn normalize_room_number(text: &str) -> &str {
    let t = text.trim();
    t.strip_suffix('호').map(str::trim_end).unwrap_or(t)
}

impl RoomKey {
    pub fn new(building: &str, number: &str) -> Result<Self, RoomKeyError> {
        let building = building.trim();
        let number = normalize_room_number(number);
        if building.is_empty() {
            return Err(RoomKeyError::MissingBuilding);
        }
        if number.is_empty() {
            return Err(RoomKeyError::MissingNumber);
        }
        if let Some(c) = number.chars().find(|c| c.is_whitespace() || c.is_control()) {
            return Err(RoomKeyError::InvalidChar(c));
        }
        Ok(Self {
            building: building.to_owned(),
            number: number.to_owned(),
        })
    }

    /// 공백과 `호`를 뺀 비교용 표기 (`공학관 302호` → `공학관302`)
    pub fn compact(&self) -> String {
        compact_room_text(&format!("{}{}", self.building, self.number))
    }
}

/// 원본에 적힌 강의실 표기에서 공백과 끝의 `호`를 뺀다. 건물 없이 적힌 `201호`는 `201`이 된다.
pub fn compact_room_text(text: &str) -> String {
    normalize_room_number(text)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

impl fmt::Display for RoomKey {
    /// 원본 자료 표기 그대로 `건물 호실` (번역하지 않는다)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.building, self.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_keep_leading_zeros_and_case() {
        assert_eq!(SectionCode::new(" 01 ").unwrap().as_str(), "01");
        assert_ne!(
            SectionCode::new("01").unwrap(),
            SectionCode::new("1").unwrap()
        );
        assert_eq!(CourseCode::new("cse2010").unwrap().as_str(), "cse2010");
        assert_eq!(SectionCode::new(""), Err(CodeError::Empty));
        assert_eq!(
            CourseCode::new("CSE 2010"),
            Err(CodeError::InvalidChar(' '))
        );
        assert_eq!(
            GroupCode::new(&"가".repeat(MAX_CODE_LEN + 1)),
            Err(CodeError::TooLong)
        );
        let json = serde_json::to_string(&SectionCode::new("01").unwrap()).unwrap();
        assert_eq!(json, "\"01\"");
        assert!(serde_json::from_str::<SectionCode>("\"\"").is_err());
    }

    #[test]
    fn room_key_strips_ho_suffix() {
        let key = RoomKey::new(" 공학관 ", "302호").unwrap();
        assert_eq!(key.number, "302");
        assert_eq!(key.to_string(), "공학관 302");
        assert_eq!(key.compact(), "공학관302");
        assert_eq!(compact_room_text("공학관 302 호"), "공학관302");
        assert_eq!(compact_room_text("201호"), "201");
        assert_eq!(RoomKey::new("", "201"), Err(RoomKeyError::MissingBuilding));
        assert_eq!(RoomKey::new("본관", "호"), Err(RoomKeyError::MissingNumber));
    }
}
