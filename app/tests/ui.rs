//! 화면 동작 테스트.
//!
//! Slint 테스트 백엔드에 소프트웨어 렌더러를 붙여 실제 시스템 글꼴로 레이아웃을 잰다.
//! 접근성 트리(역할·이름·선택·체크 상태)로 요소를 찾으므로, 화면 읽기 프로그램이 보는 정보도 함께 검사한다.
//! 스냅숏 PNG는 `target/tmp/ui-snapshots/`에 남는다 (검사하지 않고 사람이 보는 용도).

use auler::{AppWindow, init, set_language};
use i_slint_backend_testing::{
    AccessibleRole, ElementHandle, ElementQuery, TestingBackend, TestingBackendOptions,
};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize, Model};

const KO: usize = 0;
const EN: usize = 1;

fn test_window(size: LogicalSize) -> AppWindow {
    // 테스트마다 스레드가 다르고 플랫폼은 스레드마다 한 번만 정할 수 있다.
    let _ = slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
        mock_time: true,
        threading: false,
        renderer_name: Some("software".into()),
    })));
    let ui = AppWindow::new().expect("창 생성");
    ui.window().set_size(size);
    ui.show().expect("창 표시");
    ui
}

fn setup(language: usize, size: LogicalSize) -> AppWindow {
    let ui = test_window(size);
    init(&ui, language);
    ui
}

fn wide() -> LogicalSize {
    LogicalSize::new(1366.0, 768.0)
}

