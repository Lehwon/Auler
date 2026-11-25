use std::cell::RefCell;
use std::rc::Rc;

use auler_domain::model::{Placement, TermData, TermKey, TermSeason};
use auler_store::{
    ChangeMeta, FinalizationChecks, Origin, ReviewOverview, ReviewStatus, StoreError,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::workspace::{Session, refresh, timestamp};
use crate::{AppWindow, FinalRow, FinalView, Labels, ReviewQueueRow, ReviewQueueView};

#[derive(Default)]
pub(crate) struct ReviewScreen {
    reviews: Vec<ReviewOverview>,
    filter: i32,
    selected: Option<i64>,
    checks: FinalizationChecks,
    confirming: Option<u64>,
    acknowledged: bool,
    snapshot: Option<u32>,
    notice: Option<Notice>,
}

enum Notice {
    Stale,
    Blocked(FinalizationChecks),
    Store(StoreError),
    Saved,
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_review_queue_action(move |action, index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        let reviews = &mut session.review_screen;
        if action == 0 {
            reviews.filter = index.clamp(0, 5);
        } else if let Some(review) = usize::try_from(index)
            .ok()
            .and_then(|i| reviews.reviews.get(i))
        {
            reviews.selected = Some(review.id);
        }
        render_queue(&ui, &session);
    });
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_finalize_action(move |action, value| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        command(&ui, &mut session, action, value);
        render_final(&ui, &session);
    });
}

pub(crate) fn reload(
    ui: &AppWindow,
    session: &mut Session,
    reviews: Vec<ReviewOverview>,
    checks: FinalizationChecks,
) {
    session.review_screen.reviews = reviews;
    session.review_screen.checks = checks;
    render_queue(ui, session);
    render_final(ui, session);
}

fn command(ui: &AppWindow, session: &mut Session, action: i32, value: i32) {
    let key = session.data.as_ref().map(|data| data.setup.key);
    let state = &mut session.review_screen;
    state.notice = None;
    match action {
        0 => {
            state.confirming = None;
            state.acknowledged = false;
            state.snapshot = None;
            load_current(ui, session, key);
        }
        1 if ui.get_home_stats().editing && state.checks.required_passed() => {
            state.confirming = Some(session.revision);
            state.acknowledged = false;
        }
        2 => {
            state.confirming = None;
            state.acknowledged = false;
        }
        3 if state.confirming.is_some() => publish(ui, session),
        4 if !ui.get_home_stats().editing
            && state
                .snapshot
                .is_none_or(|version| version == ui.get_finalized_version() as u32) =>
        {
            correction(ui, session)
        }
        5 => state.acknowledged = value != 0,
        6 => {
            if let Ok(version) = u32::try_from(value)
                && version > 0
            {
                state.snapshot = Some(version);
            }
        }
        7 => state.snapshot = None,
        _ => {}
    }
}

fn load_current(ui: &AppWindow, session: &mut Session, key: Option<TermKey>) {
    if let Err(crate::workspace::OpenError::Store(error)) = refresh(ui, session, key) {
        session.review_screen.notice = Some(Notice::Store(error));
    }
}

fn publish(ui: &AppWindow, session: &mut Session) {
    let (Some(workspace), Some(data), Some(expected)) = (
        &session.workspace,
        &session.data,
        session.review_screen.confirming,
    ) else {
        return;
    };
    let key = data.setup.key;
    let request = format!("desktop-finalize-{key}-{expected}");
    let consent = session.review_screen.acknowledged;
    match workspace.finalize_checked(
        key,
        expected,
        consent,
        ChangeMeta::direct("Finalized timetable")
            .base(expected)
            .request(request),
    ) {
        Ok(info) => {
            session.review_screen.confirming = None;
            session.review_screen.acknowledged = false;
            session.review_screen.snapshot = Some(info.version);
            if refresh(ui, session, Some(key)).is_err() {
                session.review_screen.notice = Some(Notice::Saved);
            }
        }
        Err(StoreError::StaleRevision { .. }) => {
            session.review_screen.confirming = None;
            session.review_screen.acknowledged = false;
            let _ = refresh(ui, session, Some(key));
            session.review_screen.notice = Some(Notice::Stale);
        }
        Err(StoreError::FinalizationBlocked(checks)) => {
            session.review_screen.checks = checks;
            session.review_screen.notice = Some(Notice::Blocked(checks));
            if !checks.required_passed() {
                session.review_screen.confirming = None;
            }
        }
        Err(error) => session.review_screen.notice = Some(Notice::Store(error)),
    }
}

