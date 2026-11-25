//! 기준자료, 학기 배치, 편성 조건.
//!
//! 저장된 기록은 내부 ID로 서로를 가리킨다. 학수번호나 교원 이름이 바뀌어도 연결이
//! 끊기지 않게 하기 위해서다. 원본 자료(과목명, 교원명, 강의실, 조건 원문)는 번역하지 않는다.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::code::{CourseCode, EquipmentCode, GroupCode, InstructorCode, RoomKey, SectionCode};
use crate::time::{ClockTime, Date, PeriodTable, Weekday};

macro_rules! ids {
    ($($(#[$meta:meta])* $name:ident),* $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub i64);
        )*
    };
}

ids!(
    CourseId,
    SectionId,
    InstructorId,
    GroupId,
    RoomId,
    EquipmentId,
    ConstraintId,
);

/// 학기 구분
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TermSeason {
    #[serde(rename = "1")]
    First,
    #[serde(rename = "summer")]
    Summer,
    #[serde(rename = "2")]
    Second,
    #[serde(rename = "winter")]
    Winter,
}

impl TermSeason {
    pub const ALL: [TermSeason; 4] = [Self::First, Self::Summer, Self::Second, Self::Winter];

    /// 입력 규격의 표기 (`1`, `summer`, `2`, `winter`)
    pub fn code(self) -> &'static str {
        match self {
            Self::First => "1",
            Self::Summer => "summer",
            Self::Second => "2",
            Self::Winter => "winter",
        }
    }

    /// `1`, `1학기`, `summer`, `여름`, `여름학기` 등을 읽는다.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        let t = t.strip_suffix("학기").unwrap_or(&t).trim();
        match t {
            "1" => Some(Self::First),
            "2" => Some(Self::Second),
            "summer" | "여름" | "하계" => Some(Self::Summer),
            "winter" | "겨울" | "동계" => Some(Self::Winter),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TermKey {
    pub year: u16,
    pub season: TermSeason,
}

impl fmt::Display for TermKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.year, self.season.code())
    }
}

/// 필수 제약과 선호 제약
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    /// 어기면 확정할 수 없다
    Hard,
    /// 어기면 경고. 조교가 알고 적용할 수 있다
    Soft,
}

/// 학교마다 다른 편성 규칙. 추측해서 고정하지 않고 학기 설정으로 둔다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchoolRules {
    /// 같은 수강 집단의 필수과목끼리 겹칠 때의 강도
    pub required_course_overlap: Strength,
    /// 연강이 등록된 휴게시간을 넘어갈 수 있는지
    pub block_may_cross_break: bool,
    /// 같은 강의실에서 앞 수업 종료와 다음 수업 시작 사이에 필요한 준비·정리 시간(분)
    pub room_turnaround_minutes: u16,
}

impl Default for SchoolRules {
    fn default() -> Self {
        Self {
            required_course_overlap: Strength::Hard,
            block_may_cross_break: false,
            room_turnaround_minutes: 0,
        }
    }
}

/// 학기 설정: 기간, 수업 요일, 교시표, 학교 규칙
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermSetup {
    pub key: TermKey,
    pub start: Option<Date>,
    pub end: Option<Date>,
    pub days: Vec<Weekday>,
    pub periods: PeriodTable,
    pub rules: SchoolRules,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Course {
    pub id: CourseId,
    pub code: CourseCode,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instructor {
    pub id: InstructorId,
    pub code: InstructorCode,
    pub name: String,
    pub department: Option<String>,
}

/// 수강 집단: 같은 시간에 수업을 들어야 하는 학생 묶음 (학년, 트랙 등)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudentGroup {
    pub id: GroupId,
    pub code: GroupCode,
    pub name: String,
    pub grade: Option<u8>,
    pub track: Option<String>,
}

/// 장비 필요량을 세는 기준
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentScope {
    /// 수강생 한 명마다 필요하다 (실습용 PC 등)
    PerPerson,
    /// 강의실에 있으면 된다 (빔프로젝터 등)
    PerRoom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentKind {
    pub id: EquipmentId,
    pub code: EquipmentCode,
    pub name: String,
    pub scope: EquipmentScope,
}

/// 날짜 구간. 양 끝 날짜를 포함한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub from: Date,
    pub until: Date,
}

/// 강의실이 가진 장비. `available`은 고장 등을 뺀 정상 수량이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomEquipment {
    pub equipment: EquipmentId,
    pub owned: u32,
    pub available: u32,
    /// 점검 기간. 이 기간에는 장비를 쓸 수 없다
    pub inspection: Option<DateRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomKind {
    Lecture,
    Lab,
    Seminar,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub id: RoomId,
    pub key: RoomKey,
    pub kind: RoomKind,
    /// 관리 부서
    pub department: Option<String>,
    pub total_seats: u32,
    /// 고정 장비 등을 빼고 실제로 앉을 수 있는 좌석
    pub available_seats: u32,
    pub equipment: Vec<RoomEquipment>,
    /// 다른 학과와 함께 쓰는 공용실
    pub shared: bool,
    /// 공용실 외부 일정을 마지막으로 확인한 날. 없으면 미확인이고, 빈 방으로 보지 않는다
    pub schedule_checked: Option<Date>,
    /// 강의실 정보의 출처 (예: 시설팀 2027-02 목록)
    pub info_source: Option<String>,
}

impl Room {
    /// 공용실인데 외부 일정을 확인하지 않았으면 배정 전에 관리 부서 확인이 필요하다.
    pub fn needs_external_check(&self) -> bool {
        self.shared && self.schedule_checked.is_none()
    }
}

/// 수업 형태
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassKind {
    Lecture,
    Lab,
    Mixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrollment {
    pub count: u32,
    /// 아직 확정되지 않은 예상 인원
    pub provisional: bool,
}

/// 분반을 듣는 수강 집단. `required`면 그 집단의 필수과목이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupLink {
    pub group: GroupId,
    pub required: bool,
}

/// 분반에 필요한 장비. 1인당 장비면 수강생 한 명당 수량이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentNeed {
    pub equipment: EquipmentId,
    pub quantity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub course: CourseId,
    pub code: SectionCode,
    pub instructors: Vec<InstructorId>,
    pub groups: Vec<GroupLink>,
    pub enrollment: Enrollment,
    pub kind: ClassKind,
    /// 필요한 강의실 유형. 없으면 유형을 가리지 않는다
    pub room_kind: Option<RoomKind>,
    /// 배치 단위별 연속 교시 수. 주당 3시간을 2시간과 1시간으로 나누면 `[2, 1]`
    pub split: Vec<u8>,
    pub equipment: Vec<EquipmentNeed>,
}

