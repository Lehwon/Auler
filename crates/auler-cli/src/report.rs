use std::io::{self, Write};
use std::path::Path;

use auler_domain::model::*;
use auler_domain::resolve::{Issue, IssueKind, Severity};
use auler_domain::template::{INFO_SHEET, find_sheet};
use auler_domain::time::{ClockTime, Weekday};
use auler_engine::{
    Finding, OccupancyError, Reason, RequiredResource, Resource, SolveResult, occupancy,
};
use auler_io::xlsx::Problem;
use serde::Serialize;

#[derive(Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub source: Option<SourceRef>,
    pub path: Option<String>,
    pub detail: String,
    pub guidance: &'static str,
}

impl Diagnostic {
    pub fn arguments(detail: &str, usage: &'static str) -> Self {
        Self {
            code: "invalid_arguments",
            severity: "error",
            source: None,
            path: None,
            detail: detail.into(),
            guidance: usage,
        }
    }

    pub fn file(code: &'static str, path: &Path, detail: String) -> Self {
        Self {
            code,
            severity: "error",
            source: Some(SourceRef {
                file: Some(path.to_string_lossy().into_owned()),
                ..SourceRef::default()
            }),
            path: None,
            detail,
            guidance: "파일 경로, 파일 내용, 지원하는 입력 형식을 확인하세요.",
        }
    }

    pub fn xlsx(issue: auler_io::xlsx::Diagnostic) -> Self {
        let column = |key| {
            issue
                .source
                .sheet
                .as_deref()
                .and_then(find_sheet)
                .and_then(|sheet| sheet.column(key))
                .map_or(key, |column| column.ko)
        };
        let detail = match &issue.kind {
            Problem::MissingSheet { key } => format!("필수 시트 없음 ({})", sheet_name(key)),
            Problem::DuplicateSheet { key } => {
                format!("같은 시트가 여러 개 있음 ({})", sheet_name(key))
            }
            Problem::UnknownSheet => "표준 양식에서 지원하지 않는 시트".into(),
            Problem::UnsupportedVersion { found } => format!(
                "지원하지 않는 양식 버전 ({})",
                found.as_deref().unwrap_or("값 없음")
            ),
            Problem::MissingColumn { key } => format!("필수 머리글 없음 ({})", column(key)),
            Problem::UnknownColumn { found } => format!("알 수 없는 머리글 ({found})"),
            Problem::DuplicateColumn { key } => format!("머리글 중복 ({})", column(key)),
            Problem::MissingValue { key } => format!("값이 비었음 ({})", column(key)),
            Problem::InvalidValue { key, .. } => {
                format!("셀 값의 형식이 맞지 않음 ({})", column(key))
            }
            Problem::Formula => "값 대신 수식이 들어 있음".into(),
            Problem::TermRowCount { found } => format!("입력한 학기 {found}개"),
            Problem::ConflictingConstraint { key, field } => {
                format!("같은 조건의 값이 서로 다름 ({key}, {})", column(field))
            }
        };
        Self {
            code: issue.kind.code(),
            severity: "error",
            source: Some(issue.source),
            path: None,
            detail,
            guidance: issue.kind.guidance(),
        }
    }

    pub fn input(issue: Issue) -> Self {
        let severity = match issue.severity() {
            Severity::NeedsReview => "needs_review",
            Severity::Error => "error",
        };
        let guidance = match &issue.kind {
            IssueKind::NumericCode { .. } => "원본 코드와 선행 0을 확인한 뒤 텍스트로 입력하세요.",
            IssueKind::AmbiguousRoom { .. } => "건물과 호실을 함께 입력하세요.",
            IssueKind::AmbiguousInstructor { .. } | IssueKind::InstructorNameMismatch { .. } => {
                "교원 코드와 이름을 확인하세요."
            }
            IssueKind::Unresolved { .. } => {
                "원본 행을 확인하고 값을 공통 입력 규격에 맞게 보완하세요."
            }
            IssueKind::UnsupportedSchema { .. } => "지원하는 공통 입력 규격의 문서를 사용하세요.",
            _ => "원본 위치와 필드 값을 확인한 뒤 다시 실행하세요.",
        };
        Self {
            code: issue.kind.code(),
            severity,
            source: issue.source,
            path: Some(issue.path),
            detail: input_detail(&issue.kind),
            guidance,
        }
    }
}

