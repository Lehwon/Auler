use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use auler_domain::model::{SourceRef, Target, TermData, TermKey, TermSeason, TimeWindow};
use auler_domain::resolve::{Issue, IssueKind};
use auler_domain::review::ReviewValue;
use auler_domain::{template, time::Weekday};
use auler_io::xlsx::{ImportError, Problem, WorkbookPreview, WorksheetPreview};
use auler_store::{
    ApprovalBlocker, ChangeMeta, Disposition, DraftItem, Origin, ReviewDecision, ReviewDraft,
    ReviewStatus, RowDecision, StoreError,
};
use serde_json::Value;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::file_dialog::{FileDialog, choose};
use crate::workspace::{Session, refresh};
use crate::{AppWindow, ImportRow, ImportView, Labels};

#[derive(Default)]
pub(crate) struct ImportState {
    file: String,
    stage: i32,
    preview: Option<WorkbookPreview>,
    original: Vec<WorksheetPreview>,
    draft: Option<ReviewDraft>,
    current: usize,
    selected: HashSet<String>,
    applied: usize,
    from_review: bool,
    rejecting: bool,
    error: Option<ImportNotice>,
}

enum ImportNotice {
    InvalidExtension,
    DifferentTerm(TermKey),
    Workbook(ImportError),
    Store(StoreError),
    File(std::io::Error),
    SavedNeedsReload,
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_import_command(move |action| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        command(&ui, &mut session, action);
        render(&ui, &session);
    });
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_import_select(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        if let Ok(index) = usize::try_from(index)
            && session
                .import
                .draft
                .as_ref()
                .is_some_and(|draft| index < draft.items.len())
        {
            session.import.current = index;
            render(&ui, &session);
        }
    });
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_import_check(move |index, checked| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        let import = &mut session.import;
        if let Some(row) = usize::try_from(index)
            .ok()
            .and_then(|index| import.draft.as_ref()?.items.get(index))
            .filter(|row| selectable(row))
        {
            if checked {
                import.selected.insert(row.item.path.clone());
            } else {
                import.selected.remove(&row.item.path);
            }
            render(&ui, &session);
        }
    });
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.on_open_review(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = session.borrow_mut();
        if let Some(id) = usize::try_from(index)
            .ok()
            .and_then(|index| session.review_ids.get(index))
            .copied()
        {
            open_saved(&ui, &mut session, id);
            render(&ui, &session);
        }
    });
    let (weak, session) = (ui.as_weak(), state.clone());
    ui.window().on_winit_window_event(move |_, event| {
        let Some(ui) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        if ui.get_page() == 8
            && let winit::event::WindowEvent::DroppedFile(path) = event
            && let Ok(mut session) = session.try_borrow_mut()
            && session.import.stage <= 1
        {
            load_file(&mut session, path, false);
            render(&ui, &session);
            return EventResult::PreventDefault;
        }
        EventResult::Propagate
    });
}

fn command(ui: &AppWindow, session: &mut Session, action: i32) {
    match action {
        0 | 6 => {
            let to_review = action == 6 || session.import.from_review;
            session.import = ImportState::default();
            if to_review {
                let key = target(session);
                let _ = refresh(ui, session, key);
            }
            ui.set_page(if to_review { 4 } else { 1 });
        }
        1 | 7 => match choose(ui, FileDialog::OpenWorkbook) {
            Ok(Some(path)) => {
                load_file(session, &path, action == 7);
                if action == 7 {
                    sync_review(ui, session);
                }
            }
            Ok(None) => {}
            Err(error) => session.import.error = Some(ImportNotice::File(error)),
        },
        2 => {
            if let Err(error) = save_template(ui) {
                session.import.error = Some(ImportNotice::File(error));
            }
        }
        3 => {
            create_draft(session);
            if session.import.draft.is_some() {
                let key = target(session);
                let _ = refresh(ui, session, key);
            }
        }
        4 => apply_selected(ui, session),
        5 => {
            refresh_draft(session);
            sync_review(ui, session);
        }
        8 => {
            session.import = ImportState::default();
            ui.set_page(8);
        }
        9 => {
            let state = &mut session.import;
            if state.selected.is_empty() {
                if let Some(draft) = &state.draft {
                    state.selected = draft
                        .items
                        .iter()
                        .filter(|row| selectable(row))
                        .map(|row| row.item.path.clone())
                        .collect();
                }
            } else {
                state.selected.clear();
            }
        }
        10 => session.import.stage = 2,
        11 => session.import.rejecting = true,
        12 => session.import.rejecting = false,
        13 if session.import.rejecting => reject_review(ui, session),
        _ => {}
    }
}

fn target(session: &Session) -> Option<TermKey> {
    session.data.as_ref().map(|data| data.setup.key)
}

fn load_file(session: &mut Session, path: &Path, replace: bool) {
    let Some(target) = target(session) else {
        return;
    };
    if !replace {
        session.import = ImportState {
            file: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            stage: 1,
            ..ImportState::default()
        };
    }
    let import = &mut session.import;
    import.error = None;
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        import.error = Some(ImportNotice::InvalidExtension);
        return;
    }
    let preview = match auler_io::xlsx::import_with_preview(path) {
        Ok(preview) => preview,
        Err(error) => {
            import.error = Some(ImportNotice::Workbook(error));
            return;
        }
    };
    let term = &preview.document.term;
    if term.year != target.year || TermSeason::parse(&term.season) != Some(target.season) {
        import.error = Some(ImportNotice::DifferentTerm(target));
        return;
    }
    if replace {
        let (Some(workspace), Some(draft)) = (&session.workspace, import.draft.as_ref()) else {
            return;
        };
        match workspace.refresh_review(draft.id, draft.version, Some(preview.document)) {
            Ok(draft) => {
                import.original = preview.worksheets;
                import.file = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned();
                update_preview(import, draft, true);
            }
            Err(error) => import.error = Some(ImportNotice::Store(error)),
        }
    } else {
        import.preview = Some(preview);
    }
}

