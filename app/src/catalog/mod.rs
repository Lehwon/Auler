mod values;

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;
use std::rc::Rc;

use auler_domain::model::TermData;
use auler_engine::Evaluator;
use auler_store::{ChangeMeta, Op, RecordRef, StoreError};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::file_dialog::{FileDialog, choose};
use crate::workspace::{Session, refresh};
use crate::{AppWindow, CatalogField, CatalogRow, Labels};

use values::{Item, Key, Kind, Problem};

#[derive(Default)]
pub(crate) struct Catalog {
    tab: i32,
    subtype: i32,
    filter: i32,
    grade: Option<u8>,
    grades: Vec<u8>,
    query: SharedString,
    sort: Option<(usize, bool)>,
    hidden: HashSet<usize>,
    checked: HashSet<Key>,
    current: Option<Key>,
    visible: Vec<Key>,
    draft: Option<Draft>,
    error: Option<CatalogError>,
}

struct Draft {
    mode: i32,
    key: Option<Key>,
    base: u64,
    title: SharedString,
    fields: Vec<DraftField>,
    lines: Vec<PreviewLine>,
    operations: Vec<Op>,
}

struct DraftField {
    value: SharedString,
    issue: Option<FieldIssue>,
}

enum FieldIssue {
    Duplicate,
    Invalid {
        problem: Problem,
        record: Option<String>,
    },
}

enum PreviewLine {
    Delete,
    Reference(RecordRef),
    Change {
        record: String,
        before: String,
        after: String,
    },
}

enum CatalogError {
    BatchUnavailable,
    NoChanges,
    RefreshFailed,
    Store(StoreError),
    Input(Problem),
    Export(std::io::Error),
}

impl Catalog {
    fn kind(&self) -> Kind {
        Kind::from_view(self.tab, self.subtype)
    }

    fn close(&mut self) {
        self.draft = None;
        self.current = None;
        self.error = None;
    }
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    macro_rules! handler {
        ($name:ident, |$window:ident, $session:ident $(,$argument:ident)*| $body:block) => {{
            let weak = ui.as_weak();
            let session = state.clone();
            ui.$name(move |$($argument),*| {
                let Some($window) = weak.upgrade() else { return };
                let mut state = session.borrow_mut();
                let $session = &mut *state;
                $body
            });
        }};
    }