fn input_detail(kind: &IssueKind) -> String {
    match kind {
        IssueKind::UnsupportedSchema { found } => format!("지원하지 않는 입력 규격 ({found})"),
        IssueKind::Missing => "필수 값이 비어 있음".into(),
        IssueKind::InvalidCode(error) => error.to_string(),
        IssueKind::NumericCode { text } => format!("코드가 숫자로 입력됨 ({text})"),
        IssueKind::InvalidValue { value } => format!("사용할 수 없는 값 ({value})"),
        IssueKind::InvalidRoom(error) => error.to_string(),
        IssueKind::InvalidPeriodTable(error) => error.to_string(),
        IssueKind::Duplicate { key } | IssueKind::DuplicateRow { key, .. } => {
            format!("같은 코드가 여러 번 입력됨 ({key})")
        }
        IssueKind::UnknownReference { key } => format!("참조하는 기록이 없음 ({key})"),
        IssueKind::AmbiguousRoom { text, .. } => format!("강의실을 특정할 수 없음 ({text})"),
        IssueKind::AmbiguousInstructor { name, .. } => format!("동명이인 교원이 있음 ({name})"),
        IssueKind::InstructorNameMismatch { code, name } => {
            format!("교원 코드와 이름이 일치하지 않음 ({code}, {name})")
        }
        IssueKind::AmbiguousConstraint { .. } => "일치하는 조건이 여러 개 있음".into(),
        IssueKind::SplitMismatch { weekly, split_sum } => {
            format!("주당 {weekly}교시와 분할 합계 {split_sum}교시가 다름")
        }
        IssueKind::InvalidSplitPart { periods } => {
            format!("배치 단위의 교시 수가 유효하지 않음 ({periods})")
        }
        IssueKind::AvailableExceedsTotal { available, total } => {
            format!("가용 수량 {available}이 전체 수량 {total}을 초과함")
        }
        IssueKind::ReversedRange => "범위의 시작이 끝보다 늦음".into(),
        IssueKind::AmbiguousWindow => "교시 범위와 시각 범위 중 하나를 입력해야 함".into(),
        IssueKind::TargetNotSingle => "조건 대상은 하나여야 함".into(),
        IssueKind::ConfirmedIncomplete => "확인한 조건의 시간 범위 또는 강도가 비어 있음".into(),
        IssueKind::ExistingScheduleConflict => "기존 배치와 충돌함".into(),
        IssueKind::InvalidReviewTarget => "변경할 기록을 찾을 수 없음".into(),
        IssueKind::MissingSetup => "학기 설정이 비어 있음".into(),
        IssueKind::UnverifiedDependency { row } => format!("참조하는 행이 확인 대기 중 ({row})"),
        IssueKind::PartOutOfRange { parts } => {
            format!("배치 단위가 {parts}개인 분할 범위를 벗어남")
        }
        IssueKind::SpanPastLastPeriod => "배치가 해당 요일의 마지막 교시를 넘음".into(),
        IssueKind::DayNotInTerm => "학기에 등록된 수업 요일이 아님".into(),
        IssueKind::Zero => "0보다 큰 값을 입력해야 함".into(),
        IssueKind::WeightOutOfRange { weight } => {
            format!("선호 가중치가 1~10 범위를 벗어남 ({weight})")
        }
        IssueKind::Unresolved { what, reason } => format!("{what} 확인 필요 ({reason})"),
    }
}

fn sheet_name(key: &str) -> &str {
    if key == INFO_SHEET.key {
        INFO_SHEET.ko
    } else {
        find_sheet(key).map_or(key, |sheet| sheet.ko)
    }
}

#[derive(Serialize)]
struct MeetingLabel {
    course: String,
    name: String,
    section: String,
    part: u16,
}

