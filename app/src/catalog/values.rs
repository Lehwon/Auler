use auler_domain::code::*;
use auler_domain::model::*;
use auler_domain::template;
use auler_domain::time::{Break, ClockTime, Date, Period, PeriodTable, PeriodTableError};
use auler_engine::ScheduleAudit;
use auler_store::{Op, Record, RecordRef};

use crate::Labels;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Section,
    Course,
    Instructor,
    Group,
    Room,
    Equipment,
    Period,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Key {
    Record(RecordRef),
    Period(usize),
    Break(usize),
}

pub(super) struct Item {
    pub key: Key,
    pub title: String,
    pub cells: Vec<String>,
    pub fields: Vec<String>,
    pub status: i32,
    pub provisional: bool,
}

impl Kind {
    pub fn from_view(tab: i32, sub: i32) -> Self {
        match (tab, sub) {
            (0, 1) => Self::Course,
            (1, _) => Self::Instructor,
            (2, _) => Self::Group,
            (3, 0) => Self::Room,
            (3, _) => Self::Equipment,
            (4, 0) => Self::Period,
            (4, _) => Self::Break,
            _ => Self::Section,
        }
    }

    pub fn field_keys(self) -> &'static [&'static str] {
        match self {
            Self::Section => &[
                "course_code",
                "section",
                "instructor_codes",
                "group_codes",
                "students",
                "provisional",
                "split",
                "class_kind",
                "room_kind",
                "equipment_needs",
            ],
            Self::Course => &["course_code", "course"],
            Self::Instructor => &["code", "name", "department"],
            Self::Group => &["code", "name", "year", "track"],
            Self::Room => &[
                "building",
                "room_number",
                "room_kind",
                "department",
                "total_seats",
                "available_seats",
                "shared",
                "confirmed_date",
                "source",
                "room_equipment",
            ],
            Self::Equipment => &["code", "name", "equipment_scope"],
            Self::Period | Self::Break => &["label", "start", "end"],
        }
    }

    pub fn column_keys(self) -> &'static [&'static str] {
        match self {
            Self::Section => &[
                "course_code",
                "section",
                "course",
                "instructor",
                "year",
                "students",
                "hours",
                "status",
            ],
            Self::Room => &[
                "building",
                "room_number",
                "room_kind",
                "department",
                "total_seats",
                "available_seats",
                "shared",
            ],
            _ => self.field_keys(),
        }
    }

    pub fn default_fields(self, korean: bool) -> Vec<String> {
        let no = boolean(false, korean);
        let lecture = local_enum(template::CLASS_KINDS, "lecture", korean);
        let lecture_room = local_enum(template::ROOM_KINDS, "lecture", korean);
        let per_room = local_enum(template::EQUIPMENT_SCOPES, "per_room", korean);
        (match self {
            Self::Section => vec!["", "01", "", "", "0", no, "1", lecture, "", ""],
            Self::Room => vec!["", "", lecture_room, "", "0", "0", no, "", "", ""],
            Self::Equipment => vec!["", "", per_room],
            _ => vec![""; self.field_keys().len()],
        })
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    pub fn bulk_field(self) -> Option<usize> {
        match self {
            Self::Section => Some(4),
            Self::Course => Some(1),
            Self::Instructor => Some(2),
            Self::Group => Some(2),
            Self::Room => Some(5),
            Self::Equipment => Some(2),
            _ => None,
        }
    }
}

