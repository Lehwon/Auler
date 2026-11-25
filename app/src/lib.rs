//! Auler 데스크톱 앱.
//!
//! 화면(Slint)과 상태를 연결한다. 실행 파일(`main.rs`)과 UI 테스트가 같은 `init`을 쓴다.

mod backup;
pub mod bench;
mod catalog;
mod constraints;
mod export;
mod file_dialog;
mod import;
pub mod placement;
mod review_screen;
pub mod sample;
pub mod schedule;
mod workbench;
pub mod workspace;

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, LogicalSize, Model, ModelRc, VecModel};

use schedule::{GridSize, History, Move, Slot};

slint::include_modules!();

/// 화면 언어. 인덱스는 상단바 언어 선택의 순서와 같다.
pub const LANGUAGES: [&str; 2] = ["ko", "en"];

/// 첫 창의 내용 크기. 우선 지원하는 해상도(1366×768)에 맞춘다.
pub const INITIAL_SIZE: LogicalSize = LogicalSize {
    width: 1366.0,
    height: 768.0,
};

/// 화면 검증용 분반 표의 행 수
const SAMPLE_ROWS: usize = 500;

/// 내비 화면 번호의 마지막 값 (0=홈 … 6=설정, 7=정보)
pub const LAST_PAGE: i32 = 7;

/// 정보 화면에서 열 수 있는 주소. 이 밖의 주소는 열지 않는다.
const ALLOWED_URL_PREFIXES: [&str; 1] = ["https://github.com/"];

struct State {
    cards: Rc<VecModel<CardData>>,
    grid: GridSize,
    history: RefCell<History>,
}

impl State {
    fn placements(&self) -> Vec<(Slot, bool)> {
        self.cards.iter().map(|c| (slot_of(&c), c.locked)).collect()
    }

    fn check(&self, index: i32, day: i32, start: i32) -> Result<Slot, schedule::MoveError> {
        let index = usize::try_from(index).map_err(|_| schedule::MoveError::OutOfRange)?;
        schedule::check_move(&self.placements(), index, day, start, self.grid)
    }

    fn apply(&self, index: usize, to: Slot) {
        if let Some(mut card) = self.cards.row_data(index) {
            card.day = to.day;
            card.start = to.start;
            self.cards.set_row_data(index, card);
        }
    }
}

fn slot_of(card: &CardData) -> Slot {
    Slot {
        day: card.day,
        start: card.start,
        span: card.span,
    }
}

/// 화면에 데이터를 넣고 콜백을 연결한다. 반환값을 버려도 콜백은 계속 동작한다.
pub fn init(ui: &AppWindow, language: usize) {
    let periods = sample::periods();
    let days = sample::weekdays();
    let state = Rc::new(State {
        cards: Rc::new(VecModel::from(sample::cards())),
        grid: GridSize {
            days: days.len() as i32,
            periods: periods.len() as i32,
        },
        history: RefCell::new(History::default()),
    });

    init_shell(ui, language);
    ui.set_department(sample::DEPARTMENT.into());
    ui.set_year(sample::YEAR);
    ui.set_term(sample::TERM);
    ui.set_days(ModelRc::new(VecModel::from(days)));
    ui.set_periods(ModelRc::new(VecModel::from(periods)));
    ui.set_cards(ModelRc::from(state.cards.clone()));
    ui.set_unassigned(ModelRc::new(VecModel::from(sample::unassigned())));
    ui.set_alternatives(ModelRc::new(VecModel::from(sample::alternatives())));
    set_language(ui, language);
    refresh(ui, &state);

    let weak = ui.as_weak();
    ui.on_language_selected(move |index| {
        if let Some(ui) = weak.upgrade() {
            set_language(&ui, usize::try_from(index).unwrap_or(0));
        }
    });

    let (weak, st) = (ui.as_weak(), state.clone());
    ui.on_lock_toggled(move |index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if let Some(mut card) = st.cards.row_data(index) {
            card.locked = !card.locked;
            st.cards.set_row_data(index, card);
        }
        if let Some(ui) = weak.upgrade() {
            refresh(&ui, &st);
        }
    });

    let st = state.clone();
    ui.on_can_move(move |index, day, start| st.check(index, day, start).is_ok());

    let (weak, st) = (ui.as_weak(), state.clone());
    ui.on_move_card(move |index, day, start| {
        let Ok(to) = st.check(index, day, start) else {
            return false;
        };
        let index = index as usize;
        let from = st.cards.row_data(index).map(|c| slot_of(&c)).unwrap_or(to);
        st.apply(index, to);
        st.history.borrow_mut().record(Move { index, from, to });
        if let Some(ui) = weak.upgrade() {
            refresh(&ui, &st);
        }
        true
    });

    let (weak, st) = (ui.as_weak(), state.clone());
    ui.on_undo(move || {
        let Some(m) = st.history.borrow_mut().pop_undo() else {
            return;
        };
        // 되돌릴 자리가 그사이 막혔으면(잠금, 겹침) 기록을 되돌려 놓고 아무것도 바꾸지 않는다.
        match st.check(m.index as i32, m.from.day, m.from.start) {
            Ok(_) => {
                st.apply(m.index, m.from);
                st.history.borrow_mut().push_redo(m);
            }
            Err(_) => st.history.borrow_mut().push_undo(m),
        }
        if let Some(ui) = weak.upgrade() {
            refresh(&ui, &st);
        }
    });

    let (weak, st) = (ui.as_weak(), state);
    ui.on_redo(move || {
        let Some(m) = st.history.borrow_mut().pop_redo() else {
            return;
        };
        match st.check(m.index as i32, m.to.day, m.to.start) {
            Ok(_) => {
                st.apply(m.index, m.to);
                st.history.borrow_mut().push_undo(m);
            }
            Err(_) => st.history.borrow_mut().push_redo(m),
        }
        if let Some(ui) = weak.upgrade() {
            refresh(&ui, &st);
        }
    });
}