/// 역할이 같은 요소들. 상속된 컴포넌트는 같은 항목이 여러 번 잡히므로 이름·위치·크기로 하나만 남긴다.
fn by_role(ui: &AppWindow, role: AccessibleRole) -> Vec<ElementHandle> {
    let mut seen = Vec::new();
    ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(role)
        .find_all()
        .into_iter()
        .filter(|e| {
            let p = e.absolute_position();
            let s = e.size();
            let key = (
                e.accessible_label().map(|l| l.to_string()),
                p.x as i32,
                p.y as i32,
                s.width as i32,
                s.height as i32,
            );
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
        .collect()
}

fn by_role_and_label(ui: &AppWindow, role: AccessibleRole, label: &str) -> ElementHandle {
    let wanted = label.to_string();
    ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(role)
        .match_predicate(move |e| e.accessible_label().is_some_and(|l| l == wanted.as_str()))
        .find_first()
        .unwrap_or_else(|| panic!("{role:?} 요소 '{label}'를 찾지 못함"))
}

fn has_text(ui: &AppWindow, text: &str) -> bool {
    ElementHandle::find_by_accessible_label(ui, text)
        .next()
        .is_some()
}

fn snapshot(ui: &AppWindow, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = ui.window().take_snapshot().expect("스냅숏");
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(file, shot.width(), shot.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(shot.as_bytes())
        .unwrap();
}

#[test]
fn starts_without_login_and_validates_the_new_workspace_form() {
    let ui = test_window(wide());
    auler::workspace::init(&ui, KO, None);
    assert!(ui.get_workspace_start());
    snapshot(&ui, "ko-1366x768-start");
    for label in [
        "새 작업공간",
        "기존 작업공간 열기",
        "예제 학과 열기",
        "백업 복원",
    ] {
        by_role_and_label(&ui, AccessibleRole::Button, label);
    }
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    ui.invoke_language_selected(1);
    snapshot(&ui, "en-1024x640-start");
    ui.invoke_language_selected(0);
    by_role_and_label(&ui, AccessibleRole::Button, "AI 없이 시작하기")
        .invoke_accessible_default_action();
    assert!(ui.get_workspace_start());
    assert!(!has_text(&ui, "AI 연결 (선택)"));
    by_role_and_label(&ui, AccessibleRole::Button, "새 작업공간")
        .invoke_accessible_default_action();
    assert_eq!(ui.get_setup_mode(), 0);
    ui.invoke_submit_setup(auler::SetupFields {
        year: 2027,
        season: 0,
        ..Default::default()
    });
    assert!(ui.get_workspace_error().contains("학과명을 입력"));
    assert_eq!(ui.get_setup_mode(), 0);
    by_role_and_label(&ui, AccessibleRole::Button, "취소").invoke_accessible_default_action();
    assert_eq!(ui.get_setup_mode(), -1);
    assert!(ui.get_workspace_error().is_empty());
}

#[test]
fn opens_live_workspace_home_and_preserves_term_on_language_change() {
    let path = workspace_fixture("home");
    let ui = test_window(wide());
    auler::workspace::init(&ui, KO, Some(&path));
    assert!(ui.get_workspace_open());
    assert!(!ui.get_workspace_start());
    assert_eq!(ui.get_page(), 0);
    assert_eq!(ui.get_home_stats().sections, 42);
    assert_eq!(ui.get_home_stats().rooms, 12);
    assert_eq!(ui.get_home_stats().instructors, 28);
    assert_eq!(ui.get_rows().row_count(), 42);
    assert!(ui.get_recent_work().row_count() >= 1);
    snapshot(&ui, "ko-1366x768-home");
    ui.invoke_language_selected(1);
    assert_eq!(ui.get_term_index(), 0);
    assert_eq!(ui.get_department(), "가상 학과");
    assert!(ui.get_selected_term_label().contains("First semester"));
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "en-1024x640-home");
    by_role_and_label(&ui, AccessibleRole::Button, "Finalize and export")
        .invoke_accessible_default_action();
    assert_eq!(ui.get_page(), 3);
    assert_eq!(ui.get_home_filter(), 7);
    assert!(has_text(&ui, "Validate timetable"));
    assert!(has_text(&ui, "Review the outstanding timetable checks."));
    ui.invoke_navigate_home_target(3);
    assert_eq!(ui.get_page(), 3);
    assert_eq!(ui.get_home_filter(), 3);
    ui.invoke_navigate_home_target(2);
    assert_eq!(ui.get_page(), 2);
    let constraints = ui.get_constraint_view();
    assert_eq!(constraints.tab, 3);
    assert!(constraints.rows.row_count() > 0);
    ui.hide().unwrap();
    drop(ui);
    std::fs::remove_file(path).unwrap();
}

fn workspace_fixture(case: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{case}-test-{}.auler", std::process::id()));
    let input =
        auler_domain::input::InputDocument::from_json(include_str!("../../samples/reference.json"))
            .unwrap();
    let data = auler_domain::resolve::resolve(&input).unwrap();
    let workspace = auler_store::Workspace::create(&path, "가상 학과").unwrap();
    workspace
        .import_term(
            data,
            auler_store::ChangeMeta::direct("Sample department data"),
        )
        .unwrap();
    path
}

#[test]
fn constraints_share_the_grid_and_list_and_preserve_locked_placements() {
    let path = workspace_fixture("constraints");
    let original_placements = {
        let workspace = auler_store::Workspace::open(&path).unwrap();
        let term = workspace.terms().unwrap()[0].key;
        workspace.load_term(term).unwrap().placements
    };
    let ui = test_window(wide());
    auler::workspace::init(&ui, KO, Some(&path));
    ui.invoke_navigate_home_target(2);
    by_role_and_label(&ui, AccessibleRole::Tab, "확인 대기 1");
    assert_eq!(ui.get_constraint_view().tab, 3);
    ui.invoke_constraint_action(0, 0);
    snapshot(&ui, "ko-1366x768-constraints-list");
    ui.invoke_constraint_option(0, 1);
    let before = ui.get_draft_version();
    let locked = ui.get_locked_count();
    ui.invoke_constraint_action(1, 0);
    ui.invoke_constraint_option(4, 1);
    let target_index = ui
        .get_constraint_view()
        .editor
        .targets
        .iter()
        .position(|label| label.ends_with("(CSE4010)"))
        .unwrap();
    ui.invoke_constraint_option(5, target_index as i32);
    ui.invoke_constraint_text(1, "월요일 시간 요청".into());
    ui.invoke_constraint_option(3, 1);
    let period = ui
        .get_constraint_view()
        .editor
        .period_labels
        .iter()
        .position(|label| label == "6")
        .unwrap();
    assert!(!ui.get_constraint_view().editor.can_save);
    ui.invoke_constraint_grid_cell(0, period as i32, -1);
    assert_eq!(ui.get_constraint_view().editor.affected.row_count(), 1);
    assert!(ui.get_constraint_view().editor.affected_locked);
    assert!(
        ui.get_constraint_view()
            .editor
            .affected
            .row_data(0)
            .unwrap()
            .contains("잠금")
    );
    assert!(ui.get_constraint_view().editor.can_save);
    assert_eq!(ui.get_draft_version(), before);
    ui.invoke_language_selected(1);
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    by_role_and_label(
        &ui,
        AccessibleRole::Button,
        "Mon · Period 6 · Remove avoided period",
    );
    snapshot(&ui, "en-1024x640-constraints-editor");
    let panel = by_role_and_label(&ui, AccessibleRole::Complementary, "Constraint details");
    let origin = panel.absolute_position();
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(origin.x + 12.0, origin.y + 156.0),
        delta_x: 0.0,
        delta_y: -1300.0,
    });
    slint::platform::update_timers_and_animations();
    let note = by_role_and_label(&ui, AccessibleRole::TextInput, "Private note");
    assert!(note.absolute_position().y < 530.0);
    snapshot(&ui, "en-1024x640-constraints-scrolled");
    ui.invoke_constraint_action(3, 0);
    assert_eq!(ui.get_draft_version(), before + 1);
    assert_eq!(ui.get_locked_count(), locked);
    assert!(!ui.get_constraint_view().editing);
    assert_eq!(ui.get_constraint_view().affected.row_count(), 1);
    ui.invoke_constraint_option(3, 0);
    assert!(
        ui.get_constraint_view()
            .rows
            .iter()
            .any(|row| row.title == "월요일 시간 요청" && row.selected)
    );
    ui.hide().unwrap();
    drop(ui);
    let workspace = auler_store::Workspace::open(&path).unwrap();
    let term = workspace.terms().unwrap()[0].key;
    let data = workspace.load_term(term).unwrap();
    assert_eq!(data.placements, original_placements);
    assert_eq!(
        data.placements
            .iter()
            .filter(|placement| placement.locked)
            .count(),
        locked as usize
    );
    assert!(
        data.constraints
            .iter()
            .any(
                |constraint| constraint.text.as_deref() == Some("월요일 시간 요청")
                    && constraint.is_active()
            )
    );
    drop(workspace);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn workbench_previews_manual_and_solver_changes_before_storing_them() {
    let path = workspace_fixture("workbench");
    let baseline = {
        let workspace = auler_store::Workspace::open(&path).unwrap();
        workspace
            .load_term(workspace.terms().unwrap()[0].key)
            .unwrap()
    };
    let ui = test_window(wide());
    auler::workspace::init(&ui, KO, Some(&path));
    ui.set_page(3);
    assert!(ui.get_workbench_view().loaded);
    snapshot(&ui, "ko-1366x768-workbench");
    let revision = ui.get_draft_version();
    let initial_unassigned = ui.get_unassigned_count();
    ui.invoke_work_action(0, 0);
    assert!(ui.get_workbench_view().locked);
    assert!(!ui.invoke_move_card(0, 1, 0));
    assert!(ui.get_workbench_view().move_open);
    assert!(!ui.get_workbench_view().move_allowed);
    assert_eq!(ui.get_draft_version(), revision);
    ui.invoke_work_action(6, 0);
    ui.invoke_work_action(1, 0);
    ui.invoke_language_selected(1);
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    let details = by_role_and_label(&ui, AccessibleRole::Button, "Details");
    if !details.accessible_checked().unwrap_or(false) {
        details.invoke_accessible_default_action();
    }
    by_role_and_label(&ui, AccessibleRole::Button, "Close").invoke_accessible_default_action();
    assert!(ui.get_workbench_view().choosing && ui.get_workbench_view().has_selection);
    assert!(by_role(&ui, AccessibleRole::Complementary).is_empty());
    let mut valid_location = None;
    'slots: for day in 0..ui.get_days().row_count() {
        for period in 0..ui.get_periods().row_count() {
            ui.invoke_work_slot(day as i32, period as i32);
            valid_location = ui
                .get_workbench_view()
                .locations
                .iter()
                .position(|room| room.tone <= 1);
            if valid_location.is_some() {
                break 'slots;
            }
        }
    }
    let candidate = ui
        .get_workbench_view()
        .locations
        .row_data(valid_location.unwrap())
        .unwrap();
    assert!(candidate.room_facts.contains("seats"));
    snapshot(&ui, "en-1024x640-workbench-pick");
    ui.invoke_work_action(4, valid_location.unwrap() as i32);
    assert!(ui.get_workbench_view().move_allowed);
    assert_eq!(ui.get_draft_version(), revision);
    let details = by_role_and_label(&ui, AccessibleRole::Button, "Details");
    if !details.accessible_checked().unwrap_or(false) {
        details.invoke_accessible_default_action();
    }
    snapshot(&ui, "en-1024x640-workbench-placement");
    ui.invoke_work_action(5, 0);
    assert_eq!(ui.get_draft_version(), revision + 1);
    assert_eq!(ui.get_unassigned_count(), initial_unassigned - 1);
    ui.invoke_undo();
    assert_eq!(ui.get_unassigned_count(), initial_unassigned);
    ui.invoke_redo();
    assert_eq!(ui.get_unassigned_count(), initial_unassigned - 1);
    let moved = ui.get_selected();
    assert!(moved >= 0);
    assert!(!ui.invoke_move_card(moved, -1, 0));
    assert!(ui.get_workbench_view().error.contains("weekly grid"));
    if let Some(close) = ElementHandle::find_by_accessible_label(&ui, "Close").next() {
        close.invoke_accessible_default_action();
    }
    let source = ui.get_cards().row_data(moved as usize).unwrap();
    let label = format!("{}, {}, {}", source.title, source.detail, source.slot_label);
    let card = by_role_and_label(&ui, AccessibleRole::ListItem, &label);
    let (position, size) = (card.absolute_position(), card.size());
    card.mock_drag(
        LogicalPosition::new(
            position.x + size.width / 2.0 + size.width + 6.0,
            position.y + size.height / 2.0,
        ),
        PointerEventButton::Left,
    );
    assert!(ui.get_workbench_view().move_open);
    ui.invoke_work_action(15, 0);
    let alternative = ui.get_workbench_view();
    assert!(alternative.choosing);
    assert_eq!(
        (alternative.day, alternative.period),
        (source.day + 1, source.start)
    );
    ui.invoke_work_action(2, 0);
    let before_solve = ui.get_draft_version();
    ui.invoke_work_action(10, 0);
    assert_eq!(ui.get_workbench_view().solver_stage, 1);
    ui.invoke_work_action(12, 0);
    assert_eq!(ui.get_workbench_view().solver_stage, 2);
    by_role_and_label(&ui, AccessibleRole::Form, "Auto schedule");
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Escape.into(),
    });
    assert!(ui.get_workbench_view().solver_cancelling);
    snapshot(&ui, "en-1024x640-workbench-cancelling");
    wait_for_solver(&ui);
    assert!(ui.get_workbench_view().solver_message.contains("cancelled"));
    assert!(!ui.get_workbench_view().solver_allowed);
    assert_eq!(ui.get_draft_version(), before_solve);
    ui.invoke_work_action(13, 0);
    ui.invoke_work_action(10, 0);
    ui.invoke_work_action(12, 0);
    wait_for_solver(&ui);
    let result = ui.get_workbench_view();
    assert_eq!(result.solver_stage, 3);
    assert!(
        result.solver_allowed,
        "{} · {}",
        result.solver_message, result.solver_hint
    );
    assert!(result.solver_changes.row_count() > 0);
    assert_eq!(ui.get_draft_version(), before_solve);
    snapshot(&ui, "en-1024x640-workbench-solver");
    ui.invoke_work_action(14, 0);
    assert_eq!(ui.get_draft_version(), before_solve + 1);
    assert_eq!(ui.get_workbench_view().solver_stage, 0);
    ui.hide().unwrap();
    drop(ui);
    let workspace = auler_store::Workspace::open(&path).unwrap();
    let final_data = workspace.load_term(baseline.setup.key).unwrap();
    for placement in baseline.placements.iter().filter(|p| p.locked) {
        assert!(final_data.placements.contains(placement));
    }
    assert!(
        auler_engine::Evaluator::new(&baseline)
            .audit(&final_data.placements)
            .can_finalize()
    );
    drop(workspace);
    std::fs::remove_file(path).unwrap();
}