fn create_draft(session: &mut Session) {
    let (Some(term), Some(workspace)) = (target(session), &session.workspace) else {
        return;
    };
    let import = &mut session.import;
    let Some(preview) = import.preview.as_ref() else {
        return;
    };
    match workspace.create_review(
        term,
        preview.document.clone(),
        ChangeMeta::direct(import.file.clone()).origin(Origin::Xlsx),
    ) {
        Ok(draft) => {
            import.original = import
                .preview
                .take()
                .map(|preview| preview.worksheets)
                .unwrap_or_default();
            update_preview(import, draft, true);
        }
        Err(error) => import.error = Some(ImportNotice::Store(error)),
    }
}

fn open_saved(ui: &AppWindow, session: &mut Session, id: i64) {
    let Some(workspace) = &session.workspace else {
        return;
    };
    session.import = ImportState::default();
    session.import.from_review = true;
    match workspace.review(id) {
        Ok(draft) => update_preview(&mut session.import, draft, false),
        Err(error) => session.import.error = Some(ImportNotice::Store(error)),
    }
    ui.set_page(8);
}

fn refresh_draft(session: &mut Session) {
    let import = &mut session.import;
    let (Some(workspace), Some(draft)) = (&session.workspace, import.draft.as_ref()) else {
        return;
    };
    match workspace.review(draft.id).and_then(|current| {
        if draft.version == current.version {
            workspace.refresh_review(current.id, current.version, None)
        } else {
            Ok(current)
        }
    }) {
        Ok(draft) => {
            if import
                .draft
                .as_ref()
                .is_some_and(|before| before.document != draft.document)
            {
                import.original.clear();
                import.file = draft.summary.clone();
            }
            import.selected.clear();
            update_preview(import, draft, false);
        }
        Err(error) => import.error = Some(ImportNotice::Store(error)),
    }
}

fn reject_review(ui: &AppWindow, session: &mut Session) {
    let state = &mut session.import;
    let (Some(workspace), Some(draft)) = (&session.workspace, &state.draft) else {
        return;
    };
    let paths: Vec<_> = draft
        .items
        .iter()
        .filter(|row| selectable(row) && state.selected.contains(&row.item.path))
        .map(|row| row.item.path.clone())
        .collect();
    let result = if paths.is_empty() {
        workspace.close_review(
            draft.id,
            draft.version,
            ReviewDecision::Cancelled,
            "local".into(),
        )
    } else {
        workspace.reject_review_items(draft.id, draft.version, paths, "local".into())
    };
    state.rejecting = false;
    match result {
        Ok(draft) => {
            state.selected.clear();
            update_preview(state, draft, false);
        }
        Err(error) => state.error = Some(ImportNotice::Store(error)),
    }
    sync_review(ui, session);
}

fn sync_review(ui: &AppWindow, session: &mut Session) {
    let key = target(session);
    if let Err(crate::workspace::OpenError::Store(error)) = refresh(ui, session, key) {
        session.import.error = Some(ImportNotice::Store(error));
    }
}

fn apply_selected(ui: &AppWindow, session: &mut Session) {
    let import = &mut session.import;
    let (Some(workspace), Some(draft)) = (&session.workspace, import.draft.as_ref()) else {
        return;
    };
    if import.selected.is_empty() {
        return;
    }
    let paths: Vec<_> = draft
        .items
        .iter()
        .filter(|row| selectable(row) && import.selected.contains(&row.item.path))
        .map(|row| row.item.path.clone())
        .collect();
    if paths.is_empty() {
        return;
    }
    let count = paths.len();
    let id = draft.id;
    match workspace.approve_review(
        id,
        draft.version,
        paths,
        "local".into(),
        format!("desktop-{id}-{}", draft.version),
    ) {
        Ok(_) => {
            match workspace.review(id) {
                Ok(after) => update_preview(import, after, false),
                Err(_) => import.error = Some(ImportNotice::SavedNeedsReload),
            }
            import.selected.clear();
            import.stage = 3;
            import.applied = count;
            let key = target(session);
            if refresh(ui, session, key).is_err() {
                session.import.error = Some(ImportNotice::SavedNeedsReload);
            }
        }
        Err(error) => {
            let stale = matches!(
                error,
                StoreError::StaleReview | StoreError::StaleRevision { .. }
            );
            import.error = Some(ImportNotice::Store(error));
            if stale {
                sync_review(ui, session);
            }
        }
    }
}

fn selectable(row: &DraftItem) -> bool {
    row.decision.is_none()
        && row.active_path.is_some()
        && matches!(
            row.item.disposition(),
            Disposition::New | Disposition::Update
        )
}

fn update_preview(import: &mut ImportState, draft: ReviewDraft, select_all: bool) {
    import.stage = 2;
    import.applied = 0;
    import.error = None;
    if import.file.is_empty() {
        import.file = draft.summary.clone();
    }
    if select_all {
        import.selected = draft
            .items
            .iter()
            .filter(|row| selectable(row))
            .map(|row| row.item.path.clone())
            .collect();
        import.current = draft.items.iter().position(selectable).unwrap_or_default();
    } else {
        import.selected.retain(|path| {
            draft
                .items
                .iter()
                .any(|row| selectable(row) && &row.item.path == path)
        });
    }
    import.current = import.current.min(draft.items.len().saturating_sub(1));
    import.draft = Some(draft);
}

