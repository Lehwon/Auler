//! 화면 검증 단계의 배치 규칙: 범위, 잠금, 같은 보기 안의 겹침만 본다.
//!
//! 실제 제약 검사(교원·강의실·수강 집단·장비, 필수와 선호)는 M1 편성 엔진에서
//! 구현한다. 여기서는 드래그와 키보드 이동이 같은 규칙을 쓰는지 확인하는 데 쓴다.

/// 한 배치가 차지하는 요일 열과 교시 범위 (0부터 센다)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub day: i32,
    pub start: i32,
    pub span: i32,
}

impl Slot {
    pub fn overlaps(&self, other: &Slot) -> bool {
        self.day == other.day
            && self.start < other.start + other.span
            && other.start < self.start + self.span
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub days: i32,
    pub periods: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    /// 없는 배치이거나 격자 밖
    OutOfRange,
    /// 잠긴 배치는 옮기지 않는다 (UI-STATE-04)
    Locked,
    /// 다른 배치와 겹친다
    Overlaps(usize),
}

/// `index` 배치를 (`day`, `start`)로 옮길 수 있는지 검사하고, 옮긴 자리를 돌려준다.
/// `placements`는 (자리, 잠금 여부) 목록이다.
pub fn check_move(
    placements: &[(Slot, bool)],
    index: usize,
    day: i32,
    start: i32,
    grid: GridSize,
) -> Result<Slot, MoveError> {
    let Some(&(slot, locked)) = placements.get(index) else {
        return Err(MoveError::OutOfRange);
    };
    if locked {
        return Err(MoveError::Locked);
    }
    let target = Slot {
        day,
        start,
        span: slot.span,
    };
    if day < 0 || day >= grid.days || start < 0 || start + slot.span > grid.periods {
        return Err(MoveError::OutOfRange);
    }
    match placements
        .iter()
        .enumerate()
        .find(|(i, (other, _))| *i != index && target.overlaps(other))
    {
        Some((i, _)) => Err(MoveError::Overlaps(i)),
        None => Ok(target),
    }
}

/// 서로 겹치는 배치 쌍의 수
pub fn conflict_count(slots: &[Slot]) -> usize {
    let mut count = 0;
    for (i, a) in slots.iter().enumerate() {
        count += slots[i + 1..].iter().filter(|b| a.overlaps(b)).count();
    }
    count
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub index: usize,
    pub from: Slot,
    pub to: Slot,
}

/// 초안 편집의 실행 취소·다시 실행 기록
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Move>,
    redo: Vec<Move>,
}

impl History {
    pub fn record(&mut self, m: Move) {
        self.undo.push(m);
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn pop_undo(&mut self) -> Option<Move> {
        self.undo.pop()
    }
    pub fn pop_redo(&mut self) -> Option<Move> {
        self.redo.pop()
    }
    pub fn push_undo(&mut self, m: Move) {
        self.undo.push(m);
    }
    pub fn push_redo(&mut self, m: Move) {
        self.redo.push(m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: GridSize = GridSize {
        days: 5,
        periods: 8,
    };

    fn s(day: i32, start: i32, span: i32) -> Slot {
        Slot { day, start, span }
    }

    #[test]
    fn overlap_rules() {
        assert!(s(0, 0, 2).overlaps(&s(0, 1, 1)));
        assert!(
            !s(0, 0, 2).overlaps(&s(0, 2, 1)),
            "끝 교시는 포함하지 않는다"
        );
        assert!(!s(0, 0, 2).overlaps(&s(1, 0, 2)), "다른 요일");
    }

    #[test]
    fn move_checks_range_lock_and_overlap() {
        let p = [(s(0, 0, 2), false), (s(0, 3, 1), false), (s(1, 0, 1), true)];
        assert_eq!(check_move(&p, 0, 0, 1, GRID), Ok(s(0, 1, 2)));
        assert_eq!(check_move(&p, 0, 0, 2, GRID), Err(MoveError::Overlaps(1)));
        assert_eq!(check_move(&p, 0, 0, 7, GRID), Err(MoveError::OutOfRange));
        assert_eq!(check_move(&p, 0, 5, 0, GRID), Err(MoveError::OutOfRange));
        assert_eq!(check_move(&p, 0, -1, 0, GRID), Err(MoveError::OutOfRange));
        assert_eq!(check_move(&p, 2, 2, 0, GRID), Err(MoveError::Locked));
        assert_eq!(check_move(&p, 9, 0, 0, GRID), Err(MoveError::OutOfRange));
    }

    #[test]
    fn counts_conflicting_pairs() {
        assert_eq!(conflict_count(&[s(0, 0, 2), s(0, 1, 1), s(1, 0, 1)]), 1);
        assert_eq!(conflict_count(&[s(0, 0, 1), s(0, 1, 1)]), 0);
    }

    #[test]
    fn history_clears_redo_on_new_move() {
        let m = Move {
            index: 0,
            from: s(0, 0, 1),
            to: s(0, 1, 1),
        };
        let mut h = History::default();
        h.record(m);
        let undone = h.pop_undo().unwrap();
        h.push_redo(undone);
        assert!(h.can_redo());
        h.record(m);
        assert!(!h.can_redo());
    }
}