fn wait_for_solver(ui: &AppWindow) {
    let timeout = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while ui.get_workbench_view().solver_stage == 2 && std::time::Instant::now() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(20));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(80));
    }
    assert_eq!(ui.get_workbench_view().solver_stage, 3);
}

#[test]
fn review_and_finalize_recheck_the_current_version_and_keep_an_immutable_snapshot() {
    let path = workspace_fixture("review-finalization");
    let key = {
        let store = auler_store::Workspace::open(&path).unwrap();
        let key = store.terms().unwrap()[0].key;
        let mut doc = auler_domain::input::InputDocument::new(key.year, key.season.code());
        let mut course = auler_domain::input::InputDocument::from_json(include_str!(
            "../../samples/reference.json"
        ))
        .unwrap()
        .courses
        .remove(0);
        course.name.push_str(" (review fixture)");
        doc.courses.push(course);
        store
            .create_review(
                key,
                doc.clone(),
                auler_store::ChangeMeta::direct("Course name proposal")
                    .origin(auler_store::Origin::Mcp)
                    .request("ui-review-1"),
            )
            .unwrap();
        store
            .create_review(
                key,
                doc,
                auler_store::ChangeMeta::direct("Workbook course proposal")
                    .origin(auler_store::Origin::Xlsx),
            )
            .unwrap();
        let mut instructor = store.load_term(key).unwrap().instructors.remove(0);
        instructor.department = Some("Edited after submission".into());
        store
            .apply(
                None,
                vec![auler_store::Op::Put(auler_store::Record::Instructor(
                    instructor,
                ))],
                auler_store::ChangeMeta::direct("Changed basis"),
            )
            .unwrap();
        key
    };
    let ui = test_window(wide());
    auler::workspace::init(&ui, EN, Some(&path));
    ui.set_page(5);
    assert_eq!(ui.get_export_view().versions.row_count(), 0);
    assert!(has_text(&ui, "No finalized version yet"));
    snapshot(&ui, "en-1366x768-export-empty");
    ui.set_page(4);
    ui.invoke_review_queue_action(0, 1);
    let row = ui
        .get_review_queue()
        .rows
        .iter()
        .find(|row| row.origin == "MCP")
        .unwrap();
    assert_eq!(row.status, "Base changed");
    assert_eq!(row.origin, "MCP");
    ui.invoke_review_queue_action(1, row.index);
    assert_eq!(ui.get_review_queue().request, "ui-review-1");
    snapshot(&ui, "en-1366x768-review-queue");
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "en-1024x640-review-queue");
    let workbook = ui
        .get_review_queue()
        .rows
        .iter()
        .find(|row| row.origin == "Standard XLSX")
        .unwrap();
    ui.invoke_open_review(workbook.index);
    let workbook = ui.get_import_view();
    assert!(
        workbook.from_review && workbook.can_refresh && workbook.can_replace && workbook.can_reject
    );
    assert_eq!(workbook.origin, "Standard XLSX");
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "en-1024x640-review-xlsx-stale");
    ui.invoke_import_command(0);
    ui.invoke_open_review(row.index);
    assert!(ui.get_import_view().from_review && ui.get_import_view().can_refresh);
    ui.invoke_import_command(5);
    let row = ui
        .get_import_view()
        .rows
        .iter()
        .position(|row| row.enabled)
        .unwrap();
    ui.invoke_import_check(row as i32, true);
    ui.invoke_import_command(11);
    assert!(ui.get_import_view().rejecting);
    by_role_and_label(&ui, AccessibleRole::Button, "Keep reviewing");
    press_escape(&ui);
    assert!(!ui.get_import_view().rejecting);
    ui.invoke_import_command(11);
    let before_reject = ui.get_draft_version();
    ui.invoke_import_command(13);
    assert!(
        ui.get_import_view()
            .rows
            .iter()
            .any(|row| row.state == "Rejected")
    );
    assert_eq!(ui.get_draft_version(), before_reject);
    ui.invoke_import_command(0);
    assert_eq!(ui.get_page(), 4);
    ui.invoke_review_queue_action(0, 4);
    assert_eq!(ui.get_review_queue().rows.row_count(), 1);
    ui.invoke_open_review(ui.get_review_queue().rows.row_data(0).unwrap().index);
    assert!(ui.get_import_view().complete);
    assert_eq!(ui.get_import_view().status, "Rejected");
    assert!(!has_text(&ui, "Apply selected (0)"));
    snapshot(&ui, "en-1024x640-review-complete");
    ui.invoke_navigate_home_target(7);
    assert!(!ui.get_final_view().can_finalize);
    assert!(
        !by_role_and_label(&ui, AccessibleRole::Button, "Finalize timetable")
            .accessible_enabled()
            .unwrap()
    );
    ui.invoke_navigate_home_target(3);
    ui.invoke_work_action(10, 0);
    ui.invoke_work_action(12, 0);
    wait_for_solver(&ui);
    assert!(ui.get_workbench_view().solver_allowed);
    ui.invoke_work_action(14, 0);
    ui.invoke_navigate_home_target(7);
    let checked = ui.get_draft_version();
    assert!(ui.get_final_view().can_finalize);
    assert!(ui.get_final_view().needs_ack);
    ui.invoke_finalize_action(1, 0);
    by_role_and_label(
        &ui,
        AccessibleRole::Checkbox,
        "I reviewed the remaining warnings and pending conditions.",
    );
    press_escape(&ui);
    assert!(!ui.get_final_view().confirming);
    ui.invoke_finalize_action(1, 0);
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "en-1024x640-finalize-confirm");
    ui.invoke_finalize_action(3, 0);
    assert_eq!(ui.get_draft_version(), checked);
    assert!(!ui.get_final_view().error.is_empty());
    ui.invoke_finalize_action(5, 1);
    ui.invoke_finalize_action(3, 0);
    assert!(ui.get_final_view().snapshot);
    assert!(ui.get_final_view().can_correct);
    assert_eq!(ui.get_final_view().snapshot_rows.row_count(), 68);
    assert!(
        ui.get_final_view()
            .snapshot_rows
            .iter()
            .any(|row| row.name.contains("Part 2") && row.time.contains("Period"))
    );
    assert_eq!(ui.get_finalized_version(), 1);
    assert!(!ui.get_home_stats().editing);
    ui.invoke_language_selected(0);
    snapshot(&ui, "ko-1024x640-finalized");
    verify_export_ui(&ui);
    ui.set_page(3);
    assert_eq!(ui.get_export_view().pages, 0);
    ui.invoke_finalize_action(4, 0);
    assert!(ui.get_home_stats().editing);
    assert_eq!(ui.get_finalized_version(), 1);
    ui.hide().unwrap();
    drop(ui);
    let store = auler_store::Workspace::open(&path).unwrap();
    let frozen = store.load_finalized(key, 1).unwrap();
    assert_eq!(frozen.placements, store.load_term(key).unwrap().placements);
    assert!(auler_store::FinalizationChecks::inspect(&frozen).required_passed());
    let request = format!("desktop-finalize-{key}-{checked}");
    let repeated = store
        .finalize_checked(
            key,
            checked as u64,
            true,
            auler_store::ChangeMeta::direct("Finalized timetable").request(request.clone()),
        )
        .unwrap();
    assert_eq!(repeated.version, 1);
    assert!(matches!(
        store.finalize_checked(
            key,
            store.revision().unwrap(),
            true,
            auler_store::ChangeMeta::direct("Finalized timetable").request(request)
        ),
        Err(auler_store::StoreError::FinalizationRequestConflict)
    ));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

