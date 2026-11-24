//! 첫 창의 위치와 크기.
//!
//! Windows는 새 창을 계단식으로 비켜 띄우므로 큰 창의 오른쪽과 아래가 화면 밖으로
//! 나갈 수 있다. 커서가 있는 모니터의 작업 영역 가운데에 창을 두고, 작업 영역보다
//! 크면 최대화한다. 최대화할 때도 복원 크기는 작업 영역 안으로 줄인다.
//!
//! 배율은 그 모니터의 현재 유효 DPI로 계산한다. `GetDpiForSystem`은 로그온 시점의
//! 값을 돌려주므로, 로그아웃 없이 배율을 바꾸면 틀린 값이 나온다.

use slint::{ComponentHandle, LogicalPosition, LogicalSize, PhysicalPosition};

/// 모니터 작업 영역(작업 표시줄 제외)과 창 테두리 크기. 모두 논리 픽셀이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// 제목 표시줄과 테두리를 더한 바깥 크기 - 내용 크기
    pub frame: LogicalSize,
    /// 이 모니터의 현재 배율 (위치를 물리 픽셀로 바꿀 때 쓴다)
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// 내용 크기 그대로, 바깥 창을 작업 영역 가운데에 둔다.
    Centered {
        position: LogicalPosition,
        size: LogicalSize,
    },
    /// 최대화로 시작한다. 복원하면 `restore_size` 크기로 가운데에 놓인다.
    Maximized {
        restore_position: LogicalPosition,
        restore_size: LogicalSize,
    },
}

/// 내용 크기와 작업 영역으로 첫 창 배치를 정한다.
pub fn compute(content: LogicalSize, area: WorkArea) -> Placement {
    let outer_w = content.width + area.frame.width;
    let outer_h = content.height + area.frame.height;
    if outer_w <= area.width && outer_h <= area.height {
        return Placement::Centered {
            position: center(area, outer_w, outer_h),
            size: content,
        };
    }
    let restore_size = LogicalSize::new(
        content.width.min(area.width - area.frame.width).max(1.0),
        content.height.min(area.height - area.frame.height).max(1.0),
    );
    Placement::Maximized {
        restore_position: center(
            area,
            restore_size.width + area.frame.width,
            restore_size.height + area.frame.height,
        ),
        restore_size,
    }
}

fn center(area: WorkArea, outer_w: f32, outer_h: f32) -> LogicalPosition {
    LogicalPosition::new(
        (area.x + (area.width - outer_w) / 2.0).round(),
        (area.y + (area.height - outer_h) / 2.0).round(),
    )
}

/// DPI를 배율로 바꾼다. 0이나 비정상 값이면 100%로 본다.
pub fn scale_from_dpi(dpi: u32) -> f32 {
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// 창을 보이기 전에 크기와 위치를 정한다. 모니터 정보를 못 읽으면 크기만 정한다.
pub fn place_initial_window(ui: &impl ComponentHandle, content: LogicalSize) {
    let window = ui.window();
    let Some(area) = work_area() else {
        window.set_size(content);
        return;
    };
    // 창이 만들어지기 전에는 어느 배율로 바뀔지 모르므로 위치는 물리 픽셀로 준다.
    let physical = |p: LogicalPosition| {
        PhysicalPosition::new(
            (p.x * area.scale).round() as i32,
            (p.y * area.scale).round() as i32,
        )
    };
    match compute(content, area) {
        Placement::Centered { position, size } => {
            window.set_size(size);
            window.set_position(physical(position));
        }
        Placement::Maximized {
            restore_position,
            restore_size,
        } => {
            window.set_size(restore_size);
            window.set_position(physical(restore_position));
            window.set_maximized(true);
        }
    }
}

#[cfg(windows)]
fn work_area() -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows_sys::Win32::UI::HiDpi::{
        AdjustWindowRectExForDpi, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, WS_OVERLAPPEDWINDOW};

    // SAFETY: 아래 호출은 모두 호출자가 소유한 지역 변수에만 쓴다.
    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) == 0 {
            cursor = POINT { x: 0, y: 0 };
        }
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
        let dpi = if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) >= 0 {
            dpi_x
        } else {
            0
        };
        let scale = scale_from_dpi(dpi);

        // 내용 0×0인 창의 바깥 사각형 = 제목 표시줄과 테두리 크기
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let frame_dpi = if dpi == 0 { 96 } else { dpi };
        if AdjustWindowRectExForDpi(&mut frame, WS_OVERLAPPEDWINDOW, 0, 0, frame_dpi) == 0 {
            frame = RECT {
                left: -8,
                top: -31,
                right: 8,
                bottom: 8,
            };
        }

        let work = info.rcWork;
        Some(WorkArea {
            x: work.left as f32 / scale,
            y: work.top as f32 / scale,
            width: (work.right - work.left) as f32 / scale,
            height: (work.bottom - work.top) as f32 / scale,
            frame: LogicalSize::new(
                (frame.right - frame.left) as f32 / scale,
                (frame.bottom - frame.top) as f32 / scale,
            ),
            scale,
        })
    }
}

#[cfg(not(windows))]
fn work_area() -> Option<WorkArea> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: LogicalSize = LogicalSize {
        width: 16.0,
        height: 39.0,
    };

    fn area(width: f32, height: f32) -> WorkArea {
        WorkArea {
            x: 0.0,
            y: 0.0,
            width,
            height,
            frame: FRAME,
            scale: 2.0,
        }
    }

    #[test]
    fn fits_and_is_centered() {
        // 2880×1736 작업 영역 @200% = 논리 1440×868
        let p = compute(LogicalSize::new(1366.0, 768.0), area(1440.0, 868.0));
        assert_eq!(
            p,
            Placement::Centered {
                position: LogicalPosition::new(29.0, 31.0),
                size: LogicalSize::new(1366.0, 768.0),
            }
        );
    }

    #[test]
    fn too_large_is_maximized_with_restore_size_inside_work_area() {
        // 1920×1032 작업 영역 @150% = 논리 1280×688
        let p = compute(LogicalSize::new(1366.0, 768.0), area(1280.0, 688.0));
        let Placement::Maximized {
            restore_position,
            restore_size,
        } = p
        else {
            panic!("최대화해야 한다: {p:?}");
        };
        assert!(restore_size.width + FRAME.width <= 1280.0);
        assert!(restore_size.height + FRAME.height <= 688.0);
        assert!(restore_position.x >= 0.0 && restore_position.y >= 0.0);
    }

    #[test]
    fn work_area_offset_is_respected() {
        // 작업 표시줄이 위쪽에 있거나 보조 모니터인 경우
        let mut a = area(1440.0, 868.0);
        a.x = 1920.0;
        a.y = 40.0;
        let Placement::Centered { position, .. } = compute(LogicalSize::new(1000.0, 600.0), a)
        else {
            panic!("가운데 배치여야 한다");
        };
        assert_eq!(position, LogicalPosition::new(1920.0 + 212.0, 40.0 + 115.0));
    }

    #[test]
    fn zero_dpi_falls_back_to_100_percent() {
        assert_eq!(scale_from_dpi(0), 1.0);
        assert_eq!(scale_from_dpi(144), 1.5);
        assert_eq!(scale_from_dpi(192), 2.0);
    }
}