fn correction(ui: &AppWindow, session: &mut Session) {
    let (Some(workspace), Some(data)) = (&session.workspace, &session.data) else {
        return;
    };
    let key = data.setup.key;
    let result = workspace.start_correction(
        key,
        ChangeMeta::direct("Opened correction draft").base(session.revision),
    );
    match result {
        Ok(_) => {
            session.review_screen.snapshot = None;
            if refresh(ui, session, Some(key)).is_ok() {
                ui.set_home_filter(3);
                ui.set_page(3);
            } else {
                session.review_screen.notice = Some(Notice::Saved);
            }
        }
        Err(StoreError::StaleRevision { .. }) => {
            let _ = refresh(ui, session, Some(key));
            session.review_screen.notice = Some(Notice::Stale);
        }
        Err(error) => session.review_screen.notice = Some(Notice::Store(error)),
    }
}

pub(crate) fn status_label(labels: &Labels, status: ReviewStatus) -> (SharedString, i32) {
    match status {
        ReviewStatus::NoChanges => (labels.get_no_changes(), 0),
        ReviewStatus::Applicable => (labels.get_applicable(), 2),
        ReviewStatus::NeedsReview => (labels.get_status_needs_review(), 3),
        ReviewStatus::ValidationFailed => (labels.get_validation_failed(), 4),
        ReviewStatus::PartiallyApplied | ReviewStatus::PartiallyAppliedClosed => {
            (labels.get_partially_applied(), 1)
        }
        ReviewStatus::Applied => (labels.get_applied(), 2),
        ReviewStatus::Rejected => (labels.get_rejected(), 0),
        ReviewStatus::Cancelled => (labels.get_cancelled(), 0),
        ReviewStatus::BaseChanged => (labels.get_base_changed(), 3),
    }
}

pub(crate) fn origin_label(labels: &Labels, origin: Origin) -> SharedString {
    match origin {
        Origin::Xlsx => labels.get_standard_xlsx(),
        Origin::ChatGpt => "ChatGPT".into(),
        Origin::Mcp => "MCP".into(),
        Origin::Solver => labels.get_local_solver(),
        Origin::System => "Auler".into(),
        _ => labels.get_local(),
    }
}

fn matches(review: &ReviewOverview, revision: u64, filter: i32) -> bool {
    let kinds = review.kinds.unwrap_or_default();
    match filter {
        1 => kinds.data,
        2 => kinds.constraints,
        3 => kinds.placements,
        4 => review.status(revision).is_complete(),
        5 => matches!(
            review.status(revision),
            ReviewStatus::NeedsReview | ReviewStatus::ValidationFailed | ReviewStatus::BaseChanged
        ),
        _ => true,
    }
}

fn render_queue(ui: &AppWindow, session: &Session) {
    let labels = ui.global::<Labels>();
    let state = &session.review_screen;
    let rows: Vec<_> = state
        .reviews
        .iter()
        .enumerate()
        .filter(|(_, review)| matches(review, session.revision, state.filter))
        .map(|(index, review)| {
            let (status, tone) = status_label(&labels, review.status(session.revision));
            let counts = review.counts;
            ReviewQueueRow {
                index: index as i32,
                title: review.summary.as_str().into(),
                origin: origin_label(&labels, review.origin),
                status,
                tone,
                term: term_label(&labels, review.term),
                base: labels.invoke_draft(review.base_revision.min(i32::MAX as u64) as i32),
                created: timestamp(review.submitted_at_ms),
                counts: labels.invoke_import_review_counts(
                    count(counts.new),
                    count(counts.changed),
                    count(counts.unchanged),
                    count(counts.needs_review),
                    count(counts.errors),
                ),
                selected: state.selected == Some(review.id),
            }
        })
        .collect();
    let selected = rows.iter().find(|row| row.selected).cloned();
    let request = selected
        .as_ref()
        .and_then(|row| state.reviews.get(row.index as usize))
        .and_then(|row| {
            session
                .workspace
                .as_ref()?
                .review(row.id)
                .ok()?
                .submission_id
        })
        .unwrap_or_default();
    ui.set_review_queue(ReviewQueueView {
        active: state.filter,
        tabs: model((0..=5).map(|filter| {
            let matched = state
                .reviews
                .iter()
                .filter(|review| matches(review, session.revision, filter))
                .count();
            labels.invoke_review_filter(filter, count(matched))
        })),
        rows: model(rows),
        selected: selected.is_some(),
        detail: selected.unwrap_or_default(),
        request: request.into(),
        unassigned: count(state.checks.unassigned),
        conflicts: count(state.checks.hard_conflicts),
    });
}