    handler!(on_catalog_option, |ui, session, option, value| {
        let catalog = &mut session.catalog;
        if catalog.draft.is_some() {
            render(&ui, session);
            return;
        }
        match option {
            0 => {
                catalog.tab = value.clamp(0, 4);
                catalog.subtype = 0;
            }
            1 => {
                catalog.subtype = value.clamp(0, 1);
            }
            2 => {
                catalog.filter = value.clamp(0, 3);
            }
            3 => {
                catalog.grade = value
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| catalog.grades.get(index))
                    .copied();
            }
            _ => return,
        }
        if option < 2 {
            catalog.close();
            catalog.filter = 0;
            catalog.grade = None;
            catalog.hidden.clear();
            catalog.checked.clear();
            catalog.sort = None;
        }
        render(&ui, session);
    });

    handler!(on_catalog_search, |ui, session, query| {
        session.catalog.query = query;
        render(&ui, session);
    });
    handler!(on_catalog_select, |ui, session, row| {
        if session.catalog.draft.is_none() {
            session.catalog.current = row_key(&session.catalog, row);
            session.catalog.error = None;
            render(&ui, session);
        }
    });
    handler!(on_catalog_check, |ui, session, row, checked| {
        if session.catalog.draft.is_none()
            && let Some(key) = row_key(&session.catalog, row)
        {
            if checked {
                session.catalog.checked.insert(key);
            } else {
                session.catalog.checked.remove(&key);
            }
            render(&ui, session);
        }
    });
    handler!(on_catalog_column, |ui, session, index, visible| {
        let catalog = &mut session.catalog;
        if let Ok(index) = usize::try_from(index)
            && index < catalog.kind().column_keys().len()
        {
            if visible {
                catalog.hidden.remove(&index);
            } else if catalog.hidden.len() + 1 < catalog.kind().column_keys().len() {
                catalog.hidden.insert(index);
            }
            render(&ui, session);
        }
    });
    handler!(on_catalog_sort, |ui, session, column| {
        let catalog = &mut session.catalog;
        if let Some(index) = usize::try_from(column)
            .ok()
            .and_then(|column| visible_columns(catalog).nth(column))
        {
            let descending = catalog
                .sort
                .is_some_and(|(prior, descending)| prior == index && !descending);
            catalog.sort = Some((index, descending));
            render(&ui, session);
        }
    });
    handler!(on_catalog_begin, |ui, session, mode| {
        begin(&ui, session, mode);
        render(&ui, session);
    });
    handler!(on_catalog_edit_field, |ui, session, index, value| {
        if let Ok(index) = usize::try_from(index)
            && let Some(field) = session
                .catalog
                .draft
                .as_mut()
                .and_then(|draft| draft.fields.get_mut(index))
        {
            field.value = value;
            field.issue = None;
            session.catalog.error = None;
            let model = ui.get_catalog_fields();
            if let Some(mut row) = model.row_data(index) {
                row.error = "".into();
                row.value = field.value.clone();
                model.set_row_data(index, row);
            }
            ui.set_catalog_error("".into());
        }
    });
    handler!(on_catalog_apply, |ui, session| {
        apply(&ui, session);
        render(&ui, session);
    });
    handler!(on_catalog_close, |ui, session| {
        session.catalog.close();
        render(&ui, session);
    });
    handler!(on_catalog_export, |ui, session| {
        session.catalog.error = None;
        if let Err(error) = export(&ui) {
            session.catalog.error = Some(CatalogError::Export(error));
        }
        ui.set_catalog_error(notice(
            &ui.global::<Labels>(),
            session.catalog.error.as_ref(),
        ));
    });
}

fn row_key(catalog: &Catalog, index: i32) -> Option<Key> {
    usize::try_from(index)
        .ok()
        .and_then(|index| catalog.visible.get(index))
        .copied()
}

fn visible_columns(catalog: &Catalog) -> impl Iterator<Item = usize> + '_ {
    (0..catalog.kind().column_keys().len()).filter(|index| !catalog.hidden.contains(index))
}

fn all_items(data: &TermData, catalog: &Catalog, labels: &Labels, korean: bool) -> Vec<Item> {
    let audit = Evaluator::new(data).audit(&data.placements);
    values::items(data, &audit, catalog.kind(), labels, korean)
}

