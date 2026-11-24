//! 스트레스 데이터: 분반 150개. 검증·편성 시간을 잴 때 쓴다.
//!
//! 고정한 시드로 만들어 언제나 같은 문서가 나온다. 모든 분반을 배치할 수 있다는 보장은
//! 없다(일부러 빡빡하게 만든다). 과목명과 교원명은 조합해 만든 가상 이름이다.

use auler_domain::input::*;

use crate::reference::{
    avoid, clock_window, confirmed, constraint, equipment, instructor_ref, instructor_target,
    periods_window, setup,
};

pub const SECTION_COUNT: usize = 150;
pub const INSTRUCTOR_COUNT: usize = 70;
pub const SEED: u64 = 0x4155_4C45_5232_3031;

/// SplitMix64: 의존성 없이 재현 가능한 난수
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `0..n`
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.below((hi - lo + 1) as usize) as u32
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const SURNAMES: [&str; 20] = [
    "김", "이", "박", "최", "정", "강", "조", "윤", "장", "임", "한", "오", "서", "신", "권", "황",
    "안", "송", "전", "홍",
];
const GIVEN: [&str; 24] = [
    "가온", "나래", "다솜", "라온", "마루", "바다", "새봄", "아라", "자운", "차미", "하늘", "구름",
    "누리", "도담", "로운", "미르", "보람", "소리", "온새", "이든", "주원", "채운", "하람", "한결",
];
const SUBJECTS: [&str; 20] = [
    "프로그래밍",
    "자료처리",
    "시스템",
    "네트워크",
    "보안",
    "데이터",
    "지능",
    "그래픽스",
    "설계",
    "수치해석",
    "신호처리",
    "임베디드",
    "웹",
    "모바일",
    "클라우드",
    "로봇",
    "언어",
    "컴퓨팅",
    "최적화",
    "정보이론",
];
const LEVELS: [&str; 4] = ["기초", "개론", "응용", "특강"];
const DAYS: [&str; 5] = ["mon", "tue", "wed", "thu", "fri"];

/// (코드, 학년)
const GROUPS: [(&str, u8); 10] = [
    ("Y1A", 1),
    ("Y1B", 1),
    ("Y1C", 1),
    ("Y2A", 2),
    ("Y2B", 2),
    ("Y2C", 2),
    ("Y3A", 3),
    ("Y3B", 3),
    ("Y4A", 4),
    ("Y4B", 4),
];

fn rooms(rng: &mut Rng) -> Vec<RoomInput> {
    let mut out = Vec::new();
    let mut add = |building: &str,
                   number: String,
                   kind: &str,
                   seats: u32,
                   equipment: Vec<RoomEquipmentInput>| {
        out.push(RoomInput {
            building: building.into(),
            number: number.into(),
            kind: kind.into(),
            department: Some("공과대학 행정실".into()),
            total_seats: seats,
            available_seats: None,
            equipment,
            shared: false,
            schedule_checked: None,
            info_source: None,
            source: None,
        });
    };
    let beam = || RoomEquipmentInput {
        code: "BEAM".into(),
        owned: 1,
        available: None,
        inspection_from: None,
        inspection_until: None,
    };
    for i in 0..18 {
        let seats = [40, 50, 60, 60, 80, 120][i % 6];
        let building = ["제1공학관", "제2공학관", "과학관"][i % 3];
        // 건물마다 층별로 한 곳씩: 101, 201, …, 601. 실습실은 7층, 세미나실은 8층
        add(
            building,
            format!("{}01", 1 + i / 3),
            "lecture",
            seats,
            vec![beam()],
        );
    }
    for i in 0..8 {
        let seats = rng.range(30, 40);
        let working = seats - rng.range(0, 3);
        let pc = RoomEquipmentInput {
            code: "PC".into(),
            owned: seats,
            available: (working != seats).then_some(working),
            inspection_from: None,
            inspection_until: None,
        };
        add(
            "제2공학관",
            format!("7{:02}", i + 1),
            "lab",
            seats,
            vec![pc, beam()],
        );
    }
    for i in 0..2 {
        let kit = RoomEquipmentInput {
            code: "KIT".into(),
            owned: 30,
            available: None,
            inspection_from: None,
            inspection_until: None,
        };
        add(
            "과학관",
            format!("B1{:02}", i + 1),
            "lab",
            30,
            vec![kit, beam()],
        );
    }
    for i in 0..2 {
        add(
            "제1공학관",
            format!("8{:02}", i + 1),
            "seminar",
            25,
            vec![beam()],
        );
    }
    out
}

