use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use auler_domain::input::InputDocument;
use auler_domain::model::*;
use auler_domain::resolve::resolve;
use auler_domain::time::{ClockTime, Period, PeriodTable, Weekday};
use auler_engine::{Evaluator, Level};
use auler_store::{ChangeMeta, FinalizationChecks, Origin, StoreError, TermSummary, Workspace};
use chrono::{DateTime, Datelike, Local, Utc};
use slint::{ComponentHandle, ModelRc, SharedString, StandardListViewItem, VecModel};

use crate::file_dialog::{FileDialog, choose};
use crate::{AppWindow, HomeStats, Labels, SetupFields};

const SAMPLE: &str = include_str!("../../samples/reference.json");

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) workspace: Option<Workspace>,
    pub(crate) data: Option<TermData>,
    pub(crate) revision: u64,
    pub(crate) catalog: crate::catalog::Catalog,
    pub(crate) import: crate::import::ImportState,
    pub(crate) constraints: crate::constraints::Constraints,
    pub(crate) backup: crate::backup::BackupState,
    pub(crate) export: crate::export::Export,
    pub(crate) workbench: crate::workbench::Workbench,
    pub(crate) review_screen: crate::review_screen::ReviewScreen,
    pub(crate) review_ids: Vec<i64>,
    key: Option<TermKey>,
    keys: Vec<TermKey>,
    task_targets: Vec<i32>,
}

struct View {
    department: String,
    terms: Vec<TermSummary>,
    term: Option<TermData>,
    revision: u64,
    last_finalized: Option<auler_store::FinalizedInfo>,
    changes: Vec<auler_store::ChangeRecord>,
    reviews: Vec<auler_store::ReviewOverview>,
}

impl View {
    fn load(workspace: &Workspace, selected: Option<TermKey>) -> auler_store::Result<Self> {
        let terms = workspace.terms()?;
        let key = selected.or_else(|| terms.iter().map(|term| term.key).max());
        let (last_finalized, changes, reviews) = if let Some(key) = key {
            let finalized = workspace
                .finalized(key)?
                .into_iter()
                .max_by_key(|record| record.version);
            let changes = workspace.history_for_term(key, 16)?;
            let reviews = workspace
                .reviews()?
                .into_iter()
                .filter(|record| record.term == key)
                .collect();
            (finalized, changes, reviews)
        } else {
            (None, Vec::new(), Vec::new())
        };
        let term = key
            .map(|key| {
                let frozen = terms
                    .iter()
                    .any(|summary| summary.key == key && !summary.editing);
                match last_finalized.as_ref().filter(|_| frozen) {
                    Some(snapshot) => workspace.load_finalized(key, snapshot.version),
                    None => workspace.load_term(key),
                }
            })
            .transpose()?;
        Ok(Self {
            department: workspace.department()?,
            terms,
            term,
            revision: workspace.revision()?,
            last_finalized,
            changes,
            reviews,
        })
    }
}

#[derive(Debug)]
pub(crate) enum OpenError {
    Store(StoreError),
    File(std::io::Error),
    Department,
    Calendar,
    Term,
}

impl From<StoreError> for OpenError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<std::io::Error> for OpenError {
    fn from(error: std::io::Error) -> Self {
        Self::File(error)
    }
}

type Result<T> = std::result::Result<T, OpenError>;

