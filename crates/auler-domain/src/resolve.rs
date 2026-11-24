//! 입력 문서를 검사해 내부 모델([`TermData`])로 바꾼다.
//!
//! 모든 행을 끝까지 검사해 문제를 한꺼번에 돌려준다. 문제마다 문서 안의 경로
//! (`sections[3].section`)와 원본 위치(`source`)가 붙는다. 이 단계는 문서 하나만 본다:
//! 참조는 같은 문서 안의 기록으로만 풀고, 이미 저장된 자료와의 대조(신규·갱신 구분)는
//! 검토 초안 단계에서 한다.
//!
//! 추측하지 않는다: 건물 없는 호실, 이름만 있는데 같은 이름이 여럿인 교원, 숫자로 들어온
//! 코드는 "확인 필요"로 남긴다.

use std::collections::{BTreeMap, HashMap};

use crate::code::{
    CodeError, CourseCode, EquipmentCode, GroupCode, InstructorCode, RoomKey, RoomKeyError,
    SectionCode, compact_room_text,
};
use crate::input::{
    CodeValue, ConstraintInput, InputDocument, InstructorRefInput, PeriodInput, RoomRefInput,
    SCHEMA_V1, SetupInput, TargetInput, WindowInput,
};
use crate::model::{
    ClassKind, Constraint, ConstraintId, ConstraintStatus, Course, CourseId, DateRange, Enrollment,
    EquipmentId, EquipmentKind, EquipmentNeed, EquipmentScope, GroupId, GroupLink, Instructor,
    InstructorId, MeetingRef, Placement, Room, RoomEquipment, RoomId, RoomKind, Rule, SchoolRules,
    Section, SectionId, SourceRef, Strength, StudentGroup, Target, TermData, TermKey, TermSeason,
    TermSetup, TimeWindow,
};
use crate::time::{Break, ClockTime, Date, Period, PeriodTable, PeriodTableError, Weekday};

/// 선호 조건 가중치의 범위
pub const WEIGHT_RANGE: std::ops::RangeInclusive<u8> = 1..=10;

/// 문제의 무게
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// 사람이 확인하면 쓸 수 있다 (모호한 호실, 동명이인, 숫자로 온 코드)
    NeedsReview,
    /// 고쳐야 한다
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// `schema`가 이 규격이 아니다
    UnsupportedSchema {
        found: String,
    },
    /// 필요한 값이 없다
    Missing,
    InvalidCode(CodeError),
    /// 코드가 숫자로 들어왔다. 앞자리 0이 사라졌을 수 있다
    NumericCode {
        text: String,
    },
    /// 알 수 없는 값 (요일, 시각, 날짜, 종류 등)
    InvalidValue {
        value: String,
    },
    InvalidRoom(RoomKeyError),
    InvalidPeriodTable(PeriodTableError),
    /// 같은 키가 두 번 나온다
    Duplicate {
        key: String,
    },
    /// 가리키는 기록이 문서에 없다
    UnknownReference {
        key: String,
    },
    /// 건물 없이 적힌 호실 등 하나로 정할 수 없는 강의실. 후보가 하나여도 추측하지 않는다
    AmbiguousRoom {
        text: String,
        candidates: Vec<RoomKey>,
    },
    /// 이름만 있는데 같은 이름의 교원이 여럿이다
    AmbiguousInstructor {
        name: String,
        candidates: Vec<InstructorCode>,
    },
    /// 코드와 이름이 같은 교원을 가리키지 않는다
    InstructorNameMismatch {
        code: String,
        name: String,
    },
    /// 분할 교시 수의 합이 주당 시수와 다르다
    SplitMismatch {
        weekly: u8,
        split_sum: u32,
    },
    /// 분할 단위가 0교시이거나 하루 교시 수보다 길다
    InvalidSplitPart {
        periods: u8,
    },
    /// 가용 수량(좌석, 장비)이 보유 수량보다 많다
    AvailableExceedsTotal {
        available: u32,
        total: u32,
    },
    /// 시작이 끝보다 늦다 (날짜, 시각, 교시)
    ReversedRange,
    /// 시간 범위를 교시와 시각 두 방식으로 함께 적었거나 둘 다 적지 않았다
    AmbiguousWindow,
    /// 조건 대상이 하나가 아니다
    TargetNotSingle,
    /// 확인된 조건인데 내용이나 강도가 없다
    ConfirmedIncomplete,
    /// 배치 단위가 분반의 분할 범위를 넘는다
    PartOutOfRange {
        parts: usize,
    },
    /// 배치가 마지막 교시를 넘어간다
    SpanPastLastPeriod,
    /// 학기 수업 요일이 아닌 날에 배치했다
    DayNotInTerm,
    /// 0이면 안 되는 수량이 0이다
    Zero,
    /// 가중치가 1~10 밖이다
    WeightOutOfRange {
        weight: u8,
    },
    /// 변환하는 쪽이 풀지 못하고 남긴 원본 행
    Unresolved {
        what: String,
        reason: String,
    },
}

impl IssueKind {
    pub fn severity(&self) -> Severity {
        match self {
            Self::NumericCode { .. }
            | Self::AmbiguousRoom { .. }
            | Self::AmbiguousInstructor { .. }
            | Self::InstructorNameMismatch { .. }
            | Self::Unresolved { .. } => Severity::NeedsReview,
            _ => Severity::Error,
        }
    }

    /// 번역 키와 로그에 쓰는 고정 이름
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema { .. } => "unsupported_schema",
            Self::Missing => "missing",
            Self::InvalidCode(_) => "invalid_code",
            Self::NumericCode { .. } => "numeric_code",
            Self::InvalidValue { .. } => "invalid_value",
            Self::InvalidRoom(_) => "invalid_room",
            Self::InvalidPeriodTable(_) => "invalid_period_table",
            Self::Duplicate { .. } => "duplicate",
            Self::UnknownReference { .. } => "unknown_reference",
            Self::AmbiguousRoom { .. } => "ambiguous_room",
            Self::AmbiguousInstructor { .. } => "ambiguous_instructor",
            Self::InstructorNameMismatch { .. } => "instructor_name_mismatch",
            Self::SplitMismatch { .. } => "split_mismatch",
            Self::InvalidSplitPart { .. } => "invalid_split_part",
            Self::AvailableExceedsTotal { .. } => "available_exceeds_total",
            Self::ReversedRange => "reversed_range",
            Self::AmbiguousWindow => "ambiguous_window",
            Self::TargetNotSingle => "target_not_single",
            Self::ConfirmedIncomplete => "confirmed_incomplete",
            Self::PartOutOfRange { .. } => "part_out_of_range",
            Self::SpanPastLastPeriod => "span_past_last_period",
            Self::DayNotInTerm => "day_not_in_term",
            Self::Zero => "zero",
            Self::WeightOutOfRange { .. } => "weight_out_of_range",
            Self::Unresolved { .. } => "unresolved",
        }
    }
}

