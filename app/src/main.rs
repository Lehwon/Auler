// 릴리스 빌드에서는 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use auler::bench::{self, Scenario};
use auler::{AppWindow, INITIAL_SIZE};
use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let page = std::env::var("AULER_PAGE")
        .ok()
        .and_then(|p| p.parse::<i32>().ok());
    let scenario = Scenario::from_env();
    if page.is_some() || scenario.is_some() {
        auler::init(&ui, auler::startup_language());
    } else {
        let workspace = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        auler::workspace::init(&ui, auler::startup_language(), workspace.as_deref());
    }
    if let Some(page) = page {
        ui.set_page(page.clamp(0, auler::LAST_PAGE));
    }

    // 개발용: AULER_BENCH(scroll|drag|idle)로 상호작용 벤치마크를 돌리고 끝낸다.
    let bench = scenario.map(|scenario| {
        ui.set_page(scenario.page());
        bench::start(&ui, scenario)
    });

    auler::placement::place_initial_window(&ui, INITIAL_SIZE);
    ui.run()?;
    drop(bench);
    Ok(())
}