fn verify_export_ui(ui: &AppWindow) {
    use auler::ExportCommand;
    ui.set_page(5);
    assert_eq!(ui.get_export_view().versions.row_count(), 1);
    assert_eq!(ui.get_export_view().version, 0);
    ui.invoke_export_command(ExportCommand::Language, 0);
    ui.invoke_export_command(ExportCommand::Grouping, 1);
    assert!(ui.get_export_view().targets.row_count() > 0);
    ui.invoke_export_command(ExportCommand::Preview, 0);
    wait_for_export(ui);
    let exported = ui.get_export_view();
    assert!(exported.error.is_empty(), "{}", exported.error);
    assert!(exported.pages > 0 && exported.preview.size().width > 100);
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(ui, "ko-1024x640-output-preview");
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(425.0, 460.0),
        delta_x: 0.0,
        delta_y: -600.0,
    });
    slint::platform::update_timers_and_animations();
    let room_field = by_role_and_label(ui, AccessibleRole::Checkbox, "강의실");
    assert!(room_field.absolute_position().y + room_field.size().height < 552.0);
    snapshot(ui, "ko-1024x640-output-settings-end");
    ui.window().set_size(wide());
    slint::platform::update_timers_and_animations();
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(200));
    wait_for_export(ui);
    snapshot(ui, "ko-1366x768-output-preview");
    if ui.get_export_view().pages > 1 {
        ui.invoke_export_command(ExportCommand::Next, 0);
        wait_for_export(ui);
        assert_eq!(ui.get_export_view().page_index, 1);
        snapshot(ui, "ko-1366x768-output-next");
    }
    ui.invoke_export_command(ExportCommand::Format, 1);
    ui.invoke_export_command(ExportCommand::Layout, 1);
    ui.invoke_export_command(ExportCommand::Grouping, 0);
    assert_eq!(ui.get_export_view().pages, 0);
    assert_eq!(ui.get_export_view().rows.row_count(), 68);
    assert!(
        ui.get_export_view()
            .rows
            .iter()
            .any(|row| row.row_data(4).unwrap().text == "01")
    );
    snapshot(ui, "ko-1366x768-output-xlsx");
    ui.invoke_export_command(ExportCommand::Tab, 1);
    assert!(ui.get_export_view().constraints.row_count() > 0);
    ui.invoke_export_command(ExportCommand::Constraint, 0);
    assert!(
        ui.get_export_view()
            .constraints
            .row_data(0)
            .unwrap()
            .checked
    );
    snapshot(ui, "ko-1366x768-output-handover");

    ui.invoke_export_command(ExportCommand::Tab, 2);
    let submission = ui.get_export_view();
    assert!(
        submission.ready && submission.profile_headers,
        "{}",
        submission.error
    );
    assert_eq!(submission.profile_encoding, "CP949");
    assert_eq!(submission.rows.row_count(), 68);
    by_role_and_label(ui, AccessibleRole::Button, "예시 JSON 저장");
    assert!(
        submission
            .rows
            .iter()
            .any(|row| row.row_data(1).unwrap().text == "01")
    );
    snapshot(ui, "ko-1366x768-school-submission");
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(ui, "ko-1024x640-school-submission");

    ui.invoke_language_selected(1);
    ui.invoke_export_command(ExportCommand::Tab, 0);
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    let mut view = ui.get_export_view();
    assert!(view.columns.iter().any(|column| column.title == "강의실"));
    view.overwrite = "C:/Reports/Timetable.xlsx".into();
    ui.set_export_view(view);
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    by_role_and_label(ui, AccessibleRole::Button, "Cancel");
    assert!(has_text(ui, "Replace existing file?"));
    snapshot(ui, "en-1024x640-output-replace");
    press_escape(ui);
    assert!(ui.get_export_view().overwrite.is_empty());
    ui.invoke_language_selected(0);
}

