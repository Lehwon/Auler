use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use auler_store::{StoreError, VerifiedRestore, Workspace};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::file_dialog::{FileDialog, choose};
use crate::workspace::{Session, activate};
use crate::{AppWindow, BackupCount, BackupView, Labels};

#[derive(Default)]
pub(crate) struct BackupState {
    pending: Option<VerifiedRestore>,
    error: Option<StoreError>,
    opening_path: Option<PathBuf>,
    saved: Option<(PathBuf, bool)>,
    job: Option<Receiver<auler_store::Result<Completed>>>,
    timer: Timer,
}

enum Task {
    Backup {
        source: PathBuf,
        destination: PathBuf,
    },
    Restore {
        source: PathBuf,
        destination: PathBuf,
    },
    Open(VerifiedRestore),
}

enum Completed {
    Backup(PathBuf),
    Restore(VerifiedRestore),
    Open(Workspace),
}

impl Task {
    fn run(self) -> auler_store::Result<Completed> {
        match self {
            Self::Backup {
                source,
                destination,
            } => {
                Workspace::backup_file(source, &destination)?;
                Ok(Completed::Backup(destination))
            }
            Self::Restore {
                source,
                destination,
            } => Workspace::prepare_restore(source, destination).map(Completed::Restore),
            Self::Open(restored) => restored.open().map(Completed::Open),
        }
    }
}

pub(crate) fn bind(ui: &AppWindow, state: &Rc<RefCell<Session>>) {
    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_save_backup(move || {
        let Some(ui) = weak.upgrade() else { return };
        request(&ui, &context, |session| {
            let Some(workspace) = &session.workspace else {
                return Ok(None);
            };
            let Some(destination) = choose(&ui, FileDialog::SaveBackup)? else {
                return Ok(None);
            };
            Ok(Some(Task::Backup {
                source: workspace.path().to_owned(),
                destination,
            }))
        });
    });

    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_restore_backup(move || {
        let Some(ui) = weak.upgrade() else { return };
        request(&ui, &context, |_| {
            let Some(source) = choose(&ui, FileDialog::OpenBackup)? else {
                return Ok(None);
            };
            let Some(destination) = choose(&ui, FileDialog::SaveWorkspace)? else {
                return Ok(None);
            };
            Ok(Some(Task::Restore {
                source,
                destination,
            }))
        });
    });

    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_keep_workspace(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        if let Some(restored) = session.backup.pending.take() {
            session.backup.saved = Some((restored.path().to_owned(), true));
        }
        render(&ui, &session);
    });

    let (weak, context) = (ui.as_weak(), state.clone());
    ui.on_open_restored(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut session = context.borrow_mut();
        if let Some(restored) = session.backup.pending.take() {
            session.backup.opening_path = Some(restored.path().to_owned());
            if let Err(error) = launch(&ui, &context, &mut session, Task::Open(restored)) {
                session.backup.error = Some(error);
            }
            render(&ui, &session);
        }
    });
}

fn request(
    ui: &AppWindow,
    state: &Rc<RefCell<Session>>,
    pick: impl FnOnce(&Session) -> auler_store::Result<Option<Task>>,
) {
    let mut session = state.borrow_mut();
    if session.backup.job.is_some() || session.backup.pending.is_some() {
        return;
    }
    ui.set_workspace_error(SharedString::default());
    session.backup.error = None;
    session.backup.opening_path = None;
    match pick(&session) {
        Ok(Some(task)) => {
            if let Err(error) = launch(ui, state, &mut session, task) {
                session.backup.error = Some(error);
            }
        }
        Ok(None) => {}
        Err(error) => session.backup.error = Some(error),
    }
    render(ui, &session);
}

fn launch(
    ui: &AppWindow,
    state: &Rc<RefCell<Session>>,
    session: &mut Session,
    task: Task,
) -> auler_store::Result<()> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("auler-backup".into())
        .spawn(move || {
            let _ = sender.send(task.run());
        })?;
    let backup = &mut session.backup;
    backup.saved = None;
    backup.job = Some(receiver);
    let (weak, state) = (ui.as_weak(), Rc::downgrade(state));
    backup
        .timer
        .start(TimerMode::Repeated, Duration::from_millis(45), move || {
            let (Some(ui), Some(state)) = (weak.upgrade(), state.upgrade()) else {
                return;
            };
            let Ok(mut session) = state.try_borrow_mut() else {
                return;
            };
            poll(&ui, &mut session);
        });
    Ok(())
}