pub(crate) fn render(ui: &AppWindow, session: &mut Session) {
    let catalog = &mut session.catalog;
    let labels = ui.global::<Labels>();
    let kind = catalog.kind();
    catalog.grades = session
        .data
        .iter()
        .flat_map(|data| data.groups.iter().filter_map(|group| group.grade))
        .collect();
    catalog.grades.sort_unstable();
    catalog.grades.dedup();
    catalog.grade = catalog.grade.filter(|grade| catalog.grades.contains(grade));
    let items = session.data.as_ref().map_or_else(Vec::new, |data| {
        all_items(data, catalog, &labels, ui.get_language_index() == 0)
    });
    let query = catalog.query.trim().to_lowercase();
    let mut filtered: Vec<_> = items
        .iter()
        .filter(|item| {
            let status = match catalog.filter {
                0 => true,
                3 => item.provisional,
                status => item.status == status,
            };
            let grade = catalog.grade.is_none_or(|grade| {
                kind == Kind::Section
                    && item.cells[4]
                        .split(',')
                        .any(|value| value.trim().parse::<u8>().ok() == Some(grade))
            });
            status
                && grade
                && (query.is_empty()
                    || item
                        .cells
                        .iter()
                        .any(|cell| cell.to_lowercase().contains(&query)))
        })
        .collect();
    if let Some((column, descending)) = catalog.sort {
        let numeric = matches!(
            kind.column_keys()[column],
            "students" | "year" | "total_seats" | "available_seats"
        );
        filtered.sort_by(|left, right| {
            let left = &left.cells[column];
            let right = &right.cells[column];
            let order = if numeric {
                left.parse::<u32>()
                    .ok()
                    .cmp(&right.parse::<u32>().ok())
                    .then_with(|| left.cmp(right))
            } else {
                left.cmp(right)
            };
            if descending { order.reverse() } else { order }
        });
    }
    let columns: Vec<_> = visible_columns(catalog).collect();
    let position_of_status = columns
        .iter()
        .position(|&index| kind.column_keys()[index] == "status")
        .map_or(-1, |index| index as i32);
    catalog.visible = filtered.iter().map(|item| item.key).collect();
    ui.set_catalog_rows(model(filtered.into_iter().map(|item| {
        CatalogRow {
            label: item.title.clone().into(),
            checked: catalog.checked.contains(&item.key),
            current: catalog.current == Some(item.key),
            status_column: position_of_status,
            status_tone: item.status,
            cells: model(
                columns
                    .iter()
                    .map(|&column| SharedString::from(item.cells[column].as_str())),
            ),
        }
    })));
    ui.set_catalog_columns(model(
        columns
            .iter()
            .map(|&index| labels.invoke_catalog_label(kind.column_keys()[index].into())),
    ));
    ui.set_catalog_column_weights(model(columns.iter().map(
        |&index| match kind.column_keys()[index] {
            "course" | "name" | "instructor" | "department" => 1.8,
            "course_code" | "building" | "room_kind" => 1.3,
            "section" | "year" | "students" => 0.8,
            "status" => 1.65,
            _ => 1.0,
        },
    )));
    ui.set_catalog_all_columns(model(
        kind.column_keys()
            .iter()
            .map(|&key| labels.invoke_catalog_label(key.into())),
    ));
    ui.set_catalog_visible_columns(model(
        (0..kind.column_keys().len()).map(|index| !catalog.hidden.contains(&index)),
    ));
    ui.set_catalog_sort_column(
        catalog
            .sort
            .and_then(|(index, _)| columns.iter().position(|&column| column == index))
            .map_or(-1, |index| index as i32),
    );
    ui.set_catalog_descending(catalog.sort.is_some_and(|(_, descending)| descending));
    ui.set_catalog_tab(catalog.tab);
    ui.set_catalog_subtype(catalog.subtype);
    ui.set_catalog_filter(catalog.filter);
    ui.set_catalog_grades(model(
        std::iter::once(labels.get_all_grades()).chain(
            catalog
                .grades
                .iter()
                .map(|grade| labels.invoke_grade_label(i32::from(*grade))),
        ),
    ));
    ui.set_catalog_grade(
        catalog
            .grade
            .and_then(|grade| catalog.grades.iter().position(|value| *value == grade))
            .map_or(0, |index| (index + 1) as i32),
    );
    ui.set_catalog_query(catalog.query.clone());
    ui.set_catalog_error(notice(&labels, catalog.error.as_ref()));
    ui.set_catalog_checked_count(i32::try_from(catalog.checked.len()).unwrap_or(i32::MAX));

    let (title, mode, fields, lines) = if let Some(draft) = &catalog.draft {
        let title = match draft.mode {
            1 => labels.get_catalog_add(),
            3 | 4 => labels.get_catalog_edit_selected(),
            5 => labels.invoke_catalog_delete_title(draft.title.clone()),
            _ => draft.title.clone(),
        };
        let field_keys = if draft.mode == 3 {
            kind.bulk_field()
                .map(|index| &kind.field_keys()[index..=index])
                .unwrap_or_default()
        } else {
            kind.field_keys()
        };
        let fields = field_keys
            .iter()
            .zip(&draft.fields)
            .map(|(key, field)| CatalogField {
                label: labels.invoke_catalog_label((*key).into()),
                value: field.value.clone(),
                error: match &field.issue {
                    Some(FieldIssue::Duplicate) => labels.get_catalog_duplicate(),
                    Some(FieldIssue::Invalid { problem, record }) => {
                        let reason = value_message(&labels, problem);
                        record.as_ref().map_or_else(
                            || reason.clone(),
                            |record| {
                                labels.invoke_catalog_record_error(
                                    record.as_str().into(),
                                    reason.clone(),
                                )
                            },
                        )
                    }
                    None => "".into(),
                },
            })
            .collect();
        let lines = draft
            .lines
            .iter()
            .map(|line| match line {
                PreviewLine::Delete => labels.get_catalog_delete_preview(),
                PreviewLine::Reference(reference) => session.data.as_ref().map_or_else(
                    || labels.get_catalog_other_reference(),
                    |data| dependency_label(&labels, data, *reference),
                ),
                PreviewLine::Change {
                    record,
                    before,
                    after,
                } => labels.invoke_catalog_change_line(
                    record.as_str().into(),
                    before.as_str().into(),
                    after.as_str().into(),
                ),
            })
            .collect();
        (title, draft.mode, fields, lines)
    } else if let Some(item) = items.iter().find(|item| Some(item.key) == catalog.current) {
        let values = session.data.as_ref().map_or_else(
            || item.fields.clone(),
            |data| values::readable_fields(data, item, ui.get_language_index() == 0),
        );
        let fields = kind
            .field_keys()
            .iter()
            .zip(values)
            .map(|(key, value)| CatalogField {
                label: labels.invoke_catalog_detail_label((*key).into()),
                value: value.into(),
                error: "".into(),
            })
            .collect();
        (item.title.as_str().into(), 0, fields, Vec::new())
    } else {
        ("".into(), 0, Vec::new(), Vec::new())
    };
    ui.set_catalog_detail_title(title);
    ui.set_catalog_editor(mode);
    ui.set_catalog_fields(model(fields));
    ui.set_catalog_preview(model(lines));
}

