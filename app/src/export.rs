use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use auler_domain::model::{ConstraintId, TermKey};
use auler_io::export::{Fields, Grouping, Language, Layout, Snapshot, View};
use auler_io::pdf;
use auler_io::submission::{self, Profile};
use auler_store::FinalizedInfo;
use slint::{
    ComponentHandle, Image, ModelRc, SharedString, StandardListViewItem, Timer, TimerMode, VecModel,
};

use crate::file_dialog::{FileDialog, choose};
use crate::workspace::{Session, timestamp};
use crate::{AppWindow, ExportCommand, ExportConstraint, ExportView, Labels};

#[derive(Default)]
pub(crate) struct Export {
    identity: Option<(PathBuf, TermKey)>,
    versions: Vec<FinalizedInfo>,
    version: Option<u32>,
    snapshot: Option<Arc<Snapshot>>,
    mode: Mode,
    school_profile: Profile,
    school_profile_path: Option<PathBuf>,
    excel: bool,
    language: Option<Language>,
    grouping: i32,
    target: View,
    list: bool,
    portrait: bool,
    fields: Fields,
    conditions: BTreeSet<ConstraintId>,
    preview: Image,
    preview_width: Option<u32>,
    page: usize,
    pages: usize,
    zoom: i32,
    replacement: Option<Destination>,
    saved: Option<(PathBuf, bool)>,
    error: Option<OutputError>,
    job: Option<Receiver<Result<Finished, OutputError>>>,
    timer: Timer,
    resize_timer: Timer,
}

struct Destination {
    path: PathBuf,
    replace: bool,
    example_profile: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Timetable = 0,
    Handover = 1,
    School = 2,
}

enum Finished {
    Preview {
        png: Vec<u8>,
        page: usize,
        pages: usize,
    },
    Saved(PathBuf, bool),
}

#[derive(Debug)]
enum OutputError {
    Detail(String),
    Extension(&'static str),
    NotRegular,
    Exists,
    InvalidPreview,
    WorkerStopped,
    OpenPdf,
    Profile(submission::Error),
}

impl From<io::Error> for OutputError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Self::Exists
        } else {
            Self::Detail(error.to_string())
        }
    }
}

impl OutputError {
    fn localized(&self, labels: &Labels, language: Language) -> SharedString {
        match self {
            Self::Detail(error) => labels.invoke_export_error(error.as_str().into()),
            Self::Extension(extension) => labels.invoke_export_extension((*extension).into()),
            Self::NotRegular => labels.get_export_regular_file(),
            Self::Exists => labels.get_export_path_taken(),
            Self::InvalidPreview => labels.get_export_invalid_preview(),
            Self::WorkerStopped => labels.get_export_worker_stopped(),
            Self::OpenPdf => labels.get_export_open_pdf_error(),
            Self::Profile(error) => error.message(language).into(),
        }
    }
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_export_closed(move || {
        let (Some(ui), Ok(mut session)) = (weak.upgrade(), context.try_borrow_mut()) else {
            return;
        };
        session.export.clear_preview();
        session.export.resize_timer.stop();
        session.export.replacement = None;
        render(&ui, &session.export);
    });
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_export_viewport(move |width| {
        let (Some(ui), Ok(mut session)) = (weak.upgrade(), context.try_borrow_mut()) else {
            return;
        };
        let width = (width * ui.window().scale_factor()).round() as u32;
        let export = &mut session.export;
        let width = Some(width.clamp(100, 2400));
        if export.preview_width == width {
            return;
        }
        export.preview_width = width;
        if export.pages == 0 {
            return;
        }
        let (weak, context) = (weak.clone(), Rc::downgrade(&context));
        export.resize_timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(150),
            move || {
                let (Some(ui), Some(context)) = (weak.upgrade(), context.upgrade()) else {
                    return;
                };
                let Ok(mut session) = context.try_borrow_mut() else {
                    return;
                };
                let export = &mut session.export;
                if ui.get_page() == 5
                    && export.job.is_none()
                    && export.pages > 0
                    && export.replacement.is_none()
                {
                    resize_preview(&ui, export);
                    if export.job.is_some() {
                        poll_job(&ui, &context, export);
                    }
                    render(&ui, export);
                }
            },
        );
    });
    let (weak, state) = (ui.as_weak(), state.clone());
    ui.on_export_command(move |action, index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = state.borrow_mut();
        let export = &mut session.export;
        if export.job.is_some() {
            return;
        }
        if export.replacement.is_some()
            && !matches!(action, ExportCommand::Confirm | ExportCommand::Cancel)
        {
            return;
        }
        if let Err(error) = command(&ui, export, action, index) {
            export.error = Some(error);
        }
        if export.job.is_some() {
            poll_job(&ui, &state, export);
        }
        if matches!(action, ExportCommand::Version) {
            reload(&ui, &mut session);
        } else {
            render(&ui, export);
        }
    });
}