fn poll(ui: &AppWindow, session: &mut Session) {
    let backup = &mut session.backup;
    let Some(receiver) = &backup.job else {
        return;
    };
    let completed = receiver.try_recv();
    if matches!(completed, Err(TryRecvError::Empty)) {
        return;
    }
    backup.job = None;
    backup.timer.stop();
    match completed {
        Err(_) => backup.error = Some(StoreError::WriterGone),
        Ok(Err(error)) => backup.error = Some(error),
        Ok(Ok(Completed::Backup(path))) => backup.saved = Some((path, false)),
        Ok(Ok(Completed::Restore(restored))) => backup.pending = Some(restored),
        Ok(Ok(Completed::Open(workspace))) => match activate(ui, session, workspace, false) {
            Ok(()) => return,
            Err(error) => session.backup.error = Some(error),
        },
    }
    render(ui, session);
}

pub(crate) fn render(ui: &AppWindow, session: &Session) {
    let labels = ui.global::<Labels>();
    let backup = &session.backup;
    let mut view = BackupView {
        current_path: session
            .workspace
            .as_ref()
            .map_or_else(SharedString::default, |workspace| display(workspace.path())),
        pending: backup.pending.is_some(),
        busy: backup.job.is_some(),
        error: backup
            .error
            .as_ref()
            .map_or_else(SharedString::default, |error| match error {
                StoreError::NotWorkspace => labels.get_not_workspace(),
                StoreError::AlreadyExists => labels.get_backup_new_path(),
                StoreError::TooNew { .. } => labels.get_workspace_too_new(),
                StoreError::BackupPermissions(_) => labels.get_backup_permissions(),
                StoreError::BackupIntegrity => labels.get_backup_integrity(),
                StoreError::WriterGone => labels.get_backup_stopped(),
                error => labels.invoke_backup_error(error.to_string().into()),
            }),
        ..BackupView::default()
    };
    if !view.error.is_empty()
        && let Some(path) = &backup.opening_path
    {
        view.error = labels.invoke_backup_open_error(display(path), view.error);
    }
    if let Some((path, restored)) = &backup.saved {
        view.saved = display(path);
        view.restored = *restored;
    }
    if let Some(restored) = backup.pending.as_ref() {
        let summary = restored.summary();
        view.pending_path = display(restored.path());
        view.department = summary.department.as_str().into();
        let counts = [
            (labels.get_backup_terms(), summary.terms),
            (labels.get_backup_courses(), summary.courses),
            (labels.get_backup_instructors(), summary.instructors),
            (labels.get_backup_rooms(), summary.rooms),
            (labels.get_backup_sections(), summary.sections),
            (labels.get_backup_placements(), summary.placements),
            (labels.get_backup_versions(), summary.finalizations),
        ];
        view.counts = ModelRc::new(VecModel::from(
            counts
                .into_iter()
                .map(|(label, value)| BackupCount {
                    label,
                    value: value.to_string().into(),
                })
                .collect::<Vec<_>>(),
        ));
    }
    ui.set_backup_view(view);
}

