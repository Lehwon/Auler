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

fn setup(language: usize, size: LogicalSize) -> AppWindow {
    // 테스트마다 스레드가 다르고 플랫폼은 스레드마다 한 번만 정할 수 있다.
    let _ = slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
        mock_time: true,
        threading: false,
        renderer_name: Some("software".into()),
    })));
    let ui = AppWindow::new().expect("창 생성");
    init(&ui, language);
    ui.window().set_size(size);
    ui.show().expect("창 표시");
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
    let label = label.to_string();
    ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(role)
        .match_predicate(move |e| e.accessible_label().is_some_and(|l| l == label.as_str()))
        .find_first()
        .unwrap_or_else(|| panic!("{role:?} 요소를 찾지 못함"))
}

fn has_text(ui: &AppWindow, text: &str) -> bool {
    ElementHandle::find_by_accessible_label(ui, text)
        .next()
        .is_some()
}

fn card(ui: &AppWindow, index: usize) -> auler::CardData {
    ui.get_cards().row_data(index).expect("카드")
}

/// 리뷰 1: 어떤 화면·언어·폭에서도 레이아웃 최소 크기가 창 최소 크기(1024×640)를 넘지 않아야 잘리지 않는다.
#[test]
fn layout_fits_minimum_window_on_every_page() {
    for size in [wide(), LogicalSize::new(1024.0, 640.0)] {
        let ui = setup(KO, size);
        for language in [KO, EN] {
            set_language(&ui, language);
            for page in 0..=6 {
                ui.set_page(page);
                let (w, h) = (ui.get_layout_min_width(), ui.get_layout_min_height());
                eprintln!("min-size size={}x{} lang={language} page={page}: {w:.0}x{h:.0}", size.width, size.height);
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

/// 좁은 창에서는 상세 패널이 오버레이로 바뀐다 (명세서 3.2).
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
}

/// 리뷰 5: 잠금 스위치를 누른 뒤에도 다른 카드를 고르면 그 카드의 잠금 상태를 보여야 한다.
#[test]
fn lock_switch_follows_the_selected_card() {
    let ui = setup(KO, wide());
    ui.set_selected(3);
    let switch = by_role_and_label(&ui, AccessibleRole::Switch, "잠금");
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

/// 리뷰 5: 1교시 배치는 "월 1교시", 연강은 "목 3~4교시". 범위 밖 선택은 안내 문구.
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

/// 리뷰 4: 내비 항목은 탭 역할, 현재 화면은 선택 상태, 기본 동작으로 화면을 바꾼다.
#[test]
fn nav_items_are_selectable_tabs() {
    let ui = setup(KO, wide());
    let tabs = by_role(&ui, AccessibleRole::Tab);
    assert_eq!(tabs.len(), 7);
    let data = by_role_and_label(&ui, AccessibleRole::Tab, "기준자료");
    assert_eq!(data.accessible_item_selected(), Some(false));
    data.invoke_accessible_default_action();
    assert_eq!(ui.get_page(), 1);
    assert_eq!(data.accessible_item_selected(), Some(true));
    let review = by_role_and_label(&ui, AccessibleRole::Tab, "검토함");
    assert_eq!(review.accessible_description().as_deref(), Some("대기 3건"));
}

/// 리뷰 4: 비활성 아이콘 버튼은 접근성 기본 동작으로도 눌리지 않는다.
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

/// AC-05: 끌어서 놓기와 키보드(Ctrl+←)가 같은 결과를 낸다.
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

/// 리뷰 6: 영어 화면에서는 앱이 만드는 문구가 모두 영어다. 원본 자료(과목명 등)는 그대로다.
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

/// 리뷰 4: 입력란과 선택 상자에 이름이 있고, 장식 문자가 이름에 섞이지 않는다.
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
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
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
    ] {
        let ui = setup(language, size);
        ui.set_page(page);
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
}
