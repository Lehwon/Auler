// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use auler::bench::{self, Scenario};
use auler::{AppWindow, INITIAL_SIZE};
use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    auler::init(&ui, auler::startup_language());

    // 개발용: AULER_PAGE(0~7)로 시작 화면을 고정한다 (캡처·측정 자동화).
    if let Some(page) = std::env::var("AULER_PAGE")
        .ok()
        .and_then(|p| p.parse::<i32>().ok())
    {
        ui.set_page(page.clamp(0, auler::LAST_PAGE));
    }

    // 개발용: AULER_BENCH(scroll|drag|idle)로 상호작용 벤치마크를 돌리고 끝낸다.
    let bench = Scenario::from_env().map(|scenario| {
        ui.set_page(scenario.page());
        bench::start(&ui, scenario)
    });

    auler::placement::place_initial_window(&ui, INITIAL_SIZE);
    ui.run()?;
    drop(bench);
    Ok(())
}