impl std::fmt::Display for MeetingLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {}분반 {}번째 배치",
            self.course, self.name, self.section, self.part
        )
    }
}

#[derive(Serialize)]
struct PlacementLine {
    meeting: MeetingLabel,
    day: Weekday,
    period: Option<String>,
    end_period: Option<String>,
    span: Option<u8>,
    start: Option<ClockTime>,
    end: Option<ClockTime>,
    room: Option<String>,
    locked: bool,
}

impl PlacementLine {
    fn slot(&self) -> String {
        let day = self.day.ko();
        let first = self.period.as_deref().unwrap_or("-");
        let periods = match self.end_period.as_deref() {
            Some(last) if first != last => format!("{first}~{last}교시"),
            Some(_) => format!("{first}교시"),
            None => first.to_owned(),
        };
        match self.start.zip(self.end) {
            Some((start, end)) => format!("{day}요일 {periods} ({start}~{end})"),
            None => format!("{day}요일 {periods}"),
        }
    }
}

#[derive(Serialize)]
struct ReasonLine {
    code: &'static str,
    level: &'static str,
    detail: String,
    weight: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrences: Option<usize>,
}

#[derive(Serialize)]
struct UnassignedLine {
    meeting: MeetingLabel,
    cause: &'static str,
    candidate_count: Option<usize>,
    checked_against_preview: usize,
    reasons: Vec<ReasonLine>,
}

#[derive(Serialize)]
struct WarningLine {
    meeting: MeetingLabel,
    finding: ReasonLine,
}

#[derive(Serialize)]
struct BlockedLockLine {
    placement: PlacementLine,
    reasons: Vec<ReasonLine>,
}

#[derive(Serialize)]
struct CapacityLine {
    resource_kind: &'static str,
    resource: String,
    needed: usize,
    available: usize,
}

#[derive(Serialize)]
pub struct ResultReport {
    term: TermKey,
    status: &'static str,
    improvement: &'static str,
    elapsed_ms: f64,
    total_meetings: usize,
    placements: Vec<PlacementLine>,
    unassigned: Vec<UnassignedLine>,
    blocked_locks: Vec<BlockedLockLine>,
    capacity_shortfalls: Vec<CapacityLine>,
    preference_cost: u64,
    preferences: Vec<WarningLine>,
    pending_constraints: usize,
}

impl ResultReport {
    pub fn new(data: &TermData, result: &SolveResult) -> Self {
        let mut placements: Vec<_> = result
            .placements
            .iter()
            .map(|p| placement_label(data, p))
            .collect();
        placements.sort_by_key(|row| (row.day, row.start));
        Self {
            term: data.setup.key,
            status: result.status.code(),
            improvement: result.improvement.code(),
            elapsed_ms: result.elapsed.as_secs_f64() * 1000.0,
            total_meetings: data.meetings().count(),
            pending_constraints: data
                .constraints
                .iter()
                .filter(|c| c.status == ConstraintStatus::Pending)
                .count(),
            placements,
            unassigned: result
                .unassigned
                .iter()
                .map(|record| UnassignedLine {
                    meeting: meeting_label(data, record.meeting),
                    cause: record.cause.code(),
                    candidate_count: record.candidate_count,
                    checked_against_preview: record.checked_against_preview,
                    reasons: record
                        .rejections
                        .iter()
                        .map(|reason| ReasonLine {
                            occurrences: Some(reason.occurrences),
                            ..describe(data, &reason.example)
                        })
                        .collect(),
                })
                .collect(),
            blocked_locks: result
                .blocked_locks
                .iter()
                .map(|record| BlockedLockLine {
                    placement: placement_label(data, &record.placement),
                    reasons: record
                        .evaluation
                        .findings
                        .iter()
                        .map(|reason| describe(data, reason))
                        .collect(),
                })
                .collect(),
            capacity_shortfalls: result
                .capacity_shortfalls
                .iter()
                .map(|record| CapacityLine {
                    resource_kind: record.resource.code(),
                    resource: match record.resource {
                        RequiredResource::Instructor(id) => data
                            .instructors
                            .iter()
                            .find(|person| person.id == id)
                            .map(|person| format!("{} ({})", person.name, person.code)),
                        RequiredResource::Group(id) => data
                            .groups
                            .iter()
                            .find(|group| group.id == id)
                            .map(|group| format!("{} ({})", group.name, group.code)),
                    }
                    .unwrap_or_else(|| format!("{:?}", record.resource)),
                    needed: record.needed_periods,
                    available: record.available_periods,
                })
                .collect(),
            preference_cost: result.preferences.cost,
            preferences: result
                .preferences
                .warnings
                .iter()
                .map(|(meeting, finding)| WarningLine {
                    meeting: meeting_label(data, *meeting),
                    finding: describe(data, finding),
                })
                .collect(),
        }
    }
}