pub fn init(ui: &AppWindow, language: usize, initial_workspace: Option<&Path>) {
    crate::init_shell(ui, language);
    ui.set_live_workspace(true);
    ui.set_workspace_start(true);
    ui.set_page(0);
    ui.set_selected(-1);
    let state = Rc::new(RefCell::new(Session::default()));
    crate::catalog::bind(ui, &state);
    crate::import::bind(ui, &state);
    crate::constraints::bind(ui, &state);
    crate::backup::bind(ui, &state);
    crate::export::bind(ui, &state);
    crate::workbench::bind(ui, &state);
    crate::review_screen::bind(ui, &state);
    if let Some(path) = initial_workspace {
        perform(ui, || open_path(ui, &mut state.borrow_mut(), path, false));
    }

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_open_workspace(move || {
        let Some(ui) = weak.upgrade() else { return };
        perform(&ui, || {
            if let Some(path) = choose(&ui, FileDialog::OpenWorkspace)? {
                open_path(&ui, &mut session.borrow_mut(), &path, false)?;
            }
            Ok(())
        });
    });

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_open_example(move || {
        let Some(ui) = weak.upgrade() else { return };
        perform(&ui, || {
            let path = example_path();
            let mut session = session.borrow_mut();
            if path.exists() {
                return open_path(&ui, &mut session, &path, true);
            }
            let data = InputDocument::from_json(SAMPLE)
                .ok()
                .and_then(|document| resolve(&document).ok())
                .ok_or(OpenError::Calendar)?;
            std::fs::create_dir_all(path.parent().expect("sample directory"))?;
            let workspace = Workspace::create(&path, "컴퓨터공학과")?;
            workspace.import_term(
                data,
                ChangeMeta::direct("Sample department data").origin(Origin::System),
            )?;
            activate(&ui, &mut session, workspace, true)?;
            Ok(())
        });
    });

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_begin_setup(move |mode| {
        let Some(ui) = weak.upgrade() else { return };
        let active = session.borrow().key;
        let default_key = match (mode, active) {
            (2, Some(key)) => next_term(key),
            _ => TermKey {
                year: current_year(),
                season: TermSeason::First,
            },
        };
        ui.set_workspace_error(SharedString::default());
        ui.set_setup_year(i32::from(default_key.year));
        ui.set_setup_season(season_index(default_key.season));
        ui.set_setup_mode(mode);
    });

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_submit_setup(move |fields| {
        let Some(ui) = weak.upgrade() else { return };
        perform(&ui, || submit(&ui, &mut session.borrow_mut(), fields));
    });

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_term_selected(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        perform(&ui, || {
            let mut session = session.borrow_mut();
            let Some(key) = usize::try_from(index)
                .ok()
                .and_then(|index| session.keys.get(index))
                .copied()
            else {
                return Ok(());
            };
            let old = session.key;
            if let Err(error) = refresh(&ui, &mut session, Some(key)) {
                ui.set_term_index(
                    old.and_then(|old| session.keys.iter().position(|key| *key == old))
                        .map_or(-1, |index| index as i32),
                );
                return Err(error);
            }
            ui.set_page(0);
            ui.set_home_filter(0);
            ui.set_selected(-1);
            Ok(())
        });
    });

    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_open_recent_task(move |index| {
        let (Some(ui), Ok(index)) = (weak.upgrade(), usize::try_from(index)) else {
            return;
        };
        let target = session.borrow().task_targets.get(index).copied();
        if let Some(target) = target {
            ui.invoke_navigate_home_target(target);
        }
    });

    let weak = ui.as_weak();
    ui.on_language_selected(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        crate::set_shell_language(&ui, index.max(0) as usize);
        let mut session = state.borrow_mut();
        let key = session.key;
        perform(&ui, || refresh(&ui, &mut session, key));
    });
}

fn perform(ui: &AppWindow, action: impl FnOnce() -> Result<()>) {
    ui.set_workspace_error(SharedString::default());
    if let Err(error) = action() {
        let labels = ui.global::<Labels>();
        let message = match error {
            OpenError::Department => labels.get_enter_department(),
            OpenError::Term => labels.get_invalid_term(),
            OpenError::Calendar => labels.get_invalid_calendar(),
            OpenError::Store(StoreError::Locked) => labels.get_workspace_locked(),
            OpenError::Store(StoreError::AlreadyExists) => labels.get_workspace_exists(),
            OpenError::Store(StoreError::NotWorkspace) => labels.get_not_workspace(),
            OpenError::Store(StoreError::TooNew { .. }) => labels.get_workspace_too_new(),
            OpenError::Store(StoreError::TermExists(_)) => labels.get_term_exists(),
            OpenError::Store(StoreError::Io(error)) | OpenError::File(error) => {
                labels.invoke_workspace_error(error.to_string().into())
            }
            OpenError::Store(error) => labels.invoke_workspace_error(error.to_string().into()),
        };
        ui.set_workspace_error(message);
    }
}