impl Section {
    /// 주당 시수 (분할한 교시 수의 합)
    pub fn weekly_periods(&self) -> u32 {
        self.split.iter().map(|&n| n as u32).sum()
    }

    pub fn meetings(&self) -> impl Iterator<Item = MeetingRef> + '_ {
        (0..self.split.len()).map(|part| MeetingRef {
            section: self.id,
            part: part as u8,
        })
    }
}

/// 배치 단위: 분반의 `part`번째 수업 (연강은 한 단위, 분할 수업은 단위마다 따로)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MeetingRef {
    pub section: SectionId,
    pub part: u8,
}

/// 학기 배치. `start`는 교시표의 순번(0부터)이고 길이는 분반의 분할에서 온다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub meeting: MeetingRef,
    pub day: Weekday,
    pub start: u8,
    pub room: Option<RoomId>,
    pub locked: bool,
}

/// 편성 조건의 대상
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Instructor(InstructorId),
    Section(SectionId),
    Group(GroupId),
    Room(RoomId),
}

/// 시간 범위. 교시로 적거나(교시표 순번, 양 끝 포함) 시각으로 적는다(시작 포함, 종료 제외).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub enum TimeWindow {
    Periods {
        day: Weekday,
        first: u8,
        last: u8,
    },
    Clock {
        day: Weekday,
        start: ClockTime,
        end: ClockTime,
    },
}

impl TimeWindow {
    pub fn day(&self) -> Weekday {
        match *self {
            Self::Periods { day, .. } | Self::Clock { day, .. } => day,
        }
    }

    /// 이 범위가 덮는 교시 순번들
    pub fn period_range(&self, table: &PeriodTable) -> std::ops::Range<usize> {
        match *self {
            Self::Periods { first, last, .. } => {
                let end = (last as usize + 1).min(table.len());
                (first as usize).min(end)..end
            }
            Self::Clock { start, end, .. } => table.overlapping(start, end),
        }
    }
}

/// 조건의 내용
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    /// 이 시간에는 배치하지 않는다. 필수면 불가 시간, 선호면 가능하면 피할 시간이다.
    AvoidTimes { windows: Vec<TimeWindow> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConstraintStatus {
    /// 조교가 확인했다. 편성에 적용한다
    Confirmed,
    /// 대상·시간·강도가 확인되지 않았다. 적용하지 않는다
    Pending,
}

/// 원본 위치: 어느 파일의 어느 시트·행·열에서 왔는지
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// 1부터 세는 행 번호
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<u32>,
    /// 열 이름 (`C`) 또는 머리글
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: ConstraintId,
    pub target: Target,
    /// 해석이 끝나지 않은 요청은 내용이 없다
    pub rule: Option<Rule>,
    pub strength: Option<Strength>,
    /// 선호 조건끼리의 상대 중요도 (1 이상)
    pub weight: u8,
    pub status: ConstraintStatus,
    /// 적용 기간. 없으면 학기 전체
    pub period: Option<DateRange>,
    /// 접수한 요청 원문
    pub text: Option<String>,
    pub source: Option<SourceRef>,
    pub confirmed_by: Option<String>,
    /// 내부 메모. 출력과 인계에 기본으로 넣지 않는다
    pub note: Option<String>,
}