fn placement_label(data: &TermData, placement: &Placement) -> PlacementLine {
    let start_period = usize::from(placement.start);
    let span = data.meeting_span(placement.meeting);
    let time = data
        .section(placement.meeting.section)
        .and_then(|section| occupancy(&data.setup, section, placement).ok());
    PlacementLine {
        meeting: meeting_label(data, placement.meeting),
        day: placement.day,
        period: data
            .setup
            .periods
            .get(start_period)
            .map(|period| period.label.clone()),
        end_period: span
            .and_then(|span| span.checked_sub(1))
            .and_then(|offset| data.setup.periods.get(start_period + usize::from(offset)))
            .map(|period| period.label.clone()),
        span,
        start: time.map(|time| time.start),
        end: time.map(|time| time.end),
        room: placement
            .room
            .and_then(|id| data.room(id))
            .map(|room| room.key.to_string()),
        locked: placement.locked,
    }
}

fn meeting_label(data: &TermData, meeting: MeetingRef) -> MeetingLabel {
    let section = data.section(meeting.section);
    let course = section.and_then(|s| data.course(s.course));
    MeetingLabel {
        course: course.map_or_else(|| meeting.section.0.to_string(), |c| c.code.to_string()),
        name: course.map_or_else(String::new, |c| c.name.clone()),
        section: section.map_or_else(String::new, |s| s.code.to_string()),
        part: u16::from(meeting.part) + 1,
    }
}

