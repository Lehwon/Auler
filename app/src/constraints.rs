use std::cell::RefCell;
use std::rc::Rc;

use auler_domain::model::*;
use auler_domain::time::{ClockTime, Date, Weekday};
use auler_engine::{Evaluator, Reason};
use auler_store::{ChangeMeta, Op, Record, RecordRef, StoreError};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::workspace::{Session, refresh};
use crate::{
    AppWindow, ConstraintCell, ConstraintEditor, ConstraintGridRow, ConstraintRow, ConstraintView,
    Labels,
};

#[derive(Default)]
pub(crate) struct Constraints {
    tab: i32,
    target_filter: i32,
    period_filter: i32,
    query: String,
    grid: bool,
    selected: Option<ConstraintId>,
    visible: Vec<ConstraintId>,
    edit: Option<Edit>,
    deleting: Option<u64>,
    notice: Option<Notice>,
}

struct Edit {
    id: Option<ConstraintId>,
    base: u64,
    target_kind: i32,
    target: Option<Target>,
    strength: i32,
    confirmed: bool,
    from: String,
    until: String,
    weight: String,
    request: String,
    source: Option<SourceRef>,
    note: String,
    windows: Vec<TimeWindow>,
    window_index: Option<usize>,
    window_dirty: bool,
    window: WindowInput,
}

#[derive(Default)]
struct WindowInput {
    day: i32,
    mode: i32,
    first: i32,
    last: i32,
    start: String,
    end: String,
}

enum Notice {
    Target,
    Timing,
    WindowNotAdded,
    DuplicateWindow,
    Dates,
    Strength,
    Weight,
    Stale,
    Store(StoreError),
    Saved,
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, state_clone) = (ui.as_weak(), state.clone());
    ui.on_constraint_action(move |action, index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = state_clone.borrow_mut();
        command(&ui, &mut session, action, index);
        render(&ui, &mut session);
    });
    let (weak, state_clone) = (ui.as_weak(), state.clone());
    ui.on_constraint_option(move |field, value| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = state_clone.borrow_mut();
        set_option(&mut session, field, value);
        render(&ui, &mut session);
    });
    let (weak, state_clone) = (ui.as_weak(), state.clone());
    ui.on_constraint_text(move |field_index, value| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = state_clone.borrow_mut();
        let state = &mut session.constraints;
        state.notice = None;
        if field_index == 0 {
            state.query = value.to_string();
        } else if let Some(edit) = &mut state.edit {
            let field = match field_index {
                1 => &mut edit.request,
                2 => &mut edit.from,
                3 => &mut edit.until,
                4 => &mut edit.weight,
                5 => &mut edit.note,
                6 => &mut edit.window.start,
                7 => &mut edit.window.end,
                _ => return,
            };
            *field = value.to_string();
            if matches!(field_index, 6 | 7) {
                edit.window_dirty = true;
            }
        }
        render(&ui, &mut session);
    });
    let (weak, state_clone) = (ui.as_weak(), state.clone());
    ui.on_constraint_grid_cell(move |day, period, first_row| {
        let Some(ui) = weak.upgrade() else { return };
        let mut guard = state_clone.borrow_mut();
        let session = &mut *guard;
        let (Some(data), Ok(day), Ok(period)) = (
            session.data.as_ref(),
            usize::try_from(day),
            u8::try_from(period),
        ) else {
            return;
        };
        let Some(&weekday) = data.setup.days.get(day) else {
            return;
        };
        if usize::from(period) >= data.setup.periods.len() {
            return;
        }
        if let Some(edit) = &mut session.constraints.edit {
            if edit.window_dirty
                || edit.windows.iter().any(|window| {
                    window.day() == weekday
                        && matches!(window, TimeWindow::Clock { .. })
                        && window
                            .period_range(&data.setup.periods)
                            .contains(&usize::from(period))
                })
            {
                return;
            }
            toggle_period(&mut edit.windows, weekday, period);
            edit.window_index = None;
            session.constraints.notice = None;
        } else {
            select_grid_condition(session, weekday, period, first_row);
        }
        render(&ui, session);
    });
}