/// 검사에서 찾은 문제 하나
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// 문서 안의 위치 (`sections[3].section`)
    pub path: String,
    pub kind: IssueKind,
    /// 그 행의 원본 위치
    pub source: Option<SourceRef>,
}

impl Issue {
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} {:?}", self.path, self.kind.code(), self.kind)
    }
}

/// 문서를 검사해 내부 모델로 바꾼다. 문제가 하나라도 있으면 전체 목록을 돌려준다.
/// 내부 ID는 종류마다 1부터 차례로 붙인다.
pub fn resolve(doc: &InputDocument) -> Result<TermData, Vec<Issue>> {
    let mut r = Resolver::default();
    if doc.schema != SCHEMA_V1 {
        r.push(
            "schema",
            IssueKind::UnsupportedSchema {
                found: doc.schema.clone(),
            },
            None,
        );
    }
    let season = TermSeason::parse(&doc.term.season);
    if season.is_none() {
        r.invalid("term.season", &doc.term.season, None);
    }
    let key = season.map(|season| TermKey {
        year: doc.term.year,
        season,
    });
    let setup = match &doc.setup {
        Some(s) => r.setup(s, key),
        None => {
            r.push("setup", IssueKind::Missing, None);
            None
        }
    };

    let equipment = r.equipment(doc);
    let rooms = r.rooms(doc, &equipment);
    let instructors = r.instructors(doc);
    let groups = r.groups(doc);
    let courses = r.courses(doc);
    let ctx = Context {
        setup: setup.as_ref(),
        equipment: &equipment,
        rooms: &rooms,
        instructors: &instructors,
        groups: &groups,
        courses: &courses,
    };
    let sections = r.sections(doc, &ctx);
    let constraints = r.constraints(doc, &ctx, &sections);
    let placements = r.placements(doc, &ctx, &sections);
    for (i, u) in doc.unresolved.iter().enumerate() {
        r.push(
            format!("unresolved[{i}]"),
            IssueKind::Unresolved {
                what: u.what.trim().to_owned(),
                reason: u.reason.trim().to_owned(),
            },
            u.source.as_ref(),
        );
    }

    match setup {
        Some(setup) if r.issues.is_empty() => Ok(TermData {
            setup,
            courses,
            instructors,
            groups,
            equipment,
            rooms,
            sections,
            constraints,
            placements,
        }),
        _ => Err(r.issues),
    }
}

/// 이미 풀어 둔 기록들
struct Context<'a> {
    setup: Option<&'a TermSetup>,
    equipment: &'a [EquipmentKind],
    rooms: &'a [Room],
    instructors: &'a [Instructor],
    groups: &'a [StudentGroup],
    courses: &'a [Course],
}

#[derive(Default)]
struct Resolver {
    issues: Vec<Issue>,
}

impl Resolver {
    fn push(&mut self, path: impl Into<String>, kind: IssueKind, source: Option<&SourceRef>) {
        self.issues.push(Issue {
            path: path.into(),
            kind,
            source: source.cloned(),
        });
    }

    fn invalid(&mut self, path: impl Into<String>, value: &str, source: Option<&SourceRef>) {
        self.push(
            path,
            IssueKind::InvalidValue {
                value: value.to_owned(),
            },
            source,
        );
    }

    /// 코드 검사. 숫자로 들어온 코드는 확인 필요로 남기되 값은 돌려줘서 뒤 참조가 이어지게 한다.
    fn code<T>(
        &mut self,
        path: &str,
        value: &CodeValue,
        source: Option<&SourceRef>,
        make: fn(&str) -> Result<T, CodeError>,
    ) -> Option<T> {
        match make(&value.text) {
            Ok(code) => {
                if value.from_number {
                    self.push(
                        path,
                        IssueKind::NumericCode {
                            text: value.text.clone(),
                        },
                        source,
                    );
                }
                Some(code)
            }
            Err(e) => {
                self.push(path, IssueKind::InvalidCode(e), source);
                None
            }
        }
    }

