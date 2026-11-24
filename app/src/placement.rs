//! 첫 창 위치와 크기.
//!
//! Windows는 새 창을 계단식으로 비켜 띄우므로 큰 창의 오른쪽과 아래가 화면 밖으로
//! 나갈 수 있다. 창을 작업 영역 가운데에 두고, 작업 영역보다 크면 최대화한다.

use slint::{ComponentHandle, LogicalSize, PhysicalPosition};

/// 제목 표시줄과 테두리를 포함한 창 바깥 크기를 어림하는 여유값 (논리 픽셀)
const FRAME_MARGIN: LogicalSize = LogicalSize {
    width: 16.0,
    height: 48.0,
};

pub fn place_initial_window(ui: &impl ComponentHandle, content: LogicalSize) {
    let Some(area) = work_area() else { return };
    let outer_w = ((content.width + FRAME_MARGIN.width) * area.scale).round() as i32;
    let outer_h = ((content.height + FRAME_MARGIN.height) * area.scale).round() as i32;

    if outer_w > area.width || outer_h > area.height {
        ui.window().set_maximized(true);
    } else {
        ui.window().set_position(PhysicalPosition::new(
            area.left + (area.width - outer_w) / 2,
            area.top + (area.height - outer_h) / 2,
        ));
    }
}

/// 주 모니터의 작업 영역(작업 표시줄 제외, 물리 픽셀)과 배율
struct WorkArea {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    scale: f32,
}

#[cfg(windows)]
fn work_area() -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: SPI_GETWORKAREA는 pvParam에 RECT 하나를 쓴다.
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut rect).cast(), 0) };
    if ok == 0 {
        return None;
    }
    // SAFETY: 인자가 없는 조회 함수다.
    let dpi = unsafe { GetDpiForSystem() };
    Some(WorkArea {
        left: rect.left,
        top: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
        scale: dpi as f32 / 96.0,
    })
}

#[cfg(not(windows))]
fn work_area() -> Option<WorkArea> {
    None
}