fn display(path: &Path) -> SharedString {
    path.to_string_lossy().as_ref().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use auler_domain::input::InputDocument;
    use auler_domain::resolve::resolve;
    use auler_engine::{LocalSolver, SolveOptions, Solver};
    use auler_store::ChangeMeta;
    use i_slint_backend_testing::{TestingBackend, TestingBackendOptions, mock_elapsed_time};
    use slint::platform::{Key, WindowEvent};
    use slint::{LogicalSize, Model};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn verified_restore_waits_for_confirmation_and_keeps_the_current_workspace_on_error() {
        let capture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/tmp/ui-snapshots");
        std::fs::create_dir_all(&capture).unwrap();
        let fixture = tempfile::Builder::new()
            .prefix("backup-fixture-")
            .tempdir_in(&capture)
            .unwrap();
        let original = fixture.path().join("active.auler");
        let backup = fixture.path().join("private.auler-backup");
        let skipped = fixture.path().join("keep-working.auler");
        let restored = fixture.path().join("restored.auler");
        let data = InputDocument::from_json(include_str!("../../samples/reference.json")).unwrap();
        let mut data = resolve(&data).unwrap();
        data.placements = LocalSolver
            .solve(
                &data,
                SolveOptions::default(),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .placements;
        let term = data.setup.key;
        let workspace = Workspace::create(&original, "가상 학과").unwrap();
        let meta = || ChangeMeta::direct("Backup fixture");
        workspace.import_term(data, meta()).unwrap();
        let revision = workspace.revision().unwrap();
        workspace
            .finalize_checked(term, revision, true, meta())
            .unwrap();

        let _ =
            slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some("software".into()),
            })));
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(LogicalSize::new(1024.0, 640.0));
        ui.show().unwrap();
        crate::init_shell(&ui, 0);
        ui.set_live_workspace(true);
        let state = Rc::new(RefCell::new(Session::default()));
        bind(&ui, &state);
        activate(&ui, &mut state.borrow_mut(), workspace, false).unwrap();
        ui.set_page(5);
        ui.set_export_backup_active(true);
        run(
            &ui,
            &state,
            Task::Backup {
                source: original.clone(),
                destination: backup.clone(),
            },
        );
        assert!(backup.is_file());
        assert!(ui.get_backup_view().error.is_empty());
        capture_image(&ui, &capture.join("ko-1024x640-backup-saved.png"));

        let invalid = fixture.path().join("invalid.auler-backup");
        std::fs::write(&invalid, b"not sqlite").unwrap();
        run(
            &ui,
            &state,
            Task::Restore {
                source: invalid,
                destination: restored.clone(),
            },
        );
        assert!(!ui.get_backup_view().error.is_empty());
        assert!(!restored.exists());
        assert_eq!(state.borrow().workspace.as_ref().unwrap().path(), original);
        capture_image(&ui, &capture.join("ko-1024x640-backup-failed.png"));

        run(
            &ui,
            &state,
            Task::Restore {
                source: backup.clone(),
                destination: skipped.clone(),
            },
        );
        assert_eq!(state.borrow().workspace.as_ref().unwrap().path(), original);
        let prepared = ui.get_backup_view();
        assert!(prepared.pending && prepared.error.is_empty());
        assert!(!skipped.with_extension("auler-wal").exists());
        assert!(!skipped.with_extension("auler-shm").exists());
        assert!(
            prepared
                .counts
                .iter()
                .any(|item| item.label == "배치" && item.value == "68")
        );
        assert!(
            prepared
                .counts
                .iter()
                .any(|item| item.label == "분반" && item.value == "42")
        );
        assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
        capture_image(&ui, &capture.join("ko-1024x640-backup-verified.png"));
        let mut long_path = prepared;
        long_path.pending_path = format!("C:/{}restored.auler", "directory-".repeat(210)).into();
        ui.set_backup_view(long_path);
        capture_image(&ui, &capture.join("ko-1024x640-backup-long-path.png"));
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        assert!(!ui.get_backup_view().pending);
        assert_eq!(state.borrow().workspace.as_ref().unwrap().path(), original);
        assert!(skipped.is_file());
        ui.set_workspace_start(true);
        capture_image(&ui, &capture.join("ko-1024x640-backup-retained-start.png"));
        ui.set_workspace_start(false);

        let unavailable = fixture.path().join("moved-after-verification.auler");
        run(
            &ui,
            &state,
            Task::Restore {
                source: backup.clone(),
                destination: unavailable.clone(),
            },
        );
        assert!(ui.get_backup_view().pending);
        std::fs::rename(&unavailable, fixture.path().join("moved.auler")).unwrap();
        ui.invoke_open_restored();
        wait_for_job(&ui);
        assert!(
            ui.get_backup_view()
                .error
                .contains("moved-after-verification.auler")
        );
        assert_eq!(state.borrow().workspace.as_ref().unwrap().path(), original);

        crate::set_shell_language(&ui, 1);
        crate::workspace::refresh(&ui, &mut state.borrow_mut(), Some(term)).unwrap();
        ui.set_page(6);
        run(
            &ui,
            &state,
            Task::Restore {
                source: backup,
                destination: restored.clone(),
            },
        );
        assert!(ui.get_backup_view().pending);
        capture_image(&ui, &capture.join("en-1024x640-backup-verified.png"));
        ui.invoke_open_restored();
        assert!(ui.get_backup_view().busy);
        assert_eq!(state.borrow().workspace.as_ref().unwrap().path(), original);
        wait_for_job(&ui);
        let session = state.borrow();
        let active = session.workspace.as_ref().unwrap();
        assert_eq!(active.path(), restored);
        assert_eq!(active.load_finalized(term, 1).unwrap().placements.len(), 68);
        assert_eq!(ui.get_page(), 0);
        drop(session);
        ui.hide().unwrap();
        drop(ui);
        drop(state);
        if std::env::var_os("AULER_KEEP_BACKUP_FIXTURE").is_some() {
            println!("fixture: {}", fixture.keep().display());
        }
    }

    fn run(ui: &AppWindow, state: &Rc<RefCell<Session>>, task: Task) {
        request(ui, state, |_| Ok(Some(task)));
        wait_for_job(ui);
    }

    fn wait_for_job(ui: &AppWindow) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while ui.get_backup_view().busy && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            mock_elapsed_time(Duration::from_millis(50));
        }
        assert!(!ui.get_backup_view().busy);
    }

    fn capture_image(ui: &AppWindow, path: &Path) {
        let shot = ui.window().take_snapshot().unwrap();
        let mut encoder = png::Encoder::new(
            std::fs::File::create(path).unwrap(),
            shot.width(),
            shot.height(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(shot.as_bytes())
            .unwrap();
    }
}