    fn name(&mut self, path: &str, name: &str, source: Option<&SourceRef>) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            self.push(path, IssueKind::Missing, source);
            None
        } else {
            Some(name.to_owned())
        }
    }

    fn opt_text(text: &Option<String>) -> Option<String> {
        text.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    }

    fn date(
        &mut self,
        path: &str,
        text: &Option<String>,
        source: Option<&SourceRef>,
    ) -> Option<Date> {
        let text = text.as_deref()?;
        let date = Date::parse(text);
        if date.is_none() {
            self.invalid(path, text, source);
        }
        date
    }

    fn clock(&mut self, path: &str, text: &str, source: Option<&SourceRef>) -> Option<ClockTime> {
        let time = ClockTime::parse(text);
        if time.is_none() {
            self.invalid(path, text, source);
        }
        time
    }

    fn weekday(&mut self, path: &str, text: &str, source: Option<&SourceRef>) -> Option<Weekday> {
        let day = Weekday::parse(text);
        if day.is_none() {
            self.invalid(path, text, source);
        }
        day
    }

    fn strength(&mut self, path: &str, text: &str, source: Option<&SourceRef>) -> Option<Strength> {
        match text.trim().to_ascii_lowercase().as_str() {
            "hard" => Some(Strength::Hard),
            "soft" => Some(Strength::Soft),
            _ => {
                self.invalid(path, text, source);
                None
            }
        }
    }

    fn room_kind(
        &mut self,
        path: &str,
        text: &str,
        source: Option<&SourceRef>,
    ) -> Option<RoomKind> {
        match text.trim().to_ascii_lowercase().as_str() {
            "lecture" => Some(RoomKind::Lecture),
            "lab" => Some(RoomKind::Lab),
            "seminar" => Some(RoomKind::Seminar),
            "other" => Some(RoomKind::Other),
            _ => {
                self.invalid(path, text, source);
                None
            }
        }
    }

    fn duplicate<K: Ord + Clone>(
        &mut self,
        seen: &mut BTreeMap<K, ()>,
        key: &K,
        label: impl FnOnce() -> String,
        path: &str,
        source: Option<&SourceRef>,
    ) -> bool {
        if seen.insert(key.clone(), ()).is_some() {
            self.push(path, IssueKind::Duplicate { key: label() }, source);
            true
        } else {
            false
        }
    }

    /// 학기 키가 잘못됐어도 설정의 문제는 모두 찾아 둔다.
    fn setup(&mut self, s: &SetupInput, key: Option<TermKey>) -> Option<TermSetup> {
        let start = self.date("setup.start", &s.start, None);
        let end = self.date("setup.end", &s.end, None);
        if let (Some(a), Some(b)) = (start, end)
            && a > b
        {
            self.push("setup.end", IssueKind::ReversedRange, None);
        }

        let mut days = Vec::new();
        for (i, text) in s.days.iter().enumerate() {
            let path = format!("setup.days[{i}]");
            if let Some(day) = self.weekday(&path, text, None) {
                if days.contains(&day) {
                    self.push(
                        path,
                        IssueKind::Duplicate {
                            key: day.code().into(),
                        },
                        None,
                    );
                } else {
                    days.push(day);
                }
            }
        }
        if s.days.is_empty() {
            self.push("setup.days", IssueKind::Missing, None);
        }
        days.sort();

        let periods = self.period_rows("setup.periods", &s.periods, |label, start, end| Period {
            label,
            start,
            end,
        });
        let breaks = self.period_rows("setup.breaks", &s.breaks, |label, start, end| Break {
            label,
            start,
            end,
        });
        let table = match (periods, breaks) {
            (Some(p), Some(b)) => match PeriodTable::new(p, b) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.push("setup.periods", IssueKind::InvalidPeriodTable(e), None);
                    None
                }
            },
            _ => None,
        };

        let mut rules = SchoolRules::default();
        if let Some(input) = &s.rules {
            if let Some(text) = &input.required_course_overlap
                && let Some(strength) =
                    self.strength("setup.rules.required_course_overlap", text, None)
            {
                rules.required_course_overlap = strength;
            }
            if let Some(b) = input.block_may_cross_break {
                rules.block_may_cross_break = b;
            }
            if let Some(m) = input.room_turnaround_minutes {
                rules.room_turnaround_minutes = m;
            }
        }

        Some(TermSetup {
            key: key?,
            start,
            end,
            days,
            periods: table?,
            rules,
        })
    }

    fn period_rows<T>(
        &mut self,
        path: &str,
        rows: &[PeriodInput],
        make: fn(String, ClockTime, ClockTime) -> T,
    ) -> Option<Vec<T>> {
        let before = self.issues.len();
        let out: Vec<T> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let label = p.label.text.trim().to_owned();
                if label.is_empty() {
                    self.push(format!("{path}[{i}].label"), IssueKind::Missing, None);
                }
                let start = self.clock(&format!("{path}[{i}].start"), &p.start, None);
                let end = self.clock(&format!("{path}[{i}].end"), &p.end, None);
                Some(make(label, start?, end?))
            })
            .collect();
        (self.issues.len() == before).then_some(out)
    }

    fn equipment(&mut self, doc: &InputDocument) -> Vec<EquipmentKind> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        for (i, e) in doc.equipment.iter().enumerate() {
            let src = e.source.as_ref();
            let path = format!("equipment[{i}]");
            let code = self.code(&format!("{path}.code"), &e.code, src, EquipmentCode::new);
            let name = self.name(&format!("{path}.name"), &e.name, src);
            let scope = match e.scope.trim() {
                "per_person" => Some(EquipmentScope::PerPerson),
                "per_room" => Some(EquipmentScope::PerRoom),
                other => {
                    self.invalid(format!("{path}.scope"), other, src);
                    None
                }
            };
            let (Some(code), Some(name), Some(scope)) = (code, name, scope) else {
                continue;
            };
            if self.duplicate(
                &mut seen,
                &code,
                || code.to_string(),
                &format!("{path}.code"),
                src,
            ) {
                continue;
            }
            out.push(EquipmentKind {
                id: EquipmentId(out.len() as i64 + 1),
                code,
                name,
                scope,
            });
        }
        out
    }

    fn rooms(&mut self, doc: &InputDocument, equipment: &[EquipmentKind]) -> Vec<Room> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        for (i, r) in doc.rooms.iter().enumerate() {
            let src = r.source.as_ref();
            let path = format!("rooms[{i}]");
            let key = match RoomKey::new(&r.building, &r.number.text) {
                Ok(k) => Some(k),
                Err(e) => {
                    self.push(format!("{path}.number"), IssueKind::InvalidRoom(e), src);
                    None
                }
            };
            let kind = self.room_kind(&format!("{path}.kind"), &r.kind, src);
            let available_seats = r.available_seats.unwrap_or(r.total_seats);
            if available_seats > r.total_seats {
                self.push(
                    format!("{path}.available_seats"),
                    IssueKind::AvailableExceedsTotal {
                        available: available_seats,
                        total: r.total_seats,
                    },
                    src,
                );
            }
            let mut items = Vec::new();
            for (j, e) in r.equipment.iter().enumerate() {
                let epath = format!("{path}.equipment[{j}]");
                let Some(code) =
                    self.code(&format!("{epath}.code"), &e.code, src, EquipmentCode::new)
                else {
                    continue;
                };
                let Some(kind) = equipment.iter().find(|k| k.code == code) else {
                    self.push(
                        format!("{epath}.code"),
                        IssueKind::UnknownReference {
                            key: code.to_string(),
                        },
                        src,
                    );
                    continue;
                };
                let available = e.available.unwrap_or(e.owned);
                if available > e.owned {
                    self.push(
                        format!("{epath}.available"),
                        IssueKind::AvailableExceedsTotal {
                            available,
                            total: e.owned,
                        },
                        src,
                    );
                }
                let from = self.date(&format!("{epath}.inspection_from"), &e.inspection_from, src);
                let until = self.date(
                    &format!("{epath}.inspection_until"),
                    &e.inspection_until,
                    src,
                );
                let inspection = match (from, until) {
                    (Some(from), Some(until)) if from <= until => Some(DateRange { from, until }),
                    (Some(_), Some(_)) => {
                        self.push(
                            format!("{epath}.inspection_until"),
                            IssueKind::ReversedRange,
                            src,
                        );
                        None
                    }
                    (None, None) => None,
                    _ => {
                        self.push(format!("{epath}.inspection_until"), IssueKind::Missing, src);
                        None
                    }
                };
                if items.iter().any(|x: &RoomEquipment| x.equipment == kind.id) {
                    self.push(
                        format!("{epath}.code"),
                        IssueKind::Duplicate {
                            key: code.to_string(),
                        },
                        src,
                    );
                    continue;
                }
                items.push(RoomEquipment {
                    equipment: kind.id,
                    owned: e.owned,
                    available,
                    inspection,
                });
            }
            let checked = self.date(
                &format!("{path}.schedule_checked"),
                &r.schedule_checked,
                src,
            );
            let (Some(key), Some(kind)) = (key, kind) else {
                continue;
            };
            if self.duplicate(
                &mut seen,
                &key,
                || key.to_string(),
                &format!("{path}.number"),
                src,
            ) {
                continue;
            }
            out.push(Room {
                id: RoomId(out.len() as i64 + 1),
                key,
                kind,
                department: Self::opt_text(&r.department),
                total_seats: r.total_seats,
                available_seats,
                equipment: items,
                shared: r.shared,
                schedule_checked: checked,
                info_source: Self::opt_text(&r.info_source),
            });
        }
        out
    }

    fn instructors(&mut self, doc: &InputDocument) -> Vec<Instructor> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        for (i, t) in doc.instructors.iter().enumerate() {
            let src = t.source.as_ref();
            let path = format!("instructors[{i}]");
            let code = self.code(&format!("{path}.code"), &t.code, src, InstructorCode::new);
            let name = self.name(&format!("{path}.name"), &t.name, src);
            let (Some(code), Some(name)) = (code, name) else {
                continue;
            };
            if self.duplicate(
                &mut seen,
                &code,
                || code.to_string(),
                &format!("{path}.code"),
                src,
            ) {
                continue;
            }
            out.push(Instructor {
                id: InstructorId(out.len() as i64 + 1),
                code,
                name,
                department: Self::opt_text(&t.department),
            });
        }
        out
    }

    fn groups(&mut self, doc: &InputDocument) -> Vec<StudentGroup> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        for (i, g) in doc.groups.iter().enumerate() {
            let src = g.source.as_ref();
            let path = format!("groups[{i}]");
            let code = self.code(&format!("{path}.code"), &g.code, src, GroupCode::new);
            let name = self.name(&format!("{path}.name"), &g.name, src);
            let (Some(code), Some(name)) = (code, name) else {
                continue;
            };
            if self.duplicate(
                &mut seen,
                &code,
                || code.to_string(),
                &format!("{path}.code"),
                src,
            ) {
                continue;
            }
            out.push(StudentGroup {
                id: GroupId(out.len() as i64 + 1),
                code,
                name,
                grade: g.grade,
                track: Self::opt_text(&g.track),
            });
        }
        out
    }

    fn courses(&mut self, doc: &InputDocument) -> Vec<Course> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        for (i, c) in doc.courses.iter().enumerate() {
            let src = c.source.as_ref();
            let path = format!("courses[{i}]");
            let code = self.code(&format!("{path}.code"), &c.code, src, CourseCode::new);
            let name = self.name(&format!("{path}.name"), &c.name, src);
            let (Some(code), Some(name)) = (code, name) else {
                continue;
            };
            if self.duplicate(
                &mut seen,
                &code,
                || code.to_string(),
                &format!("{path}.code"),
                src,
            ) {
                continue;
            }
            out.push(Course {
                id: CourseId(out.len() as i64 + 1),
                code,
                name,
            });
        }
        out
    }

    fn instructor_ref(
        &mut self,
        path: &str,
        r: &InstructorRefInput,
        src: Option<&SourceRef>,
        all: &[Instructor],
    ) -> Option<InstructorId> {
        let name = r.name.as_deref().map(str::trim).filter(|n| !n.is_empty());
        if let Some(code) = &r.code {
            let code = self.code(&format!("{path}.code"), code, src, InstructorCode::new)?;
            let Some(found) = all.iter().find(|t| t.code == code) else {
                self.push(
                    format!("{path}.code"),
                    IssueKind::UnknownReference {
                        key: code.to_string(),
                    },
                    src,
                );
                return None;
            };
            if let Some(name) = name
                && name != found.name
            {
                self.push(
                    format!("{path}.name"),
                    IssueKind::InstructorNameMismatch {
                        code: code.to_string(),
                        name: name.to_owned(),
                    },
                    src,
                );
            }
            return Some(found.id);
        }
        let Some(name) = name else {
            self.push(path, IssueKind::Missing, src);
            return None;
        };
        let matches: Vec<&Instructor> = all.iter().filter(|t| t.name == name).collect();
        match matches.as_slice() {
            [one] => Some(one.id),
            [] => {
                self.push(
                    format!("{path}.name"),
                    IssueKind::UnknownReference {
                        key: name.to_owned(),
                    },
                    src,
                );
                None
            }
            many => {
                self.push(
                    format!("{path}.name"),
                    IssueKind::AmbiguousInstructor {
                        name: name.to_owned(),
                        candidates: many.iter().map(|t| t.code.clone()).collect(),
                    },
                    src,
                );
                None
            }
        }
    }

    fn room_ref(
        &mut self,
        path: &str,
        r: &RoomRefInput,
        src: Option<&SourceRef>,
        rooms: &[Room],
    ) -> Option<RoomId> {
        let building = r
            .building
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty());
        if let (Some(building), Some(number)) = (building, &r.number) {
            return match RoomKey::new(building, &number.text) {
                Ok(key) => match rooms.iter().find(|room| room.key == key) {
                    Some(room) => Some(room.id),
                    None => {
                        self.push(
                            path,
                            IssueKind::UnknownReference {
                                key: key.to_string(),
                            },
                            src,
                        );
                        None
                    }
                },
                Err(e) => {
                    self.push(path, IssueKind::InvalidRoom(e), src);
                    None
                }
            };
        }
        // 원본 표기 그대로 온 경우: 공백과 `호`만 무시하고 건물+호실이 정확히 같은 강의실만 잇는다.
        let text = match (&r.text, &r.number) {
            (Some(t), _) => t.clone(),
            (None, Some(n)) => n.text.clone(),
            (None, None) => {
                self.push(path, IssueKind::Missing, src);
                return None;
            }
        };
        let compact = compact_room_text(&text);
        if let Some(room) = rooms.iter().find(|room| room.key.compact() == compact) {
            return Some(room.id);
        }
        // 후보: 호실 번호가 같은 강의실. 표기 앞부분이 알 수 없는 건물 이름이어도 후보로 보인다
        // (`IT관201` → 201호들). `B101`처럼 번호의 일부인 글자는 건물로 보지 않는다.
        let candidates: Vec<RoomKey> = rooms
            .iter()
            .filter(|room| {
                compact
                    .strip_suffix(&compact_room_text(&room.key.number))
                    .is_some_and(|rest| !rest.ends_with(|c: char| c.is_ascii_alphanumeric()))
            })
            .map(|room| room.key.clone())
            .collect();
        let kind = if candidates.is_empty() {
            IssueKind::UnknownReference { key: text }
        } else {
            IssueKind::AmbiguousRoom { text, candidates }
        };
        self.push(path, kind, src);
        None
    }

    fn sections(&mut self, doc: &InputDocument, ctx: &Context<'_>) -> Vec<Section> {
        let mut seen = BTreeMap::new();
        let mut out = Vec::new();
        let day_len = ctx.setup.map(|s| s.periods.len());
        for (i, s) in doc.sections.iter().enumerate() {
            let src = s.source.as_ref();
            let path = format!("sections[{i}]");
            let course_code = self.code(&format!("{path}.course"), &s.course, src, CourseCode::new);
            let code = self.code(
                &format!("{path}.section"),
                &s.section,
                src,
                SectionCode::new,
            );
            let course = course_code.as_ref().and_then(|cc| {
                let found = ctx.courses.iter().find(|c| &c.code == cc);
                if found.is_none() {
                    self.push(
                        format!("{path}.course"),
                        IssueKind::UnknownReference {
                            key: cc.to_string(),
                        },
                        src,
                    );
                }
                found
            });

            let mut instructors = Vec::new();
            for (j, t) in s.instructors.iter().enumerate() {
                if let Some(id) = self.instructor_ref(
                    &format!("{path}.instructors[{j}]"),
                    t,
                    src,
                    ctx.instructors,
                ) {
                    if instructors.contains(&id) {
                        self.push(
                            format!("{path}.instructors[{j}]"),
                            IssueKind::Duplicate {
                                key: format!("{id:?}"),
                            },
                            src,
                        );
                    } else {
                        instructors.push(id);
                    }
                }
            }

            let mut groups = Vec::new();
            for (j, g) in s.groups.iter().enumerate() {
                let gpath = format!("{path}.groups[{j}].group");
                let Some(code) = self.code(&gpath, &g.group, src, GroupCode::new) else {
                    continue;
                };
                match ctx.groups.iter().find(|x| x.code == code) {
                    Some(found) if groups.iter().any(|l: &GroupLink| l.group == found.id) => {
                        self.push(
                            gpath,
                            IssueKind::Duplicate {
                                key: code.to_string(),
                            },
                            src,
                        );
                    }
                    Some(found) => groups.push(GroupLink {
                        group: found.id,
                        required: g.required,
                    }),
                    None => self.push(
                        gpath,
                        IssueKind::UnknownReference {
                            key: code.to_string(),
                        },
                        src,
                    ),
                }
            }

            let kind = match s.kind.trim().to_ascii_lowercase().as_str() {
                "lecture" => Some(ClassKind::Lecture),
                "lab" => Some(ClassKind::Lab),
                "mixed" => Some(ClassKind::Mixed),
                other => {
                    self.invalid(format!("{path}.kind"), other, src);
                    None
                }
            };
            let room_kind = s
                .room_kind
                .as_deref()
                .and_then(|k| self.room_kind(&format!("{path}.room_kind"), k, src));

            if s.weekly_periods == 0 {
                self.push(format!("{path}.weekly_periods"), IssueKind::Zero, src);
            }
            let split = s.split.clone().unwrap_or_else(|| vec![s.weekly_periods]);
            let split_sum: u32 = split.iter().map(|&n| n as u32).sum();
            let mut split_ok = true;
            if split_sum != s.weekly_periods as u32 {
                split_ok = false;
                self.push(
                    format!("{path}.split"),
                    IssueKind::SplitMismatch {
                        weekly: s.weekly_periods,
                        split_sum,
                    },
                    src,
                );
            }
            for (j, &part) in split.iter().enumerate() {
                if part == 0 || day_len.is_some_and(|len| part as usize > len) {
                    split_ok = false;
                    self.push(
                        format!("{path}.split[{j}]"),
                        IssueKind::InvalidSplitPart { periods: part },
                        src,
                    );
                }
            }
            if s.enrollment == 0 {
                self.push(format!("{path}.enrollment"), IssueKind::Zero, src);
            }

            let mut needs = Vec::new();
            for (j, e) in s.equipment.iter().enumerate() {
                let epath = format!("{path}.equipment[{j}]");
                let Some(code) =
                    self.code(&format!("{epath}.code"), &e.code, src, EquipmentCode::new)
                else {
                    continue;
                };
                let Some(kind) = ctx.equipment.iter().find(|k| k.code == code) else {
                    self.push(
                        format!("{epath}.code"),
                        IssueKind::UnknownReference {
                            key: code.to_string(),
                        },
                        src,
                    );
                    continue;
                };
                if e.quantity == 0 {
                    self.push(format!("{epath}.quantity"), IssueKind::Zero, src);
                    continue;
                }
                needs.push(EquipmentNeed {
                    equipment: kind.id,
                    quantity: e.quantity,
                });
            }

            let (Some(course), Some(code), Some(kind)) = (course, code, kind) else {
                continue;
            };
            let key = (course.code.clone(), code.clone());
            if self.duplicate(
                &mut seen,
                &key,
                || format!("{}-{}", key.0, key.1),
                &format!("{path}.section"),
                src,
            ) || !split_ok
            {
                continue;
            }
            out.push(Section {
                id: SectionId(out.len() as i64 + 1),
                course: course.id,
                code,
                instructors,
                groups,
                enrollment: Enrollment {
                    count: s.enrollment,
                    provisional: s.provisional,
                },
                kind,
                room_kind,
                split,
                equipment: needs,
            });
        }
        out
    }

    fn section_ref(
        &mut self,
        path: &str,
        course: &CodeValue,
        section: &CodeValue,
        src: Option<&SourceRef>,
        ctx: &Context<'_>,
        sections: &[Section],
    ) -> Option<SectionId> {
        let cc = self.code(&format!("{path}.course"), course, src, CourseCode::new);
        let sc = self.code(&format!("{path}.section"), section, src, SectionCode::new);
        let (cc, sc) = (cc?, sc?);
        let found = ctx
            .courses
            .iter()
            .find(|c| c.code == cc)
            .and_then(|c| sections.iter().find(|s| s.course == c.id && s.code == sc));
        if found.is_none() {
            self.push(
                path,
                IssueKind::UnknownReference {
                    key: format!("{cc}-{sc}"),
                },
                src,
            );
        }
        found.map(|s| s.id)
    }

    fn target(
        &mut self,
        path: &str,
        t: &TargetInput,
        src: Option<&SourceRef>,
        ctx: &Context<'_>,
        sections: &[Section],
    ) -> Option<Target> {
        let count = [
            t.instructor.is_some(),
            t.section.is_some(),
            t.group.is_some(),
            t.room.is_some(),
        ]
        .into_iter()
        .filter(|&b| b)
        .count();
        if count != 1 {
            self.push(path, IssueKind::TargetNotSingle, src);
            return None;
        }
        if let Some(r) = &t.instructor {
            return self
                .instructor_ref(&format!("{path}.instructor"), r, src, ctx.instructors)
                .map(Target::Instructor);
        }
        if let Some(s) = &t.section {
            return self
                .section_ref(
                    &format!("{path}.section"),
                    &s.course,
                    &s.section,
                    src,
                    ctx,
                    sections,
                )
                .map(Target::Section);
        }
        if let Some(g) = &t.group {
            let gpath = format!("{path}.group");
            let code = self.code(&gpath, g, src, GroupCode::new)?;
            let found = ctx.groups.iter().find(|x| x.code == code);
            if found.is_none() {
                self.push(
                    gpath,
                    IssueKind::UnknownReference {
                        key: code.to_string(),
                    },
                    src,
                );
            }
            return found.map(|g| Target::Group(g.id));
        }
        let r = t.room.as_ref()?;
        self.room_ref(&format!("{path}.room"), r, src, ctx.rooms)
            .map(Target::Room)
    }

    fn window(
        &mut self,
        path: &str,
        w: &WindowInput,
        src: Option<&SourceRef>,
        setup: Option<&TermSetup>,
    ) -> Option<TimeWindow> {
        let day = self.weekday(&format!("{path}.day"), &w.day, src);
        let by_period = w.first_period.is_some() || w.last_period.is_some();
        let by_clock = w.start.is_some() || w.end.is_some();
        if by_period == by_clock {
            self.push(path, IssueKind::AmbiguousWindow, src);
            return None;
        }
        if by_clock {
            let (Some(start), Some(end)) = (&w.start, &w.end) else {
                self.push(path, IssueKind::Missing, src);
                return None;
            };
            let start = self.clock(&format!("{path}.start"), start, src);
            let end = self.clock(&format!("{path}.end"), end, src);
            let (day, start, end) = (day?, start?, end?);
            if start >= end {
                self.push(format!("{path}.end"), IssueKind::ReversedRange, src);
                return None;
            }
            return Some(TimeWindow::Clock { day, start, end });
        }
        let Some(first) = &w.first_period else {
            self.push(format!("{path}.first_period"), IssueKind::Missing, src);
            return None;
        };
        let last = w.last_period.as_ref().unwrap_or(first);
        let table = &setup?.periods;
        let mut index = |field: &str, label: &CodeValue| {
            let found = table.index_of(&label.text);
            if found.is_none() {
                self.push(
                    format!("{path}.{field}"),
                    IssueKind::UnknownReference {
                        key: label.text.clone(),
                    },
                    src,
                );
            }
            found
        };
        let first = index("first_period", first);
        let last = index("last_period", last);
        let (day, first, last) = (day?, first?, last?);
        if first > last {
            self.push(format!("{path}.last_period"), IssueKind::ReversedRange, src);
            return None;
        }
        Some(TimeWindow::Periods {
            day,
            first: first as u8,
            last: last as u8,
        })
    }

    fn constraints(
        &mut self,
        doc: &InputDocument,
        ctx: &Context<'_>,
        sections: &[Section],
    ) -> Vec<Constraint> {
        let mut keys = BTreeMap::new();
        let mut out = Vec::new();
        for (i, c) in doc.constraints.iter().enumerate() {
            out.extend(self.constraint(i, c, ctx, sections, &mut keys, out.len()));
        }
        out
    }

    fn constraint(
        &mut self,
        i: usize,
        c: &ConstraintInput,
        ctx: &Context<'_>,
        sections: &[Section],
        keys: &mut BTreeMap<String, ()>,
        count: usize,
    ) -> Option<Constraint> {
        let src = c.source.as_ref();
        let path = format!("constraints[{i}]");
        if let Some(key) = Self::opt_text(&c.key) {
            self.duplicate(keys, &key, || key.clone(), &format!("{path}.key"), src);
        }
        let target = self.target(&format!("{path}.target"), &c.target, src, ctx, sections);
        let status = match c.status.trim() {
            "confirmed" => Some(ConstraintStatus::Confirmed),
            "pending" => Some(ConstraintStatus::Pending),
            other => {
                self.invalid(format!("{path}.status"), other, src);
                None
            }
        };
        let strength = c
            .strength
            .as_deref()
            .and_then(|s| self.strength(&format!("{path}.strength"), s, src));
        let weight = c.weight.unwrap_or(1);
        if !WEIGHT_RANGE.contains(&weight) {
            self.push(
                format!("{path}.weight"),
                IssueKind::WeightOutOfRange { weight },
                src,
            );
        }
        let rule = match &c.rule {
            None => None,
            Some(rule) if rule.kind.trim() == "avoid_times" => {
                if rule.windows.is_empty() {
                    self.push(format!("{path}.rule.windows"), IssueKind::Missing, src);
                }
                let before = self.issues.len();
                let windows: Vec<TimeWindow> = rule
                    .windows
                    .iter()
                    .enumerate()
                    .filter_map(|(j, w)| {
                        self.window(&format!("{path}.rule.windows[{j}]"), w, src, ctx.setup)
                    })
                    .collect();
                (self.issues.len() == before).then_some(Rule::AvoidTimes { windows })
            }
            Some(rule) => {
                self.invalid(format!("{path}.rule.kind"), &rule.kind, src);
                None
            }
        };
        if status == Some(ConstraintStatus::Confirmed) && (c.rule.is_none() || strength.is_none()) {
            self.push(path.clone(), IssueKind::ConfirmedIncomplete, src);
        }

        let from = self.date(&format!("{path}.from"), &c.from, src);
        let until = self.date(&format!("{path}.until"), &c.until, src);
        let term = ctx.setup.map(|s| (s.start, s.end));
        let period = match (from, until) {
            (None, None) => None,
            (from, until) => {
                let from = from.or(term.and_then(|t| t.0));
                let until = until.or(term.and_then(|t| t.1));
                match (from, until) {
                    (Some(from), Some(until)) if from <= until => Some(DateRange { from, until }),
                    (Some(_), Some(_)) => {
                        self.push(format!("{path}.until"), IssueKind::ReversedRange, src);
                        None
                    }
                    // 한쪽 끝만 적었는데 학기 기간이 없어 채울 수 없다
                    _ => {
                        self.push(format!("{path}.from"), IssueKind::Missing, src);
                        None
                    }
                }
            }
        };

        Some(Constraint {
            id: ConstraintId(count as i64 + 1),
            target: target?,
            rule,
            strength,
            weight,
            status: status?,
            period,
            text: Self::opt_text(&c.text),
            source: c.source.clone(),
            confirmed_by: Self::opt_text(&c.confirmed_by),
            note: Self::opt_text(&c.note),
        })
    }

    fn placements(
        &mut self,
        doc: &InputDocument,
        ctx: &Context<'_>,
        sections: &[Section],
    ) -> Vec<Placement> {
        let mut seen: HashMap<MeetingRef, ()> = HashMap::new();
        let mut out = Vec::new();
        for (i, p) in doc.placements.iter().enumerate() {
            let src = p.source.as_ref();
            let path = format!("placements[{i}]");
            let section = self
                .section_ref(&path, &p.course, &p.section, src, ctx, sections)
                .and_then(|id| sections.iter().find(|s| s.id == id));
            let day = self.weekday(&format!("{path}.day"), &p.day, src);
            if let (Some(day), Some(setup)) = (day, ctx.setup)
                && !setup.days.contains(&day)
            {
                self.push(format!("{path}.day"), IssueKind::DayNotInTerm, src);
            }
            let start = ctx.setup.and_then(|setup| {
                let found = setup.periods.index_of(&p.start_period.text);
                if found.is_none() {
                    self.push(
                        format!("{path}.start_period"),
                        IssueKind::UnknownReference {
                            key: p.start_period.text.clone(),
                        },
                        src,
                    );
                }
                found
            });
            let room = p
                .room
                .as_ref()
                .and_then(|r| self.room_ref(&format!("{path}.room"), r, src, ctx.rooms));
            if p.room.is_some() && room.is_none() {
                continue;
            }
            let (Some(section), Some(day), Some(start)) = (section, day, start) else {
                continue;
            };
            let part = p.part.unwrap_or(1);
            let Some(&span) = (part as usize)
                .checked_sub(1)
                .and_then(|k| section.split.get(k))
            else {
                self.push(
                    format!("{path}.part"),
                    IssueKind::PartOutOfRange {
                        parts: section.split.len(),
                    },
                    src,
                );
                continue;
            };
            let setup = ctx.setup.expect("교시를 찾았으면 학기 설정이 있다");
            if !setup.periods.contains_span(start, span as usize) {
                self.push(
                    format!("{path}.start_period"),
                    IssueKind::SpanPastLastPeriod,
                    src,
                );
                continue;
            }
            let meeting = MeetingRef {
                section: section.id,
                part: part - 1,
            };
            if seen.insert(meeting, ()).is_some() {
                self.push(
                    format!("{path}.part"),
                    IssueKind::Duplicate {
                        key: format!("{}-{}", p.course, p.section),
                    },
                    src,
                );
                continue;
            }
            out.push(Placement {
                meeting,
                day,
                start: start as u8,
                room,
                locked: p.locked,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::*;

    fn base() -> InputDocument {
        let mut doc = InputDocument::new(2027, "1");
        doc.setup = Some(SetupInput {
            start: Some("2027-03-02".into()),
            end: Some("2027-06-15".into()),
            days: ["mon", "tue", "wed", "thu", "fri"]
                .map(String::from)
                .to_vec(),
            periods: (0..8)
                .map(|i| PeriodInput {
                    label: CodeValue::text((i + 1).to_string()),
                    start: format!("{:02}:00", 9 + i),
                    end: format!("{:02}:50", 9 + i),
                })
                .collect(),
            breaks: vec![],
            rules: None,
        });
        doc.equipment.push(EquipmentInput {
            code: "PC".into(),
            name: "실습용 PC".into(),
            scope: "per_person".into(),
            source: None,
        });
        for (b, n) in [("공학관", "302"), ("공학관", "201"), ("본관", "201")] {
            doc.rooms.push(RoomInput {
                building: b.into(),
                number: n.into(),
                kind: "lab".into(),
                department: None,
                total_seats: 32,
                available_seats: None,
                equipment: vec![RoomEquipmentInput {
                    code: "PC".into(),
                    owned: 32,
                    available: Some(30),
                    inspection_from: None,
                    inspection_until: None,
                }],
                shared: b == "본관",
                schedule_checked: None,
                info_source: None,
                source: None,
            });
        }
        for (code, name) in [("T01", "김민수"), ("T02", "김민수"), ("T03", "이서연")] {
            doc.instructors.push(InstructorInput {
                code: code.into(),
                name: name.into(),
                department: None,
                source: None,
            });
        }
        doc.groups.push(GroupInput {
            code: "Y2".into(),
            name: "2학년".into(),
            grade: Some(2),
            track: None,
            source: None,
        });
        doc.courses.push(CourseInput {
            code: "CSE2010".into(),
            name: "자료구조".into(),
            source: None,
        });
        doc.sections.push(SectionInput {
            course: "CSE2010".into(),
            section: "01".into(),
            instructors: vec![InstructorRefInput {
                code: Some("T03".into()),
                name: None,
            }],
            groups: vec![GroupLinkInput {
                group: "Y2".into(),
                required: true,
            }],
            enrollment: 28,
            provisional: false,
            kind: "lab".into(),
            room_kind: Some("lab".into()),
            weekly_periods: 3,
            split: Some(vec![2, 1]),
            equipment: vec![EquipmentNeedInput {
                code: "PC".into(),
                quantity: 1,
            }],
            source: None,
        });
        doc
    }

    fn kinds(issues: &[Issue]) -> Vec<(&str, &'static str)> {
        issues
            .iter()
            .map(|i| (i.path.as_str(), i.kind.code()))
            .collect()
    }

    #[test]
    fn clean_document_resolves() {
        let data = resolve(&base()).unwrap();
        assert_eq!(data.rooms.len(), 3);
        let s = &data.sections[0];
        assert_eq!(s.code.as_str(), "01");
        assert_eq!(s.weekly_periods(), 3);
        assert_eq!(data.instructor(s.instructors[0]).unwrap().name, "이서연");
        assert_eq!(data.rooms[0].equipment[0].available, 30);
        assert!(data.rooms[2].needs_external_check());
    }

    #[test]
    fn numeric_section_code_needs_review() {
        let mut doc = base();
        doc.sections[0].section = CodeValue::number(1);
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(kinds(&issues), [("sections[0].section", "numeric_code")]);
        assert_eq!(issues[0].severity(), Severity::NeedsReview);
    }

    #[test]
    fn room_without_building_is_not_guessed() {
        let mut doc = base();
        doc.placements.push(PlacementInput {
            course: "CSE2010".into(),
            section: "01".into(),
            part: None,
            day: "수".into(),
            start_period: "1".into(),
            room: Some(RoomRefInput {
                text: Some("201호".into()),
                ..Default::default()
            }),
            locked: false,
            source: Some(SourceRef {
                file: Some("편성.xlsx".into()),
                sheet: Some("Sheet1".into()),
                row: Some(7),
                column: Some("F".into()),
            }),
        });
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(issues.len(), 1);
        let IssueKind::AmbiguousRoom { candidates, .. } = &issues[0].kind else {
            panic!("{:?}", issues[0]);
        };
        assert_eq!(candidates.len(), 2);
        assert_eq!(issues[0].source.as_ref().unwrap().row, Some(7));

        // 건물과 호실이 정확하면 띄어쓰기와 `호`가 달라도 잇는다
        doc.placements[0].room = Some(RoomRefInput {
            text: Some("공학관302호".into()),
            ..Default::default()
        });
        let data = resolve(&doc).unwrap();
        let room = data.room(data.placements[0].room.unwrap()).unwrap();
        assert_eq!(room.key.to_string(), "공학관 302");
        assert_eq!(data.placements[0].day, Weekday::Wed);
    }

    #[test]
    fn homonym_by_name_needs_review() {
        let mut doc = base();
        doc.sections[0].instructors = vec![InstructorRefInput {
            code: None,
            name: Some("김민수".into()),
        }];
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(
            kinds(&issues),
            [("sections[0].instructors[0].name", "ambiguous_instructor")]
        );
        let IssueKind::AmbiguousInstructor { candidates, .. } = &issues[0].kind else {
            unreachable!()
        };
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn constraints_keep_strength_and_pending_requests() {
        let mut doc = base();
        let target = TargetInput {
            instructor: Some(InstructorRefInput {
                code: Some("T03".into()),
                name: None,
            }),
            ..Default::default()
        };
        let clock = |day: &str, s: &str, e: &str| WindowInput {
            day: day.into(),
            first_period: None,
            last_period: None,
            start: Some(s.into()),
            end: Some(e.into()),
        };
        doc.constraints = vec![
            ConstraintInput {
                key: Some("c1".into()),
                target: target.clone(),
                rule: Some(RuleInput {
                    kind: "avoid_times".into(),
                    windows: vec![clock("wed", "13:00", "16:00")],
                }),
                strength: Some("soft".into()),
                weight: None,
                status: "confirmed".into(),
                from: None,
                until: None,
                text: Some("수요일 13~16시는 가능하면 비워 주세요.".into()),
                confirmed_by: None,
                note: None,
                source: None,
            },
            ConstraintInput {
                key: Some("c2".into()),
                target: target.clone(),
                rule: None,
                strength: None,
                weight: None,
                status: "pending".into(),
                from: None,
                until: None,
                text: Some("오전에 수업하기 어렵습니다.".into()),
                confirmed_by: None,
                note: None,
                source: None,
            },
        ];
        let data = resolve(&doc).unwrap();
        assert!(data.constraints[0].is_active());
        assert!(!data.constraints[1].is_active());

        // 확인됐다면서 내용이 없으면 오류
        doc.constraints[1].status = "confirmed".into();
        // 교시와 시각을 함께 적으면 오류
        let mut both = clock("thu", "09:00", "10:00");
        both.first_period = Some("1".into());
        doc.constraints[0].rule.as_mut().unwrap().windows.push(both);
        // 적용 기간이 거꾸로면 오류
        doc.constraints[0].from = Some("2027-05-01".into());
        doc.constraints[0].until = Some("2027-04-01".into());
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(
            kinds(&issues),
            [
                ("constraints[0].rule.windows[1]", "ambiguous_window"),
                ("constraints[0].until", "reversed_range"),
                ("constraints[1]", "confirmed_incomplete"),
            ]
        );
    }

    #[test]
    fn all_problems_are_reported_together() {
        let mut doc = base();
        doc.schema = "auler.input.v0".into();
        doc.term.season = "3".into();
        doc.sections[0].split = Some(vec![2, 2]);
        doc.sections[0].groups[0].group = "Y9".into();
        doc.rooms[0].equipment[0].available = Some(40);
        doc.courses.push(doc.courses[0].clone());
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(
            kinds(&issues),
            [
                ("schema", "unsupported_schema"),
                ("term.season", "invalid_value"),
                ("rooms[0].equipment[0].available", "available_exceeds_total"),
                ("courses[1].code", "duplicate"),
                ("sections[0].groups[0].group", "unknown_reference"),
                ("sections[0].split", "split_mismatch"),
            ]
        );
    }

    #[test]
    fn placements_must_fit_the_day() {
        let mut doc = base();
        let place = |part: Option<u8>, day: &str, start: &str| PlacementInput {
            course: "CSE2010".into(),
            section: "01".into(),
            part,
            day: day.into(),
            start_period: start.into(),
            room: None,
            locked: true,
            source: None,
        };
        doc.placements = vec![
            place(Some(1), "mon", "8"),
            place(Some(3), "tue", "1"),
            place(None, "sat", "1"),
        ];
        let issues = resolve(&doc).unwrap_err();
        assert_eq!(
            kinds(&issues),
            [
                ("placements[0].start_period", "span_past_last_period"),
                ("placements[1].part", "part_out_of_range"),
                ("placements[2].day", "day_not_in_term"),
            ]
        );
        doc.placements = vec![place(Some(1), "mon", "7"), place(Some(2), "tue", "8")];
        let data = resolve(&doc).unwrap();
        assert_eq!(data.placements[1].meeting.part, 1);
        assert!(data.placements.iter().all(|p| p.locked));
    }
}