fn poll_job(ui: &AppWindow, state: &Rc<RefCell<Session>>, export: &Export) {
    let (weak, state) = (ui.as_weak(), Rc::downgrade(state));
    export
        .timer
        .start(TimerMode::Repeated, Duration::from_millis(40), move || {
            let (Some(ui), Some(state)) = (weak.upgrade(), state.upgrade()) else {
                return;
            };
            let Ok(mut session) = state.try_borrow_mut() else {
                return;
            };
            let export = &mut session.export;
            if !export.poll() {
                return;
            }
            if ui.get_page() != 5 {
                export.clear_preview();
            } else if export.pages > 0 && export.error.is_none() {
                resize_preview(&ui, export);
            }
            if export.job.is_some() {
                poll_job(&ui, &state, export);
            }
            render(&ui, export);
        });
}

fn resize_preview(ui: &AppWindow, export: &mut Export) {
    if export
        .preview_width
        .is_some_and(|width| width.abs_diff(export.preview.size().width) > 3)
        && let Err(error) = export.launch(ui, Some(export.page), None)
    {
        export.error = Some(error);
    }
}

pub(crate) fn reload(ui: &AppWindow, session: &mut Session) {
    let export = &mut session.export;
    let (Some(workspace), Some(data)) = (&session.workspace, &session.data) else {
        *export = Export::default();
        render(ui, export);
        return;
    };
    let term = data.setup.key;
    if export
        .identity
        .as_ref()
        .is_none_or(|(path, key)| path != workspace.path() || *key != term)
    {
        *export = Export {
            identity: Some((workspace.path().to_owned(), term)),
            language: Some(shell_language(ui)),
            ..Export::default()
        };
    }
    let mut versions = match workspace.finalized(term) {
        Ok(versions) => versions,
        Err(error) => {
            export.error = Some(OutputError::Detail(error.to_string()));
            render(ui, export);
            return;
        }
    };
    versions.reverse();
    let latest = versions.first().map(|item| item.version);
    if export.version == export.versions.first().map(|item| item.version)
        && latest != export.version
    {
        export.version = latest;
    }
    export.versions = versions;
    if let Some(version) = export.version
        && export
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.info().version != version)
    {
        export.invalidate();
        export.conditions.clear();
        export.snapshot = match Snapshot::load(workspace, term, version) {
            Ok(snapshot) => Some(Arc::new(snapshot)),
            Err(error) => {
                export.error = Some(OutputError::Detail(error.to_string()));
                None
            }
        };
    }
    export.normalize_target();
    render(ui, export);
}

