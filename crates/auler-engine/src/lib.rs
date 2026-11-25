//! 학기 시간표의 배치 평가와 충돌 검사.

mod evaluation;
mod occupancy;
mod solver;

pub use evaluation::*;
pub use occupancy::{Occupancy, OccupancyError, occupancy};
pub use solver::*;
