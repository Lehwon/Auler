//! 요일, 시각, 날짜, 교시표.
//!
//! 교시표는 학교마다 다르다. 교시 길이가 일정하다고 가정하지 않고, 교시마다 시작·종료
//! 시각을 둔다. 휴게시간(점심 등)은 교시 사이의 빈 시간 중 이름을 붙여 따로 등록한 것이다.

use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// 요일. 직렬화하면 `mon`…`sun`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub const ALL: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];

    /// 월요일이 0
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// 입력 규격의 요일 코드 (`mon`…`sun`)
    pub fn code(self) -> &'static str {
        ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][self.index()]
    }

    /// 한국어 한 글자 이름 (`월`…`일`). 표준 양식과 원본 자료를 읽을 때 쓴다.
    pub fn ko(self) -> &'static str {
        ["월", "화", "수", "목", "금", "토", "일"][self.index()]
    }

    /// `mon`, `Monday`, `월`, `월요일`을 읽는다. 앞뒤 공백과 대소문자는 무시한다.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        let t = t.as_str();
        Self::ALL.into_iter().find(|d| {
            let code = d.code();
            let en_full = [
                "monday",
                "tuesday",
                "wednesday",
                "thursday",
                "friday",
                "saturday",
                "sunday",
            ][d.index()];
            t == code || t == en_full || t == d.ko() || t.strip_suffix("요일") == Some(d.ko())
        })
    }
}

impl fmt::Display for Weekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// 하루 안의 시각(분 단위). `24:00`은 하루의 끝을 나타낼 때만 쓴다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockTime(u16);

impl ClockTime {
    pub const MIDNIGHT_END: u16 = 24 * 60;

    pub const fn from_minutes(minutes: u16) -> Option<Self> {
        if minutes <= Self::MIDNIGHT_END {
            Some(Self(minutes))
        } else {
            None
        }
    }

    pub const fn from_hm(hour: u8, minute: u8) -> Option<Self> {
        if minute >= 60 {
            return None;
        }
        Self::from_minutes(hour as u16 * 60 + minute as u16)
    }

    /// 00:00부터 지난 분
    pub fn minutes(self) -> u16 {
        self.0
    }

    /// `9:00`, `09:00`을 읽는다.
    pub fn parse(text: &str) -> Option<Self> {
        let (h, m) = text.trim().split_once(':')?;
        if h.is_empty() || h.len() > 2 || m.len() != 2 {
            return None;
        }
        let h: u8 = h.parse().ok()?;
        let m: u8 = m.parse().ok()?;
        Self::from_hm(h, m)
    }
}

impl fmt::Display for ClockTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.0 / 60, self.0 % 60)
    }
}

impl Serialize for ClockTime {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClockTime {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| de::Error::custom(format!("시각 형식이 아님: {text}")))
    }
}

/// 달력 날짜 (그레고리력)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (year >= 1 && (1..=days).contains(&day)).then_some(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// `2027-03-02`를 읽는다.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('-');
        let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return None;
        }
        Self::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| de::Error::custom(format!("날짜 형식이 아님: {text}")))
    }
}

/// 하나의 교시. `label`은 학교가 부르는 이름(`1`, `0`, `1A` 등)이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Period {
    pub label: String,
    pub start: ClockTime,
    pub end: ClockTime,
}

/// 이름 붙은 휴게시간 (점심 등). 교시 사이의 빈 시간 안에 있어야 한다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Break {
    pub label: String,
    pub start: ClockTime,
    pub end: ClockTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeriodTableError {
    /// 교시가 하나도 없다
    Empty,
    /// 교시 이름이 비었다 (교시 순번)
    EmptyLabel(usize),
    /// 같은 이름의 교시가 둘 이상이다
    DuplicateLabel(String),
    /// 시작이 종료보다 늦거나 같다 (교시 순번)
    EmptyRange(usize),
    /// 앞 교시와 겹치거나 순서가 거꾸로다 (교시 순번)
    Overlap(usize),
    /// 휴게시간이 교시와 겹치거나 시간 범위가 잘못됐다 (휴게 순번)
    BadBreak(usize),
}

impl fmt::Display for PeriodTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "교시가 없음"),
            Self::EmptyLabel(i) => write!(f, "{}번째 교시의 이름이 비었음", i + 1),
            Self::DuplicateLabel(l) => write!(f, "교시 이름 중복: {l}"),
            Self::EmptyRange(i) => write!(f, "{}번째 교시의 시작이 종료보다 늦음", i + 1),
            Self::Overlap(i) => write!(f, "{}번째 교시가 앞 교시와 겹침", i + 1),
            Self::BadBreak(i) => write!(f, "{}번째 휴게시간이 교시와 겹침", i + 1),
        }
    }
}

