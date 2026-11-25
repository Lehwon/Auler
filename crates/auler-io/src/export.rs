//! Timetable output from an immutable, stored finalization.

use std::collections::BTreeSet;

use auler_domain::model::{
    ConstraintId, Course, InstructorId, Placement, Room, RoomId, Rule, Section, Strength, Target,
    TermData, TermKey, TermSeason, TimeWindow,
};
use auler_domain::time::{Period, Weekday};
use auler_store::{FinalizedInfo, StoreError, Workspace};
use rust_xlsxwriter::{Color, Format, FormatAlign, Workbook, XlsxError};
use serde::Serialize;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Ko,
    En,
}

impl Language {
    pub fn choose<T>(self, ko: T, en: T) -> T {
        match self {
            Self::Ko => ko,
            Self::En => en,
        }
    }

    pub fn day(self, day: Weekday) -> &'static str {
        let en = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        self.choose(day.ko(), en[day.index()])
    }

    pub fn term(self, term: TermKey) -> String {
        let (ko, en) = match term.season {
            TermSeason::First => ("1학기", "Spring"),
            TermSeason::Second => ("2학기", "Fall"),
            TermSeason::Summer => ("여름학기", "Summer"),
            TermSeason::Winter => ("겨울학기", "Winter"),
        };
        self.choose(
            format!("{}학년도 {ko}", term.year),
            format!("{en} {}", term.year),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum View {
    #[default]
    All,
    Year(Option<u8>),
    Instructor(InstructorId),
    Room(RoomId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    All,
    Year,
    Instructor,
    Room,
}

#[derive(Clone, Copy)]
pub enum Layout {
    Grid,
    List,
}

#[derive(Clone, Copy)]
pub struct Fields {
    pub course_name: bool,
    pub instructors: bool,
    pub room: bool,
}

impl Default for Fields {
    fn default() -> Self {
        Self {
            course_name: true,
            instructors: true,
            room: true,
        }
    }
}

pub struct Snapshot {
    data: TermData,
    department: String,
    info: FinalizedInfo,
}

#[derive(Clone, Copy)]
pub struct Meeting<'a> {
    pub placement: &'a Placement,
    pub section: &'a Section,
    pub course: &'a Course,
    pub room: &'a Room,
    pub first: &'a Period,
    pub last: &'a Period,
    pub span: u8,
}

pub struct Table {
    pub headings: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub struct HandoverChoice {
    pub id: ConstraintId,
    pub title: String,
    pub detail: String,
}

impl Snapshot {
    pub fn load(workspace: &Workspace, term: TermKey, version: u32) -> auler_store::Result<Self> {
        let data = workspace.load_finalized(term, version)?;
        let info = workspace
            .finalized(term)?
            .into_iter()
            .find(|item| item.version == version)
            .ok_or_else(|| StoreError::Invalid("Finalized version is unavailable".into()))?;
        let snapshot = Self {
            data,
            department: workspace.department()?,
            info,
        };
        let placed: BTreeSet<_> = snapshot
            .data
            .placements
            .iter()
            .map(|placement| placement.meeting)
            .collect();
        if placed.is_empty()
            || placed.len() != snapshot.data.placements.len()
            || !placed
                .iter()
                .copied()
                .eq(snapshot.data.meetings().collect::<BTreeSet<_>>())
            || snapshot
                .data
                .placements
                .iter()
                .any(|placement| snapshot.meeting(placement).is_none())
        {
            return Err(StoreError::Invalid(
                "Finalized timetable has incomplete placements".into(),
            ));
        }
        Ok(snapshot)
    }

    fn meeting<'a>(&'a self, placement: &'a Placement) -> Option<Meeting<'a>> {
        let section = self.data.section(placement.meeting.section)?;
        let course = self.data.course(section.course)?;
        let room = self.data.room(placement.room?)?;
        let span = self.data.meeting_span(placement.meeting)?;
        let start = usize::from(placement.start);
        let first = self.data.setup.periods.get(start)?;
        let last = self
            .data
            .setup
            .periods
            .get(start + usize::from(span.checked_sub(1)?))?;
        self.data
            .setup
            .days
            .contains(&placement.day)
            .then_some(Meeting {
                placement,
                section,
                course,
                room,
                first,
                last,
                span,
            })
    }

    pub fn info(&self) -> &FinalizedInfo {
        &self.info
    }

    pub fn department(&self) -> &str {
        &self.department
    }

    pub fn meetings(&self, view: View) -> Vec<Meeting<'_>> {
        let mut meetings = self
            .data
            .placements
            .iter()
            .filter_map(|placement| self.meeting(placement))
            .filter(|meeting| match view {
                View::All => true,
                View::Year(grade) => self.years(meeting.section).contains(&grade),
                View::Instructor(id) => meeting.section.instructors.contains(&id),
                View::Room(id) => meeting.room.id == id,
            })
            .collect::<Vec<_>>();
        meetings.sort_by_key(|item| {
            (
                item.placement.day,
                item.placement.start,
                &item.course.code,
                &item.section.code,
                item.placement.meeting.part,
            )
        });
        meetings
    }

    fn years(&self, section: &Section) -> BTreeSet<Option<u8>> {
        let grades = section
            .groups
            .iter()
            .filter_map(|link| self.data.group(link.group))
            .filter_map(|group| group.grade)
            .map(Some)
            .collect::<BTreeSet<_>>();
        if grades.is_empty() {
            BTreeSet::from([None])
        } else {
            grades
        }
    }

    pub fn views(&self, grouping: Grouping) -> Vec<View> {
        let mut choices = match grouping {
            Grouping::All => return vec![View::All],
            Grouping::Year => self
                .data
                .sections
                .iter()
                .flat_map(|section| self.years(section))
                .map(View::Year)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
            Grouping::Instructor => self
                .data
                .instructors
                .iter()
                .map(|person| View::Instructor(person.id))
                .collect(),
            Grouping::Room => self
                .data
                .rooms
                .iter()
                .map(|room| View::Room(room.id))
                .collect(),
        };
        match grouping {
            Grouping::Year => choices.sort_by_key(|view| match *view {
                View::Year(Some(grade)) => u16::from(grade),
                _ => u16::MAX,
            }),
            _ => choices.sort_by_cached_key(|view| self.view_label(*view, Language::En)),
        }
        choices
    }

    pub fn view_label(&self, view: View, language: Language) -> String {
        match view {
            View::All => language.choose("전체 분반", "All sections").into(),
            View::Year(None) => language.choose("학년 미지정", "No year assigned").into(),
            View::Year(Some(grade)) => {
                language.choose(format!("{grade}학년"), format!("Year {grade}"))
            }
            View::Instructor(id) => self
                .data
                .instructor(id)
                .map(|person| format!("{} · {}", person.name, person.code))
                .unwrap_or_default(),
            View::Room(id) => self
                .data
                .room(id)
                .map(|room| room.key.to_string())
                .unwrap_or_default(),
        }
    }

    pub fn version_label(&self, language: Language) -> String {
        let version = self.info.version;
        language.choose(
            format!("확정본 v{version}"),
            format!("Final version {version}"),
        )
    }

    pub fn instructors(&self, meeting: Meeting<'_>) -> String {
        meeting
            .section
            .instructors
            .iter()
            .filter_map(|id| self.data.instructor(*id))
            .map(|person| person.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn table(&self, view: View, language: Language, layout: Layout, fields: Fields) -> Table {
        let meetings = self.meetings(view);
        match layout {
            Layout::List => self.list(&meetings, language, fields),
            Layout::Grid => self.grid(&meetings, language, fields, false),
        }
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn print_grid(&self, view: View, language: Language, fields: Fields) -> Table {
        self.grid(&self.meetings(view), language, fields, true)
    }

    fn list(&self, meetings: &[Meeting<'_>], language: Language, fields: Fields) -> Table {
        let columns = [
            (true, language.choose("요일", "Day")),
            (true, language.choose("시작", "Start")),
            (true, language.choose("종료", "End")),
            (true, language.choose("학수번호", "Course code")),
            (true, language.choose("분반", "Section")),
            (fields.course_name, language.choose("과목명", "Course name")),
            (fields.instructors, language.choose("교원", "Instructor")),
            (fields.room, language.choose("강의실", "Room")),
        ];
        let headings = columns
            .iter()
            .filter(|(show, _)| *show)
            .map(|(_, label)| (*label).into())
            .collect();
        let rows = meetings
            .iter()
            .map(|&meeting| {
                let values = [
                    language.day(meeting.placement.day).into(),
                    meeting.first.start.to_string(),
                    meeting.last.end.to_string(),
                    meeting.course.code.to_string(),
                    meeting.section.code.to_string(),
                    meeting.course.name.clone(),
                    self.instructors(meeting),
                    meeting.room.key.to_string(),
                ];
                values
                    .into_iter()
                    .zip(columns)
                    .filter(|(_, (show, _))| *show)
                    .map(|(value, _)| value)
                    .collect()
            })
            .collect();
        Table { headings, rows }
    }

    fn grid(
        &self,
        meetings: &[Meeting<'_>],
        language: Language,
        fields: Fields,
        print: bool,
    ) -> Table {
        let days = &self.data.setup.days;
        let mut headings = vec![language.choose("교시", "Period").into()];
        headings.extend(days.iter().map(|day| language.day(*day).into()));
        let mut rows = Vec::new();
        for (index, period) in self.data.setup.periods.periods().iter().enumerate() {
            let label = format!("{}\n{}–{}", period.label, period.start, period.end);
            let day_entries = days
                .iter()
                .map(|day| {
                    meetings
                        .iter()
                        .copied()
                        .filter(|meeting| {
                            let start = usize::from(meeting.placement.start);
                            meeting.placement.day == *day
                                && (start..start + usize::from(meeting.span)).contains(&index)
                        })
                        .map(|meeting| self.grid_cell(meeting, fields))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            if print {
                // A class gets its own row position so page breaks repeat the period label.
                let height = day_entries.iter().map(Vec::len).max().unwrap_or(0).max(1);
                for offset in 0..height {
                    let mut row = vec![label.clone()];
                    row.extend(
                        day_entries
                            .iter()
                            .map(|entries| entries.get(offset).cloned().unwrap_or_default()),
                    );
                    rows.push(row);
                }
            } else {
                let mut row = vec![label];
                row.extend(day_entries.into_iter().map(|entries| entries.join("\n\n")));
                rows.push(row);
            }
        }
        Table { headings, rows }
    }

    fn grid_cell(&self, meeting: Meeting<'_>, fields: Fields) -> String {
        let mut parts = vec![format!(
            "{} · {}",
            meeting.course.code, meeting.section.code
        )];
        if fields.course_name {
            parts.push(meeting.course.name.clone());
        }
        if fields.instructors {
            parts.push(self.instructors(meeting));
        }
        if fields.room {
            parts.push(meeting.room.key.to_string());
        }
        parts.retain(|part| !part.is_empty());
        parts.join("\n")
    }

    pub fn xlsx(
        &self,
        view: View,
        language: Language,
        layout: Layout,
        fields: Fields,
    ) -> Result<Vec<u8>, XlsxError> {
        let table = self.table(view, language, layout, fields);
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name(language.choose("시간표", "Timetable"))?;
        let title = Format::new().set_bold().set_font_size(13);
        let header = Format::new()
            .set_bold()
            .set_background_color(Color::RGB(0xE1EDF6));
        let body = Format::new().set_text_wrap().set_align(FormatAlign::Top);
        let last_column = table.headings.len().saturating_sub(1) as u16;
        sheet.merge_range(
            0,
            0,
            0,
            last_column,
            &format!("{} · {}", self.department(), language.term(self.info.term)),
            &title,
        )?;
        sheet.merge_range(
            1,
            0,
            1,
            last_column,
            &format!(
                "{} · {}",
                self.version_label(language),
                self.view_label(view, language)
            ),
            &title,
        )?;
        sheet.set_freeze_panes(4, u16::from(matches!(layout, Layout::Grid)))?;
        for (column, label) in table.headings.iter().enumerate() {
            sheet.set_column_width(
                column as u16,
                if matches!(layout, Layout::Grid) {
                    25
                } else {
                    19
                },
            )?;
            sheet.write_string_with_format(3, column as u16, label, &header)?;
        }
        for (index, row) in table.rows.iter().enumerate() {
            let row_index = index as u32 + 4;
            if matches!(layout, Layout::Grid) {
                let lines = row
                    .iter()
                    .map(|cell| cell.lines().count())
                    .max()
                    .unwrap_or(1);
                sheet.set_row_height(row_index, (lines as f64 * 16.0 + 10.0).min(409.0))?;
            }
            for (column, value) in row.iter().enumerate() {
                sheet.write_string_with_format(row_index, column as u16, value, &body)?;
            }
        }
        workbook.save_to_buffer()
    }

    pub fn handover_choices(
        &self,
        language: Language,
    ) -> impl Iterator<Item = HandoverChoice> + '_ {
        self.data
            .constraints
            .iter()
            .filter(|constraint| constraint.is_active())
            .filter_map(move |constraint| {
                let name = match constraint.target {
                    Target::Instructor(id) => self.view_label(View::Instructor(id), language),
                    Target::Room(id) => self.view_label(View::Room(id), language),
                    Target::Group(id) => self
                        .data
                        .group(id)
                        .map(|group| group.name.clone())
                        .unwrap_or_default(),
                    Target::Section(id) => self
                        .data
                        .section(id)
                        .and_then(|section| {
                            let course = self.data.course(section.course)?;
                            Some(format!("{} · {}", course.name, section.code))
                        })
                        .unwrap_or_default(),
                };
                let strength = match constraint.strength? {
                    Strength::Hard => language.choose("필수", "Required"),
                    Strength::Soft => language.choose("선호", "Preference"),
                };
                let Rule::AvoidTimes { windows } = constraint.rule.as_ref()?;
                let mut detail = windows
                    .iter()
                    .map(|window| {
                        let day = language.day(window.day());
                        let time = match *window {
                            TimeWindow::Periods { first, last, .. } => {
                                self.data.setup.periods.span_time(
                                    usize::from(first),
                                    usize::from(last.saturating_sub(first)) + 1,
                                )
                            }
                            TimeWindow::Clock { start, end, .. } => Some((start, end)),
                        };
                        time.map_or_else(
                            || day.to_string(),
                            |(start, end)| format!("{day} {start}–{end}"),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if let Some(period) = constraint.period {
                    detail.push_str(&format!(" · {}–{}", period.from, period.until));
                }
                Some(HandoverChoice {
                    id: constraint.id,
                    title: format!("{name} · {strength}"),
                    detail,
                })
            })
    }

    pub fn handover(&self, selected: &BTreeSet<ConstraintId>) -> serde_json::Result<Vec<u8>> {
        let rooms: Vec<_> = self.data.rooms.iter().map(|room| serde_json::json!({
            "id": room.id, "key": room.key, "kind": room.kind, "department": room.department,
            "total_seats": room.total_seats, "available_seats": room.available_seats,
            "equipment": room.equipment, "shared": room.shared, "schedule_checked": room.schedule_checked,
        })).collect();
        let constraints: Vec<_> = self.data.constraints.iter()
            .filter(|constraint| constraint.is_active() && selected.contains(&constraint.id))
            .map(|constraint| serde_json::json!({
                "id": constraint.id, "target": constraint.target, "rule": constraint.rule,
                "strength": constraint.strength, "weight": constraint.weight, "period": constraint.period,
                "status": constraint.status,
            })).collect();
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "auler-handover", "format_version": 1, "department": self.department,
            "finalized_version": self.info.version, "finalized_at_ms": self.info.at_ms,
            "setup": self.data.setup, "courses": self.data.courses, "instructors": self.data.instructors,
            "groups": self.data.groups, "equipment": self.data.equipment, "rooms": rooms,
            "sections": self.data.sections, "placements": self.data.placements, "constraints": constraints,
        }))
    }
}