fn fields(values: impl IntoIterator<Item = String>) -> Vec<DraftField> {
    values
        .into_iter()
        .map(|value| DraftField {
            value: value.into(),
            issue: None,
        })
        .collect()
}

fn begin(ui: &AppWindow, session: &mut Session, mode: i32) {
    let (Some(data), Some(workspace)) = (&session.data, &session.workspace) else {
        return;
    };
    if !ui.get_home_stats().editing || session.catalog.draft.is_some() {
        return;
    }
    let labels = ui.global::<Labels>();
    let catalog = &mut session.catalog;
    catalog.error = None;
    let kind = catalog.kind();
    let items = all_items(data, catalog, &labels, ui.get_language_index() == 0);
    let selected = items.iter().find(|item| Some(item.key) == catalog.current);
    let mut draft = Draft {
        mode,
        key: catalog.current,
        base: session.revision,
        title: "".into(),
        fields: Vec::new(),
        lines: Vec::new(),
        operations: Vec::new(),
    };
    match mode {
        1 => {
            draft.key = None;
            draft.fields = fields(kind.default_fields(ui.get_language_index() == 0));
        }
        2 => {
            let Some(item) = selected else { return };
            draft.title = item.title.as_str().into();
            draft.fields = fields(item.fields.clone());
        }
        3 => {
            if kind.bulk_field().is_none() {
                catalog.error = Some(CatalogError::BatchUnavailable);
                return;
            }
            if catalog.checked.is_empty() {
                return;
            }
            draft.fields = fields([String::new()]);
        }
        5 => {
            let Some(item) = selected else { return };
            draft.title = item.title.as_str().into();
            match values::remove(data, kind, item.key) {
                Ok(op) => {
                    let meta = ChangeMeta::direct("Delete data record").base(draft.base);
                    if let Err(error) =
                        workspace.preview_changes(Some(data.setup.key), vec![op.clone()], meta)
                    {
                        if let StoreError::InUse { used_by, .. } = &error {
                            draft
                                .lines
                                .extend(used_by.iter().copied().map(PreviewLine::Reference));
                        }
                        catalog.error = Some(CatalogError::Store(error));
                    } else {
                        draft.lines.push(PreviewLine::Delete);
                        draft.operations.push(op);
                    }
                }
                Err(error) => catalog.error = Some(CatalogError::Input(error)),
            }
        }
        _ => return,
    }
    catalog.draft = Some(draft);
}