impl Constraint {
    /// 편성에 적용하는 조건인지: 확인됐고 내용과 강도가 있어야 한다
    pub fn is_active(&self) -> bool {
        self.status == ConstraintStatus::Confirmed && self.rule.is_some() && self.strength.is_some()
    }
}

/// 한 학기의 편성에 필요한 자료 전체. 저장소가 채우고 편성 엔진이 읽는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermData {
    pub setup: TermSetup,
    pub courses: Vec<Course>,
    pub instructors: Vec<Instructor>,
    pub groups: Vec<StudentGroup>,
    pub equipment: Vec<EquipmentKind>,
    pub rooms: Vec<Room>,
    pub sections: Vec<Section>,
    pub constraints: Vec<Constraint>,
    pub placements: Vec<Placement>,
}

impl TermData {
    pub fn course(&self, id: CourseId) -> Option<&Course> {
        self.courses.iter().find(|c| c.id == id)
    }

    pub fn section(&self, id: SectionId) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }

    pub fn instructor(&self, id: InstructorId) -> Option<&Instructor> {
        self.instructors.iter().find(|i| i.id == id)
    }

    pub fn group(&self, id: GroupId) -> Option<&StudentGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn room(&self, id: RoomId) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    pub fn equipment_kind(&self, id: EquipmentId) -> Option<&EquipmentKind> {
        self.equipment.iter().find(|e| e.id == id)
    }

    /// 학수번호와 분반 코드로 분반을 찾는다
    pub fn find_section(&self, course: &CourseCode, code: &SectionCode) -> Option<&Section> {
        let course = self.courses.iter().find(|c| &c.code == course)?;
        self.sections
            .iter()
            .find(|s| s.course == course.id && &s.code == code)
    }

    pub fn find_room(&self, key: &RoomKey) -> Option<&Room> {
        self.rooms.iter().find(|r| &r.key == key)
    }

    /// 배치 단위의 연속 교시 수
    pub fn meeting_span(&self, meeting: MeetingRef) -> Option<u8> {
        self.section(meeting.section)?
            .split
            .get(meeting.part as usize)
            .copied()
    }

    /// 모든 배치 단위 (분반 순서, 단위 순서)
    pub fn meetings(&self) -> impl Iterator<Item = MeetingRef> + '_ {
        self.sections.iter().flat_map(Section::meetings)
    }

    pub fn placement(&self, meeting: MeetingRef) -> Option<&Placement> {
        self.placements.iter().find(|p| p.meeting == meeting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_season_reads_common_forms() {
        assert_eq!(TermSeason::parse("1학기"), Some(TermSeason::First));
        assert_eq!(TermSeason::parse(" 2 "), Some(TermSeason::Second));
        assert_eq!(TermSeason::parse("여름학기"), Some(TermSeason::Summer));
        assert_eq!(TermSeason::parse("Winter"), Some(TermSeason::Winter));
        assert_eq!(TermSeason::parse("3"), None);
        assert_eq!(serde_json::to_string(&TermSeason::First).unwrap(), "\"1\"");
    }

    #[test]
    fn time_window_maps_to_periods() {
        let table = PeriodTable::uniform(ClockTime::from_hm(9, 0).unwrap(), 8, 50, 10);
        let by_period = TimeWindow::Periods {
            day: Weekday::Thu,
            first: 0,
            last: 0,
        };
        assert_eq!(by_period.period_range(&table), 0..1);
        let past_end = TimeWindow::Periods {
            day: Weekday::Thu,
            first: 6,
            last: 12,
        };
        assert_eq!(past_end.period_range(&table), 6..8);
        let by_clock = TimeWindow::Clock {
            day: Weekday::Wed,
            start: ClockTime::from_hm(13, 0).unwrap(),
            end: ClockTime::from_hm(16, 0).unwrap(),
        };
        assert_eq!(by_clock.period_range(&table), 4..7);
        let json = serde_json::to_string(&by_clock).unwrap();
        assert_eq!(
            json,
            r#"{"by":"clock","day":"wed","start":"13:00","end":"16:00"}"#
        );
    }

    #[test]
    fn only_confirmed_complete_constraints_apply() {
        let mut c = Constraint {
            id: ConstraintId(1),
            target: Target::Instructor(InstructorId(1)),
            rule: None,
            strength: None,
            weight: 1,
            status: ConstraintStatus::Pending,
            period: None,
            text: Some("오전에 수업하기 어렵습니다.".into()),
            source: None,
            confirmed_by: None,
            note: None,
        };
        assert!(!c.is_active());
        c.status = ConstraintStatus::Confirmed;
        assert!(!c.is_active(), "내용 없이 확인만 된 조건은 적용하지 않는다");
        c.rule = Some(Rule::AvoidTimes { windows: vec![] });
        c.strength = Some(Strength::Soft);
        assert!(c.is_active());
    }
}