fn select_grid_condition(session: &mut Session, day: Weekday, period: u8, first_row: i32) {
    let (Some(data), Ok(first)) = (&session.data, usize::try_from(first_row)) else {
        return;
    };
    let state = &mut session.constraints;
    let matches = |id: ConstraintId| {
        data.constraints
            .iter()
            .find(|constraint| constraint.id == id)
            .is_some_and(|constraint| {
                windows(constraint).iter().any(|window| {
                    window.day() == day
                        && window
                            .period_range(&data.setup.periods)
                            .contains(&usize::from(period))
                })
            })
    };
    let after_selected = state
        .selected
        .and_then(|selected| {
            state
                .visible
                .iter()
                .position(|&id| id == selected && matches(id))
        })
        .map_or(first, |selected| selected + 1);
    state.selected = state
        .visible
        .iter()
        .copied()
        .skip(after_selected)
        .find(|&id| matches(id))
        .or_else(|| state.visible.get(first).copied());
    state.deleting = None;
    state.notice = None;
}

fn set_option(session: &mut Session, field: i32, value: i32) {
    let state = &mut session.constraints;
    state.notice = None;
    match field {
        0..=2 if state.edit.is_none() => match field {
            0 => state.tab = value.clamp(0, 3),
            1 => state.target_filter = value.clamp(0, 4),
            _ => state.period_filter = value.clamp(0, 2),
        },
        3 => state.grid = value != 0,
        _ => {
            let (Some(data), Some(edit)) = (&session.data, &mut state.edit) else {
                return;
            };
            match field {
                4 => {
                    edit.target_kind = value.clamp(0, 3);
                    edit.target = None;
                }
                5 => {
                    edit.target = usize::try_from(value).ok().and_then(|index| {
                        target_options(data, edit.target_kind)
                            .get(index)
                            .map(|(target, _)| *target)
                    })
                }
                6 => edit.strength = value.clamp(0, 2),
                7 => edit.confirmed = value != 0,
                8..=11 => {
                    edit.window_dirty = true;
                    match field {
                        8 => edit.window.day = value,
                        9 => edit.window.mode = value.clamp(0, 1),
                        10 => {
                            if edit.window.last == edit.window.first || edit.window.last < value {
                                edit.window.last = value;
                            }
                            edit.window.first = value;
                        }
                        _ => edit.window.last = value,
                    }
                }
                _ => {}
            }
        }
    }
}