fn apply(ui: &AppWindow, session: &mut Session) {
    let (Some(data), Some(workspace)) = (&session.data, &session.workspace) else {
        return;
    };
    if !ui.get_home_stats().editing {
        return;
    }
    let labels = ui.global::<Labels>();
    let catalog = &mut session.catalog;
    let kind = catalog.kind();
    let Some(draft) = catalog.draft.as_ref() else {
        return;
    };
    let values: Vec<_> = draft
        .fields
        .iter()
        .map(|field| field.value.to_string())
        .collect();
    let meta = ChangeMeta::direct(match draft.mode {
        1 => "Add data record",
        4 => "Apply selected data changes",
        5 => "Delete data record",
        _ => "Edit data record",
    })
    .base(draft.base);
    let key = data.setup.key;
    catalog.error = None;
    let operations = match draft.mode {
        1 | 2 => {
            let next = if draft.mode == 1 && kind == Kind::Section {
                match workspace.next_section_id() {
                    Ok(next) => Some(next),
                    Err(error) => {
                        catalog.error = Some(CatalogError::Store(error));
                        return;
                    }
                }
            } else {
                None
            };
            match values::edit(data, kind, draft.key, &values, next) {
                Ok(op) => vec![op],
                Err(error) => {
                    show_value_problem(catalog, error, None);
                    return;
                }
            }
        }
        3 => {
            let Some(column) = kind.bulk_field() else {
                return;
            };
            let Some(value) = values.first() else { return };
            let items = all_items(data, catalog, &labels, ui.get_language_index() == 0);
            let mut operations = Vec::new();
            let mut lines = Vec::new();
            for item in items
                .iter()
                .filter(|item| catalog.checked.contains(&item.key))
            {
                let mut row = item.fields.clone();
                let before = std::mem::replace(&mut row[column], value.clone());
                if &before == value {
                    continue;
                }
                match values::edit(data, kind, Some(item.key), &row, None) {
                    Ok(op) => operations.push(op),
                    Err(problem) => {
                        show_value_problem(catalog, problem, Some(item.title.clone()));
                        return;
                    }
                }
                lines.push(PreviewLine::Change {
                    record: item.title.clone(),
                    before,
                    after: value.clone(),
                });
            }
            if operations.is_empty() {
                catalog.error = Some(CatalogError::NoChanges);
                return;
            }
            if let Err(error) = workspace.preview_changes(Some(key), operations.clone(), meta) {
                catalog.error = Some(CatalogError::Store(error));
                return;
            }
            if let Some(draft) = &mut catalog.draft {
                draft.mode = 4;
                draft.fields.clear();
                draft.operations = operations;
                draft.lines = lines;
            }
            return;
        }
        4 | 5 if !draft.operations.is_empty() => draft.operations.clone(),
        _ => return,
    };
    if let Err(error) = workspace.apply(Some(key), operations, meta) {
        if matches!(&error, StoreError::Duplicate(_))
            && let Some(draft) = &mut catalog.draft
            && matches!(draft.mode, 1 | 2)
        {
            let identity = usize::from(matches!(kind, Kind::Section | Kind::Room));
            draft.fields[identity].issue = Some(FieldIssue::Duplicate);
            return;
        }
        catalog.error = Some(CatalogError::Store(error));
        return;
    }
    catalog.close();
    catalog.checked.clear();
    if refresh(ui, session, Some(key)).is_err() {
        session.catalog.error = Some(CatalogError::RefreshFailed);
    }
}

fn show_value_problem(catalog: &mut Catalog, problem: Problem, record: Option<String>) {
    let index = if record.is_some() {
        Some(0)
    } else {
        problem.field()
    };
    if let Some(index) = index
        && let Some(field) = catalog
            .draft
            .as_mut()
            .and_then(|draft| draft.fields.get_mut(index))
    {
        field.issue = Some(FieldIssue::Invalid { problem, record });
        return;
    }
    catalog.error = Some(CatalogError::Input(problem));
}