fn wait_for_export(ui: &AppWindow) {
    let timeout = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while ui.get_export_view().busy && std::time::Instant::now() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(20));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(80));
    }
    assert!(!ui.get_export_view().busy);
}

fn press_escape(ui: &AppWindow) {
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Escape.into(),
    });
}

#[test]
fn catalog_edits_persist_and_bulk_changes_require_a_preview() {
    let path = workspace_fixture("catalog");
    let ui = test_window(wide());
    auler::workspace::init(&ui, KO, Some(&path));
    ui.set_page(1);
    assert_eq!(ui.get_catalog_rows().row_count(), 42);
    assert_eq!(
        ui.get_catalog_rows()
            .row_data(0)
            .unwrap()
            .cells
            .row_data(1)
            .unwrap(),
        "01"
    );
    for label in ["과목·분반", "교원", "수강 집단", "강의실·장비", "교시표"] {
        by_role_and_label(&ui, AccessibleRole::Tab, label);
    }
    ui.invoke_catalog_select(0);
    snapshot(&ui, "ko-1366x768-catalog");
    ui.window().set_size(LogicalSize::new(1024.0, 640.0));
    slint::platform::update_timers_and_animations();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "ko-1024x640-catalog-detail");
    ui.invoke_catalog_close();

    ui.invoke_language_selected(1);
    ui.invoke_catalog_option(0, 3);
    let columns = by_role_and_label(&ui, AccessibleRole::Button, "Columns");
    columns.invoke_accessible_default_action();
    assert!(ui.get_layout_min_width() <= 1024.0 && ui.get_layout_min_height() <= 640.0);
    snapshot(&ui, "en-1024x640-catalog-columns");
    columns.invoke_accessible_default_action();
    ui.invoke_language_selected(0);
    ui.invoke_catalog_option(0, 1);
    assert_eq!(ui.get_catalog_rows().row_count(), 28);
    ui.invoke_catalog_begin(1);
    ui.invoke_catalog_apply();
    assert!(
        !ui.get_catalog_fields()
            .row_data(0)
            .unwrap()
            .error
            .is_empty()
    );
    ui.invoke_language_selected(1);
    assert_eq!(ui.get_catalog_detail_title(), "Add record");
    assert!(
        ui.get_catalog_fields()
            .row_data(0)
            .unwrap()
            .error
            .contains("Check the format")
    );
    ui.invoke_catalog_edit_field(0, "T-NEW".into());
    assert!(
        ui.get_catalog_fields()
            .row_data(0)
            .unwrap()
            .error
            .is_empty()
    );
    ui.invoke_language_selected(0);
    assert_eq!(ui.get_catalog_fields().row_data(0).unwrap().value, "T-NEW");
    ui.invoke_catalog_edit_field(1, "새 교원".into());
    ui.invoke_catalog_apply();
    assert_eq!(ui.get_catalog_rows().row_count(), 29);
    ui.invoke_catalog_search("T-NEW".into());
    assert_eq!(ui.get_catalog_rows().row_count(), 1);
    ui.invoke_catalog_select(0);
    ui.invoke_catalog_begin(2);
    ui.invoke_catalog_edit_field(1, "수정한 교원".into());
    ui.invoke_catalog_apply();
    assert_eq!(
        ui.get_catalog_rows().row_data(0).unwrap().label,
        "수정한 교원"
    );
    ui.invoke_catalog_select(0);
    ui.invoke_catalog_begin(5);
    assert_eq!(ui.get_catalog_rows().row_count(), 1);
    assert_eq!(ui.get_catalog_editor(), 5);
    ui.invoke_catalog_apply();
    assert_eq!(ui.get_catalog_rows().row_count(), 0);

    ui.invoke_catalog_search("".into());
    ui.invoke_catalog_option(0, 0);
    let enrollment = |index| {
        ui.get_catalog_rows()
            .row_data(index)
            .unwrap()
            .cells
            .row_data(5)
            .unwrap()
    };
    let old = enrollment(0);
    ui.invoke_catalog_check(0, true);
    ui.invoke_catalog_check(1, true);
    ui.invoke_catalog_begin(3);
    ui.invoke_catalog_edit_field(0, "999".into());
    ui.invoke_catalog_apply();
    assert_eq!(ui.get_catalog_editor(), 4);
    assert_eq!(ui.get_catalog_preview().row_count(), 2);
    assert_eq!(enrollment(0), old);
    ui.invoke_catalog_apply();
    assert_eq!(enrollment(0), "999");
    assert_eq!(enrollment(1), "999");

    ui.hide().unwrap();
    drop(ui);
    let workspace = auler_store::Workspace::open(&path).unwrap();
    let key = workspace.terms().unwrap()[0].key;
    let term = workspace.load_term(key).unwrap();
    assert_eq!(
        term.sections
            .iter()
            .filter(|section| section.enrollment.count == 999)
            .count(),
        2
    );
    assert_eq!(term.instructors.len(), 28);
    drop(workspace);
    std::fs::remove_file(path).unwrap();
}

