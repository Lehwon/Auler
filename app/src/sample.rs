//! 화면 검증용 가상 데이터. 실제 학과 자료가 아니다.
//!
//! 과목명, 교원명, 학과명, 강의실, 조건 이름은 원본 자료로 보고 번역하지 않는다
//! (명세서 7.3). 상태, 단위, 사유처럼 앱이 만드는 문구는 Slint의 `Labels`에서
//! 번역한다.

use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};

use crate::{AltData, CardData, PeriodRow, UnassignedData};

pub const DEPARTMENT: &str = "컴퓨터공학과";
pub const YEAR: i32 = 2027;
pub const TERM: i32 = 1;

/// 학기에 열린 요일 (0=월 … 6=일)
pub fn weekdays() -> Vec<i32> {
    (0..5).collect()
}

/// 기본 교시표 예시: 09:00부터 1시간 간격 8교시 (그림 6). 실제로는 학기 설정에서 온다.
pub fn periods() -> Vec<PeriodRow> {
    (0..8)
        .map(|i| PeriodRow {
            label: SharedString::from(format!("{}", i + 1)),
            time: SharedString::from(format!("{:02}:00", 9 + i)),
        })
        .collect()
}

/// (과목, 교원 · 강의실, 요일 열, 시작 교시, 교시 수, 색, 잠금, 인원, 좌석)
type CardRow = (
    &'static str,
    &'static str,
    i32,
    i32,
    i32,
    i32,
    bool,
    i32,
    i32,
);

/// 그림 6의 배치를 바탕으로 한 카드 12개. (요일 열, 시작 교시, 교시 수)는 0부터 센다.
pub fn cards() -> Vec<CardData> {
    #[rustfmt::skip]
    let raw: [CardRow; 12] = [
        ("자료구조 01", "김교원 · 공학관 302", 0, 0, 1, 0, true, 28, 32),
        ("프로그래밍 01", "박교원 · 공학관 201", 2, 0, 2, 1, false, 32, 40),
        ("컴퓨터개론 01", "최교원 · 본관 201", 1, 2, 2, 2, false, 35, 35),
        ("자료구조 01", "김교원 · 공학관 302", 3, 2, 2, 0, false, 28, 32),
        ("프로그래밍 01", "박교원 · 공학관 201", 4, 4, 1, 1, false, 32, 40),
        ("전공수학 01", "강교원 · 공학관 301", 0, 5, 2, 3, false, 30, 30),
        ("전공영어 01", "정교원 · 본관 202", 1, 5, 1, 3, false, 24, 30),
        ("기초실습 02", "이교원 · 공학관 305", 2, 6, 1, 2, false, 26, 36),
        ("이산수학 01", "윤교원 · 본관 203", 1, 0, 2, 1, true, 33, 40),
        ("데이터베이스 01", "윤교원 · 공학관 303", 3, 5, 2, 0, false, 24, 30),
        ("운영체제 02", "정교원 · 공학관 304", 4, 0, 2, 2, false, 26, 30),
        ("캡스톤디자인 01", "강교원 · 공학관 401", 4, 6, 2, 3, true, 18, 24),
    ];
    raw.iter()
        .map(
            |&(title, detail, day, start, span, hue, locked, students, capacity)| CardData {
                title: title.into(),
                detail: detail.into(),
                day,
                start,
                span,
                hue,
                locked,
                students,
                capacity,
            },
        )
        .collect()
}

/// 미배정 분반. reason: 0 = 장비 후보 부족, 1 = 교원 시간 충돌
pub fn unassigned() -> Vec<UnassignedData> {
    vec![
        UnassignedData {
            title: "자료구조 02".into(),
            code: "CS201".into(),
            students: 30,
            remaining: 1,
            reason: 0,
        },
        UnassignedData {
            title: "운영체제 01".into(),
            code: "CS301".into(),
            students: 26,
            remaining: 2,
            reason: 1,
        },
    ]
}

/// 대안 배치 예시 (그림 6). verdict: 0 = 가능, 1 = 선호 경고, 2 = 불가.
/// 실제 후보는 M1 편성 엔진이 계산한다.
pub fn alternatives() -> Vec<AltData> {
    vec![
        AltData {
            day: 1,
            start: 0,
            span: 2,
            room: "공학관 302".into(),
            verdict: 0,
            affected: 0,
            constraint: SharedString::new(),
            shortage: 0,
        },
        AltData {
            day: 2,
            start: 4,
            span: 2,
            room: "공학관 302".into(),
            verdict: 1,
            affected: 0,
            constraint: "수요일 오후 회피".into(),
            shortage: 0,
        },
        AltData {
            day: 3,
            start: 4,
            span: 2,
            room: "공학관 301".into(),
            verdict: 2,
            affected: 0,
            constraint: SharedString::new(),
            shortage: 2,
        },
    ]
}

/// 큰 표 확인용 분반 행. `status`는 화면 언어로 번역된 [배정 완료, 미배정, 확인 필요].
pub fn section_rows(
    count: usize,
    status: &[SharedString; 3],
) -> ModelRc<ModelRc<StandardListViewItem>> {
    const COURSES: [&str; 8] = [
        "자료구조",
        "프로그래밍",
        "컴퓨터개론",
        "운영체제",
        "데이터베이스",
        "캡스톤디자인",
        "이산수학",
        "컴퓨터구조",
    ];
    const INSTRUCTORS: [&str; 7] = [
        "김교원",
        "이교원",
        "박교원",
        "최교원",
        "정교원",
        "윤교원",
        "강교원",
    ];

    let rows: Vec<ModelRc<StandardListViewItem>> = (0..count)
        .map(|i| {
            let cells: [SharedString; 8] = [
                format!("CS{}", 101 + (i / 3) % 400).into(),
                section_code(i % 3 + 1),
                COURSES[i % COURSES.len()].into(),
                INSTRUCTORS[i % INSTRUCTORS.len()].into(),
                format!("{}", i % 4 + 1).into(),
                format!("{}", 18 + i % 20).into(),
                if i % 2 == 0 { "3 (2+1)" } else { "2" }.into(),
                status[i % status.len()].clone(),
            ];
            let items: Vec<StandardListViewItem> = cells
                .iter()
                .map(|c| StandardListViewItem::from(c.as_str()))
                .collect();
            ModelRc::new(VecModel::from(items))
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

/// 분반 코드는 숫자가 아니라 문자열이다. 앞자리 0을 보존한다 (계획 D5).
pub fn section_code(n: usize) -> SharedString {
    format!("{n:02}").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Slot, conflict_count};

    #[test]
    fn sample_cards_do_not_overlap_and_fit_the_grid() {
        let cards = cards();
        let slots: Vec<Slot> = cards
            .iter()
            .map(|c| Slot {
                day: c.day,
                start: c.start,
                span: c.span,
            })
            .collect();
        assert_eq!(conflict_count(&slots), 0);
        let (days, periods) = (weekdays().len() as i32, periods().len() as i32);
        for c in &cards {
            assert!(c.day >= 0 && c.day < days, "{}", c.title);
            assert!(c.start >= 0 && c.start + c.span <= periods, "{}", c.title);
            assert!(c.span >= 1);
        }
    }

    #[test]
    fn section_codes_keep_leading_zero() {
        assert_eq!(section_code(1), "01");
        assert_eq!(section_code(12), "12");
    }
}