fn submit(ui: &AppWindow, session: &mut Session, fields: SetupFields) -> Result<()> {
    let key = term_key(&fields)?;
    let mode = ui.get_setup_mode();
    if mode == 0 {
        let name = fields.department.trim();
        if name.is_empty() {
            return Err(OpenError::Department);
        }
        let setup = make_setup(&fields, key)?;
        let Some(path) = choose(ui, FileDialog::SaveWorkspace)? else {
            return Ok(());
        };
        let workspace = Workspace::create(&path, name)?;
        workspace.create_term(setup, ChangeMeta::direct("Created term"))?;
        activate(ui, session, workspace, false)?;
    } else if let Some(workspace) = &session.workspace {
        if mode == 2 {
            let source = session.key.ok_or(OpenError::Term)?;
            workspace.copy_term(source, key, ChangeMeta::direct("Copied term"))?;
        } else {
            workspace.create_term(
                make_setup(&fields, key)?,
                ChangeMeta::direct("Created term"),
            )?;
        }
        refresh(ui, session, Some(key))?;
    }
    ui.set_setup_mode(-1);
    ui.set_page(0);
    ui.set_selected(-1);
    Ok(())
}

fn open_path(ui: &AppWindow, session: &mut Session, path: &Path, example: bool) -> Result<()> {
    if let Some(workspace) = &session.workspace
        && same_path(workspace.path(), path)
    {
        let key = session.key;
        refresh(ui, session, key)?;
        ui.set_workspace_start(false);
        ui.set_page(0);
        return Ok(());
    }
    let workspace = Workspace::open(path)?;
    activate(ui, session, workspace, example)?;
    Ok(())
}

pub(crate) fn activate(
    ui: &AppWindow,
    session: &mut Session,
    workspace: Workspace,
    example: bool,
) -> auler_store::Result<()> {
    let view = View::load(&workspace, None)?;
    session.workspace = Some(workspace);
    session.backup = crate::backup::BackupState::default();
    session.catalog = crate::catalog::Catalog::default();
    session.import = crate::import::ImportState::default();
    session.constraints = crate::constraints::Constraints::default();
    session.workbench = crate::workbench::Workbench::default();
    session.review_screen = crate::review_screen::ReviewScreen::default();
    render(ui, session, view);
    ui.set_workspace_start(false);
    ui.set_workspace_open(true);
    ui.set_export_backup_active(false);
    ui.set_sample_workspace(example);
    ui.set_page(0);
    ui.set_home_filter(0);
    ui.set_selected(-1);
    Ok(())
}

pub(crate) fn refresh(ui: &AppWindow, session: &mut Session, key: Option<TermKey>) -> Result<()> {
    if let Some(workspace) = &session.workspace {
        let view = View::load(workspace, key)?;
        render(ui, session, view);
    } else {
        crate::backup::render(ui, session);
    }
    Ok(())
}