fn command(ui: &AppWindow, session: &mut Session, action: i32, index: i32) {
    let Some(data) = &session.data else { return };
    let state = &mut session.constraints;
    state.notice = None;
    if action == 0 && state.edit.is_none() {
        state.selected = usize::try_from(index)
            .ok()
            .and_then(|index| state.visible.get(index))
            .copied();
        state.deleting = None;
    }
    if action == 4 {
        state.edit = None;
        state.deleting = None;
        return;
    }
    let selected = data
        .constraints
        .iter()
        .find(|constraint| Some(constraint.id) == state.selected);
    let editable = ui.get_home_stats().editing;
    match action {
        1 | 2 if editable && state.edit.is_none() => {
            let original = if action == 2 { selected } else { None };
            let mut edit = Edit::new(original, session.revision);
            if action == 1 {
                edit.strength = match state.tab {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
                edit.confirmed = matches!(state.tab, 1 | 2);
            }
            state.edit = Some(edit);
            state.deleting = None;
        }
        3 if editable => save(ui, session, false),
        5 if editable && selected.is_some() && state.edit.is_none() => {
            state.deleting = Some(session.revision)
        }
        6 if editable && state.deleting.is_some() => save(ui, session, true),
        7..=10 if editable => {
            if let Some(edit) = &mut state.edit {
                window_command(data, edit, action, index, &mut state.notice);
            }
        }
        11 if state.edit.is_none() => state.selected = None,
        _ => {}
    }
}

fn window_command(
    data: &TermData,
    edit: &mut Edit,
    action: i32,
    index: i32,
    notice: &mut Option<Notice>,
) {
    let index = usize::try_from(index).ok();
    match action {
        7 => {
            if edit.window_dirty {
                *notice = Some(Notice::WindowNotAdded);
                return;
            }
            if let Some(window) = index.and_then(|index| edit.windows.get(index)).copied() {
                edit.window = WindowInput::from_window(window, data);
                edit.window_index = index;
            }
        }
        8 => {
            let Some(window) = edit.window.parse(data) else {
                *notice = Some(Notice::Timing);
                return;
            };
            if edit
                .windows
                .iter()
                .enumerate()
                .any(|(index, entry)| Some(index) != edit.window_index && *entry == window)
            {
                *notice = Some(Notice::DuplicateWindow);
                return;
            }
            if let Some(index) = edit.window_index {
                edit.windows[index] = window;
            } else {
                edit.windows.push(window);
            }
            edit.window_index = None;
            edit.window_dirty = false;
        }
        9 => {
            if let Some(index) = index.filter(|index| *index < edit.windows.len()) {
                edit.windows.remove(index);
                match edit.window_index {
                    Some(current) if current == index => {
                        edit.window_index = None;
                        edit.window_dirty = false;
                        edit.window = WindowInput::default();
                    }
                    Some(current) if current > index => edit.window_index = Some(current - 1),
                    _ => {}
                }
            }
        }
        _ => {
            edit.window_index = None;
            edit.window_dirty = false;
            edit.window = WindowInput::default();
        }
    }
}

impl Edit {
    fn new(original: Option<&Constraint>, base: u64) -> Self {
        let period = original.and_then(|constraint| constraint.period);
        let windows = original
            .and_then(|constraint| constraint.rule.as_ref())
            .map_or_else(Vec::new, |Rule::AvoidTimes { windows }| windows.clone());
        Self {
            base,
            id: original.map(|c| c.id),
            target_kind: original.map_or(0, |c| target_kind(c.target)),
            target: original.map(|c| c.target),
            strength: match original.and_then(|c| c.strength) {
                Some(Strength::Hard) => 1,
                Some(Strength::Soft) => 2,
                None => 0,
            },
            confirmed: original.is_some_and(|c| c.status == ConstraintStatus::Confirmed),
            from: period.map(|p| p.from.to_string()).unwrap_or_default(),
            until: period.map(|p| p.until.to_string()).unwrap_or_default(),
            weight: original.map_or(1, |c| c.weight).to_string(),
            request: original.and_then(|c| c.text.clone()).unwrap_or_default(),
            source: original.and_then(|c| c.source.clone()),
            note: original.and_then(|c| c.note.clone()).unwrap_or_default(),
            windows,
            window_index: None,
            window_dirty: false,
            window: WindowInput::default(),
        }
    }

    fn constraint(&self, data: &TermData, id: ConstraintId) -> Result<Constraint, Notice> {
        if self.window_dirty {
            return Err(Notice::WindowNotAdded);
        }
        let target = self
            .target
            .filter(|&target| target_name(data, target).is_some())
            .ok_or(Notice::Target)?;
        let period = match (self.from.trim(), self.until.trim()) {
            ("", "") => None,
            (start, end) => {
                let from = Date::parse(start).ok_or(Notice::Dates)?;
                let until = Date::parse(end).ok_or(Notice::Dates)?;
                if from > until
                    || data.setup.start.is_some_and(|start| from < start)
                    || data.setup.end.is_some_and(|end| until > end)
                {
                    return Err(Notice::Dates);
                }
                Some(DateRange { from, until })
            }
        };
        let strength = match self.strength {
            1 => Some(Strength::Hard),
            2 => Some(Strength::Soft),
            _ => None,
        };
        if self.confirmed && strength.is_none() {
            return Err(Notice::Strength);
        }
        if self.confirmed && self.windows.is_empty() {
            return Err(Notice::Timing);
        }
        let weight = if strength == Some(Strength::Soft) {
            self.weight
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|weight| (1..=10).contains(weight))
                .ok_or(Notice::Weight)?
        } else {
            1
        };
        Ok(Constraint {
            id,
            target,
            strength,
            weight,
            status: if self.confirmed {
                ConstraintStatus::Confirmed
            } else {
                ConstraintStatus::Pending
            },
            rule: (!self.windows.is_empty()).then(|| Rule::AvoidTimes {
                windows: self.windows.clone(),
            }),
            period,
            text: nonempty(&self.request),
            source: self.source.clone(),
            confirmed_by: self.confirmed.then(|| "local".into()),
            note: nonempty(&self.note),
        })
    }
}

fn nonempty(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_owned())
}

impl WindowInput {
    fn parse(&self, data: &TermData) -> Option<TimeWindow> {
        let day = *data.setup.days.get(usize::try_from(self.day).ok()?)?;
        if self.mode == 1 {
            let start = ClockTime::parse(self.start.trim())?;
            let end = ClockTime::parse(self.end.trim())?;
            return (start < end).then_some(TimeWindow::Clock { day, start, end });
        }
        let first = u8::try_from(self.first).ok()?;
        let last = u8::try_from(self.last).ok()?;
        (first <= last && usize::from(last) < data.setup.periods.len())
            .then_some(TimeWindow::Periods { day, first, last })
    }