/// 건수, 실행 취소 가능 여부 등 파생 값을 다시 계산한다.
fn refresh(ui: &AppWindow, state: &State) {
    let slots: Vec<Slot> = state.cards.iter().map(|c| slot_of(&c)).collect();
    ui.set_locked_count(state.cards.iter().filter(|c| c.locked).count() as i32);
    ui.set_conflict_count(schedule::conflict_count(&slots) as i32);
    ui.set_unassigned_count(ui.get_unassigned().row_count() as i32);
    let history = state.history.borrow();
    ui.set_can_undo(history.can_undo());
    ui.set_can_redo(history.can_redo());
}

/// 화면 언어를 바꾼다. Rust가 만드는 문구(표의 상태 열)도 새 언어로 다시 만든다.
pub fn set_language(ui: &AppWindow, index: usize) {
    set_shell_language(ui, index);
    let labels = ui.global::<Labels>();
    let status = [
        labels.get_status_assigned(),
        labels.get_status_unassigned(),
        labels.get_status_needs_review(),
    ];
    ui.set_rows(sample::section_rows(SAMPLE_ROWS, &status));
}

pub(crate) fn init_shell(ui: &AppWindow, language: usize) {
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.on_open_url(|url| {
        open_url(&url);
    });
    set_shell_language(ui, language);
}

pub(crate) fn set_shell_language(ui: &AppWindow, index: usize) {
    let index = index.min(LANGUAGES.len() - 1);
    // "en"은 원문(영어)을 고른다. 번역이 빠진 문구는 원문으로 남는다.
    let _ = slint::select_bundled_translation(LANGUAGES[index]);
    ui.set_language_index(index as i32);
}

/// 시작 언어: `AULER_LANG`(ko|en)이 있으면 그것, 없으면 시스템 언어가 한국어일 때 한국어.
pub fn startup_language() -> usize {
    match std::env::var("AULER_LANG").ok().as_deref() {
        Some("en") => 1,
        Some("ko") => 0,
        _ if system_language_is_korean() => 0,
        _ => 1,
    }
}

fn system_language_is_korean() -> bool {
    sys_locale::get_locale().is_some_and(|l| l.to_ascii_lowercase().starts_with("ko"))
}

/// 정해 둔 https 주소인지 검사한다. 공백·따옴표·제어 문자가 섞인 주소는 거절한다.
pub fn is_allowed_url(url: &str) -> bool {
    ALLOWED_URL_PREFIXES.iter().any(|p| url.starts_with(p))
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
}

/// 정보 화면의 링크를 기본 브라우저로 연다. 허용한 주소가 아니면 아무것도 하지 않는다.
pub fn open_url(url: &str) -> bool {
    is_allowed_url(url) && shell_open(std::ffi::OsStr::new(url))
}

#[cfg(windows)]
fn shell_open(target: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let file = target.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: 두 문자열은 NUL로 끝나고 호출 동안 살아 있다. 나머지 포인터는 null이 허용된다.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 32보다 크면 성공 (ShellExecute 규약)
    result as isize > 32
}

#[cfg(not(windows))]
fn shell_open(_target: &std::ffi::OsStr) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::is_allowed_url;

    #[test]
    fn only_listed_https_urls_can_be_opened() {
        assert!(is_allowed_url("https://github.com/Lehwon/Auler"));
        assert!(is_allowed_url(
            "https://github.com/Lehwon/Auler/blob/main/THIRD-PARTY.md"
        ));
        assert!(!is_allowed_url("http://github.com/Lehwon"), "http는 막는다");
        assert!(!is_allowed_url("https://example.com/"));
        assert!(!is_allowed_url("file:///C:/Windows/System32/cmd.exe"));
        assert!(!is_allowed_url("https://github.com/a b"));
        assert!(!is_allowed_url("https://github.com/\"x"));
    }
}