fn command(
    ui: &AppWindow,
    export: &mut Export,
    action: ExportCommand,
    index: i32,
) -> Result<(), OutputError> {
    use ExportCommand::*;
    export.error = None;
    let selection = usize::try_from(index).ok();
    match action {
        Tab => {
            export.mode = match index {
                1 => Mode::Handover,
                2 => Mode::School,
                _ => Mode::Timetable,
            };
            export.replacement = None;
            export.invalidate();
        }
        LoadProfile if export.mode == Mode::School => {
            if let Some(path) = choose(ui, FileDialog::OpenSchoolProfile)? {
                export.school_profile =
                    Profile::from_json(&std::fs::read(&path)?).map_err(OutputError::Profile)?;
                export.school_profile_path = Some(path);
                export.invalidate();
            }
        }
        ExampleProfile if export.mode == Mode::School => {
            export.school_profile = Profile::default();
            export.school_profile_path = None;
            export.invalidate();
        }
        SaveExampleProfile if export.mode == Mode::School && export.snapshot.is_some() => {
            if let Some(path) = choose(ui, FileDialog::SaveHandover)? {
                let mut out = destination(path, "json")?;
                out.example_profile = true;
                export.stage_save(ui, out)?;
            }
        }
        Version => {
            if let Some(record) = selection.and_then(|index| export.versions.get(index)) {
                export.version = Some(record.version);
            }
        }
        Format => {
            export.excel = index == 1;
            export.invalidate();
        }
        Grouping => {
            export.grouping = index.clamp(0, 3);
            export.normalize_target();
            export.invalidate();
        }
        Target => {
            if let Some(target) = selection.and_then(|index| export.targets().get(index).copied()) {
                export.target = target;
                export.invalidate();
            }
        }
        Language => {
            export.language = Some(if index == 0 {
                auler_io::export::Language::Ko
            } else {
                auler_io::export::Language::En
            });
            export.invalidate();
        }
        Layout => {
            export.list = index == 1;
            export.invalidate();
        }
        Orientation => {
            export.portrait = index == 1;
            export.invalidate();
        }
        CourseName => {
            export.fields.course_name = index != 0;
            export.invalidate();
        }
        Instructor => {
            export.fields.instructors = index != 0;
            export.invalidate();
        }
        Room => {
            export.fields.room = index != 0;
            export.invalidate();
        }
        Constraint => {
            if let Some(choice) = export.snapshot.as_ref().and_then(|snapshot| {
                selection.and_then(|index| snapshot.handover_choices(shell_language(ui)).nth(index))
            }) {
                let id = choice.id;
                if !export.conditions.remove(&id) {
                    export.conditions.insert(id);
                }
                export.saved = None;
            }
        }
        ZoomOut => export.zoom = (export.zoom() - 20).max(60),
        ZoomIn => export.zoom = (export.zoom() + 20).min(160),
        Preview if export.can_preview() => export.launch(ui, Some(0), None)?,
        Previous if export.can_preview() && export.page > 0 => {
            export.launch(ui, Some(export.page - 1), None)?
        }
        Next if export.can_preview() && export.page + 1 < export.pages => {
            export.launch(ui, Some(export.page + 1), None)?
        }
        Save if export.snapshot.is_some()
            && (export.mode != Mode::Timetable || !export.empty()) =>
        {
            if export.mode == Mode::School
                && let Some(snapshot) = export.snapshot.as_ref()
            {
                export
                    .school_profile
                    .prepare(snapshot)
                    .map_err(OutputError::Profile)?;
            }
            let (dialog, extension) = match export.mode {
                Mode::Handover => (FileDialog::SaveHandover, "json"),
                Mode::School => (FileDialog::SaveSchoolSubmission, "csv"),
                Mode::Timetable if export.excel => (FileDialog::SaveWorkbook, "xlsx"),
                Mode::Timetable => (FileDialog::SavePdf, "pdf"),
            };
            if let Some(path) = choose(ui, dialog)? {
                export.stage_save(ui, destination(path, extension)?)?;
            }
        }
        Confirm => {
            if let Some(destination) = export.replacement.take() {
                export.launch(ui, None, Some(destination))?;
            }
        }
        Cancel => {
            export.replacement = None;
        }
        Print => {
            if let Some((path, true)) = export.saved.as_ref()
                && !crate::shell_open(path.as_os_str())
            {
                return Err(OutputError::OpenPdf);
            }
        }
        _ => {}
    }
    Ok(())
}

impl Export {
    fn invalidate(&mut self) {
        self.clear_preview();
        self.saved = None;
        self.replacement = None;
    }

    fn clear_preview(&mut self) {
        self.preview = Image::default();
        self.page = 0;
        self.pages = 0;
    }

    fn layout(&self) -> Layout {
        if self.list {
            Layout::List
        } else {
            Layout::Grid
        }
    }

    fn locale(&self, ui: &AppWindow) -> Language {
        self.language.unwrap_or_else(|| shell_language(ui))
    }

    fn zoom(&self) -> i32 {
        if self.zoom == 0 { 100 } else { self.zoom }
    }