pub(crate) fn render(ui: &AppWindow, session: &Session) {
    let labels = ui.global::<Labels>();
    let state = &session.import;
    let target_term =
        target(session).map_or_else(SharedString::default, |term| term_label(&labels, term));
    let mut view = ImportView {
        stage: state.stage,
        file_name: state.file.as_str().into(),
        target_term,
        from_review: state.from_review,
        rejecting: state.rejecting,
        error: state
            .error
            .as_ref()
            .map_or_else(SharedString::default, |error| notice(&labels, error)),
        ..ImportView::default()
    };
    if let Some(preview) = &state.preview {
        view.can_review = true;
        view.summary = labels.get_import_valid_workbook();
        view.sheets = model(preview.worksheets.iter().map(|sheet| {
            labels.invoke_import_sheet(
                sheet.name.as_str().into(),
                count(sheet.last_row as usize),
                count(sheet.columns as usize),
            )
        }));
    }
    let korean = ui.get_language_index() == 0;
    if let Some(draft) = &state.draft {
        render_review(
            &labels,
            state,
            draft,
            session.revision,
            korean,
            session.data.as_ref(),
            &mut view,
        );
    }
    if let Some(ImportNotice::Workbook(ImportError::Validation(issues))) = &state.error {
        view.issues = model(
            issues
                .iter()
                .map(|issue| workbook_issue(&labels, &issue.kind, &issue.source, korean)),
        );
    }
    ui.set_import_view(view);
}

fn render_review(
    labels: &Labels,
    state: &ImportState,
    draft: &ReviewDraft,
    revision: u64,
    korean: bool,
    data: Option<&TermData>,
    view: &mut ImportView,
) {
    let counts = draft.counts();
    view.selected_count = count(state.selected.len());
    let status = draft.status(revision);
    view.complete = status.is_complete();
    view.origin = crate::review_screen::origin_label(labels, draft.origin);
    (view.status, view.status_tone) = crate::review_screen::status_label(labels, status);
    view.can_reject = state.from_review && !status.is_complete() && !draft.term_finalized;
    view.can_refresh = status == ReviewStatus::BaseChanged
        || matches!(
            state.error,
            Some(ImportNotice::Store(
                StoreError::StaleReview | StoreError::StaleRevision { .. }
            ))
        );
    view.has_remaining =
        !status.is_complete() && !matches!(state.error, Some(ImportNotice::SavedNeedsReload));
    view.can_replace =
        draft.origin == Origin::Xlsx && !status.is_complete() && !draft.term_finalized;
    view.can_select =
        !status.is_complete() && !draft.term_finalized && draft.items.iter().any(selectable);
    let blocker = draft.approval_blocker(revision);
    let missing_selection = draft
        .items
        .iter()
        .filter(|row| state.selected.contains(&row.item.path))
        .any(|row| {
            row.item.prerequisites.iter().any(|path| {
                !state.selected.contains(path)
                    && draft
                        .items
                        .iter()
                        .any(|required| &required.item.path == path && selectable(required))
            })
        });
    view.can_apply =
        blocker.is_none() && !view.can_refresh && !state.selected.is_empty() && !missing_selection;
    if view.error.is_empty()
        && let Some(blocker) = blocker
    {
        view.error = match blocker {
            ApprovalBlocker::BaseChanged { .. } => labels.get_import_stale(),
            ApprovalBlocker::DocumentIssues(_) | ApprovalBlocker::MissingSetup(_) => {
                labels.get_import_check_document()
            }
            ApprovalBlocker::Finalized(_) => labels.get_import_term_finalized(),
            _ => SharedString::default(),
        };
    }
    if view.error.is_empty() && missing_selection {
        view.error = labels.get_import_select_dependencies();
    }
    view.summary = if state.stage == 3 {
        labels.invoke_import_success(count(state.applied))
    } else {
        labels.invoke_import_review_counts(
            count(counts.new),
            count(counts.changed),
            count(counts.unchanged),
            count(counts.needs_review),
            count(counts.errors),
        )
    };
    view.rows = model(draft.items.iter().enumerate().map(|(index, row)| {
        let (state_text, tone) = match row.decision {
            Some(RowDecision::Applied { .. }) => (labels.get_applied(), 2),
            Some(RowDecision::Rejected { .. }) => (labels.get_rejected(), 0),
            Some(RowDecision::Undone { .. }) => (labels.get_import_undone(), 3),
            None => match row.item.disposition() {
                Disposition::New => (labels.get_import_new(), 2),
                Disposition::Update => (labels.get_import_updated(), 1),
                Disposition::Unchanged => (labels.get_no_changes(), 0),
                Disposition::NeedsReview => (labels.get_status_needs_review(), 3),
                Disposition::Error => (labels.get_validation_failed(), 4),
            },
        };
        ImportRow {
            title: row_title(labels, row, korean),
            state: state_text,
            tone,
            source: source_text(row.item.source.as_ref()),
            checked: state.selected.contains(&row.item.path) && selectable(row),
            enabled: state.stage == 2
                && selectable(row)
                && !status.is_complete()
                && !draft.term_finalized,
            current: state.current == index,
        }
    }));
    let mut issues: Vec<_> = draft
        .issues
        .iter()
        .map(|issue| input_issue(labels, issue))
        .collect();
    if let Some(row) = draft.items.get(state.current) {
        view.current_source = source_text(row.item.source.as_ref());
        let workbook_row = row.item.source.as_ref().and_then(|source| {
            let sheet = source.sheet.as_deref()?;
            let row = source.row?;
            let cells = state
                .original
                .iter()
                .find(|worksheet| worksheet.name == sheet)?
                .original_rows
                .get(&row)?;
            Some((row, cells))
        });
        view.has_original = workbook_row.is_some();
        let mut original = if let Some((number, cells)) = workbook_row {
            cells
                .iter()
                .map(|cell| format!("{}{number}   {}", cell.column, cell.text).into())
                .collect()
        } else {
            json_lines(labels, &row.item.input, korean)
        };
        let mut seen = HashSet::new();
        let prefix = format!("{}.", row.item.path);
        for (path, source) in &draft.document.locations {
            if !path.starts_with(&prefix)
                || row
                    .item
                    .source
                    .as_ref()
                    .is_some_and(|main| main.sheet == source.sheet && main.row == source.row)
            {
                continue;
            }
            let location = source_text(Some(source));
            if !seen.insert(location.clone()) {
                continue;
            }
            let text = state
                .original
                .iter()
                .find(|sheet| Some(sheet.name.as_str()) == source.sheet.as_deref())
                .and_then(|sheet| sheet.original_rows.get(&source.row?))
                .and_then(|cells| {
                    cells
                        .iter()
                        .find(|cell| Some(cell.column.as_str()) == source.column.as_deref())
                })
                .map_or("", |cell| cell.text.as_str());
            original.push(if text.is_empty() {
                location
            } else {
                format!("{location}   {text}").into()
            });
        }
        view.original = model(original);
        view.converted = model(converted_lines(labels, draft, row, data, korean));
        issues.extend(
            row.item
                .issues
                .iter()
                .map(|issue| input_issue(labels, issue)),
        );
        let missing = row
            .item
            .prerequisites
            .iter()
            .filter(|path| {
                !state.selected.contains(*path)
                    && draft
                        .items
                        .iter()
                        .any(|required| &required.item.path == *path && selectable(required))
            })
            .count();
        if missing > 0 {
            issues.push(labels.invoke_import_prerequisites(count(missing)));
        }
    }
    view.issues = model(issues);
}