fn card(ui: &AppWindow, index: usize) -> auler::CardData {
    ui.get_cards().row_data(index).expect("카드")
}

/// 어떤 화면·언어·폭에서도 레이아웃 최소 크기가 창 최소 크기(1024×640)를 넘지 않아야 잘리지 않는다.
#[test]
fn layout_fits_minimum_window_on_every_page() {
    for size in [wide(), LogicalSize::new(1024.0, 640.0)] {
        let ui = setup(KO, size);
        for language in [KO, EN] {
            set_language(&ui, language);
            for page in 0..=auler::LAST_PAGE {
                ui.set_page(page);
                let (w, h) = (ui.get_layout_min_width(), ui.get_layout_min_height());
                eprintln!(
                    "min-size size={}x{} lang={language} page={page}: {w:.0}x{h:.0}",
                    size.width, size.height
                );
                assert!(
                    w <= 1024.0 && h <= 640.0,
                    "창 {}×{}, 언어 {language}, 화면 {page}: 최소 {w}×{h}",
                    size.width,
                    size.height
                );
            }
        }
    }
}

/// 좁은 창에서는 상세 패널이 오버레이로 바뀐다.
#[test]
fn narrow_window_switches_detail_panel_to_overlay() {
    let ui = setup(KO, wide());
    assert!(!ui.get_compact_timetable());
    assert_eq!(
        by_role(&ui, AccessibleRole::Complementary).len(),
        1,
        "넓은 창: 고정 상세 패널"
    );

    ui.window().set_size(LogicalSize::new(1100.0, 680.0));
    // `changed width` 콜백은 이벤트 루프가 한 번 돌 때 실행된다.
    slint::platform::update_timers_and_animations();
    assert!(ui.get_compact_timetable());
    assert!(
        by_role(&ui, AccessibleRole::Complementary).is_empty(),
        "닫힌 오버레이"
    );

    by_role_and_label(
        &ui,
        AccessibleRole::ListItem,
        "자료구조 01, 김교원 · 공학관 302, 목 3~4교시",
    )
    .invoke_accessible_default_action();
    assert_eq!(
        by_role(&ui, AccessibleRole::Complementary).len(),
        1,
        "카드를 고르면 오버레이가 열린다"
    );

    by_role_and_label(&ui, AccessibleRole::Button, "닫기").invoke_accessible_default_action();
    assert!(by_role(&ui, AccessibleRole::Complementary).is_empty());
    let toggle = by_role_and_label(&ui, AccessibleRole::Button, "상세 정보");
    let filter = by_role_and_label(&ui, AccessibleRole::Combobox, "보기");
    assert_eq!(toggle.size().height, filter.size().height);
    assert_eq!(toggle.accessible_checked(), Some(false));
    toggle.invoke_accessible_default_action();
    assert_eq!(toggle.accessible_checked(), Some(true));
    assert_eq!(by_role(&ui, AccessibleRole::Complementary).len(), 1);
}