    fn from_window(window: TimeWindow, data: &TermData) -> Self {
        let day = data
            .setup
            .days
            .iter()
            .position(|&day| day == window.day())
            .map_or(-1, |index| index as i32);
        match window {
            TimeWindow::Periods { first, last, .. } => Self {
                day,
                first: first.into(),
                last: last.into(),
                ..Self::default()
            },
            TimeWindow::Clock { start, end, .. } => Self {
                day,
                mode: 1,
                start: start.to_string(),
                end: end.to_string(),
                ..Self::default()
            },
        }
    }
}

fn toggle_period(windows: &mut Vec<TimeWindow>, day: Weekday, period: u8) {
    let mut removed = false;
    let mut next = Vec::with_capacity(windows.len() + 1);
    for &window in windows.iter() {
        if let TimeWindow::Periods {
            day: on,
            first,
            last,
        } = window
            && on == day
            && (first..=last).contains(&period)
        {
            removed = true;
            if first < period {
                next.push(TimeWindow::Periods {
                    day,
                    first,
                    last: period - 1,
                });
            }
            if period < last {
                next.push(TimeWindow::Periods {
                    day,
                    first: period + 1,
                    last,
                });
            }
        } else {
            next.push(window);
        }
    }
    if !removed {
        next.push(TimeWindow::Periods {
            day,
            first: period,
            last: period,
        });
    }
    *windows = next;
}

fn save(ui: &AppWindow, session: &mut Session, remove: bool) {
    let (Some(workspace), Some(data)) = (&session.workspace, &session.data) else {
        return;
    };
    let state = &mut session.constraints;
    let (base, id, op, summary) = if remove {
        let Some(id) = state.selected else { return };
        (
            state.deleting.unwrap_or(session.revision),
            id,
            Op::Remove(RecordRef::Constraint(id)),
            "Removed constraint",
        )
    } else {
        let Some(edit) = &state.edit else { return };
        let id = match edit.id.map_or_else(|| workspace.next_constraint_id(), Ok) {
            Ok(id) => id,
            Err(error) => {
                state.notice = Some(Notice::Store(error));
                return;
            }
        };
        match edit.constraint(data, id) {
            Ok(constraint) => (
                edit.base,
                id,
                Op::Put(Record::Constraint(constraint)),
                "Saved constraint",
            ),
            Err(notice) => {
                state.notice = Some(notice);
                return;
            }
        }
    };
    let term = data.setup.key;
    match workspace.apply(Some(term), vec![op], ChangeMeta::direct(summary).base(base)) {
        Ok(_) => {
            state.selected = (!remove).then_some(id);
            state.deleting = None;
            state.edit = None;
            if refresh(ui, session, Some(term)).is_err() {
                session.constraints.notice = Some(Notice::Saved);
            }
        }
        Err(StoreError::StaleRevision { .. }) => {
            state.deleting = None;
            let _ = refresh(ui, session, Some(term));
            session.constraints.notice = Some(Notice::Stale);
        }
        Err(error) => state.notice = Some(Notice::Store(error)),
    }
}

fn target_kind(target: Target) -> i32 {
    match target {
        Target::Instructor(_) => 0,
        Target::Section(_) => 1,
        Target::Group(_) => 2,
        Target::Room(_) => 3,
    }
}

fn section_name(data: &TermData, id: SectionId) -> Option<String> {
    let section = data.section(id)?;
    let course = data.course(section.course)?;
    Some(format!(
        "{} · {} ({})",
        course.name, section.code, course.code
    ))
}

fn target_name(data: &TermData, target: Target) -> Option<String> {
    match target {
        Target::Instructor(id) => data
            .instructor(id)
            .map(|person| format!("{} ({})", person.name, person.code)),
        Target::Section(id) => section_name(data, id),
        Target::Group(id) => data
            .group(id)
            .map(|group| format!("{} ({})", group.name, group.code)),
        Target::Room(id) => data.room(id).map(|room| room.key.to_string()),
    }
}