    fn targets(&self) -> Vec<View> {
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot.views(match self.grouping {
                1 => Grouping::Year,
                2 => Grouping::Instructor,
                3 => Grouping::Room,
                _ => Grouping::All,
            })
        })
    }

    fn normalize_target(&mut self) {
        let targets = self.targets();
        if !targets.contains(&self.target) {
            self.target = targets.first().copied().unwrap_or_default();
        }
    }

    fn empty(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.meetings(self.target).is_empty())
    }

    fn can_preview(&self) -> bool {
        self.mode == Mode::Timetable && !self.excel && !self.empty()
    }

    fn stage_save(&mut self, ui: &AppWindow, out: Destination) -> Result<(), OutputError> {
        if out.replace {
            self.replacement = Some(out);
            Ok(())
        } else {
            self.launch(ui, None, Some(out))
        }
    }

    fn launch(
        &mut self,
        ui: &AppWindow,
        page: Option<usize>,
        out: Option<Destination>,
    ) -> Result<(), OutputError> {
        let Some(snapshot) = self.snapshot.clone() else {
            return Ok(());
        };
        let (sender, receiver) = mpsc::channel();
        let mode = self.mode;
        let profile = self.school_profile.clone();
        let excel = self.excel;
        let conditions = self.conditions.clone();
        let preview_width = self.preview_width;
        let options = pdf::TimetableOptions {
            locale: self.locale(ui),
            target: self.target,
            layout: self.layout(),
            fields: self.fields,
            landscape: !self.portrait,
        };
        std::thread::Builder::new()
            .name("auler-export".into())
            .spawn(move || {
                let result = if let Some(out) = out {
                    let bytes = if out.example_profile {
                        Ok(submission::EXAMPLE.as_bytes().to_vec())
                    } else {
                        match mode {
                            Mode::Handover => snapshot
                                .handover(&conditions)
                                .map_err(|error| OutputError::Detail(error.to_string())),
                            Mode::School => profile
                                .prepare(&snapshot)
                                .map(|report| report.bytes)
                                .map_err(OutputError::Profile),
                            Mode::Timetable if excel => snapshot
                                .xlsx(
                                    options.target,
                                    options.locale,
                                    options.layout,
                                    options.fields,
                                )
                                .map_err(|error| OutputError::Detail(error.to_string())),
                            Mode::Timetable => pdf::timetable(&snapshot, &options, None)
                                .map(|output| output.pdf)
                                .map_err(OutputError::Detail),
                        }
                    };
                    bytes
                        .and_then(|bytes| save(&out, &bytes).map_err(OutputError::from))
                        .map(|()| Finished::Saved(out.path, mode == Mode::Timetable && !excel))
                } else if let Some(page) = page {
                    let preview = pdf::Preview {
                        index: page,
                        width: preview_width,
                    };
                    pdf::timetable(&snapshot, &options, Some(preview))
                        .map_err(OutputError::Detail)
                        .and_then(|output| {
                            let png = output.preview.ok_or(OutputError::InvalidPreview)?;
                            Ok(Finished::Preview {
                                png,
                                page,
                                pages: output.pages,
                            })
                        })
                } else {
                    Err(OutputError::WorkerStopped)
                };
                let _ = sender.send(result);
            })
            .map_err(OutputError::from)?;
        self.job = Some(receiver);
        Ok(())
    }

    fn poll(&mut self) -> bool {
        let Some(receiver) = &self.job else {
            self.timer.stop();
            return false;
        };
        match receiver.try_recv() {
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => self.error = Some(OutputError::WorkerStopped),
            Ok(Err(error)) => self.error = Some(error),
            Ok(Ok(Finished::Saved(path, pdf))) => self.saved = Some((path, pdf)),
            Ok(Ok(Finished::Preview { png, page, pages })) => {
                match Image::load_from_data(&png, Some("png")) {
                    Ok(preview) => {
                        self.preview = preview;
                        self.page = page;
                        self.pages = pages;
                    }
                    Err(_) => self.error = Some(OutputError::InvalidPreview),
                }
            }
        }
        self.job = None;
        self.timer.stop();
        true
    }
}

fn destination(path: PathBuf, extension: &'static str) -> Result<Destination, OutputError> {
    if !path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        return Err(OutputError::Extension(extension));
    }
    let replace = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => true,
        Ok(_) => return Err(OutputError::NotRegular),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    Ok(Destination {
        path,
        replace,
        example_profile: false,
    })
}

fn save(out: &Destination, bytes: &[u8]) -> io::Result<()> {
    let directory = out
        .path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut stage = tempfile::NamedTempFile::new_in(directory)?;
    stage.write_all(bytes)?;
    stage.as_file().sync_all()?;
    if out.replace {
        stage.persist(&out.path)
    } else {
        stage.persist_noclobber(&out.path)
    }
    .map_err(|error| error.error)?;
    Ok(())
}