fn render(ui: &AppWindow, session: &mut Session, view: View) {
    let labels = ui.global::<Labels>();
    let term_label =
        |key: TermKey| labels.invoke_academic_term(i32::from(key.year), season_index(key.season));
    if session.key != view.term.as_ref().map(|term| term.setup.key) {
        session.catalog = crate::catalog::Catalog::default();
        session.import = crate::import::ImportState::default();
        session.constraints = crate::constraints::Constraints::default();
        session.workbench = crate::workbench::Workbench::default();
        session.review_screen = crate::review_screen::ReviewScreen::default();
    }
    session.key = view.term.as_ref().map(|term| term.setup.key);
    session.keys = view.terms.iter().map(|term| term.key).collect();
    ui.set_department(view.department.into());
    ui.set_term_options(model(session.keys.iter().copied().map(term_label)));
    let index = session
        .key
        .and_then(|key| session.keys.iter().position(|candidate| *candidate == key));
    ui.set_term_index(index.map_or(-1, |index| index as i32));
    ui.set_selected_term_label(session.key.map(term_label).unwrap_or_default());
    ui.set_draft_version(count(view.revision as usize));
    let version = view
        .last_finalized
        .as_ref()
        .map_or(0, |record| record.version as i32);
    ui.set_finalized_version(version);
    let mut stats = HomeStats {
        editing: index.is_some_and(|index| view.terms[index].editing),
        finalized: view
            .last_finalized
            .as_ref()
            .map(|record| {
                labels.invoke_last_finalized(
                    labels.invoke_finalized(version),
                    timestamp(record.at_ms),
                )
            })
            .unwrap_or_default(),
        ..HomeStats::default()
    };
    let mut finalization = FinalizationChecks::default();
    if let Some(data) = &view.term {
        let audit = Evaluator::new(data).audit(&data.placements);
        finalization = FinalizationChecks::from_audit(data, &audit);
        stats.sections = count(data.sections.len());
        stats.rooms = count(data.rooms.len());
        stats.instructors = count(data.instructors.len());
        stats.pending = count(finalization.pending_constraints);
        stats.unassigned = count(finalization.unassigned);
        stats.external_checks = count(finalization.external_checks);
        stats.can_finalize = stats.editing && finalization.required_passed();
        stats.conflicts = count(finalization.hard_conflicts);
        display_data(ui, data, &audit);
    } else {
        ui.set_rows(ModelRc::default());
        ui.set_cards(ModelRc::default());
        ui.set_periods(ModelRc::default());
        ui.set_days(ModelRc::default());
        ui.set_unassigned(ModelRc::default());
        ui.set_locked_count(0);
        ui.set_unassigned_count(0);
        ui.set_hard_conflicts(ModelRc::default());
    }
    ui.set_conflict_count(stats.conflicts);
    ui.set_home_stats(stats);

    let mut records = Vec::new();
    let mut open_reviews = 0;
    session.review_ids.clear();
    for review in &view.reviews {
        let status = review.status(view.revision);
        open_reviews += usize::from(!status.is_complete());
        let (caption, _) = crate::review_screen::status_label(&labels, status);
        session.review_ids.push(review.id);
        records.push((
            review.submitted_at_ms,
            5,
            task_row(
                review.summary.as_str().into(),
                review.origin,
                caption,
                review.submitted_at_ms,
                &labels,
            ),
        ));
    }
    for change in view.changes {
        let target = match change.origin {
            Origin::Solver => 3,
            Origin::Xlsx | Origin::ChatGpt | Origin::Mcp => 5,
            _ => 1,
        };
        let summary = match change.summary.as_str() {
            "Created term" | "Sample department data: 학기 만들기" => {
                labels.get_created_term()
            }
            "Copied term" => labels.get_copied_term(),
            "Saved constraint" => labels.get_constraint_saved(),
            "Removed constraint" => labels.get_constraint_removed(),
            "Placed meeting" => labels.get_work_placed(),
            "Locked meeting" => labels.get_work_locked(),
            "Unlocked meeting" => labels.get_work_unlocked(),
            "Applied schedule result" => labels.get_work_solver_applied(),
            "Finalized timetable" => labels.get_finalized_timetable(),
            "Opened correction draft" => labels.get_finalize_correction_opened(),
            "Sample department data" => labels.get_sample_data(),
            _ => change.summary.into(),
        };
        records.push((
            change.at_ms,
            target,
            task_row(
                summary,
                change.origin,
                labels.get_complete(),
                change.at_ms,
                &labels,
            ),
        ));
    }
    records.sort_by_key(|(at, _, _)| std::cmp::Reverse(*at));
    records.truncate(16);
    ui.set_review_count(count(open_reviews));
    session.task_targets = records.iter().map(|(_, target, _)| *target).collect();
    ui.set_recent_work(model(records.into_iter().map(|(_, _, row)| row)));
    session.data = view.term;
    session.revision = view.revision;
    crate::review_screen::reload(ui, session, view.reviews, finalization);
    crate::catalog::render(ui, session);
    crate::import::render(ui, session);
    crate::constraints::render(ui, session);
    crate::workbench::render(ui, session);
    crate::export::reload(ui, session);
    crate::backup::render(ui, session);
}