fn target_options(data: &TermData, kind: i32) -> Vec<(Target, String)> {
    let mut result: Vec<_> = match kind {
        0 => data
            .instructors
            .iter()
            .map(|p| (Target::Instructor(p.id), format!("{} ({})", p.name, p.code)))
            .collect(),
        1 => data
            .sections
            .iter()
            .filter_map(|s| Some((Target::Section(s.id), section_name(data, s.id)?)))
            .collect(),
        2 => data
            .groups
            .iter()
            .map(|g| (Target::Group(g.id), format!("{} ({})", g.name, g.code)))
            .collect(),
        _ => data
            .rooms
            .iter()
            .map(|r| (Target::Room(r.id), r.key.to_string()))
            .collect(),
    };
    result.sort_by(|a, b| a.1.cmp(&b.1));
    result
}

fn tab(constraint: &Constraint) -> i32 {
    if !constraint.is_active() {
        3
    } else if constraint.strength == Some(Strength::Hard) {
        1
    } else {
        2
    }
}

fn window_name(labels: &Labels, data: &TermData, window: TimeWindow) -> SharedString {
    let day = labels.invoke_weekday(window.day().index() as i32);
    let periods = data.setup.periods.periods();
    let label = |index| {
        periods
            .get(usize::from(index))
            .map_or("?", |period| period.label.as_str())
    };
    match window {
        TimeWindow::Periods { first, last, .. } if first == last => {
            labels.invoke_constraint_window_period(day, label(first).into())
        }
        TimeWindow::Periods { first, last, .. } => {
            labels.invoke_constraint_window_span(day, label(first).into(), label(last).into())
        }
        TimeWindow::Clock { start, end, .. } => format!("{day} · {start}–{end}").into(),
    }
}

fn windows(constraint: &Constraint) -> &[TimeWindow] {
    let Some(Rule::AvoidTimes { windows }) = &constraint.rule else {
        return &[];
    };
    windows
}