fn render(ui: &AppWindow, export: &Export) {
    let labels = ui.global::<Labels>();
    let locale = export.locale(ui);
    let interface = shell_language(ui);
    let targets = export.targets();
    let snapshot = export.snapshot.as_deref();
    let mut view = ExportView {
        tab: export.mode as i32,
        format: i32::from(export.excel),
        grouping: export.grouping,
        language: i32::from(locale == Language::En),
        layout: i32::from(export.list),
        orientation: i32::from(export.portrait),
        course_name: export.fields.course_name,
        instructor: export.fields.instructors,
        room: export.fields.room,
        ready: snapshot.is_some(),
        busy: export.job.is_some(),
        empty: export.empty(),
        preview: export.preview.clone(),
        page_index: export.page as i32,
        pages: export.pages as i32,
        zoom: export.zoom(),
        version: export
            .versions
            .iter()
            .position(|item| Some(item.version) == export.version)
            .map_or(-1, |index| index as i32),
        versions: model(
            export
                .versions
                .iter()
                .map(|item| labels.invoke_finalized(item.version as i32)),
        ),
        target: targets
            .iter()
            .position(|target| *target == export.target)
            .map_or(-1, |index| index as i32),
        saved: export
            .saved
            .as_ref()
            .map_or_else(SharedString::default, |(path, _)| {
                path.to_string_lossy().as_ref().into()
            }),
        can_print: export.saved.as_ref().is_some_and(|(_, pdf)| *pdf),
        overwrite: export
            .replacement
            .as_ref()
            .map_or_else(SharedString::default, |out| {
                out.path.to_string_lossy().as_ref().into()
            }),
        error: export
            .error
            .as_ref()
            .map_or_else(SharedString::default, |error| {
                error.localized(&labels, interface)
            }),
        ..ExportView::default()
    };
    if let Some(snapshot) = snapshot {
        view.source_version = labels.invoke_finalized(snapshot.info().version as i32);
        view.version_detail = timestamp(snapshot.info().at_ms);
        view.targets = model(
            targets
                .into_iter()
                .map(|target| snapshot.view_label(target, interface).into()),
        );
        if export.mode == Mode::Handover {
            view.constraints =
                model(
                    snapshot
                        .handover_choices(interface)
                        .map(|choice| ExportConstraint {
                            title: choice.title.into(),
                            detail: choice.detail.into(),
                            checked: export.conditions.contains(&choice.id),
                        }),
                );
        } else if export.mode == Mode::School {
            view.profile_name = if export.school_profile_path.is_some() {
                export.school_profile.name().into()
            } else {
                labels.get_example_school_profile()
            };
            view.profile_file = export
                .school_profile_path
                .as_ref()
                .and_then(|path| path.file_name())
                .map_or_else(SharedString::default, |name| {
                    name.to_string_lossy().as_ref().into()
                });
            view.profile_encoding = export.school_profile.encoding().label().into();
            view.profile_headers = export.school_profile.include_headers();
            match export.school_profile.prepare(snapshot) {
                Ok(report) => {
                    view.empty = report.table.rows.is_empty();
                    let widths = report
                        .table
                        .headings
                        .iter()
                        .map(|header| {
                            let label_width: f32 = header
                                .chars()
                                .map(|character| if character.is_ascii() { 7.5 } else { 13.0 })
                                .sum();
                            (label_width + 38.0).clamp(64.0, 238.0)
                        })
                        .collect();
                    render_table(&mut view, report.table, widths);
                }
                Err(error) => {
                    view.ready = false;
                    if view.error.is_empty() {
                        view.error = error.message(interface).into();
                    }
                }
            }
        } else if export.excel {
            let table = snapshot.table(export.target, locale, export.layout(), export.fields);
            let widths = if export.list {
                let mut widths = vec![74.0, 78.0, 70.0, 118.0, 82.0];
                if export.fields.course_name {
                    widths.push(132.0);
                }
                if export.fields.instructors {
                    widths.push(108.0);
                }
                if export.fields.room {
                    widths.push(108.0);
                }
                widths
            } else {
                vec![118.0; table.headings.len()]
            };
            render_table(&mut view, table, widths);
        }
    }
    ui.set_export_view(view);
}

fn render_table(view: &mut ExportView, table: auler_io::export::Table, widths: Vec<f32>) {
    view.columns = model(
        table
            .headings
            .into_iter()
            .zip(widths)
            .map(|(header, width)| {
                let mut column = slint::language::TableColumn::default();
                column.title = header.into();
                column.width = width;
                column
            }),
    );
    view.rows = model(table.rows.into_iter().map(|row| {
        model(
            row.into_iter()
                .map(|value| StandardListViewItem::from(value.replace('\n', " · ").as_str())),
        )
    }));
}

fn model<T: Clone + 'static>(values: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(values.into_iter().collect::<Vec<_>>()))
}

fn shell_language(ui: &AppWindow) -> Language {
    if ui.get_language_index() == 0 {
        Language::Ko
    } else {
        Language::En
    }
}

#[cfg(test)]
mod tests {
    use super::{destination, save};

    #[test]
    fn saving_a_new_output_preserves_a_file_created_after_selection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("timetable.pdf");
        let new = destination(path.clone(), "pdf").unwrap();
        std::fs::write(&path, b"existing").unwrap();
        assert!(save(&new, b"report").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        let confirmed = destination(path.clone(), "pdf").unwrap();
        assert!(confirmed.replace);
        save(&confirmed, b"report").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"report");
    }
}