/// 분반 150개짜리 문서
pub fn stress() -> InputDocument {
    let mut rng = Rng(SEED);
    let mut doc = InputDocument::new(2027, "2");
    let mut s = setup();
    s.start = Some("2027-09-01".into());
    s.end = Some("2027-12-14".into());
    doc.setup = Some(s);
    doc.equipment = equipment();
    doc.rooms = rooms(&mut rng);
    doc.instructors = (0..INSTRUCTOR_COUNT)
        .map(|i| InstructorInput {
            code: format!("S{:03}", i + 1).into(),
            name: format!("{}{}", rng.pick(&SURNAMES), rng.pick(&GIVEN)),
            department: Some("컴퓨터학부".into()),
            source: None,
        })
        .collect();
    doc.groups = GROUPS
        .iter()
        .map(|&(code, grade)| GroupInput {
            code: code.into(),
            name: format!("{grade}학년 {}반", &code[2..]),
            grade: Some(grade),
            track: None,
            source: None,
        })
        .collect();

    let mut load = vec![0u32; INSTRUCTOR_COUNT];
    let mut course_no = 0;
    while doc.sections.len() < SECTION_COUNT {
        course_no += 1;
        let year = 1 + (course_no % 4) as u8;
        let code = format!("CS{year}{course_no:03}");
        let name = format!("{}{}", rng.pick(&SUBJECTS), rng.pick(&LEVELS));
        doc.courses.push(CourseInput {
            code: code.as_str().into(),
            name,
            source: None,
        });
        let lab = rng.chance(25);
        let kit = !lab && rng.chance(5);
        let required = year <= 2 && rng.chance(50);
        let split: Vec<u8> = rng
            .pick(&[&[2u8][..], &[2, 1], &[3], &[3], &[2, 2]])
            .to_vec();
        let groups: Vec<&str> = GROUPS.iter().filter(|g| g.1 == year).map(|g| g.0).collect();
        let count = (rng.range(1, 4) as usize).min(SECTION_COUNT - doc.sections.len());
        for k in 0..count {
            // 교원은 담당이 가장 적은 사람들 중에서 고른다 (최대 3분반)
            let min = *load.iter().min().expect("교원이 있다");
            let pool: Vec<usize> = (0..INSTRUCTOR_COUNT).filter(|&i| load[i] == min).collect();
            let teacher = *rng.pick(&pool);
            load[teacher] += 1;
            let group = groups[k % groups.len()];
            let enrollment = if lab || kit {
                rng.range(20, 30)
            } else {
                rng.range(25, 110)
            };
            let equipment = if lab {
                vec![EquipmentNeedInput {
                    code: "PC".into(),
                    quantity: 1,
                }]
            } else if kit {
                vec![EquipmentNeedInput {
                    code: "KIT".into(),
                    quantity: 1,
                }]
            } else {
                Vec::new()
            };
            doc.sections.push(SectionInput {
                course: code.as_str().into(),
                section: format!("{:02}", k + 1).into(),
                instructors: vec![instructor_ref(&format!("S{:03}", teacher + 1))],
                groups: vec![GroupLinkInput {
                    group: group.into(),
                    required,
                }],
                enrollment,
                provisional: rng.chance(10),
                kind: if lab || kit { "lab" } else { "lecture" }.into(),
                room_kind: Some(if lab || kit { "lab" } else { "lecture" }.into()),
                weekly_periods: split.iter().sum(),
                split: (split.len() > 1).then(|| split.clone()),
                equipment,
                source: None,
            });
        }
    }

    for i in 0..80 {
        let teacher = format!("S{:03}", rng.below(INSTRUCTOR_COUNT) + 1);
        let day = *rng.pick(&DAYS);
        let first = rng.range(1, 7);
        let last = first + rng.range(0, 2);
        let key = format!("X{:03}", i + 1);
        let c = match i % 8 {
            0..=3 => confirmed(
                constraint(&key, instructor_target(&teacher), "불가 시간"),
                "hard",
                avoid(vec![periods_window(
                    day,
                    &first.to_string(),
                    &last.to_string(),
                )]),
            ),
            4..=6 => confirmed(
                constraint(&key, instructor_target(&teacher), "가능하면 피할 시간"),
                "soft",
                avoid(vec![clock_window(
                    day,
                    &format!("{:02}:00", 8 + first),
                    &format!("{:02}:00", 9 + last),
                )]),
            ),
            _ => constraint(&key, instructor_target(&teacher), "확인이 필요한 요청"),
        };
        doc.constraints.push(c);
    }
    doc
}