fn render_final(ui: &AppWindow, session: &Session) {
    let labels = ui.global::<Labels>();
    let (Some(workspace), Some(data)) = (&session.workspace, &session.data) else {
        ui.set_final_view(FinalView::default());
        return;
    };
    let state = &session.review_screen;
    let checks = state.checks;
    let key = data.setup.key;
    let has_final = ui.get_finalized_version() > 0;
    let editing = ui.get_home_stats().editing;
    let selected = state
        .snapshot
        .or((!editing && has_final).then_some(ui.get_finalized_version() as u32));
    let mut view = FinalView {
        has_term: true,
        has_final,
        latest_version: ui.get_finalized_version(),
        editing,
        term: term_label(&labels, key),
        base: labels.invoke_draft(
            state
                .confirming
                .unwrap_or(session.revision)
                .min(i32::MAX as u64) as i32,
        ),
        next_version: labels.invoke_finalized(ui.get_finalized_version().saturating_add(1)),
        placed: count(checks.placed),
        locked: count(checks.locked),
        unassigned: count(checks.unassigned),
        conflicts: count(checks.hard_conflicts),
        external_checks: count(checks.external_checks),
        missing_locks: count(checks.missing_locks),
        pending: count(checks.pending_constraints),
        preferences: count(checks.preference_warnings),
        provisional: count(checks.provisional_placements),
        can_finalize: editing
            && checks.required_passed()
            && state.confirming.is_none_or(|base| base == session.revision),
        needs_ack: checks.needs_acknowledgment(),
        acknowledged: state.acknowledged,
        confirming: state.confirming.is_some(),
        hint: if checks.sections == 0 {
            labels.get_finalize_add_sections()
        } else if checks.required_passed() {
            labels.get_finalize_checks_passed()
        } else {
            labels.get_finalize_checks_pending()
        },
        error: if state
            .confirming
            .is_some_and(|base| base != session.revision)
        {
            labels.get_work_stale()
        } else {
            state
                .notice
                .as_ref()
                .map_or_else(SharedString::default, |notice| notice_text(&labels, notice))
        },
        ..FinalView::default()
    };
    if let Some(version) = selected {
        let result = workspace.finalized(key).and_then(|mut versions| {
            versions.sort_by_key(|info| info.version);
            let snapshot = workspace.load_finalized(key, version)?;
            Ok((snapshot, versions))
        });
        match result {
            Ok((snapshot, versions)) => {
                view.snapshot = true;
                view.version_index = versions
                    .iter()
                    .position(|info| info.version == version)
                    .map_or(-1, |i| i as i32);
                view.versions = model(
                    versions
                        .iter()
                        .map(|info| labels.invoke_finalized(info.version as i32)),
                );
                view.version_ids = model(versions.iter().map(|info| info.version as i32));
                view.version_date = versions
                    .iter()
                    .find(|info| info.version == version)
                    .map_or_else(SharedString::default, |info| timestamp(info.at_ms));
                view.version_title = labels.invoke_finalized(version as i32);
                view.can_correct = !editing && version == ui.get_finalized_version() as u32;
                let mut placements = snapshot.placements.iter().collect::<Vec<_>>();
                placements.sort_by_key(|p| (p.day, p.start, p.meeting));
                view.snapshot_rows = model(
                    placements
                        .into_iter()
                        .filter_map(|p| snapshot_row(&labels, &snapshot, p)),
                );
            }
            Err(error) => view.error = labels.invoke_workspace_error(error.to_string().into()),
        }
    }
    ui.set_final_view(view);
}

fn snapshot_row(labels: &Labels, data: &TermData, placement: &Placement) -> Option<FinalRow> {
    let section = data.section(placement.meeting.section)?;
    let course = data.course(section.course)?;
    let first = data.setup.periods.get(usize::from(placement.start))?;
    let span = data.meeting_span(placement.meeting)?.saturating_sub(1);
    let last = data
        .setup
        .periods
        .get(usize::from(placement.start) + usize::from(span))?;
    let day = labels.invoke_weekday(placement.day.index() as i32);
    let title: SharedString =
        format!("{} · {} · {}", course.code, course.name, section.code).into();
    let placement_title = if section.split.len() > 1 {
        labels.invoke_work_part(title, i32::from(placement.meeting.part) + 1)
    } else {
        title
    };
    let slot = if span == 0 {
        labels.invoke_constraint_window_period(day, first.label.as_str().into())
    } else {
        labels.invoke_constraint_window_span(
            day,
            first.label.as_str().into(),
            last.label.as_str().into(),
        )
    };
    Some(FinalRow {
        name: placement_title,
        time: format!("{slot} · {}–{}", first.start, last.end).into(),
        room: placement
            .room
            .and_then(|id| data.room(id))
            .map_or_else(SharedString::default, |room| room.key.to_string().into()),
        locked: placement.locked,
    })
}

fn notice_text(labels: &Labels, notice: &Notice) -> SharedString {
    match notice {
        Notice::Stale => labels.get_work_stale(),
        Notice::Blocked(checks) if checks.required_passed() => labels.get_finalize_acknowledge(),
        Notice::Blocked(_) => labels.get_finalize_checks_pending(),
        Notice::Saved => labels.get_catalog_refresh_failed(),
        Notice::Store(error) => labels.invoke_workspace_error(error.to_string().into()),
    }
}

fn term_label(labels: &Labels, term: TermKey) -> SharedString {
    let season = TermSeason::ALL
        .iter()
        .position(|season| *season == term.season)
        .unwrap_or(0);
    labels.invoke_academic_term(i32::from(term.year), season as i32)
}

fn count(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn model<T: Clone + 'static>(items: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items.into_iter().collect::<Vec<_>>()))
}