fn source_text(source: Option<&SourceRef>) -> SharedString {
    let Some(source) = source else {
        return "".into();
    };
    let cell = match source.row {
        Some(row) => format!("{}{}", source.column.as_deref().unwrap_or(""), row),
        None => source.column.clone().unwrap_or_default(),
    };
    [
        source.file.as_deref().unwrap_or(""),
        source.sheet.as_deref().unwrap_or(""),
        &cell,
    ]
    .into_iter()
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
    .into()
}

fn row_title(labels: &Labels, row: &DraftItem, korean: bool) -> SharedString {
    let input = &row.item.input;
    if let (Some(course), Some(section)) = (input.get("course"), input.get("section")) {
        return format!("{} · {}", display_json(course), display_json(section)).into();
    }
    if let (Some(building), Some(number)) = (input.get("building"), input.get("number")) {
        return format!("{} {}", display_json(building), display_json(number)).into();
    }
    for key in ["name", "code", "number", "text"] {
        if let Some(value) = input.get(key)
            && !value.is_null()
        {
            return display_json(value).into();
        }
    }
    if row.item.path == "setup" {
        return labels.get_import_term_setup();
    }
    let sheet = row
        .item
        .source
        .as_ref()
        .and_then(|source| source.sheet.as_deref());
    sheet
        .and_then(auler_domain::template::find_sheet)
        .map_or_else(
            || row.item.path.as_str().into(),
            |sheet| if korean { sheet.ko } else { sheet.en }.into(),
        )
}

fn field_label(labels: &Labels, key: &str, korean: bool) -> SharedString {
    let key = match key {
        "enrollment" => "students",
        "instructors" => "instructor_codes",
        "groups" => "group_codes",
        "equipment" => "equipment_needs",
        "schedule_checked" => "confirmed_date",
        "info_source" => "source",
        "scope" => "equipment_scope",
        "grade" => "year",
        "weekly_periods" => "hours",
        "number" => "room_number",
        other => other,
    };
    let label = labels.invoke_catalog_detail_label(key.into());
    if key == "status" || label != labels.invoke_catalog_detail_label("status".into()) {
        return label;
    }
    let label = labels.invoke_import_field(key.into());
    if !label.is_empty() {
        return label;
    }
    template::SHEETS_V1
        .iter()
        .flat_map(|sheet| sheet.columns)
        .find(|column| column.key == key)
        .map_or_else(
            || key.replace('_', " ").into(),
            |column| if korean { column.ko } else { column.en }.into(),
        )
}

fn json_lines(labels: &Labels, value: &Value, korean: bool) -> Vec<SharedString> {
    match value {
        Value::Object(map) => map
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "id" | "source"))
            .filter_map(|(key, value)| {
                let text = field_text(labels, key, value, korean);
                (!text.is_empty())
                    .then(|| format!("{}   {text}", field_label(labels, key, korean)).into())
            })
            .collect(),
        value => vec![field_text(labels, "", value, korean).into()],
    }
}

