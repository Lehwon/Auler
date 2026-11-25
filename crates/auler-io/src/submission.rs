//! Configured school submission CSV from an immutable snapshot.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use auler_domain::time::Weekday;
use encoding_rs::EUC_KR;
use serde::{Deserialize, Deserializer, de};

use crate::export::{Language, Meeting, Snapshot, Table, View};

pub const EXAMPLE: &str = include_str!("../../../templates/school-submission-example.json");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    #[default]
    #[serde(rename = "utf-8", alias = "utf8")]
    Utf8,
    Cp949,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Cp949 => "CP949",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    format_version: u32,
    name: String,
    encoding: Encoding,
    include_headers: bool,
    #[serde(default, deserialize_with = "unique_codes")]
    day_codes: BTreeMap<Weekday, String>,
    #[serde(default, deserialize_with = "unique_codes")]
    period_codes: BTreeMap<u16, String>,
    columns: Vec<Column>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Column {
    field: Field,
    header: String,
}

fn unique_codes<'de, Key, Input>(input: Input) -> Result<BTreeMap<Key, String>, Input::Error>
where
    Key: Deserialize<'de> + Ord + fmt::Display,
    Input: Deserializer<'de>,
{
    struct CodeMap<Key>(PhantomData<Key>);

    impl<'de, Key: Deserialize<'de> + Ord + fmt::Display> de::Visitor<'de> for CodeMap<Key> {
        type Value = BTreeMap<Key, String>;

        fn expecting(&self, format: &mut fmt::Formatter<'_>) -> fmt::Result {
            format.write_str("a map of non-repeated keys to code strings")
        }

        fn visit_map<Access: de::MapAccess<'de>>(
            self,
            mut input: Access,
        ) -> Result<Self::Value, Access::Error> {
            let mut codes = BTreeMap::new();
            while let Some((key, value)) = input.next_entry::<Key, String>()? {
                match codes.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(de::Error::custom(format_args!(
                            "code key {} is repeated",
                            entry.key()
                        )));
                    }
                }
            }
            Ok(codes)
        }
    }

    input.deserialize_map(CodeMap(PhantomData))
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Field {
    CourseCode,
    CourseName,
    SectionCode,
    MeetingNumber,
    DayCode,
    FirstPeriodCode,
    LastPeriodCode,
    StartTime,
    EndTime,
    Building,
    RoomNumber,
    Room,
    InstructorNames,
}

#[derive(Debug)]
pub enum Error {
    InvalidJson(serde_json::Error),
    Version(u32),
    MissingName,
    NoColumns,
    BlankHeader(usize),
    MissingDay(Weekday),
    MissingPeriod(u16),
    Unrepresentable { row: usize, column: usize },
    Csv(String),
}

impl Error {
    pub fn message(&self, language: Language) -> String {
        match self {
            Self::InvalidJson(error) => language.choose(format!("학교 양식 JSON을 읽을 수 없습니다. {error}"), format!("The school profile JSON could not be read. {error}")),
            Self::Version(version) => language.choose(format!("학교 양식 버전 {version}을 지원하지 않습니다."), format!("School profile version {version} is unsupported.")),
            Self::MissingName => language.choose("양식 이름을 입력하세요.", "Enter a profile name.").into(),
            Self::NoColumns => language.choose("출력할 열을 한 개 이상 지정하세요.", "Specify at least one output column.").into(),
            Self::BlankHeader(column) => language.choose(format!("{column}번 열의 머리글을 입력하세요."), format!("Enter a header for column {column}.")),
            Self::MissingDay(day) => language.choose(format!("{}요일의 제출 코드를 입력하세요.", day.ko()), format!("Enter a submission code for {}.", language.day(*day))),
            Self::MissingPeriod(period) => language.choose(format!("{period}번째 교시의 제출 코드를 입력하세요."), format!("Enter a submission code for period {period}.")),
            Self::Unrepresentable { row, column } => language.choose(format!("파일의 {row}행 {column}열에 CP949로 저장할 수 없는 글자가 있습니다. 자료를 확인하거나 UTF-8 양식을 사용하세요."), format!("File row {row}, column {column} contains a character unavailable in CP949. Check the data or use a UTF-8 profile.")),
            Self::Csv(error) => language.choose(format!("제출 CSV를 만들 수 없습니다. {error}"), format!("Could not create the submission CSV. {error}")),
        }
    }
}

