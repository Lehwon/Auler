use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use auler_domain::model::*;
use auler_engine::{
    Evaluation, Evaluator, Level, LocalSolver, Progress, SolveOptions, SolveResult, SolveStatus,
};
use auler_store::{ChangeMeta, Op, Origin, Record, RecordRef, StoreError};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::workspace::{Session, refresh};
use crate::{AppWindow, CardData, Labels, PeriodRow, WorkChange, WorkChoice, WorkItem, WorkView};

#[derive(Default)]
pub(crate) struct Workbench {
    filter_kind: i32,
    filter: usize,
    query: String,
    pending_query: String,
    list: bool,
    selected: Option<MeetingRef>,
    cards: Vec<MeetingRef>,
    pending: Vec<MeetingRef>,
    locations: Vec<(Placement, Evaluation)>,
    day: usize,
    period: usize,
    choosing: bool,
    draft: Option<DraftMove>,
    error: Option<WorkError>,
    solver_scope: i32,
    solver: SolverState,
    timer: Timer,
}

struct DraftMove {
    placement: Placement,
    assessment: Evaluation,
    base: u64,
}

enum WorkError {
    Stale,
    Store(StoreError),
    NoPosition,
    SolverStopped,
    Saved,
}

#[derive(Default)]
enum SolverState {
    #[default]
    Idle,
    Setup,
    Running(Job),
    Preview(SolverPreview),
}

struct Job {
    signal: Arc<AtomicBool>,
    events: Receiver<JobEvent>,
    progress: Option<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.signal.store(true, Ordering::Relaxed);
    }
}

enum JobEvent {
    Progress(Progress),
    Finished(Box<SolverPreview>),
}