fn field_text(labels: &Labels, key: &str, value: &Value, korean: bool) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => labels.invoke_import_boolean(*v).to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(text) => {
            let choices = match key {
                "class_kind" => template::CLASS_KINDS,
                "kind" | "room_kind" => template::ROOM_KINDS,
                "scope" => template::EQUIPMENT_SCOPES,
                "strength" | "required_course_overlap" => template::STRENGTHS,
                "status" => template::STATUSES,
                _ => &[],
            };
            if matches!(key, "days" | "day")
                && let Some(day) = Weekday::parse(text)
            {
                return labels.invoke_weekday(day.index() as i32).to_string();
            }
            choices
                .iter()
                .find(|choice| choice.matches(text))
                .map_or_else(
                    || text.clone(),
                    |choice| {
                        if korean {
                            choice.ko.into()
                        } else {
                            let mut text = choice.en.to_owned();
                            if let Some(first) = text.get_mut(..1) {
                                first.make_ascii_uppercase();
                            }
                            text
                        }
                    },
                )
        }
        Value::Array(entries) => entries
            .iter()
            .map(|entry| field_text(labels, key, entry, korean))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(if key == "split" { " + " } else { ", " }),
        Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "id" | "source"))
            .filter_map(|(key, value)| {
                let text = field_text(labels, key, value, korean);
                (!text.is_empty()).then(|| format!("{} {text}", field_label(labels, key, korean)))
            })
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

fn converted_lines(
    labels: &Labels,
    draft: &ReviewDraft,
    row: &DraftItem,
    data: Option<&TermData>,
    korean: bool,
) -> Vec<SharedString> {
    let Some(after) = row.item.after.as_ref() else {
        return Vec::new();
    };
    let after = display_fields(labels, after, data, draft, true);
    let Some(before) = row.item.before.as_ref() else {
        return json_lines(labels, &after, korean);
    };
    let before = display_fields(labels, before, data, draft, false);
    let (Some(previous), Some(proposed)) = (before.as_object(), after.as_object()) else {
        return json_lines(labels, &after, korean);
    };
    let keys: std::collections::BTreeSet<_> = previous.keys().chain(proposed.keys()).collect();
    let changed: Vec<_> = keys
        .into_iter()
        .filter_map(|key| {
            let before = field_text(
                labels,
                key,
                previous.get(key).unwrap_or(&Value::Null),
                korean,
            );
            let after = field_text(
                labels,
                key,
                proposed.get(key).unwrap_or(&Value::Null),
                korean,
            );
            (before != after).then(|| {
                labels.invoke_catalog_change_line(
                    field_label(labels, key, korean),
                    before.into(),
                    after.into(),
                )
            })
        })
        .collect();
    if changed.is_empty() {
        json_lines(labels, &after, korean)
    } else {
        changed
    }
}

fn reference<'a, T>(
    draft: &'a ReviewDraft,
    proposed: bool,
    saved: Option<&'a T>,
    corresponding: impl Fn(&ReviewValue) -> Option<&T>,
) -> Option<&'a T> {
    if !proposed {
        return saved;
    }
    draft
        .items
        .iter()
        .find_map(|row| {
            if row.decision.is_some() {
                return None;
            }
            row.item.after.as_ref().and_then(&corresponding)
        })
        .or(saved)
}