pub struct Submission {
    pub table: Table,
    pub bytes: Vec<u8>,
}

impl Default for Profile {
    fn default() -> Self {
        Self::from_json(EXAMPLE.as_bytes()).expect("the included school profile is valid")
    }
}

impl Profile {
    pub fn from_json(json: &[u8]) -> Result<Self, Error> {
        let profile: Self = serde_json::from_slice(json).map_err(Error::InvalidJson)?;
        if profile.format_version != 1 {
            return Err(Error::Version(profile.format_version));
        }
        if profile.name.trim().is_empty() {
            return Err(Error::MissingName);
        }
        if profile.columns.is_empty() {
            return Err(Error::NoColumns);
        }
        if let Some(index) = profile
            .columns
            .iter()
            .position(|column| column.header.trim().is_empty())
        {
            return Err(Error::BlankHeader(index + 1));
        }
        Ok(profile)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn include_headers(&self) -> bool {
        self.include_headers
    }

    pub fn prepare(&self, snapshot: &Snapshot) -> Result<Submission, Error> {
        let headings: Vec<_> = self
            .columns
            .iter()
            .map(|column| column.header.clone())
            .collect();
        let rows = snapshot
            .meetings(View::All)
            .into_iter()
            .map(|meeting| {
                self.columns
                    .iter()
                    .map(|column| self.value(column.field, snapshot, meeting))
                    .collect()
            })
            .collect::<Result<Vec<Vec<String>>, _>>()?;
        let records = self
            .include_headers
            .then_some(&headings)
            .into_iter()
            .chain(&rows);
        let mut csv = csv::WriterBuilder::new()
            .terminator(csv::Terminator::CRLF)
            .from_writer(Vec::new());
        for record in records.clone() {
            csv.write_record(record)
                .map_err(|error| Error::Csv(error.to_string()))?;
        }
        let utf8 = csv
            .into_inner()
            .map_err(|error| Error::Csv(error.to_string()))?;
        let bytes = match self.encoding {
            Encoding::Utf8 => utf8,
            Encoding::Cp949 => {
                let text =
                    String::from_utf8(utf8).map_err(|error| Error::Csv(error.to_string()))?;
                let (encoded, _, invalid) = EUC_KR.encode(&text);
                if invalid {
                    for (row, record) in records.enumerate() {
                        if let Some(column) = record.iter().position(|value| EUC_KR.encode(value).2)
                        {
                            return Err(Error::Unrepresentable {
                                row: row + 1,
                                column: column + 1,
                            });
                        }
                    }
                    return Err(Error::Csv("CP949".into()));
                }
                encoded.into_owned()
            }
        };
        Ok(Submission {
            table: Table { headings, rows },
            bytes,
        })
    }

    fn value(
        &self,
        field: Field,
        snapshot: &Snapshot,
        meeting: Meeting<'_>,
    ) -> Result<String, Error> {
        let first = u16::from(meeting.placement.start) + 1;
        let period = |number| {
            self.period_codes
                .get(&number)
                .filter(|code| !code.trim().is_empty())
                .cloned()
                .ok_or(Error::MissingPeriod(number))
        };
        Ok(match field {
            Field::CourseCode => meeting.course.code.to_string(),
            Field::CourseName => meeting.course.name.clone(),
            Field::SectionCode => meeting.section.code.to_string(),
            Field::MeetingNumber => (u16::from(meeting.placement.meeting.part) + 1).to_string(),
            Field::DayCode => self
                .day_codes
                .get(&meeting.placement.day)
                .filter(|code| !code.trim().is_empty())
                .cloned()
                .ok_or(Error::MissingDay(meeting.placement.day))?,
            Field::FirstPeriodCode => period(first)?,
            Field::LastPeriodCode => period(first + u16::from(meeting.span) - 1)?,
            Field::StartTime => meeting.first.start.to_string(),
            Field::EndTime => meeting.last.end.to_string(),
            Field::Building => meeting.room.key.building.clone(),
            Field::RoomNumber => meeting.room.key.number.clone(),
            Field::Room => meeting.room.key.to_string(),
            Field::InstructorNames => snapshot.instructors(meeting),
        })
    }
}