fn display_data(ui: &AppWindow, data: &TermData, audit: &auler_engine::ScheduleAudit) {
    let labels = ui.global::<Labels>();
    ui.set_hard_conflicts(model(
        audit
            .checks
            .iter()
            .filter(|record| {
                record
                    .evaluation
                    .findings
                    .iter()
                    .any(|finding| finding.level == Level::Hard)
            })
            .filter_map(|record| {
                let placement = &record.placement;
                let section = data.section(placement.meeting.section)?;
                let course = data.course(section.course)?;
                let period = data.setup.periods.get(usize::from(placement.start))?;
                Some(
                    format!(
                        "{} {} · {} {}",
                        course.name,
                        section.code,
                        labels.invoke_weekday(placement.day.index() as i32),
                        period.label
                    )
                    .into(),
                )
            }),
    ));
    let rows = data.sections.iter().filter_map(|section| {
        let course = data.course(section.course)?;
        let instructor = section
            .instructors
            .iter()
            .filter_map(|id| data.instructor(*id))
            .map(|person| person.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let grades = section
            .groups
            .iter()
            .filter_map(|link| data.group(link.group))
            .filter_map(|group| group.grade)
            .map(|grade| grade.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let needs_time = audit
            .unassigned
            .iter()
            .any(|meeting| meeting.section == section.id)
            || audit
                .missing_rooms
                .iter()
                .any(|record| record.placement.meeting.section == section.id);
        Some(row([
            course.code.to_string().into(),
            section.code.to_string().into(),
            course.name.clone().into(),
            instructor.into(),
            grades.into(),
            section.enrollment.count.to_string().into(),
            section.weekly_periods().to_string().into(),
            if needs_time {
                labels.get_status_unassigned()
            } else {
                labels.get_status_assigned()
            },
        ]))
    });
    ui.set_rows(model(rows));
    ui.set_locked_count(count(
        data.placements
            .iter()
            .filter(|placement| placement.locked)
            .count(),
    ));
    ui.set_unassigned_count(count(audit.unassigned.len() + audit.missing_rooms.len()));
}

fn task_row(
    summary: SharedString,
    origin: Origin,
    status: SharedString,
    at_ms: i64,
    labels: &Labels,
) -> ModelRc<StandardListViewItem> {
    let source = match origin {
        Origin::Solver => labels.get_local_solver(),
        Origin::Xlsx => labels.get_standard_xlsx(),
        Origin::ChatGpt => "ChatGPT".into(),
        Origin::Mcp => "MCP".into(),
        Origin::System => "Auler".into(),
        _ => labels.get_local(),
    };
    row([summary, source, status, timestamp(at_ms)])
}

fn term_key(fields: &SetupFields) -> Result<TermKey> {
    let year = u16::try_from(fields.year)
        .ok()
        .filter(|year| (1900..=9999).contains(year))
        .ok_or(OpenError::Term)?;
    let season = usize::try_from(fields.season)
        .ok()
        .and_then(|index| TermSeason::ALL.get(index))
        .copied()
        .ok_or(OpenError::Term)?;
    Ok(TermKey { year, season })
}

fn make_setup(fields: &SetupFields, key: TermKey) -> Result<TermSetup> {
    let days: Vec<_> = Weekday::ALL
        .into_iter()
        .filter(|day| fields.days & (1 << day.index()) != 0)
        .collect();
    let first = i32::from(
        ClockTime::parse(&fields.first_start)
            .ok_or(OpenError::Calendar)?
            .minutes(),
    );
    if days.is_empty()
        || !(1..=32).contains(&fields.periods)
        || !(1..=240).contains(&fields.minutes)
        || !(0..=240).contains(&fields.gap)
    {
        return Err(OpenError::Calendar);
    }
    let to_clock = |minutes| {
        u16::try_from(minutes)
            .ok()
            .and_then(ClockTime::from_minutes)
            .ok_or(OpenError::Calendar)
    };
    let periods = (0..fields.periods)
        .map(|index| {
            let start = first + index * (fields.minutes + fields.gap);
            Ok(Period {
                label: (index + 1).to_string(),
                start: to_clock(start)?,
                end: to_clock(start + fields.minutes)?,
            })
        })
        .collect::<Result<_>>()?;
    let periods = PeriodTable::new(periods, Vec::new()).map_err(|_| OpenError::Calendar)?;
    Ok(TermSetup {
        key,
        start: None,
        end: None,
        days,
        periods,
        rules: SchoolRules::default(),
    })
}

fn current_year() -> u16 {
    Local::now().year() as u16
}

fn next_term(key: TermKey) -> TermKey {
    match key.season {
        TermSeason::First | TermSeason::Summer => TermKey {
            season: TermSeason::Second,
            ..key
        },
        TermSeason::Second | TermSeason::Winter => TermKey {
            year: key.year.saturating_add(1).min(9999),
            season: TermSeason::First,
        },
    }
}

fn season_index(season: TermSeason) -> i32 {
    TermSeason::ALL
        .iter()
        .position(|candidate| *candidate == season)
        .unwrap_or_default() as i32
}

pub(crate) fn timestamp(millis: i64) -> SharedString {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|date| {
            date.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
                .into()
        })
        .unwrap_or_default()
}

fn example_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Auler")
        .join("Samples")
        .join("reference.auler")
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn count(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn row<const N: usize>(cells: [SharedString; N]) -> ModelRc<StandardListViewItem> {
    model(cells.into_iter().map(StandardListViewItem::from))
}

fn model<T: Clone + 'static>(values: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(values.into_iter().collect::<Vec<_>>()))
}