fn describe(data: &TermData, finding: &Finding) -> ReasonLine {
    let detail = match &finding.reason {
        Reason::UnknownSection(_) => "분반을 찾을 수 없음".into(),
        Reason::Occupancy(OccupancyError::DayOutsideTerm) => "수업 요일에 포함되지 않음".into(),
        Reason::Occupancy(OccupancyError::UnknownPart) => "분반의 배치 단위를 찾을 수 없음".into(),
        Reason::Occupancy(OccupancyError::PeriodSpanOutsideDay) => "마지막 교시를 넘는 배치".into(),
        Reason::Locked(_) | Reason::UnlockRequired(_) => {
            "잠금 해제 후 배치를 변경할 수 있음".into()
        }
        Reason::DuplicateMeeting => "같은 배치 단위가 중복됨".into(),
        Reason::RoomRequired => "강의실이 지정되지 않음".into(),
        Reason::UnknownRoom(_) => "지정된 강의실을 찾을 수 없음".into(),
        Reason::WrongRoomKind { required, actual } => {
            format!(
                "{} 유형 필요, 후보는 {} 유형",
                room_kind(*required),
                room_kind(*actual)
            )
        }
        Reason::NotEnoughSeats { needed, available } => {
            format!("가용 좌석 {available}석, 필요 {needed}석")
        }
        Reason::UnknownEquipment(_) => "필요 장비를 기준자료에서 찾을 수 없음".into(),
        Reason::NotEnoughEquipment {
            equipment,
            needed,
            available,
        } => {
            let name = data
                .equipment_kind(*equipment)
                .map_or("장비", |kind| &kind.name);
            format!("{name} 정상 수량 {available}, 필요 {needed}")
        }
        Reason::EquipmentInspection { equipment, period } => {
            let name = data
                .equipment_kind(*equipment)
                .map_or("장비", |kind| &kind.name);
            format!("{name} 점검 기간 ({}~{})", period.from, period.until)
        }
        Reason::Overlap {
            other,
            resource,
            other_locked,
        } => {
            let resource = match resource {
                Resource::Section(_) => "같은 분반".to_owned(),
                Resource::Instructor(id) => data
                    .instructor(*id)
                    .map_or_else(|| "교원".into(), |person| format!("교원 {}", person.name)),
                Resource::Room(id) => data
                    .room(*id)
                    .map_or_else(|| "강의실".into(), |room| format!("강의실 {}", room.key)),
                Resource::RequiredGroup(id) => data.group(*id).map_or_else(
                    || "필수과목 수강 집단".into(),
                    |group| format!("필수과목 수강 집단 {}", group.name),
                ),
            };
            format!(
                "{resource}의 시간이 {}{}와 겹침",
                meeting_label(data, *other),
                if *other_locked { " [잠김]" } else { "" }
            )
        }
        Reason::RoomTurnaround {
            needed, available, ..
        } => format!("준비·정리 가능 {available}분, 필요 {needed}분"),
        Reason::Constraint { id, target } => {
            let target_label = match target {
                Target::Instructor(id) => data.instructor(*id).map(|person| person.name.clone()),
                Target::Section(id) => data.section(*id).and_then(|section| {
                    data.course(section.course)
                        .map(|course| format!("{} {}분반", course.name, section.code))
                }),
                Target::Group(id) => data.group(*id).map(|group| group.name.clone()),
                Target::Room(id) => data.room(*id).map(|room| room.key.to_string()),
            };
            let mut detail = target_label.map_or_else(
                || "편성 조건과 겹침".into(),
                |label| format!("{label}의 편성 조건과 겹침"),
            );
            if let Some(text) = data
                .constraints
                .iter()
                .find(|condition| condition.id == *id)
                .and_then(|condition| condition.text.as_deref())
                .filter(|text| !text.trim().is_empty())
            {
                detail.push_str(&format!(" ({text})"));
            }
            detail
        }
        Reason::ExternalRoomSchedule(id) => data.room(*id).map_or_else(
            || "공용실 외부 일정 확인 필요".into(),
            |room| format!("공용실 {}의 외부 일정 확인 필요", room.key),
        ),
        Reason::ProvisionalEnrollment(expected) => format!("수강 인원 {expected}명은 잠정 값"),
        Reason::CrossesBreak(breaks) => format!(
            "휴게시간을 가로지름 ({})",
            breaks
                .iter()
                .map(|b| format!("{} {}~{}", b.label, b.start, b.end))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    ReasonLine {
        code: finding.reason.code(),
        level: finding.level.code(),
        detail,
        weight: finding.weight,
        occurrences: None,
    }
}

fn room_kind(kind: RoomKind) -> &'static str {
    match kind {
        RoomKind::Lecture => "일반 강의실",
        RoomKind::Lab => "실습실",
        RoomKind::Seminar => "세미나실",
        RoomKind::Other => "기타",
    }
}

fn write_json(mut target: impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut target, value).map_err(|error| {
        io::Error::new(error.io_error_kind().unwrap_or(io::ErrorKind::Other), error)
    })?;
    writeln!(target)
}