impl std::error::Error for PeriodTableError {}

/// 검사를 마친 교시표. 교시는 시각 순서로 겹치지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PeriodTableParts", into = "PeriodTableParts")]
pub struct PeriodTable {
    periods: Vec<Period>,
    breaks: Vec<Break>,
}

#[derive(Serialize, Deserialize)]
struct PeriodTableParts {
    periods: Vec<Period>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    breaks: Vec<Break>,
}

impl TryFrom<PeriodTableParts> for PeriodTable {
    type Error = PeriodTableError;

    fn try_from(p: PeriodTableParts) -> Result<Self, Self::Error> {
        Self::new(p.periods, p.breaks)
    }
}

impl From<PeriodTable> for PeriodTableParts {
    fn from(t: PeriodTable) -> Self {
        Self {
            periods: t.periods,
            breaks: t.breaks,
        }
    }
}

impl PeriodTable {
    pub fn new(periods: Vec<Period>, mut breaks: Vec<Break>) -> Result<Self, PeriodTableError> {
        if periods.is_empty() {
            return Err(PeriodTableError::Empty);
        }
        for (i, p) in periods.iter().enumerate() {
            if p.label.trim().is_empty() {
                return Err(PeriodTableError::EmptyLabel(i));
            }
            if p.start >= p.end {
                return Err(PeriodTableError::EmptyRange(i));
            }
            if i > 0 && periods[i - 1].end > p.start {
                return Err(PeriodTableError::Overlap(i));
            }
            if periods[..i].iter().any(|q| q.label == p.label) {
                return Err(PeriodTableError::DuplicateLabel(p.label.clone()));
            }
        }
        breaks.sort_by_key(|b| b.start);
        for (i, b) in breaks.iter().enumerate() {
            let inside_gap = periods
                .windows(2)
                .any(|w| w[0].end <= b.start && b.end <= w[1].start);
            if b.start >= b.end || !inside_gap {
                return Err(PeriodTableError::BadBreak(i));
            }
        }
        Ok(Self { periods, breaks })
    }

    /// `first`에서 시작해 `minutes`분 수업과 `passing`분 쉬는 시간을 `count`번 되풀이한다.
    pub fn uniform(first: ClockTime, count: usize, minutes: u16, passing: u16) -> Self {
        let periods = (0..count)
            .map(|i| {
                let start = first.minutes() + i as u16 * (minutes + passing);
                Period {
                    label: (i + 1).to_string(),
                    start: ClockTime(start),
                    end: ClockTime(start + minutes),
                }
            })
            .collect();
        Self::new(periods, Vec::new()).expect("균일한 교시표는 항상 유효하다")
    }

    pub fn periods(&self) -> &[Period] {
        &self.periods
    }

    pub fn breaks(&self) -> &[Break] {
        &self.breaks
    }