pub(super) fn readable_fields(data: &TermData, item: &Item, korean: bool) -> Vec<String> {
    let mut result = item.fields.clone();
    if let Key::Record(RecordRef::Section(id)) = item.key
        && let Some(section) = data.section(id)
    {
        result[2] = section
            .instructors
            .iter()
            .filter_map(|&id| data.instructor(id))
            .map(|person| format!("{} ({})", person.name, person.code))
            .collect::<Vec<_>>()
            .join(", ");
        result[3] = section
            .groups
            .iter()
            .filter_map(|link| {
                data.group(link.group).map(|group| {
                    format!(
                        "{} ({}){}",
                        group.name,
                        group.code,
                        if link.required {
                            if korean { " · 필수" } else { " · Required" }
                        } else {
                            ""
                        }
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
        result[9] = section
            .equipment
            .iter()
            .filter_map(|need| {
                data.equipment_kind(need.equipment)
                    .map(|equipment| format!("{} × {}", equipment.name, need.quantity))
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    result
}

fn item(key: RecordRef, title: String, fields: Vec<String>) -> Item {
    Item {
        key: Key::Record(key),
        title,
        cells: fields.clone(),
        fields,
        status: 0,
        provisional: false,
    }
}

pub(super) fn items(
    data: &TermData,
    audit: &ScheduleAudit,
    kind: Kind,
    labels: &Labels,
    korean: bool,
) -> Vec<Item> {
    match kind {
        Kind::Section => data
            .sections
            .iter()
            .filter_map(|section| {
                let course = data.course(section.course)?;
                let people: Vec<_> = section
                    .instructors
                    .iter()
                    .filter_map(|&id| data.instructor(id))
                    .collect();
                let groups: Vec<_> = section
                    .groups
                    .iter()
                    .filter_map(|link| data.group(link.group).map(|group| (link, group)))
                    .collect();
                let unassigned = audit
                    .unassigned
                    .iter()
                    .any(|meeting| meeting.section == section.id)
                    || audit
                        .missing_rooms
                        .iter()
                        .any(|entry| entry.placement.meeting.section == section.id);
                let split = section
                    .split
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join("+");
                let fields = vec![
                    course.code.to_string(),
                    section.code.to_string(),
                    people
                        .iter()
                        .map(|person| list_code(person.code.as_str()))
                        .collect::<Vec<_>>()
                        .join("; "),
                    groups
                        .iter()
                        .map(|(link, group)| {
                            format!(
                                "{} {}",
                                list_code(group.code.as_str()),
                                boolean(link.required, korean)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                    section.enrollment.count.to_string(),
                    boolean(section.enrollment.provisional, korean).into(),
                    split.clone(),
                    local_enum(template::CLASS_KINDS, class_code(section.kind), korean).into(),
                    local_enum(
                        template::ROOM_KINDS,
                        section.room_kind.map_or("", room_code),
                        korean,
                    )
                    .into(),
                    section
                        .equipment
                        .iter()
                        .filter_map(|need| {
                            data.equipment_kind(need.equipment).map(|equipment| {
                                format!("{} {}", list_code(equipment.code.as_str()), need.quantity)
                            })
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                ];
                let cells = vec![
                    course.code.to_string(),
                    section.code.to_string(),
                    course.name.clone(),
                    people
                        .iter()
                        .map(|person| person.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    groups
                        .iter()
                        .filter_map(|(_, group)| group.grade)
                        .map(|grade| grade.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    section.enrollment.count.to_string(),
                    if section.split.len() > 1 {
                        format!("{} ({split})", section.weekly_periods())
                    } else {
                        split
                    },
                    if unassigned {
                        labels.get_status_unassigned()
                    } else {
                        labels.get_status_assigned()
                    }
                    .to_string(),
                ];
                Some(Item {
                    key: Key::Record(RecordRef::Section(section.id)),
                    title: format!("{} {}", course.name, section.code),
                    cells,
                    fields,
                    status: if unassigned { 2 } else { 1 },
                    provisional: section.enrollment.provisional,
                })
            })
            .collect(),
        Kind::Course => data
            .courses
            .iter()
            .map(|record| {
                item(
                    RecordRef::Course(record.id),
                    record.name.clone(),
                    vec![record.code.to_string(), record.name.clone()],
                )
            })
            .collect(),
        Kind::Instructor => data
            .instructors
            .iter()
            .map(|record| {
                item(
                    RecordRef::Instructor(record.id),
                    record.name.clone(),
                    vec![
                        record.code.to_string(),
                        record.name.clone(),
                        record.department.clone().unwrap_or_default(),
                    ],
                )
            })
            .collect(),
        Kind::Group => data
            .groups
            .iter()
            .map(|record| {
                item(
                    RecordRef::Group(record.id),
                    record.name.clone(),
                    vec![
                        record.code.to_string(),
                        record.name.clone(),
                        record
                            .grade
                            .map(|grade| grade.to_string())
                            .unwrap_or_default(),
                        record.track.clone().unwrap_or_default(),
                    ],
                )
            })
            .collect(),
        Kind::Room => data
            .rooms
            .iter()
            .map(|record| {
                let fields = vec![
                    record.key.building.clone(),
                    record.key.number.clone(),
                    local_enum(template::ROOM_KINDS, room_code(record.kind), korean).into(),
                    record.department.clone().unwrap_or_default(),
                    record.total_seats.to_string(),
                    record.available_seats.to_string(),
                    boolean(record.shared, korean).into(),
                    record
                        .schedule_checked
                        .map(|date| date.to_string())
                        .unwrap_or_default(),
                    record.info_source.clone().unwrap_or_default(),
                    record
                        .equipment
                        .iter()
                        .filter_map(|entry| {
                            let gear = data.equipment_kind(entry.equipment)?;
                            let mut text = format!(
                                "{} {} {}",
                                list_code(gear.code.as_str()),
                                entry.owned,
                                entry.available
                            );
                            if let Some(dates) = entry.inspection {
                                text.push_str(&format!(" {} {}", dates.from, dates.until));
                            }
                            Some(text)
                        })
                        .collect::<Vec<_>>()
                        .join("; "),
                ];
                let mut result = item(RecordRef::Room(record.id), record.key.to_string(), fields);
                result.cells.truncate(7);
                result
            })
            .collect(),
        Kind::Equipment => data
            .equipment
            .iter()
            .map(|record| {
                item(
                    RecordRef::Equipment(record.id),
                    record.name.clone(),
                    vec![
                        record.code.to_string(),
                        record.name.clone(),
                        local_enum(
                            template::EQUIPMENT_SCOPES,
                            if record.scope == EquipmentScope::PerPerson {
                                "per_person"
                            } else {
                                "per_room"
                            },
                            korean,
                        )
                        .into(),
                    ],
                )
            })
            .collect(),
        Kind::Period => data
            .setup
            .periods
            .periods()
            .iter()
            .enumerate()
            .map(|(index, record)| {
                time_item(Key::Period(index), &record.label, record.start, record.end)
            })
            .collect(),
        Kind::Break => data
            .setup
            .periods
            .breaks()
            .iter()
            .enumerate()
            .map(|(index, record)| {
                time_item(Key::Break(index), &record.label, record.start, record.end)
            })
            .collect(),
    }
}

fn time_item(key: Key, label: &str, start: ClockTime, end: ClockTime) -> Item {
    let fields = vec![label.to_owned(), start.to_string(), end.to_string()];
    Item {
        key,
        title: label.to_owned(),
        cells: fields.clone(),
        fields,
        status: 0,
        provisional: false,
    }
}

pub(super) enum Problem {
    Field(usize),
    Reference(usize),
    Seats(u32),
    EndBeforeStart,
    PeriodTable(PeriodTableError),
    CalendarInUse,
    TooManyRecords,
}

impl Problem {
    pub fn field(&self) -> Option<usize> {
        match self {
            Self::Field(index) | Self::Reference(index) => Some(*index),
            Self::Seats(_) => Some(5),
            Self::EndBeforeStart => Some(2),
            Self::PeriodTable(
                PeriodTableError::DuplicateLabel(_) | PeriodTableError::EmptyLabel(_),
            ) => Some(0),
            Self::PeriodTable(PeriodTableError::Empty)
            | Self::CalendarInUse
            | Self::TooManyRecords => None,
            Self::PeriodTable(_) => Some(2),
        }
    }
}

pub(super) fn edit(
    data: &TermData,
    kind: Kind,
    key: Option<Key>,
    fields: &[String],
    next_section: Option<SectionId>,
) -> Result<Op, Problem> {
    let field = |index: usize| fields.get(index).map(|text| text.trim()).unwrap_or("");
    let required = |index| {
        if field(index).is_empty() {
            Err(Problem::Field(index))
        } else {
            Ok(field(index).to_owned())
        }
    };
    let optional = |index| (!field(index).is_empty()).then(|| field(index).to_owned());
    let part = |index| {
        field(index)
            .parse::<u32>()
            .map_err(|_| Problem::Field(index))
    };
    let code = |index| normalize_code(field(index)).map_err(|_| Problem::Field(index));
    let date = |index| {
        optional(index)
            .map(|value| Date::parse(&value).ok_or(Problem::Field(index)))
            .transpose()
    };
    let yes = |index| parse_bool(field(index)).ok_or(Problem::Field(index));
    let existing = match key {
        Some(Key::Record(record)) => Some(id_value(record)),
        _ => None,
    };
    let id = |last: Option<i64>| {
        existing.map(Ok).unwrap_or_else(|| {
            last.unwrap_or(0)
                .checked_add(1)
                .ok_or(Problem::TooManyRecords)
        })
    };
    let record = match kind {
        Kind::Course => Record::Course(Course {
            id: CourseId(id(data.courses.iter().map(|record| record.id.0).max())?),
            code: CourseCode::try_from(code(0)?).map_err(|_| Problem::Field(0))?,
            name: required(1)?,
        }),
        Kind::Instructor => Record::Instructor(Instructor {
            id: InstructorId(id(data.instructors.iter().map(|record| record.id.0).max())?),
            code: InstructorCode::try_from(code(0)?).map_err(|_| Problem::Field(0))?,
            name: required(1)?,
            department: optional(2),
        }),
        Kind::Group => Record::Group(StudentGroup {
            id: GroupId(id(data.groups.iter().map(|record| record.id.0).max())?),
            code: GroupCode::try_from(code(0)?).map_err(|_| Problem::Field(0))?,
            name: required(1)?,
            grade: optional(2)
                .map(|_| {
                    field(2)
                        .parse::<u8>()
                        .ok()
                        .filter(|grade| *grade > 0)
                        .ok_or(Problem::Field(2))
                })
                .transpose()?,
            track: optional(3),
        }),
        Kind::Equipment => Record::Equipment(EquipmentKind {
            id: EquipmentId(id(data.equipment.iter().map(|record| record.id.0).max())?),
            code: EquipmentCode::try_from(code(0)?).map_err(|_| Problem::Field(0))?,
            name: required(1)?,
            scope: match enum_key(template::EQUIPMENT_SCOPES, field(2)) {
                Some("per_person") => EquipmentScope::PerPerson,
                Some("per_room") => EquipmentScope::PerRoom,
                _ => return Err(Problem::Field(2)),
            },
        }),
        Kind::Room => {
            let total_seats = part(4)?;
            let available_seats = part(5)?;
            if available_seats > total_seats {
                return Err(Problem::Seats(total_seats));
            }
            let equipment = each(field(9))
                .map(|entry| parse_room_equipment(data, &entry))
                .collect::<Result<Vec<_>, _>>()?;
            Record::Room(Room {
                id: RoomId(id(data.rooms.iter().map(|record| record.id.0).max())?),
                key: RoomKey::new(&required(0)?, field(1)).map_err(|_| Problem::Field(1))?,
                kind: parse_room_kind(field(2)).ok_or(Problem::Field(2))?,
                department: optional(3),
                total_seats,
                available_seats,
                equipment,
                shared: yes(6)?,
                schedule_checked: date(7)?,
                info_source: optional(8),
            })
        }
        Kind::Section => {
            let course_code = code(0)?;
            let course = data
                .courses
                .iter()
                .find(|item| item.code.as_str() == course_code)
                .ok_or(Problem::Reference(0))?;
            let instructors = each(field(2))
                .map(|entry| {
                    data.instructors
                        .iter()
                        .find(|item| item.code.as_str() == entry)
                        .map(|item| item.id)
                        .ok_or(Problem::Reference(2))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let groups = each(field(3))
                .map(|entry| {
                    let mut fields = entry.split_whitespace();
                    let code = fields.next().ok_or(Problem::Field(3))?;
                    let required = fields
                        .next()
                        .map(|value| parse_bool(value).ok_or(Problem::Field(3)))
                        .transpose()?
                        .unwrap_or(false);
                    let group = data
                        .groups
                        .iter()
                        .find(|item| item.code.as_str() == code)
                        .ok_or(Problem::Reference(3))?;
                    if fields.next().is_some() {
                        return Err(Problem::Field(3));
                    }
                    Ok(GroupLink {
                        group: group.id,
                        required,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let split = field(6)
                .split('+')
                .map(|entry| {
                    entry
                        .trim()
                        .parse::<u8>()
                        .ok()
                        .filter(|span| *span > 0 && usize::from(*span) <= data.setup.periods.len())
                        .ok_or(Problem::Field(6))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let kind = match enum_key(template::CLASS_KINDS, field(7)) {
                Some("lecture") => ClassKind::Lecture,
                Some("lab") => ClassKind::Lab,
                Some("mixed") => ClassKind::Mixed,
                _ => return Err(Problem::Field(7)),
            };
            let equipment = each(field(9))
                .map(|entry| {
                    let mut fields = entry.split_whitespace();
                    let equipment = find_equipment(data, fields.next().ok_or(Problem::Field(9))?)
                        .ok_or(Problem::Reference(9))?;
                    let quantity = fields
                        .next()
                        .and_then(|value| value.parse::<u32>().ok())
                        .filter(|value| *value > 0)
                        .ok_or(Problem::Field(9))?;
                    if fields.next().is_some() {
                        return Err(Problem::Field(9));
                    }
                    Ok(EquipmentNeed {
                        equipment,
                        quantity,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Record::Section(Section {
                id: existing
                    .map(SectionId)
                    .or(next_section)
                    .ok_or(Problem::TooManyRecords)?,
                course: course.id,
                code: SectionCode::try_from(code(1)?).map_err(|_| Problem::Field(1))?,
                instructors,
                groups,
                enrollment: Enrollment {
                    count: part(4)?,
                    provisional: yes(5)?,
                },
                kind,
                split,
                room_kind: optional(8)
                    .map(|_| parse_room_kind(field(8)).ok_or(Problem::Field(8)))
                    .transpose()?,
                equipment,
            })
        }
        Kind::Period | Kind::Break => {
            let label = required(0)?;
            let start = ClockTime::parse(field(1)).ok_or(Problem::Field(1))?;
            let end = ClockTime::parse(field(2)).ok_or(Problem::Field(2))?;
            if end <= start {
                return Err(Problem::EndBeforeStart);
            }
            return calendar(data, kind, key, Some((label, start, end)));
        }
    };
    Ok(Op::Put(record))
}

pub(super) fn remove(data: &TermData, kind: Kind, key: Key) -> Result<Op, Problem> {
    match key {
        Key::Record(record) => Ok(Op::Remove(record)),
        _ => calendar(data, kind, Some(key), None),
    }
}

fn calendar(
    data: &TermData,
    kind: Kind,
    key: Option<Key>,
    value: Option<(String, ClockTime, ClockTime)>,
) -> Result<Op, Problem> {
    let mut periods = data.setup.periods.periods().to_vec();
    let mut breaks = data.setup.periods.breaks().to_vec();
    if kind == Kind::Period {
        match (key, value) {
            (Some(Key::Period(index)), Some((label, start, end))) => {
                periods[index] = Period { label, start, end }
            }
            (None, Some((label, start, end))) => periods.push(Period { label, start, end }),
            (Some(Key::Period(index)), None) => {
                // Removing a middle slot changes the meaning of stored period indices.
                let references = data.placements.iter().any(|placement| {
                    usize::from(placement.start) + usize::from(data.meeting_span(placement.meeting).unwrap_or(1)) > index
                }) || data.constraints.iter().any(|constraint| {
                    matches!(&constraint.rule, Some(Rule::AvoidTimes { windows }) if windows.iter().any(|window| matches!(window, TimeWindow::Periods { last, .. } if usize::from(*last) >= index)))
                });
                if references {
                    return Err(Problem::CalendarInUse);
                }
                periods.remove(index);
            }
            _ => return Err(Problem::Field(0)),
        }
    } else {
        match (key, value) {
            (Some(Key::Break(index)), Some((label, start, end))) => {
                breaks[index] = Break { label, start, end }
            }
            (None, Some((label, start, end))) => breaks.push(Break { label, start, end }),
            (Some(Key::Break(index)), None) => {
                breaks.remove(index);
            }
            _ => return Err(Problem::Field(0)),
        }
    }
    let mut setup = data.setup.clone();
    setup.periods = PeriodTable::new(periods, breaks).map_err(Problem::PeriodTable)?;
    Ok(Op::SetSetup(setup))
}

fn id_value(reference: RecordRef) -> i64 {
    match reference {
        RecordRef::Course(id) => id.0,
        RecordRef::Section(id) => id.0,
        RecordRef::Instructor(id) => id.0,
        RecordRef::Group(id) => id.0,
        RecordRef::Room(id) => id.0,
        RecordRef::Equipment(id) => id.0,
        RecordRef::Constraint(id) => id.0,
        RecordRef::Placement(meeting) => meeting.section.0,
    }
}

fn boolean(value: bool, korean: bool) -> &'static str {
    match (value, korean) {
        (true, true) => "예",
        (false, true) => "아니오",
        (true, false) => "Yes",
        (false, false) => "No",
    }
}

fn local_enum(table: &[template::EnumValue], key: &'static str, korean: bool) -> &'static str {
    table
        .iter()
        .find(|option| option.key == key)
        .map_or(key, |option| if korean { option.ko } else { option.en })
}

fn parse_bool(value: &str) -> Option<bool> {
    let value = value.to_ascii_lowercase();
    if template::TRUE_WORDS.contains(&value.as_str()) {
        Some(true)
    } else if template::FALSE_WORDS.contains(&value.as_str()) {
        Some(false)
    } else {
        None
    }
}

fn class_code(kind: ClassKind) -> &'static str {
    match kind {
        ClassKind::Lecture => "lecture",
        ClassKind::Lab => "lab",
        ClassKind::Mixed => "mixed",
    }
}

fn room_code(kind: RoomKind) -> &'static str {
    match kind {
        RoomKind::Lecture => "lecture",
        RoomKind::Lab => "lab",
        RoomKind::Seminar => "seminar",
        RoomKind::Other => "other",
    }
}

fn parse_room_kind(value: &str) -> Option<RoomKind> {
    match enum_key(template::ROOM_KINDS, value) {
        Some("lecture") => Some(RoomKind::Lecture),
        Some("lab") => Some(RoomKind::Lab),
        Some("seminar") => Some(RoomKind::Seminar),
        Some("other") => Some(RoomKind::Other),
        _ => None,
    }
}

fn enum_key(table: &[template::EnumValue], text: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|option| option.matches(text))
        .map(|option| option.key)
}

fn list_code(code: &str) -> String {
    code.replace('\\', "\\\\").replace(';', "\\;")
}

fn each(value: &str) -> impl Iterator<Item = String> {
    let mut entries = Vec::new();
    let mut entry = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ';' => entries.push(std::mem::take(&mut entry)),
            '\\' if matches!(chars.peek(), Some('\\' | ';')) => {
                entry.push(chars.next().expect("escape character was peeked"));
            }
            _ => entry.push(ch),
        }
    }
    entries.push(entry);
    entries
        .into_iter()
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
}

fn find_equipment(data: &TermData, code: &str) -> Option<EquipmentId> {
    data.equipment
        .iter()
        .find(|item| item.code.as_str() == code)
        .map(|item| item.id)
}

fn parse_room_equipment(data: &TermData, row: &str) -> Result<RoomEquipment, Problem> {
    let values: Vec<_> = row.split_whitespace().collect();
    let [code, owned, available, rest @ ..] = values.as_slice() else {
        return Err(Problem::Field(9));
    };
    let equipment = find_equipment(data, code).ok_or(Problem::Reference(9))?;
    let owned: u32 = owned.parse().map_err(|_| Problem::Field(9))?;
    let available: u32 = available.parse().map_err(|_| Problem::Field(9))?;
    if available > owned {
        return Err(Problem::Field(9));
    }
    let inspection = match rest {
        [] => None,
        [from, until] => {
            let from = Date::parse(from).ok_or(Problem::Field(9))?;
            let until = Date::parse(until).ok_or(Problem::Field(9))?;
            if from > until {
                return Err(Problem::Field(9));
            }
            Some(DateRange { from, until })
        }
        _ => return Err(Problem::Field(9)),
    };
    Ok(RoomEquipment {
        equipment,
        owned,
        available,
        inspection,
    })
}

#[cfg(test)]
mod tests {
    use super::{each, list_code};

    #[test]
    fn reference_lists_preserve_existing_codes_with_delimiters() {
        let codes = ["P;01", r"GROUP\X", r"EQ\;02"];
        let joined = codes.map(list_code).join("; ");
        let parsed: Vec<_> = each(&joined).collect();
        assert_eq!(parsed, codes);
    }
}