/// 잠금 스위치를 누른 뒤에도 다른 카드를 고르면 그 카드의 잠금 상태를 보여야 한다.
#[test]
fn lock_switch_follows_the_selected_card() {
    let ui = setup(KO, wide());
    ui.set_selected(3);
    let switch = by_role_and_label(&ui, AccessibleRole::Switch, "잠금");
    assert!(switch.size().width >= 24.0 && switch.size().height >= 24.0);
    assert_eq!(switch.accessible_checked(), Some(false));

    switch.invoke_accessible_default_action();
    assert!(card(&ui, 3).locked, "모델이 바뀐다");
    assert_eq!(switch.accessible_checked(), Some(true));

    ui.set_selected(1);
    assert_eq!(switch.accessible_checked(), Some(false), "잠기지 않은 카드");
    ui.set_selected(0);
    assert_eq!(switch.accessible_checked(), Some(true), "잠긴 카드");
    assert_eq!(ui.get_locked_count(), 4);
}

/// 1교시 배치는 "월 1교시", 연강은 "목 3~4교시". 범위 밖 선택은 안내 문구.
#[test]
fn placement_text_and_selection_bounds() {
    let ui = setup(KO, wide());
    ui.set_selected(0);
    assert!(has_text(&ui, "월 1교시"));
    ui.set_selected(3);
    assert!(has_text(&ui, "목 3~4교시"));

    set_language(&ui, EN);
    assert!(has_text(&ui, "Thu, periods 3–4"));
    ui.set_selected(0);
    assert!(has_text(&ui, "Mon, period 1"));

    ui.set_selected(99);
    assert!(has_text(&ui, "Select a card to see its details."));
}

/// 내비 항목은 탭 역할, 현재 화면은 선택 상태, 기본 동작으로 화면을 바꾼다.
#[test]
fn nav_items_are_selectable_tabs() {
    let ui = setup(KO, wide());
    let tabs = by_role(&ui, AccessibleRole::Tab);
    assert_eq!(tabs.len(), 8);
    let data = by_role_and_label(&ui, AccessibleRole::Tab, "기준자료");
    assert_eq!(data.accessible_item_selected(), Some(false));
    data.invoke_accessible_default_action();
    assert_eq!(ui.get_page(), 1);
    assert_eq!(data.accessible_item_selected(), Some(true));
    let review = by_role_and_label(&ui, AccessibleRole::Tab, "검토함");
    assert_eq!(review.accessible_description().as_deref(), Some("대기 3건"));
}

/// 비활성 아이콘 버튼은 접근성 기본 동작으로도 눌리지 않는다.
#[test]
fn disabled_icon_button_ignores_default_action() {
    let ui = setup(KO, wide());
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let c = calls.clone();
    ui.on_undo(move || c.set(c.get() + 1));

    let undo = by_role_and_label(&ui, AccessibleRole::Button, "실행 취소");
    assert_eq!(undo.accessible_enabled(), Some(false));
    undo.invoke_accessible_default_action();
    assert_eq!(calls.get(), 0);
}

/// 이동 규칙, 실행 취소·다시 실행, 잠금·겹침 거절
#[test]
fn move_undo_redo_and_rejections() {
    let ui = setup(KO, wide());
    assert!(!ui.invoke_move_card(0, 1, 3), "잠긴 카드는 옮기지 않는다");
    assert!(!ui.invoke_move_card(3, 3, 4), "데이터베이스 01과 겹친다");
    assert!(!ui.invoke_move_card(3, 3, 7), "격자 밖");

    assert!(ui.invoke_move_card(3, 3, 0));
    assert_eq!((card(&ui, 3).day, card(&ui, 3).start), (3, 0));
    assert!(ui.get_can_undo());

    by_role_and_label(&ui, AccessibleRole::Button, "실행 취소").invoke_accessible_default_action();
    assert_eq!((card(&ui, 3).day, card(&ui, 3).start), (3, 2));
    assert!(ui.get_can_redo());

    by_role_and_label(&ui, AccessibleRole::Button, "다시 실행").invoke_accessible_default_action();
    assert_eq!((card(&ui, 3).day, card(&ui, 3).start), (3, 0));
    assert_eq!(ui.get_conflict_count(), 0);
}

/// 끌어서 놓기와 키보드(Ctrl+←)가 같은 결과를 낸다.
#[test]
fn drag_and_keyboard_move_give_the_same_result() {
    const LABEL: &str = "자료구조 01, 김교원 · 공학관 302, 목 3~4교시";

    let dragged = setup(KO, wide());
    let el = by_role_and_label(&dragged, AccessibleRole::ListItem, LABEL);
    let (pos, size) = (el.absolute_position(), el.size());
    let column = size.width + 6.0;
    el.mock_drag(
        LogicalPosition::new(pos.x + size.width / 2.0 - column, pos.y + size.height / 2.0),
        PointerEventButton::Left,
    );
    let by_drag = (card(&dragged, 3).day, card(&dragged, 3).start);

    let keyed = setup(KO, wide());
    by_role_and_label(&keyed, AccessibleRole::ListItem, LABEL)
        .mock_single_click(PointerEventButton::Left);
    for event in [
        WindowEvent::KeyPressed {
            text: Key::Control.into(),
        },
        WindowEvent::KeyPressed {
            text: Key::LeftArrow.into(),
        },
        WindowEvent::KeyReleased {
            text: Key::LeftArrow.into(),
        },
        WindowEvent::KeyReleased {
            text: Key::Control.into(),
        },
    ] {
        keyed.window().dispatch_event(event);
    }
    let by_key = (card(&keyed, 3).day, card(&keyed, 3).start);

    assert_eq!(by_drag, (2, 2), "수요일 같은 교시로 옮겨진다");
    assert_eq!(by_drag, by_key);
}