fn display_fields(
    labels: &Labels,
    value: &ReviewValue,
    data: Option<&TermData>,
    draft: &ReviewDraft,
    proposed: bool,
) -> Value {
    let json = serde_json::to_value(value).unwrap_or(Value::Null);
    let Some(mut fields) = json.get("value").and_then(Value::as_object).cloned() else {
        return json;
    };
    fields.remove("id");
    fields.remove("source");
    fields.remove("note");
    let course = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.course(id)),
            |value| match value {
                ReviewValue::Course(course) if course.id == id => Some(course),
                _ => None,
            },
        )
    };
    let section = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.section(id)),
            |value| match value {
                ReviewValue::Section(section) if section.id == id => Some(section),
                _ => None,
            },
        )
    };
    let instructor = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.instructor(id)),
            |value| match value {
                ReviewValue::Instructor(person) if person.id == id => Some(person),
                _ => None,
            },
        )
        .map(|person| format!("{} ({})", person.name, person.code))
    };
    let group = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.group(id)),
            |value| match value {
                ReviewValue::Group(group) if group.id == id => Some(group),
                _ => None,
            },
        )
        .map(|group| format!("{} ({})", group.name, group.code))
    };
    let room = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.room(id)),
            |value| match value {
                ReviewValue::Room(room) if room.id == id => Some(room),
                _ => None,
            },
        )
        .map(|room| room.key.to_string())
    };
    let equipment = |id| {
        reference(
            draft,
            proposed,
            data.and_then(|data| data.equipment_kind(id)),
            |value| match value {
                ReviewValue::Equipment(equipment) if equipment.id == id => Some(equipment),
                _ => None,
            },
        )
        .map(|equipment| format!("{} ({})", equipment.name, equipment.code))
    };
    let section_title = |id| {
        let section = section(id)?;
        Some(format!(
            "{} · {}",
            course(section.course)?.code,
            section.code
        ))
    };
    let current_setup = data.map(|data| &data.setup);
    let setup = reference(draft, proposed, current_setup, |value| match value {
        ReviewValue::Setup(setup) => Some(setup),
        _ => None,
    });
    let period_label = |index: u8| {
        setup
            .and_then(|setup| setup.periods.periods().get(usize::from(index)))
            .map_or_else(
                || (u16::from(index) + 1).to_string(),
                |period| period.label.clone(),
            )
    };
    match value {
        ReviewValue::Setup(setup) => {
            fields.remove("key");
            fields.remove("periods");
            fields.remove("rules");
            fields.insert(
                "term".into(),
                term_label(labels, setup.key).to_string().into(),
            );
            fields.insert(
                "periods".into(),
                format_slots(
                    setup
                        .periods
                        .periods()
                        .iter()
                        .map(|p| (&p.label, p.start, p.end)),
                )
                .into(),
            );
            fields.insert(
                "breaks".into(),
                format_slots(
                    setup
                        .periods
                        .breaks()
                        .iter()
                        .map(|b| (&b.label, b.start, b.end)),
                )
                .into(),
            );
            fields.insert(
                "required_course_overlap".into(),
                serde_json::to_value(setup.rules.required_course_overlap).unwrap_or_default(),
            );
            fields.insert(
                "block_may_cross_break".into(),
                setup.rules.block_may_cross_break.into(),
            );
            fields.insert(
                "room_turnaround_minutes".into(),
                setup.rules.room_turnaround_minutes.into(),
            );
        }
        ReviewValue::Room(value) => {
            fields.remove("key");
            fields.remove("equipment");
            let kind = fields.remove("kind").unwrap_or_default();
            fields.insert("room_kind".into(), kind);
            fields.insert("building".into(), value.key.building.clone().into());
            fields.insert("room_number".into(), value.key.number.clone().into());
            fields.insert(
                "room_equipment".into(),
                value
                    .equipment
                    .iter()
                    .filter_map(|gear| {
                        let name = equipment(gear.equipment)?;
                        let range = gear
                            .inspection
                            .map(|range| format!(" · {}–{}", range.from, range.until))
                            .unwrap_or_default();
                        Some(format!(
                            "{name} · {} / {}{range}",
                            gear.available, gear.owned
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into(),
            );
        }
        ReviewValue::Section(value) => {
            fields.remove("course");
            fields.remove("code");
            fields.remove("enrollment");
            let kind = fields.remove("kind").unwrap_or_default();
            fields.insert("class_kind".into(), kind);
            fields.insert("section".into(), value.code.to_string().into());
            if let Some(course) = course(value.course) {
                fields.insert("course_code".into(), course.code.to_string().into());
            }
            fields.insert(
                "instructors".into(),
                value
                    .instructors
                    .iter()
                    .filter_map(|id| instructor(*id))
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into(),
            );
            fields.insert(
                "groups".into(),
                value
                    .groups
                    .iter()
                    .filter_map(|link| {
                        let group = group(link.group)?;
                        Some(if link.required {
                            format!("{group} · {}", labels.get_import_required())
                        } else {
                            group
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into(),
            );
            fields.insert(
                "equipment".into(),
                value
                    .equipment
                    .iter()
                    .filter_map(|need| {
                        Some(format!(
                            "{} × {}",
                            equipment(need.equipment)?,
                            need.quantity
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into(),
            );
            fields.insert("students".into(), value.enrollment.count.into());
            fields.insert("provisional".into(), value.enrollment.provisional.into());
        }
        ReviewValue::Constraint(value) => {
            fields.remove("period");
            fields.remove("rule");
            let target = match value.target {
                Target::Instructor(id) => instructor(id),
                Target::Section(id) => section_title(id),
                Target::Group(id) => group(id),
                Target::Room(id) => room(id),
            };
            fields.insert("target".into(), target.unwrap_or_default().into());
            if let Some(range) = value.period {
                fields.insert(
                    "period".into(),
                    format!("{}–{}", range.from, range.until).into(),
                );
            }
            if let Some(auler_domain::model::Rule::AvoidTimes { windows }) = &value.rule {
                fields.insert(
                    "windows".into(),
                    windows
                        .iter()
                        .map(|window| {
                            let day = labels.invoke_weekday(window.day().index() as i32);
                            match window {
                                TimeWindow::Periods { first, last, .. } => format!(
                                    "{day} {}–{}",
                                    period_label(*first),
                                    period_label(*last)
                                ),
                                TimeWindow::Clock { start, end, .. } => {
                                    format!("{day} {start}–{end}")
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                        .into(),
                );
            }
        }
        ReviewValue::Placement(value) => {
            fields.remove("meeting");
            fields.remove("start");
            fields.insert(
                "section".into(),
                section_title(value.meeting.section)
                    .unwrap_or_default()
                    .into(),
            );
            fields.insert("part".into(), (u16::from(value.meeting.part) + 1).into());
            fields.insert("start_period".into(), period_label(value.start).into());
            fields.insert(
                "room".into(),
                value.room.and_then(room).unwrap_or_default().into(),
            );
        }
        ReviewValue::Course(_) => {
            let name = fields.remove("name").unwrap_or_default();
            fields.insert("course".into(), name);
        }
        _ => {}
    }
    Value::Object(fields)
}

fn format_slots<'a>(
    slots: impl Iterator<
        Item = (
            &'a String,
            auler_domain::time::ClockTime,
            auler_domain::time::ClockTime,
        ),
    >,
) -> String {
    slots
        .map(|(label, start, end)| format!("{label} · {start}–{end}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_json(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn input_issue(labels: &Labels, issue: &Issue) -> SharedString {
    let problem = match &issue.kind {
        IssueKind::NumericCode { .. } => labels.get_import_numeric_code(),
        IssueKind::AmbiguousRoom { .. } => labels.get_import_ambiguous_room(),
        IssueKind::AmbiguousInstructor { .. } | IssueKind::InstructorNameMismatch { .. } => {
            labels.get_import_instructor()
        }
        IssueKind::UnknownReference { .. } => labels.get_catalog_unknown_code(),
        IssueKind::Duplicate { .. } | IssueKind::DuplicateRow { .. } => {
            labels.get_catalog_duplicate()
        }
        _ => labels.get_import_check_value(),
    };
    let at = source_text(issue.source.as_ref());
    labels.invoke_import_diagnostic(
        if at.is_empty() {
            issue.path.as_str().into()
        } else {
            at
        },
        problem,
    )
}

fn workbook_issue(
    labels: &Labels,
    problem: &Problem,
    source: &SourceRef,
    korean: bool,
) -> SharedString {
    let column_label = |key: &str| {
        let sheet = source.sheet.as_deref().and_then(template::find_sheet);
        sheet
            .and_then(|sheet| sheet.columns.iter().find(|column| column.key == key))
            .map_or_else(
                || field_label(labels, key, korean),
                |column| if korean { column.ko } else { column.en }.into(),
            )
    };
    let message = match problem {
        Problem::MissingSheet { key } | Problem::DuplicateSheet { key } => {
            let spec = if *key == auler_domain::template::INFO_SHEET.key {
                Some(&auler_domain::template::INFO_SHEET)
            } else {
                auler_domain::template::find_sheet(key)
            };
            let title = spec
                .map_or(*key, |sheet| if korean { sheet.ko } else { sheet.en })
                .into();
            labels.invoke_import_check_sheet(title)
        }
        Problem::Formula => labels.get_import_formula(),
        Problem::UnsupportedVersion { .. } => labels.get_import_version(),
        Problem::TermRowCount { .. } => labels.get_import_one_term(),
        Problem::UnknownSheet => labels.get_import_unknown_sheet(),
        Problem::MissingColumn { key } => labels.invoke_import_missing_column(column_label(key)),
        Problem::DuplicateColumn { key } => {
            labels.invoke_import_duplicate_column(column_label(key))
        }
        Problem::MissingValue { key } => labels.invoke_import_missing_value(column_label(key)),
        Problem::InvalidValue { key, .. } => labels.invoke_import_invalid_value(column_label(key)),
        Problem::UnknownColumn { found } if found.is_empty() => labels.get_import_headers(),
        Problem::UnknownColumn { found } => {
            labels.invoke_import_unknown_column(found.as_str().into())
        }
        Problem::ConflictingConstraint { key, field } => {
            labels.invoke_import_conflicting_field(key.as_str().into(), column_label(field))
        }
    };
    labels.invoke_import_diagnostic(source_text(Some(source)), message)
}

fn notice(labels: &Labels, error: &ImportNotice) -> SharedString {
    match error {
        ImportNotice::InvalidExtension => labels.get_import_choose_xlsx(),
        ImportNotice::DifferentTerm(term) => {
            labels.invoke_import_wrong_term(term_label(labels, *term))
        }
        ImportNotice::Workbook(ImportError::Validation(_)) => labels.get_import_invalid_workbook(),
        ImportNotice::Workbook(_) => labels.get_import_unreadable(),
        ImportNotice::Store(StoreError::StaleReview | StoreError::StaleRevision { .. }) => {
            labels.get_import_stale()
        }
        ImportNotice::Store(StoreError::ReviewBlocked(_)) => {
            labels.get_import_select_dependencies()
        }
        ImportNotice::Store(StoreError::Finalized(_)) => labels.get_import_term_finalized(),
        ImportNotice::Store(error) => labels.invoke_workspace_error(error.to_string().into()),
        ImportNotice::File(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            labels.get_workspace_exists()
        }
        ImportNotice::File(error) => labels.invoke_workspace_error(error.to_string().into()),
        ImportNotice::SavedNeedsReload => labels.get_catalog_refresh_failed(),
    }
}

fn save_template(ui: &AppWindow) -> std::io::Result<()> {
    let Some(path) = choose(ui, FileDialog::SaveWorkbook)? else {
        return Ok(());
    };
    let bytes = if ui.get_language_index() == 0 {
        include_bytes!("../../templates/auler-standard-v1.ko.xlsx").as_slice()
    } else {
        include_bytes!("../../templates/auler-standard-v1.en.xlsx").as_slice()
    };
    std::fs::File::create_new(path)?.write_all(bytes)
}

fn term_label(labels: &Labels, term: TermKey) -> SharedString {
    let season = TermSeason::ALL
        .iter()
        .position(|season| *season == term.season)
        .unwrap_or_default();
    labels.invoke_academic_term(i32::from(term.year), season as i32)
}

fn count(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn model<T: Clone + 'static>(items: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items.into_iter().collect::<Vec<_>>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auler_domain::resolve::resolve;
    use auler_domain::template::{SHEETS_V1, XLSX_TEMPLATE_V1};
    use auler_store::Workspace;
    use i_slint_backend_testing::{TestingBackend, TestingBackendOptions};
    use rust_xlsxwriter::Workbook;
    use slint::Model;

    fn fixture(path: &Path) {
        let mut workbook = Workbook::new();
        workbook
            .add_worksheet()
            .set_name("Info")
            .unwrap()
            .write_string(1, 1, XLSX_TEMPLATE_V1)
            .unwrap();
        for spec in SHEETS_V1.iter().filter(|spec| !spec.optional) {
            let sheet = workbook.add_worksheet().set_name(spec.key).unwrap();
            for (index, field) in spec.columns.iter().enumerate() {
                let col = index as u16;
                sheet.write_string(0, col, field.key).unwrap();
                let value = match (spec.key, field.key) {
                    ("term", "year") => "2027",
                    ("term", "season") => "1",
                    ("term", "days") => "mon",
                    ("periods", "label") => "1",
                    ("periods", "start") => "09:00",
                    ("periods", "end") => "09:50",
                    ("courses", "code") | ("sections", "course") => "CS01",
                    ("courses", "name") => "자료구조",
                    ("sections", "section") => "01",
                    ("sections", "enrollment") => "28",
                    ("sections", "weekly_periods") => "1",
                    ("sections", "kind") => "lecture",
                    _ => "",
                };
                if !value.is_empty() {
                    sheet.write_string(1, col, value).unwrap();
                    if spec.key == "sections" {
                        sheet
                            .write_string(2, col, if field.key == "section" { "02" } else { value })
                            .unwrap();
                    }
                }
            }
        }
        workbook.save(path).unwrap();
    }

    #[test]
    fn workbook_preview_applies_only_the_confirmed_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("개설강좌.xlsx");
        fixture(&path);
        let doc = auler_io::xlsx::import(&path).unwrap();
        let setup = resolve(&doc).unwrap().setup;
        let term = setup.key;
        let workspace = Workspace::create(directory.path().join("import.auler"), "테스트").unwrap();
        workspace
            .create_term(setup, ChangeMeta::direct("Term"))
            .unwrap();
        let base = workspace.revision().unwrap();
        let mut session = Session::default();
        session.workspace = Some(workspace);
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            mock_time: true,
            threading: false,
            renderer_name: Some("software".into()),
        })))
        .unwrap();
        let ui = AppWindow::new().unwrap();
        crate::init_shell(&ui, 0);
        refresh(&ui, &mut session, Some(term)).ok().unwrap();
        load_file(&mut session, &path, false);
        render(&ui, &session);
        assert!(ui.get_import_view().can_review);
        assert!(
            ui.get_import_view()
                .sheets
                .iter()
                .any(|sheet| sheet.contains("sections"))
        );
        command(&ui, &mut session, 3);
        let draft = session.import.draft.as_ref().unwrap();
        let id = draft.id;
        assert_eq!(session.review_ids, vec![id]);
        let first = draft
            .items
            .iter()
            .position(|row| row.item.path == "sections[0]")
            .unwrap();
        session.import.current = first;
        render(&ui, &session);
        let view = ui.get_import_view();
        assert!(view.current_source.contains("개설강좌.xlsx · sections"));
        assert!(view.original.iter().any(|cell| cell.ends_with("01")));
        assert!(view.has_original);
        assert!(view.converted.iter().any(|line| line == "수업 유형   이론"));
        assert!(view.rows.row_data(first).unwrap().checked);
        assert!(view.can_apply);
        ui.set_live_workspace(true);
        ui.set_page(8);
        ui.window().set_size(slint::LogicalSize::new(1366.0, 768.0));
        ui.show().unwrap();
        snapshot(&ui, "ko-1366x768-import-review");
        crate::set_shell_language(&ui, 1);
        ui.window().set_size(slint::LogicalSize::new(1024.0, 640.0));
        slint::platform::update_timers_and_animations();
        assert!(refresh(&ui, &mut session, Some(term)).is_ok());
        assert!(
            ui.get_import_view()
                .converted
                .iter()
                .any(|line| line == "Class type   Lecture")
        );
        assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
        snapshot(&ui, "en-1024x640-import-review");
        let labels = ui.global::<Labels>();
        let diagnostic = workbook_issue(
            &labels,
            &Problem::MissingValue { key: "kind" },
            &SourceRef {
                file: Some("sample.xlsx".into()),
                sheet: Some("sections".into()),
                row: Some(2),
                column: Some("F".into()),
            },
            false,
        );
        assert!(diagnostic.contains("F2") && diagnostic.contains("Class type"));
        assert_eq!(
            session.workspace.as_ref().unwrap().revision().unwrap(),
            base
        );
        let draft_version = session.import.draft.as_ref().unwrap().version;
        let current = session
            .workspace
            .as_ref()
            .unwrap()
            .refresh_review(id, draft_version, None)
            .unwrap();
        apply_selected(&ui, &mut session);
        render(&ui, &session);
        assert!(ui.get_import_view().can_refresh);
        assert!(!ui.get_import_view().can_apply);
        snapshot(&ui, "en-1024x640-import-stale");
        command(&ui, &mut session, 5);
        assert!(session.import.error.is_none());
        assert_eq!(
            session.import.draft.as_ref().unwrap().version,
            current.version
        );
        assert!(session.import.selected.is_empty());
        command(&ui, &mut session, 9);
        session.import.selected.remove("sections[1]");
        apply_selected(&ui, &mut session);
        assert!(session.import.error.is_none());
        assert_eq!(session.import.stage, 3);
        let workspace = session.workspace.as_ref().unwrap();
        let data = workspace.load_term(term).unwrap();
        assert_eq!(data.sections.len(), 1);
        assert_eq!(data.sections[0].code.as_str(), "01");
        let saved = workspace.review(id).unwrap();
        assert_eq!(
            saved.status(workspace.revision().unwrap()),
            ReviewStatus::PartiallyApplied
        );
        render(&ui, &session);
        assert!(ui.get_import_view().has_remaining);
        assert!(!ui.get_import_view().rows.iter().any(|row| row.enabled));
        snapshot(&ui, "en-1024x640-import-partial");
        command(&ui, &mut session, 10);
        assert_eq!(session.import.stage, 2);
        command(&ui, &mut session, 9);
        apply_selected(&ui, &mut session);
        render(&ui, &session);
        assert_eq!(session.import.stage, 3);
        assert!(!ui.get_import_view().has_remaining);
        snapshot(&ui, "en-1024x640-import-applied");
        open_saved(&ui, &mut session, id);
        render(&ui, &session);
        assert!(!ui.get_import_view().has_original);
        assert!(ui.get_import_view().original.row_count() > 0);
        ui.hide().unwrap();
    }

    fn snapshot(ui: &AppWindow, name: &str) {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"));
        let directory = target.join("tmp/ui-snapshots");
        std::fs::create_dir_all(&directory).unwrap();
        let shot = ui.window().take_snapshot().unwrap();
        let file = std::fs::File::create(directory.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, shot.width(), shot.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(shot.as_bytes())
            .unwrap();
    }
}