    pub fn len(&self) -> usize {
        self.periods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.periods.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Period> {
        self.periods.get(index)
    }

    pub fn index_of(&self, label: &str) -> Option<usize> {
        let label = label.trim();
        self.periods.iter().position(|p| p.label == label)
    }

    /// `[start, end)`와 겹치는 교시들. 시작은 포함하고 종료는 제외한다.
    pub fn overlapping(&self, start: ClockTime, end: ClockTime) -> Range<usize> {
        let hits = |p: &Period| p.start < end && start < p.end;
        let Some(first) = self.periods.iter().position(hits) else {
            return 0..0;
        };
        let count = self.periods[first..].iter().take_while(|p| hits(p)).count();
        first..first + count
    }

    /// `first`부터 `count`개 교시가 표 안에 있는지
    pub fn contains_span(&self, first: usize, count: usize) -> bool {
        count > 0 && first + count <= self.periods.len()
    }

    /// 연속 교시 범위의 시작과 종료 시각
    pub fn span_time(&self, first: usize, count: usize) -> Option<(ClockTime, ClockTime)> {
        self.contains_span(first, count).then(|| {
            (
                self.periods[first].start,
                self.periods[first + count - 1].end,
            )
        })
    }

    /// `first`부터 `count`개 교시 사이에 등록된 휴게시간이 끼어 있는지
    pub fn crosses_break(&self, first: usize, count: usize) -> bool {
        let Some((start, end)) = self.span_time(first, count) else {
            return false;
        };
        self.breaks.iter().any(|b| start < b.end && b.start < end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> ClockTime {
        ClockTime::parse(text).unwrap()
    }

    #[test]
    fn weekday_reads_korean_and_english() {
        assert_eq!(Weekday::parse("수"), Some(Weekday::Wed));
        assert_eq!(Weekday::parse(" 목요일 "), Some(Weekday::Thu));
        assert_eq!(Weekday::parse("FRI"), Some(Weekday::Fri));
        assert_eq!(Weekday::parse("Monday"), Some(Weekday::Mon));
        assert_eq!(Weekday::parse("월욜"), None);
        assert_eq!(serde_json::to_string(&Weekday::Sat).unwrap(), "\"sat\"");
    }

    #[test]
    fn clock_and_date_round_trip() {
        assert_eq!(t("9:05").to_string(), "09:05");
        assert_eq!(ClockTime::parse("24:00").map(|c| c.minutes()), Some(1440));
        assert!(ClockTime::parse("24:01").is_none());
        assert!(ClockTime::parse("13:5").is_none());
        assert!(ClockTime::parse("1300").is_none());
        assert_eq!(Date::parse("2028-02-29").unwrap().to_string(), "2028-02-29");
        assert!(Date::parse("2027-02-29").is_none());
        assert!(Date::parse("2027-3-2").is_none());
        let json = serde_json::to_string(&Date::new(2027, 3, 2).unwrap()).unwrap();
        assert_eq!(json, "\"2027-03-02\"");
        assert!(serde_json::from_str::<ClockTime>("\"25:00\"").is_err());
    }

    #[test]
    fn overlapping_includes_start_and_excludes_end() {
        let table = PeriodTable::uniform(t("09:00"), 8, 50, 10);
        // 13:00~16:00은 5·6·7교시(13:00, 14:00, 15:00 시작)
        assert_eq!(table.overlapping(t("13:00"), t("16:00")), 4..7);
        // 14:50에 끝나는 범위는 6교시(14:00~14:50)를 포함하고, 14:50에 시작하면 포함하지 않는다
        assert_eq!(table.overlapping(t("14:00"), t("14:50")), 5..6);
        assert_eq!(table.overlapping(t("14:50"), t("15:00")), 0..0);
        assert_eq!(table.overlapping(t("07:00"), t("08:00")), 0..0);
        assert_eq!(table.span_time(5, 1), Some((t("14:00"), t("14:50"))));
        assert_eq!(table.span_time(7, 2), None);
    }

    #[test]
    fn breaks_must_sit_between_periods() {
        let p = |l: &str, s: &str, e: &str| Period {
            label: l.into(),
            start: t(s),
            end: t(e),
        };
        let periods = vec![
            p("1", "09:00", "10:15"),
            p("2", "10:30", "11:45"),
            p("3", "13:00", "14:15"),
        ];
        let lunch = Break {
            label: "점심".into(),
            start: t("12:00"),
            end: t("13:00"),
        };
        let table = PeriodTable::new(periods.clone(), vec![lunch.clone()]).unwrap();
        assert!(table.crosses_break(1, 2));
        assert!(!table.crosses_break(0, 2));

        let bad = Break {
            start: t("11:30"),
            ..lunch
        };
        assert_eq!(
            PeriodTable::new(periods.clone(), vec![bad]),
            Err(PeriodTableError::BadBreak(0))
        );
        let mut overlapping = periods.clone();
        overlapping[1].start = t("10:00");
        assert_eq!(
            PeriodTable::new(overlapping, vec![]),
            Err(PeriodTableError::Overlap(1))
        );
        let mut dup = periods;
        dup[2].label = "1".into();
        assert_eq!(
            PeriodTable::new(dup, vec![]),
            Err(PeriodTableError::DuplicateLabel("1".into()))
        );
    }

    #[test]
    fn period_table_validates_on_deserialize() {
        let ok = r#"{"periods":[{"label":"1","start":"09:00","end":"09:50"}]}"#;
        assert_eq!(serde_json::from_str::<PeriodTable>(ok).unwrap().len(), 1);
        let bad = r#"{"periods":[{"label":"1","start":"10:00","end":"09:50"}]}"#;
        assert!(serde_json::from_str::<PeriodTable>(bad).is_err());
    }
}
