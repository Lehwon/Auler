//! 화면 검증용 가상 데이터. 실제 학과 자료가 아니다.

use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};

use crate::{CardData, PeriodRow};

pub fn days(lang: &str) -> [&'static str; 5] {
    match lang {
        "ko" => ["월", "화", "수", "목", "금"],
        _ => ["Mon", "Tue", "Wed", "Thu", "Fri"],
    }
}

pub fn periods() -> Vec<PeriodRow> {
    (0..8)
        .map(|i| PeriodRow {
            label: SharedString::from(format!("{}", i + 1)),
            time: SharedString::from(format!("{:02}:00", 9 + i)),
        })
        .collect()
}

/// 그림 6의 배치를 바탕으로 한 카드 12개. (요일, 시작 교시, 교시 수)는 0부터 센다.
pub fn cards() -> Vec<CardData> {
    let raw: [(&str, &str, i32, i32, i32, i32, bool); 12] = [
        ("자료구조 01", "김교원 · 공학관 302", 0, 0, 1, 0, true),
        ("프로그래밍 01", "박교원 · 공학관 201", 2, 0, 2, 1, false),
        ("컴퓨터개론 01", "최교원 · 본관 201", 1, 2, 2, 2, false),
        ("자료구조 01", "김교원 · 공학관 302", 3, 2, 2, 0, false),
        ("프로그래밍 01", "박교원 · 공학관 201", 4, 4, 1, 1, false),
        ("전공수학 01", "강교원 · 공학관 301", 0, 5, 2, 3, false),
        ("전공영어 01", "정교원 · 본관 202", 1, 5, 1, 3, false),
        ("기초실습 02", "이교원 · 공학관 305", 2, 6, 1, 2, false),
        ("이산수학 01", "윤교원 · 본관 203", 1, 0, 2, 1, true),
        ("데이터베이스 01", "윤교원 · 공학관 303", 3, 5, 2, 0, false),
        ("운영체제 02", "정교원 · 공학관 304", 4, 0, 2, 2, false),
        ("캡스톤디자인 01", "강교원 · 공학관 401", 4, 6, 2, 3, true),
    ];
    raw.iter()
        .map(|&(title, detail, day, start, span, hue, locked)| CardData {
            title: title.into(),
            detail: detail.into(),
            day,
            start,
            span,
            hue,
            locked,
        })
        .collect()
}

/// 큰 표 확인용 분반 행
pub fn section_rows(count: usize) -> ModelRc<ModelRc<StandardListViewItem>> {
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
    const STATUS: [&str; 3] = ["배정 완료", "미배정", "확인 필요"];

    let rows: Vec<ModelRc<StandardListViewItem>> = (0..count)
        .map(|i| {
            let cells = [
                format!("CS{}", 101 + (i / 3) % 400),
                format!("{:02}", i % 3 + 1), // 분반 코드는 문자열로 앞자리 0을 보존한다
                COURSES[i % COURSES.len()].to_string(),
                INSTRUCTORS[i % INSTRUCTORS.len()].to_string(),
                format!("{}", i % 4 + 1),
                format!("{}", 18 + i % 20),
                if i % 2 == 0 {
                    "3 (2+1)".to_string()
                } else {
                    "2".to_string()
                },
                STATUS[i % STATUS.len()].to_string(),
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