fn source_name(labels: &Labels, source: Option<&SourceRef>) -> SharedString {
    let Some(source) = source else {
        return labels.get_constraint_manual();
    };
    let cell = format!(
        "{}{}",
        source.column.as_deref().unwrap_or(""),
        source.row.map(|r| r.to_string()).unwrap_or_default()
    );
    [
        source.file.as_deref().unwrap_or(""),
        source.sheet.as_deref().unwrap_or(""),
        &cell,
    ]
    .into_iter()
    .filter(|field| !field.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
    .into()
}

fn row(labels: &Labels, data: &TermData, constraint: &Constraint, selected: bool) -> ConstraintRow {
    let times = windows(constraint);
    ConstraintRow {
        title: constraint
            .text
            .as_deref()
            .unwrap_or(labels.get_constraint_avoid().as_str())
            .into(),
        target: target_name(data, constraint.target)
            .unwrap_or_default()
            .into(),
        range: constraint.period.map_or_else(
            || labels.get_constraint_whole_term(),
            |span| format!("{}–{}", span.from, span.until).into(),
        ),
        time: if times.is_empty() {
            labels.get_constraint_no_time()
        } else {
            times
                .iter()
                .map(|&slot| window_name(labels, data, slot).to_string())
                .collect::<Vec<_>>()
                .join(", ")
                .into()
        },
        kind: tab(constraint),
        strength: match constraint.strength {
            Some(Strength::Hard) => 1,
            Some(Strength::Soft) => 2,
            None => 0,
        },
        selected,
    }
}

pub(crate) fn render(ui: &AppWindow, session: &mut Session) {
    let Some(data) = &session.data else {
        ui.set_constraint_view(ConstraintView::default());
        return;
    };
    let labels = ui.global::<Labels>();
    let state = &mut session.constraints;
    let query = state.query.trim().to_lowercase();
    let matches = |constraint: &&Constraint| {
        (state.tab == 0 || state.tab == tab(constraint))
            && (state.target_filter == 0
                || state.target_filter == target_kind(constraint.target) + 1)
            && (state.period_filter == 0
                || (state.period_filter == 1) == constraint.period.is_none())
            && (query.is_empty()
                || constraint
                    .text
                    .as_deref()
                    .is_some_and(|text| text.to_lowercase().contains(&query))
                || target_name(data, constraint.target)
                    .is_some_and(|name| name.to_lowercase().contains(&query)))
    };
    let visible: Vec<_> = data.constraints.iter().filter(matches).collect();
    state.visible = visible.iter().map(|constraint| constraint.id).collect();
    let selected = data
        .constraints
        .iter()
        .find(|constraint| state.selected == Some(constraint.id));
    let tabs = (0..=3).map(|index| {
        let count = data
            .constraints
            .iter()
            .filter(|constraint| index == 0 || tab(constraint) == index)
            .count();
        labels.invoke_constraint_tab(index, count as i32)
    });
    let (detail, details, affected) = selected.map_or_else(
        || {
            (
                ConstraintRow::default(),
                ModelRc::default(),
                ModelRc::default(),
            )
        },
        |constraint| {
            (
                row(&labels, data, constraint, true),
                model(
                    [
                        labels.invoke_constraint_detail(
                            labels.get_constraint_source(),
                            source_name(&labels, constraint.source.as_ref()),
                        ),
                        labels.invoke_constraint_detail(
                            labels.get_constraint_confirmed_by(),
                            match constraint.confirmed_by.as_deref() {
                                Some("local") => labels.get_local(),
                                name => name.unwrap_or("").into(),
                            },
                        ),
                        labels.invoke_constraint_detail(
                            labels.get_constraint_note(),
                            constraint.note.as_deref().unwrap_or("").into(),
                        ),
                        if constraint.strength == Some(Strength::Soft) {
                            labels.invoke_constraint_detail(
                                labels.get_constraint_weight(),
                                constraint.weight.to_string().into(),
                            )
                        } else {
                            "".into()
                        },
                    ]
                    .into_iter()
                    .filter(|text| !text.is_empty()),
                ),
                model(impact(&labels, data, constraint).rows),
            )
        },
    );
    let mut view = ConstraintView {
        has_term: true,
        can_edit: ui.get_home_stats().editing,
        grid: state.grid,
        tab: state.tab,
        target_filter: state.target_filter,
        period_filter: state.period_filter,
        query: state.query.as_str().into(),
        tabs: model(tabs),
        rows: model(visible.iter().map(|constraint| {
            row(
                &labels,
                data,
                constraint,
                state.selected == Some(constraint.id),
            )
        })),
        show_detail: selected.is_some(),
        detail,
        details,
        affected,
        deleting: state.deleting.is_some(),
        error: state
            .notice
            .as_ref()
            .map_or_else(SharedString::default, |notice| notice_text(&labels, notice)),
        days: model(
            data.setup
                .days
                .iter()
                .map(|day| labels.invoke_weekday(day.index() as i32)),
        ),
        ..ConstraintView::default()
    };
    if let Some(edit) = &state.edit {
        view.editing = true;
        view.show_detail = true;
        view.editor = editor_view(&labels, data, edit);
    }
    if state.grid {
        view.calendar = model(grid(&labels, data, &visible, state.edit.as_ref()));
    }
    ui.set_constraint_view(view);
}

fn editor_view(labels: &Labels, data: &TermData, edit: &Edit) -> ConstraintEditor {
    let targets = target_options(data, edit.target_kind);
    let target_index = targets
        .iter()
        .position(|(target, _)| Some(*target) == edit.target)
        .map_or(-1, |index| index as i32);
    let proposed = edit.constraint(data, edit.id.unwrap_or(ConstraintId(0)));
    let affected = proposed.as_ref().map_or_else(
        |_| Impact::default(),
        |constraint| impact(labels, data, constraint),
    );
    ConstraintEditor {
        target_kind: edit.target_kind,
        targets: model(targets.into_iter().map(|(_, label)| label.into())),
        target_index,
        strength: edit.strength,
        confirmed: edit.confirmed,
        request: edit.request.as_str().into(),
        source: source_name(labels, edit.source.as_ref()),
        note: edit.note.as_str().into(),
        from: edit.from.as_str().into(),
        until: edit.until.as_str().into(),
        weight: edit.weight.as_str().into(),
        windows: model(
            edit.windows
                .iter()
                .map(|&window| window_name(labels, data, window)),
        ),
        window_index: edit.window_index.map_or(-1, |index| index as i32),
        window_dirty: edit.window_dirty,
        day: edit.window.day,
        time_mode: edit.window.mode,
        first: edit.window.first,
        last: edit.window.last,
        start: edit.window.start.as_str().into(),
        end: edit.window.end.as_str().into(),
        period_labels: model(
            data.setup
                .periods
                .periods()
                .iter()
                .map(|period| period.label.as_str().into()),
        ),
        affected: model(affected.rows),
        affected_locked: affected.locked,
        can_save: proposed.is_ok(),
        reason: proposed
            .err()
            .map_or_else(SharedString::default, |notice| notice_text(labels, &notice)),
    }
}

fn grid(
    labels: &Labels,
    data: &TermData,
    visible: &[&Constraint],
    edit: Option<&Edit>,
) -> Vec<ConstraintGridRow> {
    let width = data.setup.days.len();
    let mut cells: Vec<_> = data
        .setup
        .periods
        .periods()
        .iter()
        .map(|period| ConstraintGridRow {
            title: period.label.as_str().into(),
            time: format!("{}–{}", period.start, period.end).into(),
            cells: ModelRc::default(),
        })
        .collect();
    let mut values = vec![
        vec![
            ConstraintCell {
                first: -1,
                ..ConstraintCell::default()
            };
            width
        ];
        cells.len()
    ];
    let series: Vec<&[TimeWindow]> = if let Some(edit) = edit {
        vec![&edit.windows]
    } else {
        visible.iter().map(|c| windows(c)).collect()
    };
    for (index, windows) in series.into_iter().enumerate() {
        for &window in windows {
            let Some(day) = data.setup.days.iter().position(|&day| day == window.day()) else {
                continue;
            };
            for period in window.period_range(&data.setup.periods) {
                let cell = &mut values[period][day];
                if cell.last_seen == 0 || cell.last_seen != index as i32 + 1 {
                    cell.count += 1;
                    cell.last_seen = index as i32 + 1;
                }
                if cell.first < 0 {
                    cell.first = index as i32;
                }
                cell.clock |= matches!(window, TimeWindow::Clock { .. });
            }
        }
    }
    for (index, (row, values)) in cells.iter_mut().zip(values).enumerate() {
        row.cells = model(
            values
                .into_iter()
                .zip(&data.setup.days)
                .map(|(mut cell, &day)| {
                    let day = labels.invoke_weekday(day.index() as i32);
                    let period = data.setup.periods.periods()[index].label.as_str().into();
                    cell.label = if edit.is_some() {
                        labels.invoke_constraint_edit_cell(day, period, cell.clock, cell.count > 0)
                    } else {
                        labels.invoke_constraint_cell(day, period, cell.count)
                    };
                    cell
                }),
        );
    }
    cells
}

#[derive(Default)]
struct Impact {
    rows: Vec<SharedString>,
    locked: bool,
}

fn impact(labels: &Labels, data: &TermData, proposed: &Constraint) -> Impact {
    let mut result = Impact::default();
    if !proposed.is_active() {
        return result;
    }
    let mut candidate = data.clone();
    candidate.constraints.clear();
    candidate.constraints.push(proposed.clone());
    let evaluator = Evaluator::new(&candidate);
    for placement in &data.placements {
        if !evaluator.evaluate(placement, &[]).findings.iter().any(
            |finding| matches!(finding.reason, Reason::Constraint { id, .. } if id == proposed.id),
        ) {
            continue;
        }
        let Some(title) = section_name(data, placement.meeting.section) else {
            continue;
        };
        let Some(span) = data
            .section(placement.meeting.section)
            .and_then(|section| section.split.get(usize::from(placement.meeting.part)))
        else {
            continue;
        };
        let when = window_name(
            labels,
            data,
            TimeWindow::Periods {
                day: placement.day,
                first: placement.start,
                last: placement.start.saturating_add(span.saturating_sub(1)),
            },
        );
        result
            .rows
            .push(labels.invoke_constraint_affected(title.into(), when, placement.locked));
        result.locked |= placement.locked;
    }
    result
}

fn notice_text(labels: &Labels, notice: &Notice) -> SharedString {
    match notice {
        Notice::Target => labels.get_constraint_choose_target(),
        Notice::Timing => labels.get_constraint_choose_window(),
        Notice::WindowNotAdded => labels.get_constraint_save_window_first(),
        Notice::DuplicateWindow => labels.get_constraint_duplicate_window(),
        Notice::Dates => labels.get_constraint_check_dates(),
        Notice::Strength => labels.get_constraint_choose_strength(),
        Notice::Weight => labels.get_constraint_check_weight(),
        Notice::Stale => labels.get_catalog_stale(),
        Notice::Saved => labels.get_catalog_refresh_failed(),
        Notice::Store(error) => labels.invoke_workspace_error(error.to_string().into()),
    }
}

fn model<T: Clone + 'static>(items: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items.into_iter().collect::<Vec<_>>()))
}