fn value_message(labels: &Labels, error: &Problem) -> SharedString {
    match error {
        Problem::Field(_) => labels.get_catalog_field_invalid(),
        Problem::Reference(_) => labels.get_catalog_unknown_code(),
        Problem::Seats(total) => labels.invoke_catalog_seats_limit(total.to_string().into()),
        Problem::EndBeforeStart => labels.get_catalog_time_range(),
        Problem::PeriodTable(error) => {
            use auler_domain::time::PeriodTableError::*;
            match error {
                Empty => labels.get_catalog_calendar_empty(),
                DuplicateLabel(_) | EmptyLabel(_) => labels.get_catalog_period_label(),
                EmptyRange(_) => labels.get_catalog_time_range(),
                Overlap(_) | BadBreak(_) => labels.get_catalog_time_overlap(),
            }
        }
        Problem::CalendarInUse => labels.get_catalog_calendar_in_use(),
        Problem::TooManyRecords => labels.get_catalog_capacity(),
    }
}

fn notice(labels: &Labels, error: Option<&CatalogError>) -> SharedString {
    match error {
        None => "".into(),
        Some(CatalogError::BatchUnavailable) => labels.get_catalog_batch_unavailable(),
        Some(CatalogError::NoChanges) => labels.get_no_changes(),
        Some(CatalogError::RefreshFailed) => labels.get_catalog_refresh_failed(),
        Some(CatalogError::Input(problem)) => value_message(labels, problem),
        Some(CatalogError::Export(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            labels.get_catalog_export_exists()
        }
        Some(CatalogError::Export(error)) => {
            labels.invoke_catalog_export_error(error.to_string().into())
        }
        Some(CatalogError::Store(error)) => match error {
            StoreError::InUse { .. } => labels.get_catalog_in_use(),
            StoreError::Duplicate(_) => labels.get_catalog_duplicate(),
            StoreError::StaleRevision { .. } => labels.get_catalog_stale(),
            other => labels.invoke_workspace_error(other.to_string().into()),
        },
    }
}

fn dependency_label(labels: &Labels, data: &TermData, reference: RecordRef) -> SharedString {
    let find_section = |id| {
        data.section(id).and_then(|section| {
            data.course(section.course)
                .map(|course| format!("{} {}", course.name, section.code))
        })
    };
    match reference {
        RecordRef::Section(id) => {
            find_section(id).map(|title| labels.invoke_catalog_section_reference(title.into()))
        }
        RecordRef::Placement(meeting) => find_section(meeting.section)
            .map(|title| labels.invoke_catalog_section_reference(title.into())),
        RecordRef::Room(id) => data.room(id).map(|room| room.key.to_string().into()),
        RecordRef::Constraint(id) => data
            .constraints
            .iter()
            .find(|constraint| constraint.id == id)
            .and_then(|constraint| constraint.text.as_ref().map(|text| text.as_str().into())),
        _ => None,
    }
    .unwrap_or_else(|| labels.get_catalog_other_reference())
}

fn export(ui: &AppWindow) -> std::io::Result<()> {
    let Some(path) = choose(ui, FileDialog::SaveCatalog)? else {
        return Ok(());
    };
    let mut csv = String::from("\u{feff}");
    csv_row(&mut csv, ui.get_catalog_columns().iter());
    for row in ui.get_catalog_rows().iter() {
        csv_row(&mut csv, row.cells.iter());
    }
    std::fs::File::create_new(path)?.write_all(csv.as_bytes())
}

fn csv_row(output: &mut String, cells: impl IntoIterator<Item = SharedString>) {
    for (index, cell) in cells.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('"');
        if cell.trim_start().starts_with(['=', '+', '-', '@'])
            || cell.starts_with(['\t', '\r', '\n'])
        {
            output.push('\'');
        }
        output.push_str(&cell.replace('"', "\"\""));
        output.push('"');
    }
    output.push_str("\r\n");
}

fn model<T: Clone + 'static>(rows: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows.into_iter().collect::<Vec<_>>()))
}