pub fn write_errors(status: &str, issues: &[Diagnostic], json: bool) -> io::Result<()> {
    if json {
        return write_json(
            io::stdout().lock(),
            &serde_json::json!({ "status": status, "diagnostics": issues }),
        );
    }
    let mut out = io::stderr().lock();
    for issue in issues {
        let severity = if issue.severity == "needs_review" {
            "확인 필요"
        } else {
            "오류"
        };
        let location = issue
            .source
            .as_ref()
            .map(|source| {
                let file = source.file.as_deref().unwrap_or_default();
                let sheet = source.sheet.as_deref().unwrap_or_default();
                let col = source.column.as_deref().unwrap_or_default();
                let row = source.row.map_or_else(String::new, |row| row.to_string());
                [file, sheet, &format!("{col}{row}")]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .unwrap_or_default();
        let description = [
            location.as_str(),
            issue.path.as_deref().unwrap_or_default(),
            &issue.detail,
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        writeln!(out, "[{severity}] {description} ({})", issue.code)?;
        writeln!(out, "  {}", issue.guidance)?;
    }
    Ok(())
}

pub fn write_result(report: &ResultReport, json: bool) -> io::Result<()> {
    if json {
        return write_json(io::stdout().lock(), report);
    }
    let mut out = io::stdout().lock();
    let status = match report.status {
        "solved" => "시간표 초안 생성 완료",
        "no_solution" => "현재 조건에서 전체 편성 불가",
        "timed_out" => "제한시간 안에 전체 시간표를 찾지 못함",
        _ => "취소됨",
    };
    writeln!(
        out,
        "{}  {}  배치 {}/{}  {:.2}ms",
        report.term,
        status,
        report.placements.len(),
        report.total_meetings,
        report.elapsed_ms
    )?;
    match report.status {
        "no_solution" => writeln!(
            out,
            "아래 미배정 사유와 기존 잠금을 확인하고 편성 조건을 조정하세요."
        )?,
        "timed_out" => writeln!(out, "--time-limit-ms 값을 늘려 다시 실행할 수 있습니다.")?,
        _ => {}
    }
    if report.status != "solved" && !report.placements.is_empty() {
        writeln!(out, "부분 배치 미리보기")?;
    }
    match report.improvement {
        "timed_out" => writeln!(
            out,
            "유효한 시간표를 찾았습니다. 선호 개선은 제한시간에 종료했습니다."
        )?,
        "cancelled" => writeln!(out, "유효한 시간표를 찾았습니다. 선호 개선을 취소했습니다.")?,
        _ => {}
    }
    for row in &report.placements {
        writeln!(
            out,
            "  {}  {}  {}{}",
            row.meeting,
            row.slot(),
            row.room.as_deref().unwrap_or("-"),
            if row.locked { "  [잠김]" } else { "" }
        )?;
    }
    for row in &report.blocked_locks {
        let p = &row.placement;
        writeln!(
            out,
            "잠금 재검토  {}  {}  {}",
            p.meeting,
            p.slot(),
            p.room.as_deref().unwrap_or("강의실 미지정")
        )?;
        for reason in row
            .reasons
            .iter()
            .filter(|reason| matches!(reason.level, "hard" | "external_check"))
        {
            writeln!(out, "  {}", reason.detail)?;
        }
    }
    for row in &report.capacity_shortfalls {
        writeln!(
            out,
            "가용시간 부족  {}  필요 {}교시  가능 {}교시",
            row.resource, row.needed, row.available
        )?;
    }
    for row in &report.unassigned {
        let cause = match row.cause {
            "no_candidate" => "유효한 후보 없음",
            "conflicts" => "다른 배치와 충돌",
            "blocked_by_locks" => "기존 잠금 배치 재검토 필요",
            "time_limit" => "제한시간 내 미배정",
            _ => "편성 취소",
        };
        writeln!(out, "미배정  {}  {cause}", row.meeting)?;
        if let Some(count) = row.candidate_count {
            writeln!(
                out,
                "  기본 검사 통과 후보 {count}개, 미리보기와 대조한 후보 {}개",
                row.checked_against_preview
            )?;
        }
        for reason in &row.reasons {
            let occurrences = reason
                .occurrences
                .map_or_else(String::new, |count| format!(" (관찰 {count}건)"));
            writeln!(out, "  {}{occurrences}", reason.detail)?;
        }
    }
    if !report.placements.is_empty() {
        writeln!(out, "선호 위반 가중치 합계 {}", report.preference_cost)?;
    }
    for row in &report.preferences {
        writeln!(out, "선호 경고  {}  {}", row.meeting, row.finding.detail)?;
    }
    if report.pending_constraints > 0 {
        writeln!(out, "확인 대기 조건 {}건", report.pending_constraints)?;
    }
    Ok(())
}
