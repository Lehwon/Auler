//! 개발용 상호작용 벤치마크 (리뷰 3, 계획 0.4).
//!
//! `AULER_BENCH=scroll|drag|idle`로 실행하면 준비 시간 뒤 앱이 스스로 입력 이벤트를 넣는다.
//! Slint의 `dispatch_event`를 쓰므로 실제 마우스를 움직이지 않고, 매번 같은 입력이 들어간다.
//! 끝나면 한 줄 요약을 표준 오류에 쓰고 종료한다. 프레임 수는 `SLINT_DEBUG_PERFORMANCE`로 따로 본다.
//!
//! - scroll: 기준자료 화면의 500행 표를 2초마다 방향을 바꾸며 휠 스크롤
//! - drag: 시간표 화면의 '자료구조 01'(목 3~4교시) 카드를 누른 채 좌우·상하로 흔들고 제자리에 놓기
//! - idle: 입력 없이 같은 시간 대기 (기준값)
//!
//! 좌표는 내용 크기 1366×768 기준이다. 창이 다른 크기로 열리면 요약에 그 크기를 함께 적는다.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Timer, TimerMode};

use crate::AppWindow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Scroll,
    Drag,
    Idle,
}

impl Scenario {
    pub fn from_env() -> Option<Self> {
        match std::env::var("AULER_BENCH").ok()?.as_str() {
            "scroll" => Some(Self::Scroll),
            "drag" => Some(Self::Drag),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }

    /// 시나리오가 쓰는 화면
    pub fn page(self) -> i32 {
        match self {
            Self::Scroll => 1,
            Self::Drag | Self::Idle => 3,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Scroll => "scroll",
            Self::Drag => "drag",
            Self::Idle => "idle",
        }
    }
}

const WARMUP: Duration = Duration::from_secs(3);
const SETTLE: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(16);
/// 기준자료 표 가운데
const TABLE_POINT: LogicalPosition = LogicalPosition { x: 683.0, y: 450.0 };
/// 시간표의 '자료구조 01'(목 3~4교시) 카드 가운데
const CARD_POINT: LogicalPosition = LogicalPosition { x: 834.0, y: 395.0 };

/// 벤치마크를 시작한다. 반환된 타이머를 살려 두어야 한다.
pub fn start(ui: &AppWindow, scenario: Scenario) -> Timer {
    let seconds: u64 = std::env::var("AULER_BENCH_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let run = Duration::from_secs(seconds);
    let weak = ui.as_weak();
    let launched = Instant::now();
    let state = Rc::new(BenchState::default());

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = weak.upgrade() else { return };
        let window = ui.window();
        let elapsed = launched.elapsed();
        if elapsed < WARMUP {
            return;
        }
        let t = elapsed - WARMUP;
        if state.begin.get().is_none() {
            state.begin.set(Some(probe()));
            if scenario == Scenario::Drag {
                window.dispatch_event(WindowEvent::PointerMoved { position: CARD_POINT });
                window.dispatch_event(WindowEvent::PointerPressed {
                    position: CARD_POINT,
                    button: PointerEventButton::Left,
                });
            }
        }

        if t < run {
            let secs = t.as_secs_f32();
            match scenario {
                Scenario::Scroll => {
                    let down = (t.as_secs() / 2).is_multiple_of(2);
                    window.dispatch_event(WindowEvent::PointerMoved { position: TABLE_POINT });
                    window.dispatch_event(WindowEvent::PointerScrolled {
                        position: TABLE_POINT,
                        delta_x: 0.0,
                        delta_y: if down { -40.0 } else { 40.0 },
                    });
                }
                Scenario::Drag => {
                    // 한 열(약 131px)과 한 교시(60px) 넘게 흔들어 놓일 자리 표시가 계속 바뀌게 한다.
                    let dx = 160.0 * (secs * 3.0).sin();
                    let dy = 70.0 * (secs * 1.7).sin();
                    window.dispatch_event(WindowEvent::PointerMoved {
                        position: LogicalPosition::new(CARD_POINT.x + dx, CARD_POINT.y + dy),
                    });
                }
                Scenario::Idle => {}
            }
            state.peak_private.set(state.peak_private.get().max(private_bytes()));
            return;
        }

        if state.end.get().is_none() {
            if scenario == Scenario::Drag {
                window.dispatch_event(WindowEvent::PointerMoved { position: CARD_POINT });
                window.dispatch_event(WindowEvent::PointerReleased {
                    position: CARD_POINT,
                    button: PointerEventButton::Left,
                });
            }
            state.end.set(Some(probe()));
            return;
        }

        if t >= run + SETTLE {
            let (Some(begin), Some(end)) = (state.begin.get(), state.end.get()) else { return };
            let wall = end.wall.duration_since(begin.wall).as_secs_f64();
            let cpu = (end.cpu - begin.cpu).as_secs_f64();
            let size = window.size().to_logical(window.scale_factor());
            eprintln!(
                "AULER_BENCH scenario={} seconds={:.1} cpu_core_pct={:.1} private_before_mb={:.1} private_peak_mb={:.1} private_after_mb={:.1} scale={} content={}x{}",
                scenario.name(),
                wall,
                if wall > 0.0 { cpu / wall * 100.0 } else { 0.0 },
                mb(begin.private),
                mb(state.peak_private.get().max(end.private)),
                mb(private_bytes()),
                window.scale_factor(),
                size.width.round(),
                size.height.round(),
            );
            let _ = slint::quit_event_loop();
        }
    });
    timer
}

#[derive(Default)]
struct BenchState {
    begin: Cell<Option<Probe>>,
    end: Cell<Option<Probe>>,
    peak_private: Cell<usize>,
}

#[derive(Clone, Copy)]
struct Probe {
    wall: Instant,
    cpu: Duration,
    private: usize,
}

fn probe() -> Probe {
    Probe {
        wall: Instant::now(),
        cpu: cpu_time(),
        private: private_bytes(),
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// 이 프로세스의 사용자+커널 CPU 시간
#[cfg(windows)]
fn cpu_time() -> Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: 지역 변수에만 쓴다. GetCurrentProcess는 닫을 필요 없는 의사 핸들이다.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return Duration::ZERO;
    }
    let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// 이 프로세스의 Private Bytes (작업 관리자의 '커밋 크기'와 같은 지표)
#[cfg(windows)]
fn private_bytes() -> usize {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: 크기를 맞춘 지역 구조체에만 쓴다.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        );
        if ok == 0 { 0 } else { counters.PrivateUsage }
    }
}

#[cfg(not(windows))]
fn cpu_time() -> Duration {
    Duration::ZERO
}

#[cfg(not(windows))]
fn private_bytes() -> usize {
    0
}
