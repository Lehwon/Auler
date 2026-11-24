// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod placement;
mod sample;

use slint::{LogicalSize, ModelRc, SharedString, VecModel};

slint::include_modules!();

const LANGUAGES: [&str; 2] = ["ko", "en"];

/// 첫 창의 내용 크기. 명세서 3.2의 우선 검증 해상도(1366×768)에 맞춘다.
const INITIAL_SIZE: LogicalSize = LogicalSize {
    width: 1366.0,
    height: 768.0,
};

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // 시스템 언어가 한국어가 아니면 영어로 시작한다.
    // 개발용: AULER_LANG(ko|en), AULER_PAGE(0~6)로 시작 언어와 화면을 고정할 수 있다.
    let start = match std::env::var("AULER_LANG").ok().as_deref() {
        Some("en") => 1,
        Some("ko") => 0,
        _ if system_language_is_korean() => 0,
        _ => 1,
    };
    apply_language(&ui, start);
    if let Some(page) = std::env::var("AULER_PAGE")
        .ok()
        .and_then(|p| p.parse::<i32>().ok())
    {
        ui.set_page(page.clamp(0, 6));
    }

    ui.set_cards(ModelRc::new(VecModel::from(sample::cards())));
    ui.set_periods(ModelRc::new(VecModel::from(sample::periods())));
    ui.set_rows(sample::section_rows(500));

    let weak = ui.as_weak();
    ui.on_language_selected(move |index| {
        if let Some(ui) = weak.upgrade() {
            apply_language(&ui, index as usize);
        }
    });

    ui.window().set_size(INITIAL_SIZE);
    placement::place_initial_window(&ui, INITIAL_SIZE);

    ui.run()
}

fn apply_language(ui: &AppWindow, index: usize) {
    let index = index.min(LANGUAGES.len() - 1);
    let lang = LANGUAGES[index];
    // "en"은 원문(영어)을 고른다. 번역이 빠진 언어면 원문으로 남는다.
    let _ = slint::select_bundled_translation(lang);
    ui.set_language_index(index as i32);
    let days: Vec<SharedString> = sample::days(lang).into_iter().map(Into::into).collect();
    ui.set_days(ModelRc::new(VecModel::from(days)));
}

fn system_language_is_korean() -> bool {
    sys_locale::get_locale().is_some_and(|l| l.to_ascii_lowercase().starts_with("ko"))
}