/// 영어 화면에서는 앱이 만드는 문구가 모두 영어다. 원본 자료(과목명 등)는 그대로다.
#[test]
fn english_mode_translates_generated_text_only() {
    let ui = setup(EN, wide());
    assert!(has_text(&ui, "Mon"));
    assert!(has_text(&ui, "2027, term 1") || has_text(&ui, "컴퓨터공학과  /  2027, term 1"));
    assert!(
        ElementQuery::from_root(&ui)
            .match_descendants()
            .match_predicate(|e| e
                .accessible_label()
                .is_some_and(|l| l.contains("CS201, 30 students, 1 period left")))
            .find_first()
            .is_some()
    );
    assert!(
        ElementQuery::from_root(&ui)
            .match_descendants()
            .match_predicate(|e| e
                .accessible_label()
                .is_some_and(|l| l.contains("2 PCs short")))
            .find_first()
            .is_some()
    );
    assert!(
        ElementQuery::from_root(&ui)
            .match_descendants()
            .match_accessible_role(AccessibleRole::ListItem)
            .match_predicate(|e| e
                .accessible_label()
                .is_some_and(|l| l.starts_with("자료구조 01")))
            .find_first()
            .is_some(),
        "과목명은 번역하지 않는다"
    );

    // 표의 상태 열은 Rust가 만들므로 언어를 바꾸면 다시 만든다
    let status = |ui: &AppWindow| ui.get_rows().row_data(0).unwrap().row_data(7).unwrap().text;
    assert_eq!(status(&ui), "Assigned");
    set_language(&ui, KO);
    assert_eq!(status(&ui), "배정 완료");
}

/// 입력란과 선택 상자에 이름이 있고, 장식 문자가 이름에 섞이지 않는다.
#[test]
fn inputs_have_labels_and_no_decorative_glyphs() {
    let ui = setup(KO, wide());
    for label in ["검색", "강좌 검색", "분반 검색"] {
        by_role_and_label(&ui, AccessibleRole::TextInput, label);
    }
    for label in ["언어", "보기", "학년"] {
        by_role_and_label(&ui, AccessibleRole::Combobox, label);
    }
    let decorated = ElementQuery::from_root(&ui)
        .match_descendants()
        .match_predicate(|e| e.accessible_label().is_some_and(|l| l.contains('●')))
        .find_all();
    assert!(decorated.is_empty(), "● 같은 장식 문자를 읽지 않아야 한다");
}

/// 사람이 확인하는 스냅숏. 레이아웃이 실제 글꼴로 그려진 모습을 남긴다.
#[test]
fn write_snapshots() {
    for (name, language, size, page) in [
        ("ko-1366x768-timetable", KO, wide(), 3),
        ("en-1366x768-timetable", EN, wide(), 3),
        (
            "ko-1024x640-timetable",
            KO,
            LogicalSize::new(1024.0, 640.0),
            3,
        ),
        ("en-1024x640-data", EN, LogicalSize::new(1024.0, 640.0), 1),
        ("ko-1366x768-about", KO, wide(), 7),
        (
            "ko-1366x1300-about-full",
            KO,
            LogicalSize::new(1366.0, 1300.0),
            7,
        ),
        ("en-1024x640-about", EN, LogicalSize::new(1024.0, 640.0), 7),
    ] {
        let ui = setup(language, size);
        ui.set_page(page);
        snapshot(&ui, name);
    }
}

/// 정보 화면: 제작자·저장소·라이선스 링크, 기술 스택, 사용한 오픈소스, AboutSlint(Slint 라이선스 조건)
#[test]
fn about_page_lists_author_repository_license_and_components() {
    // 테스트 API의 요소 검색은 스크롤 영역 밖으로 잘린 요소를 건너뛴다(화면 읽기용 트리는 포함).
    // 모든 항목이 보이도록 창을 길게 잡는다.
    let ui = setup(KO, LogicalSize::new(1366.0, 1600.0));
    // 테스트에서는 브라우저를 열지 않고 요청된 주소만 모은다.
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let sink = opened.clone();
    ui.on_open_url(move |url| sink.borrow_mut().push(url.to_string()));

    by_role_and_label(&ui, AccessibleRole::Tab, "정보").invoke_accessible_default_action();
    assert_eq!(ui.get_page(), auler::LAST_PAGE);

    let links = [
        ("Lehwon", "https://github.com/Lehwon"),
        ("github.com/Lehwon/Auler", "https://github.com/Lehwon/Auler"),
        ("MIT", "https://github.com/Lehwon/Auler/blob/main/LICENSE"),
        ("Slint", "https://github.com/slint-ui/slint"),
        ("sys-locale", "https://github.com/1Password/sys-locale"),
        ("windows-sys", "https://github.com/microsoft/windows-rs"),
        ("Rust std", "https://github.com/rust-lang/rust"),
        (
            "전체 오픈소스 구성 요소와 라이선스",
            "https://github.com/Lehwon/Auler/blob/main/THIRD-PARTY.md",
        ),
    ];
    for (label, url) in links {
        let link = by_role_and_label(&ui, AccessibleRole::Button, label);
        assert!(
            link.size().width >= 24.0 && link.size().height >= 24.0,
            "{label}"
        );
        assert_eq!(
            link.accessible_description().as_deref(),
            Some(url),
            "{label}"
        );
        assert!(
            auler::is_allowed_url(url),
            "{url}는 허용 목록에 있어야 한다"
        );
        link.invoke_accessible_default_action();
    }
    let expected: Vec<String> = links.iter().map(|(_, url)| url.to_string()).collect();
    assert_eq!(*opened.borrow(), expected);

    for text in [
        "제작자",
        "저장소",
        "라이선스",
        "기술 스택",
        "사용한 오픈소스",
        "Slint Royalty-free License 2.0",
    ] {
        assert!(has_text(&ui, text), "{text}");
    }
    assert!(
        ElementHandle::find_by_element_type_name(&ui, "AboutSlint")
            .next()
            .is_some(),
        "Slint Royalty-free 라이선스 조건: 정보 화면에 AboutSlint를 표시한다"
    );
}