struct SolverPreview {
    result: SolveResult,
    base: u64,
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_work_action(move |action, index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        command(&ui, &mut session, action, index);
        if matches!(session.workbench.solver, SolverState::Running(_))
            && !session.workbench.timer.running()
        {
            let (weak, context) = (ui.as_weak(), Rc::downgrade(&context));
            session.workbench.timer.start(
                TimerMode::Repeated,
                Duration::from_millis(75),
                move || {
                    let (Some(ui), Some(context)) = (weak.upgrade(), context.upgrade()) else {
                        return;
                    };
                    let Ok(mut session) = context.try_borrow_mut() else {
                        return;
                    };
                    poll_solver(&mut session.workbench);
                    render(&ui, &mut session);
                },
            );
        }
        render(&ui, &mut session);
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_work_option(move |option, index| {
        let Some(ui) = weak.upgrade() else { return };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let mut session = context.borrow_mut();
        let work = &mut session.workbench;
        work.error = None;
        match option {
            0 => {
                work.filter_kind = index.min(3) as i32;
                work.filter = 0;
            }
            1 => work.filter = index,
            2 => work.list = index != 0,
            3 => work.day = index,
            4 => work.period = index,
            5 => work.solver_scope = index.min(2) as i32,
            _ => {}
        }
        render(&ui, &mut session);
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_work_search(move |pending, query| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        if pending {
            session.workbench.pending_query = query.to_string();
        } else {
            session.workbench.query = query.to_string();
        }
        render(&ui, &mut session);
    });
    let context = state.clone();
    ui.on_can_move(move |index, day, start| {
        let Ok(session) = context.try_borrow() else {
            return false;
        };
        if !matches!(session.workbench.solver, SolverState::Idle) {
            return false;
        }
        let Some(data) = &session.data else {
            return false;
        };
        let Some(meeting) = indexed(&session.workbench.cards, index) else {
            return false;
        };
        drag_candidate(data, meeting, day, start).is_some_and(|(_, check)| check.can_assign())
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_move_card(move |index, day, start| {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        if !ui.get_home_stats().editing {
            return false;
        }
        let mut session = context.borrow_mut();
        if !matches!(session.workbench.solver, SolverState::Idle) {
            return false;
        }
        if let Some(meeting) = indexed(&session.workbench.cards, index) {
            select(&mut session, meeting);
            let candidate = session
                .data
                .as_ref()
                .and_then(|data| drag_candidate(data, meeting, day, start));
            if candidate.is_some() {
                session.workbench.day = day as usize;
                session.workbench.period = start as usize;
            }
            preview_move(&mut session, candidate);
            render(&ui, &mut session);
        }
        false
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_lock_toggled(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        if !matches!(session.workbench.solver, SolverState::Idle) {
            return;
        }
        if index >= 0
            && let Some(meeting) = indexed(&session.workbench.cards, index)
        {
            select(&mut session, meeting);
        }
        command(&ui, &mut session, 7, 0);
        render(&ui, &mut session);
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_undo(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        command(&ui, &mut session, 8, 0);
        render(&ui, &mut session);
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_redo(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        command(&ui, &mut session, 9, 0);
        render(&ui, &mut session);
    });
    let weak = ui.as_weak();
    let context = state.clone();
    ui.on_work_slot(move |day, period| {
        let Some(ui) = weak.upgrade() else { return };
        let (Ok(day), Ok(period)) = (usize::try_from(day), usize::try_from(period)) else {
            return;
        };
        if !ui.get_home_stats().editing {
            return;
        }
        let mut session = context.borrow_mut();
        let work = &mut session.workbench;
        if work.selected.is_none() || !matches!(work.solver, SolverState::Idle) {
            return;
        }
        work.day = day;
        work.period = period;
        work.choosing = true;
        render(&ui, &mut session);
    });
}

fn indexed<T: Copy>(items: &[T], index: i32) -> Option<T> {
    items.get(usize::try_from(index).ok()?).copied()
}

fn select(session: &mut Session, meeting: MeetingRef) {
    let work = &mut session.workbench;
    if work.selected == Some(meeting) {
        return;
    }
    work.selected = Some(meeting);
    work.draft = None;
    work.error = None;
    work.choosing = false;
    work.day = 0;
    work.period = 0;
    if let Some(data) = &session.data
        && let Some(placement) = placed(data, meeting)
    {
        work.day = data
            .setup
            .days
            .iter()
            .position(|&day| day == placement.day)
            .unwrap_or_default();
        work.period = usize::from(placement.start);
    }
}

fn command(ui: &AppWindow, session: &mut Session, action: i32, index: i32) {
    let editable = ui.get_home_stats().editing;
    if !matches!(session.workbench.solver, SolverState::Idle) && !(12..=14).contains(&action) {
        return;
    }
    session.workbench.error = None;
    match action {
        0 | 1 => {
            let work = &session.workbench;
            let items = if action == 0 {
                &work.cards
            } else {
                &work.pending
            };
            if let Some(meeting) = indexed(items, index) {
                select(session, meeting);
                if action == 1 && editable {
                    session.workbench.choosing = true;
                }
            }
        }
        2 => {
            session.workbench.selected = None;
            session.workbench.choosing = false;
            session.workbench.draft = None;
        }
        3 if editable => session.workbench.choosing = !session.workbench.choosing,
        4 if editable => {
            let candidate = usize::try_from(index)
                .ok()
                .and_then(|index| session.workbench.locations.get(index))
                .cloned();
            preview_move(session, candidate);
        }
        5 if editable => apply_move(ui, session),
        6 => session.workbench.draft = None,
        7 if editable => toggle_lock(ui, session),
        8 | 9 if editable => apply_history(ui, session, action == 8),
        10 | 11 if editable && matches!(session.workbench.solver, SolverState::Idle) => {
            session.workbench.solver_scope = i32::from(action == 10);
            session.workbench.solver = SolverState::Setup;
        }
        12 if editable && matches!(session.workbench.solver, SolverState::Setup) => {
            start_solver(session)
        }
        13 => {
            if let SolverState::Running(job) = &session.workbench.solver {
                job.signal.store(true, Ordering::Relaxed);
            } else {
                session.workbench.solver = SolverState::Idle;
            }
        }
        14 if editable => apply_solver(ui, session),
        15 if editable && session.workbench.draft.is_some() => {
            session.workbench.draft = None;
            session.workbench.choosing = true;
        }
        _ => {}
    }
}

fn placed(data: &TermData, meeting: MeetingRef) -> Option<&Placement> {
    data.placements
        .iter()
        .find(|placement| placement.meeting == meeting)
}

fn preview_move(session: &mut Session, candidate: Option<(Placement, Evaluation)>) {
    let work = &mut session.workbench;
    work.choosing = false;
    work.error = None;
    work.draft = None;
    if let Some((placement, assessment)) = candidate {
        work.draft = Some(DraftMove {
            placement,
            assessment,
            base: session.revision,
        });
    } else {
        work.error = Some(WorkError::NoPosition);
    }
}

fn drag_candidate(
    data: &TermData,
    meeting: MeetingRef,
    day: i32,
    start: i32,
) -> Option<(Placement, Evaluation)> {
    let day = *data.setup.days.get(usize::try_from(day).ok()?)?;
    let start = u8::try_from(start).ok()?;
    if usize::from(start) >= data.setup.periods.len() {
        return None;
    }
    let current = placed(data, meeting)?;
    let candidate = Placement {
        day,
        start,
        ..current.clone()
    };
    let check = Evaluator::new(data).evaluate(&candidate, &data.placements);
    Some((candidate, check))
}

fn apply_move(ui: &AppWindow, session: &mut Session) {
    let (Some(data), Some(draft)) = (&session.data, &session.workbench.draft) else {
        return;
    };
    if draft.base != session.revision {
        session.workbench.error = Some(WorkError::Stale);
        return;
    }
    if !draft.assessment.can_assign()
        || placed(data, draft.placement.meeting) == Some(&draft.placement)
    {
        return;
    }
    let key = data.setup.key;
    let ops = vec![Op::Put(Record::Placement(draft.placement.clone()))];
    let meta = ChangeMeta::direct("Placed meeting").base(draft.base);
    save(ui, session, key, ops, meta);
}

fn toggle_lock(ui: &AppWindow, session: &mut Session) {
    let (Some(data), Some(meeting)) = (&session.data, session.workbench.selected) else {
        return;
    };
    let Some(placement) = placed(data, meeting) else {
        return;
    };
    let change = Placement {
        locked: !placement.locked,
        ..placement.clone()
    };
    let summary = if change.locked {
        "Locked meeting"
    } else {
        "Unlocked meeting"
    };
    let meta = ChangeMeta::direct(summary).base(session.revision);
    save(
        ui,
        session,
        data.setup.key,
        vec![Op::Put(Record::Placement(change))],
        meta,
    );
}

fn apply_history(ui: &AppWindow, session: &mut Session, undo: bool) {
    let (Some(workspace), Some(data)) = (&session.workspace, &session.data) else {
        return;
    };
    let key = data.setup.key;
    let result = if undo {
        workspace.undo()
    } else {
        workspace.redo()
    };
    after_save(ui, session, key, result);
}

fn save(
    ui: &AppWindow,
    session: &mut Session,
    key: TermKey,
    ops: Vec<Op>,
    meta: ChangeMeta,
) -> bool {
    let Some(workspace) = &session.workspace else {
        return false;
    };
    let result = workspace.apply(Some(key), ops, meta);
    let committed = result.is_ok();
    after_save(ui, session, key, result);
    committed
}

fn after_save(
    ui: &AppWindow,
    session: &mut Session,
    key: TermKey,
    result: auler_store::Result<auler_store::Applied>,
) {
    match result {
        Ok(_) => {
            session.workbench.draft = None;
            session.workbench.choosing = false;
            if refresh(ui, session, Some(key)).is_err() {
                session.workbench.error = Some(WorkError::Saved);
            }
        }
        Err(StoreError::StaleRevision { .. }) => {
            let _ = refresh(ui, session, Some(key));
            session.workbench.error = Some(WorkError::Stale);
        }
        Err(error) => session.workbench.error = Some(WorkError::Store(error)),
    }
}

fn start_solver(session: &mut Session) {
    let Some(data) = &session.data else { return };
    let work = &mut session.workbench;
    let scope = work.solver_scope;
    if scope == 2 && work.selected.is_none() {
        return;
    }
    let mut input = data.clone();
    let locks: BTreeMap<_, _> = data
        .placements
        .iter()
        .map(|p| (p.meeting, p.locked))
        .collect();
    for placement in &mut input.placements {
        if (scope == 1 && placement.room.is_some())
            || (scope == 2 && Some(placement.meeting) != work.selected)
        {
            placement.locked = true;
        }
    }
    let base = session.revision;
    let selected = (scope == 2).then(|| work.selected.into_iter().collect::<BTreeSet<_>>());
    let signal = Arc::new(AtomicBool::new(false));
    let cancelled = signal.clone();
    let (events, receiver) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("auler-scheduler".into())
        .spawn(move || {
            let mut result = LocalSolver.solve_scope(
                &input,
                SolveOptions::default(),
                &cancelled,
                &mut |progress| {
                    let _ = events.send(JobEvent::Progress(progress));
                },
                selected.as_ref(),
            );
            for placement in &mut result.placements {
                placement.locked = locks.get(&placement.meeting).copied().unwrap_or_default();
            }
            let _ = events.send(JobEvent::Finished(Box::new(SolverPreview { result, base })));
        });
    if thread.is_ok() {
        work.draft = None;
        work.solver = SolverState::Running(Job {
            signal,
            events: receiver,
            progress: None,
        });
    } else {
        work.error = Some(WorkError::SolverStopped);
    }
}

fn poll_solver(work: &mut Workbench) {
    let SolverState::Running(job) = &mut work.solver else {
        work.timer.stop();
        return;
    };
    loop {
        match job.events.try_recv() {
            Ok(JobEvent::Progress(progress)) => job.progress = Some(progress),
            Ok(JobEvent::Finished(mut result)) => {
                if job.signal.load(Ordering::Relaxed) {
                    result.result.status = SolveStatus::Cancelled;
                }
                work.solver = SolverState::Preview(*result);
                work.timer.stop();
                break;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                work.solver = SolverState::Idle;
                work.error = Some(WorkError::SolverStopped);
                work.timer.stop();
                break;
            }
        }
    }
}

fn apply_solver(ui: &AppWindow, session: &mut Session) {
    let (Some(data), SolverState::Preview(preview)) = (&session.data, &session.workbench.solver)
    else {
        return;
    };
    if preview.base != session.revision {
        session.workbench.error = Some(WorkError::Stale);
        return;
    }
    if !solver_applicable(data, preview) {
        return;
    }
    let mut ops = Vec::new();
    for after in &preview.result.placements {
        if placed(data, after.meeting) != Some(after) {
            ops.push(Op::Put(Record::Placement(after.clone())));
        }
    }
    for before in &data.placements {
        if !preview
            .result
            .placements
            .iter()
            .any(|after| after.meeting == before.meeting)
        {
            ops.push(Op::Remove(RecordRef::Placement(before.meeting)));
        }
    }
    if !ops.is_empty() {
        let summary = ChangeMeta::direct("Applied schedule result")
            .origin(Origin::Solver)
            .base(preview.base);
        if save(ui, session, data.setup.key, ops, summary) {
            session.workbench.solver = SolverState::Idle;
        }
    }
}

fn solver_applicable(data: &TermData, preview: &SolverPreview) -> bool {
    let result = &preview.result;
    if result.status == SolveStatus::Cancelled
        || !result.blocked_locks.is_empty()
        || result.placements.is_empty()
    {
        return false;
    }
    let audit = Evaluator::new(data).audit(&result.placements);
    audit.removed_locks.is_empty()
        && audit.missing_rooms.is_empty()
        && audit.checks.iter().all(|row| row.evaluation.can_assign())
}

fn refresh_locations(work: &mut Workbench, data: &TermData) {
    work.locations.clear();
    let (Some(meeting), Some(day), Ok(period)) = (
        work.selected,
        data.setup.days.get(work.day).copied(),
        u8::try_from(work.period),
    ) else {
        return;
    };
    if !work.choosing || usize::from(period) >= data.setup.periods.len() {
        return;
    }
    let locked = placed(data, meeting).is_some_and(|placement| placement.locked);
    let evaluator = Evaluator::new(data);
    let base = Placement {
        meeting,
        day,
        start: period,
        room: None,
        locked,
    };
    work.locations = evaluator
        .room_candidates(&base, &data.placements)
        .into_iter()
        .map(|candidate| {
            (
                Placement {
                    meeting,
                    day,
                    start: period,
                    room: Some(candidate.room),
                    locked,
                },
                candidate.evaluation,
            )
        })
        .collect();
    work.locations
        .sort_by_key(|(_, evaluation)| (verdict(evaluation), evaluation.preference_cost()));
}

fn verdict(check: &Evaluation) -> i32 {
    if check
        .findings
        .iter()
        .any(|finding| finding.level == Level::Hard)
    {
        3
    } else if check
        .findings
        .iter()
        .any(|finding| finding.level == Level::ExternalCheck)
    {
        2
    } else if check
        .findings
        .iter()
        .any(|finding| finding.level == Level::Preference)
    {
        1
    } else {
        0
    }
}

#[derive(Clone, Copy)]
enum Filter {
    All,
    Grade(u8),
    Instructor(InstructorId),
    Room(RoomId),
}

fn filters(data: &TermData, labels: &Labels, kind: i32) -> Vec<(Filter, SharedString)> {
    let mut options = vec![(Filter::All, labels.get_work_all())];
    match kind {
        1 => {
            let years: BTreeSet<_> = data.groups.iter().filter_map(|group| group.grade).collect();
            options.extend(years.into_iter().map(|grade| {
                (
                    Filter::Grade(grade),
                    labels.invoke_grade_label(grade.into()),
                )
            }));
        }
        2 => options.extend(data.instructors.iter().map(|p| {
            (
                Filter::Instructor(p.id),
                format!("{} ({})", p.name, p.code).into(),
            )
        })),
        3 => options.extend(
            data.rooms
                .iter()
                .map(|room| (Filter::Room(room.id), room.key.to_string().into())),
        ),
        _ => {}
    }
    options
}

fn matches(data: &TermData, meeting: MeetingRef, filter: Filter, query: &str) -> bool {
    let Some(section) = data.section(meeting.section) else {
        return false;
    };
    let in_scope = match filter {
        Filter::All => true,
        Filter::Grade(grade) => section.groups.iter().any(|link| {
            data.group(link.group)
                .is_some_and(|group| group.grade == Some(grade))
        }),
        Filter::Instructor(id) => section.instructors.contains(&id),
        Filter::Room(id) => {
            placed(data, meeting).is_none_or(|p| p.room.is_none() || p.room == Some(id))
        }
    };
    in_scope
        && (query.is_empty()
            || data.course(section.course).is_some_and(|course| {
                course.name.to_lowercase().contains(query)
                    || course.code.as_str().to_lowercase().contains(query)
            })
            || section.code.as_str().to_lowercase().contains(query)
            || section.instructors.iter().any(|&id| {
                data.instructor(id)
                    .is_some_and(|person| person.name.to_lowercase().contains(query))
            }))
}

fn text_match(query: &str) -> String {
    query.trim().to_lowercase()
}

fn title(data: &TermData, labels: &Labels, meeting: MeetingRef) -> SharedString {
    let Some(section) = data.section(meeting.section) else {
        return "".into();
    };
    let Some(course) = data.course(section.course) else {
        return "".into();
    };
    let title: SharedString = format!("{} · {}", course.name, section.code).into();
    if section.split.len() > 1 {
        labels.invoke_work_part(title, i32::from(meeting.part) + 1)
    } else {
        title
    }
}

fn people(data: &TermData, section: &Section) -> String {
    section
        .instructors
        .iter()
        .filter_map(|&id| data.instructor(id).map(|person| person.name.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn when(data: &TermData, labels: &Labels, placement: &Placement) -> SharedString {
    let periods = data.setup.periods.periods();
    let start = usize::from(placement.start);
    let last = start
        + usize::from(
            data.meeting_span(placement.meeting)
                .unwrap_or(1)
                .saturating_sub(1),
        );
    let (Some(first), Some(last_period)) = (periods.get(start), periods.get(last)) else {
        return labels.get_work_no_location();
    };
    let day = labels.invoke_weekday(placement.day.index() as i32);
    let period = if start == last {
        labels.invoke_constraint_window_period(day, first.label.as_str().into())
    } else {
        labels.invoke_constraint_window_span(
            day,
            first.label.as_str().into(),
            last_period.label.as_str().into(),
        )
    };
    format!("{period} · {}–{}", first.start, last_period.end).into()
}

fn location(data: &TermData, labels: &Labels, placement: &Placement) -> SharedString {
    let time = when(data, labels, placement);
    let room = placement.room.and_then(|id| data.room(id)).map_or_else(
        || labels.get_work_choose_room().to_string(),
        |room| room.key.to_string(),
    );
    format!("{time} · {room}").into()
}

fn findings(data: &TermData, labels: &Labels, evaluation: &Evaluation) -> Vec<SharedString> {
    use auler_engine::Reason;
    let mut ordered: Vec<_> = evaluation.findings.iter().collect();
    ordered.sort_by_key(|finding| match finding.level {
        Level::Hard => 0,
        Level::ExternalCheck => 1,
        Level::Preference => 2,
        Level::Notice => 3,
    });
    ordered
        .into_iter()
        .map(|finding| {
            let what = labels.invoke_work_reason(finding.reason.code().into());
            match finding.reason {
                Reason::Overlap { other, .. } | Reason::RoomTurnaround { other, .. } => {
                    labels.invoke_constraint_detail(what, title(data, labels, other))
                }
                Reason::NotEnoughSeats { needed, available } => labels.invoke_work_shortage(
                    what,
                    needed.to_string().into(),
                    available.to_string().into(),
                ),
                Reason::NotEnoughEquipment {
                    equipment,
                    needed,
                    available,
                } => {
                    let caption = data
                        .equipment_kind(equipment)
                        .map_or(what, |equipment| equipment.name.as_str().into());
                    labels.invoke_work_shortage(
                        caption,
                        needed.to_string().into(),
                        available.to_string().into(),
                    )
                }
                _ => what,
            }
        })
        .collect()
}

fn card(data: &TermData, labels: &Labels, placement: &Placement) -> Option<CardData> {
    let section = data.section(placement.meeting.section)?;
    let room = placement.room.and_then(|id| data.room(id));
    let details = [
        people(data, section),
        room.map_or_else(
            || labels.get_work_choose_room().to_string(),
            |r| r.key.to_string(),
        ),
    ];
    Some(CardData {
        title: title(data, labels, placement.meeting),
        detail: details
            .iter()
            .filter(|value| !value.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
            .into(),
        day: data
            .setup
            .days
            .iter()
            .position(|&day| day == placement.day)? as i32,
        start: i32::from(placement.start),
        span: i32::from(data.meeting_span(placement.meeting)?),
        hue: section.course.0.rem_euclid(4) as i32,
        locked: placement.locked,
        students: section.enrollment.count.min(i32::MAX as u32) as i32,
        capacity: room.map_or(0, |room| room.available_seats.min(i32::MAX as u32) as i32),
        slot_label: when(data, labels, placement),
        ..CardData::default()
    })
}

fn pack_lanes(cards: &mut [(MeetingRef, CardData)]) {
    cards.sort_by_key(|(meeting, card)| (card.day, card.start, *meeting));
    let mut offset = 0;
    while offset < cards.len() {
        let day = cards[offset].1.day;
        let mut overlap_end = cards[offset].1.start;
        let mut end = offset;
        let mut lanes = Vec::new();
        while let Some((_, card)) = cards.get_mut(end) {
            if card.day != day || (end > offset && card.start >= overlap_end) {
                break;
            }
            let end_period = card.start + card.span;
            let lane = lanes
                .iter()
                .position(|&until| until <= card.start)
                .unwrap_or(lanes.len());
            if lane == lanes.len() {
                lanes.push(end_period);
            } else {
                lanes[lane] = end_period;
            }
            card.lane = lane as i32;
            overlap_end = overlap_end.max(end_period);
            end += 1;
        }
        for (_, card) in &mut cards[offset..end] {
            card.lanes = lanes.len() as i32;
        }
        offset = end;
    }
}

pub(crate) fn render(ui: &AppWindow, session: &mut Session) {
    let Some(data) = &session.data else {
        ui.set_workbench_view(WorkView::default());
        return;
    };
    let labels = ui.global::<Labels>();
    let work = &mut session.workbench;
    let choices = filters(data, &labels, work.filter_kind);
    let filter = choices
        .get(work.filter)
        .map_or(Filter::All, |(filter, _)| *filter);
    let query = text_match(&work.query);
    let pending_query = text_match(&work.pending_query);
    let mut cards: Vec<_> = data
        .placements
        .iter()
        .filter(|p| matches(data, p.meeting, filter, &query))
        .filter_map(|p| Some((p.meeting, card(data, &labels, p)?)))
        .collect();
    pack_lanes(&mut cards);
    work.cards = cards.iter().map(|(meeting, _)| *meeting).collect();
    work.pending = data
        .sections
        .iter()
        .flat_map(Section::meetings)
        .filter(|&meeting| {
            placed(data, meeting).is_none_or(|p| p.room.is_none())
                && matches(data, meeting, filter, &pending_query)
        })
        .collect();
    let grid_index = work
        .selected
        .and_then(|selected| work.cards.iter().position(|&meeting| meeting == selected))
        .map_or(-1, |index| index as i32);
    let selected = work
        .selected
        .filter(|meeting| data.meeting_span(*meeting).is_some());
    work.selected = selected;
    refresh_locations(work, data);
    let evaluator = Evaluator::new(data);
    let list_items = cards.iter().filter_map(|(meeting, card)| {
        let placement = placed(data, *meeting)?;
        let check = evaluator.evaluate(placement, &data.placements);
        Some(WorkItem {
            title: card.title.clone(),
            detail: location(data, &labels, placement),
            note: data
                .section(meeting.section)
                .map(|section| people(data, section).into())
                .unwrap_or_default(),
            locked: card.locked,
            tone: verdict(&check),
            selected: Some(*meeting) == selected,
            ..WorkItem::default()
        })
    });
    let pending = work
        .pending
        .iter()
        .filter_map(|&meeting| pending_item(data, &labels, meeting, selected));
    let mut view = WorkView {
        loaded: true,
        filter_kind: work.filter_kind,
        filter: work.filter.min(choices.len().saturating_sub(1)) as i32,
        filters: model(choices.into_iter().map(|(_, label)| label)),
        query: work.query.as_str().into(),
        pending_query: work.pending_query.as_str().into(),
        list: work.list,
        rows: model(list_items),
        pending: model(pending),
        choosing: work.choosing,
        day: work.day as i32,
        day_labels: model(
            data.setup
                .days
                .iter()
                .map(|day| labels.invoke_weekday(day.index() as i32)),
        ),
        period: work.period as i32,
        periods: model(
            data.setup
                .periods
                .periods()
                .iter()
                .map(|p| format!("{} · {}–{}", p.label, p.start, p.end).into()),
        ),
        locations: model(work.locations.iter().map(|(candidate, check)| {
            let room = candidate.room.and_then(|id| data.room(id));
            WorkChoice {
                title: room.map_or_else(SharedString::default, |r| r.key.to_string().into()),
                room_facts: room.map_or_else(SharedString::default, |r| room_facts(&labels, r)),
                summary: findings(data, &labels, check)
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                tone: verdict(check),
            }
        })),
        selected: grid_index,
        has_selection: selected.is_some(),
        error: work
            .error
            .as_ref()
            .map_or_else(SharedString::default, |error| error_text(&labels, error)),
        ..WorkView::default()
    };
    if let Some(meeting) = selected {
        display_selected(data, &labels, &mut view, meeting);
    }
    if let Some(draft) = &work.draft {
        display_move(data, &labels, &mut view, draft, session.revision);
    }
    display_solver(data, &labels, &mut view, work, session.revision);
    ui.set_cards(model(cards.into_iter().map(|(_, card)| card)));
    ui.set_days(model(data.setup.days.iter().map(|day| day.index() as i32)));
    ui.set_periods(model(data.setup.periods.periods().iter().enumerate().map(
        |(index, p)| {
            let previous = index
                .checked_sub(1)
                .and_then(|index| data.setup.periods.get(index));
            let gap = data
                .setup
                .periods
                .breaks()
                .iter()
                .filter(|entry| {
                    previous.is_some_and(|prev| prev.end <= entry.start) && entry.end <= p.start
                })
                .map(|entry| entry.label.as_str())
                .collect::<Vec<_>>()
                .join(" · ");
            PeriodRow {
                label: p.label.as_str().into(),
                time: format!("{}–{}", p.start, p.end).into(),
                gap: gap.into(),
            }
        },
    )));
    if let Some(workspace) = &session.workspace {
        ui.set_can_undo(workspace.can_undo().unwrap_or_default());
        ui.set_can_redo(workspace.can_redo().unwrap_or_default());
    }
    ui.set_selected(grid_index);
    ui.set_workbench_view(view);
}

fn pending_item(
    data: &TermData,
    labels: &Labels,
    meeting: MeetingRef,
    selected: Option<MeetingRef>,
) -> Option<WorkItem> {
    let section = data.section(meeting.section)?;
    let course = data.course(section.course)?;
    let requirements = requirements(data, section);
    let counts = labels.invoke_work_meeting_counts(
        i32::from(data.meeting_span(meeting)?),
        section.enrollment.count.min(i32::MAX as u32) as i32,
    );
    let reason = if placed(data, meeting).is_some() {
        labels.get_work_choose_room()
    } else {
        labels.get_work_no_location()
    };
    Some(WorkItem {
        title: title(data, labels, meeting),
        detail: counts,
        code: course.code.to_string().into(),
        note: [reason.to_string(), requirements]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
            .into(),
        locked: placed(data, meeting).is_some_and(|p| p.locked),
        tone: 1,
        selected: Some(meeting) == selected,
    })
}

fn display_selected(data: &TermData, labels: &Labels, view: &mut WorkView, meeting: MeetingRef) {
    view.title = title(data, labels, meeting);
    let Some(section) = data.section(meeting.section) else {
        return;
    };
    let metrics = labels.invoke_work_meeting_counts(
        i32::from(data.meeting_span(meeting).unwrap_or(0)),
        section.enrollment.count.min(i32::MAX as u32) as i32,
    );
    view.metrics = data
        .course(section.course)
        .map_or(metrics.clone(), |course| {
            format!("{} · {metrics}", course.code).into()
        });
    let equipment = requirements(data, section);
    if !equipment.is_empty() {
        view.requirements =
            labels.invoke_constraint_detail(labels.get_work_equipment_required(), equipment.into());
    }
    let groups = section
        .groups
        .iter()
        .filter_map(|link| data.group(link.group))
        .map(|group| group.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let info = [people(data, section), groups];
    view.detail = info
        .iter()
        .filter(|field| !field.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
        .into();
    view.current = labels.get_work_no_location();
    if let Some(current) = placed(data, meeting) {
        view.current = location(data, labels, current);
        view.placed = true;
        view.locked = current.locked;
        let evaluation = Evaluator::new(data).evaluate(current, &data.placements);
        view.findings = model(findings(data, labels, &evaluation));
        view.verdict = verdict(&evaluation);
    }
}

fn display_move(
    data: &TermData,
    labels: &Labels,
    view: &mut WorkView,
    draft: &DraftMove,
    revision: u64,
) {
    view.move_open = true;
    view.move_to = location(data, labels, &draft.placement);
    view.move_findings = model(findings(data, labels, &draft.assessment));
    view.move_verdict = verdict(&draft.assessment);
    view.move_allowed = draft.base == revision
        && draft.assessment.can_assign()
        && placed(data, draft.placement.meeting) != Some(&draft.placement);
    if draft.base != revision {
        view.move_hint = labels.get_work_stale();
    } else if placed(data, draft.placement.meeting) == Some(&draft.placement) {
        view.move_hint = labels.get_no_changes();
    }
}

fn display_solver(
    data: &TermData,
    labels: &Labels,
    view: &mut WorkView,
    work: &Workbench,
    revision: u64,
) {
    view.solver_scope = work.solver_scope;
    match &work.solver {
        SolverState::Idle => {}
        SolverState::Setup => {
            view.solver_stage = 1;
            view.solver_message = labels.invoke_work_solve_base(
                revision.min(i32::MAX as u64) as i32,
                data.placements.iter().filter(|p| p.locked).count() as i32,
                data.constraints
                    .iter()
                    .filter(|c| c.is_active() && c.strength == Some(Strength::Hard))
                    .count() as i32,
                data.constraints
                    .iter()
                    .filter(|c| c.is_active() && c.strength == Some(Strength::Soft))
                    .count() as i32,
            );
        }
        SolverState::Running(job) => {
            view.solver_stage = 2;
            view.solver_cancelling = job.signal.load(Ordering::Relaxed);
            view.solver_message = job.progress.map_or_else(
                || labels.get_work_preparing(),
                |progress| {
                    labels.invoke_work_progress(
                        progress.phase.code().into(),
                        progress.elapsed.as_millis().to_string().into(),
                        progress.best_placed as i32,
                        progress.meetings as i32,
                    )
                },
            );
        }
        SolverState::Preview(preview) => {
            view.solver_stage = 3;
            view.solver_message = labels.invoke_work_solver_result(
                preview.result.status.code().into(),
                preview.result.placements.len() as i32,
                preview.result.unassigned.len() as i32,
            );
            let (diff, changed, removed) = solver_diff(data, labels, &preview.result);
            view.solver_changes = model(diff);
            view.solver_summary = solver_summary(data, labels, &preview.result);
            view.solver_allowed =
                preview.base == revision && changed && solver_applicable(data, preview);
            if preview.result.status == SolveStatus::Cancelled {
                view.solver_hint = labels.get_work_cancelled_draft_kept();
            } else if preview.base != revision {
                view.solver_hint = labels.get_work_stale();
            } else if !changed {
                view.solver_hint = labels.get_no_changes();
            } else if !preview.result.blocked_locks.is_empty() {
                view.solver_hint = labels.get_work_locked_conflicts();
            } else if !view.solver_allowed {
                view.solver_hint = labels.get_work_solver_needs_review();
            } else if preview.result.status != SolveStatus::Cancelled && removed > 0 {
                view.solver_hint = labels.invoke_work_removal_warning(removed);
            }
        }
    }
}

fn solver_diff(
    data: &TermData,
    labels: &Labels,
    result: &SolveResult,
) -> (Vec<WorkChange>, bool, i32) {
    let mut changes = Vec::new();
    let mut removed = 0;
    for after in &result.placements {
        let before = placed(data, after.meeting);
        if before == Some(after) {
            continue;
        }
        let from = before.map_or_else(
            || labels.get_status_unassigned(),
            |placement| location(data, labels, placement),
        );
        changes.push(WorkChange {
            title: title(data, labels, after.meeting),
            detail: labels.invoke_work_diff(from, location(data, labels, after)),
            kind: i32::from(before.is_some()),
        });
    }
    for before in &data.placements {
        if result
            .placements
            .iter()
            .any(|placement| placement.meeting == before.meeting)
        {
            continue;
        }
        changes.push(WorkChange {
            title: title(data, labels, before.meeting),
            detail: labels.invoke_work_diff(
                location(data, labels, before),
                labels.get_status_unassigned(),
            ),
            kind: 2,
        });
        removed += 1;
    }
    let changed = !changes.is_empty();
    for unassigned in &result.unassigned {
        if placed(data, unassigned.meeting).is_none() {
            changes.push(WorkChange {
                title: title(data, labels, unassigned.meeting),
                detail: labels.invoke_work_unassigned(unassigned.cause.code().into()),
                kind: 2,
            });
        }
    }
    (changes, changed, removed)
}

fn solver_summary(data: &TermData, labels: &Labels, result: &SolveResult) -> SharedString {
    let evaluator = Evaluator::new(data);
    let before = evaluator.audit(&data.placements);
    let after = evaluator.audit(&result.placements);
    let hard_count = |audit: &auler_engine::ScheduleAudit| {
        audit
            .checks
            .iter()
            .filter(|row| {
                row.evaluation
                    .findings
                    .iter()
                    .any(|finding| finding.level == Level::Hard)
            })
            .count() as i32
    };
    let before_hard = hard_count(&before);
    let after_hard = hard_count(&after);
    let before_pending = (before.unassigned.len() + before.missing_rooms.len()) as i32;
    let after_pending = (after.unassigned.len() + after.missing_rooms.len()) as i32;
    let preference_cost = |audit: &auler_engine::ScheduleAudit| {
        audit
        .checks
        .iter()
        .flat_map(|row| row.evaluation.findings.iter().filter(|finding| finding.level == Level::Preference && !matches!(finding.reason, auler_engine::Reason::Overlap { other, .. } if other < row.placement.meeting)))
        .map(|finding| u64::from(finding.weight))
        .sum::<u64>()
    };
    let locks = result.placements.iter().filter(|p| p.locked).count() as i32;
    labels.invoke_work_comparison(
        before_hard,
        after_hard,
        before_pending,
        after_pending,
        preference_cost(&before).to_string().into(),
        preference_cost(&after).to_string().into(),
        locks,
    )
}

fn error_text(labels: &Labels, error: &WorkError) -> SharedString {
    match error {
        WorkError::Stale => labels.get_work_stale(),
        WorkError::NoPosition => labels.get_work_outside_grid(),
        WorkError::SolverStopped => labels.get_work_solver_stopped(),
        WorkError::Saved => labels.get_catalog_refresh_failed(),
        WorkError::Store(error) => labels.invoke_workspace_error(error.to_string().into()),
    }
}

fn requirements(data: &TermData, section: &Section) -> String {
    section
        .equipment
        .iter()
        .filter_map(|need| {
            let equipment = data.equipment_kind(need.equipment)?;
            Some(format!("{} × {}", equipment.name, need.quantity))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn room_facts(labels: &Labels, room: &Room) -> SharedString {
    let kind = match room.kind {
        RoomKind::Lecture => 0,
        RoomKind::Lab => 1,
        RoomKind::Seminar => 2,
        RoomKind::Other => 3,
    };
    labels.invoke_work_room_facts(kind, room.available_seats.min(i32::MAX as u32) as i32)
}

fn model<T: Clone + 'static>(items: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items.into_iter().collect::<Vec<_>>()))
}
