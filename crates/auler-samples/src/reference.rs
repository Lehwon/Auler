//! 기준 학과: 가상의 컴퓨터공학과 2027년 1학기.
//!
//! 분반 42개, 교원 28명, 강의실 12개. 실제 학교나 사람과 관계없는 가상 자료다.
//! 다음 사례를 일부러 넣었다.
//!
//! - 자료구조 01분반: 28명, 1인당 실습용 PC 1대. 담당 교원의 요청 세 건
//!   (수요일 13~16시 선호 회피, 목요일 1교시 불가, 강도·범위를 모르는 "오전 수업 어려움")
//! - 목요일 14:00~14:50 실습실 후보: 공학관 301호 정상 PC 26대(부족), 302호 30대(가능),
//!   305호 다른 학과 사용(불가), 본관 201호 공용실 일정 미확인(외부 확인 필요)
//! - 이름이 같은 교원 두 명(김민수: 컴퓨터공학과, 수학과)
//! - 건물만 다르고 호실이 같은 강의실(201호, 305호)
//! - 정상 수량이 0인 장비(공학관 403호 빔프로젝터), 실습 키트가 있는 강의실은 한 곳
//! - 인원 잠정 분반, 교원 두 명이 함께 맡는 분반, 금요일에만 나오는 겸임교원
//! - 기간이 정해진 조건(4월 한 달), 수강 집단 대상 선호 조건, 미리 잠근 배치 두 개

use auler_domain::input::*;

pub const DEPARTMENT: &str = "컴퓨터공학과";
pub const YEAR: u16 = 2027;
pub const SEASON: &str = "1";

/// (코드, 이름, 소속)
const INSTRUCTORS: [(&str, &str, &str); 28] = [
    ("P01", "김민수", "컴퓨터공학과"),
    ("P02", "이서연", "컴퓨터공학과"),
    ("P03", "박지훈", "컴퓨터공학과"),
    ("P04", "최유진", "컴퓨터공학과"),
    ("P05", "정현우", "컴퓨터공학과"),
    ("P06", "강수빈", "컴퓨터공학과"),
    ("P07", "조민재", "컴퓨터공학과"),
    ("P08", "윤하은", "컴퓨터공학과"),
    ("P09", "장도윤", "컴퓨터공학과"),
    ("P10", "임서준", "컴퓨터공학과"),
    ("P11", "한지민", "컴퓨터공학과"),
    ("P12", "오승현", "컴퓨터공학과"),
    ("P13", "서예린", "컴퓨터공학과"),
    ("P14", "신동혁", "컴퓨터공학과"),
    ("P15", "권나연", "컴퓨터공학과"),
    ("P16", "황재원", "컴퓨터공학과"),
    ("P17", "안소희", "컴퓨터공학과"),
    ("P18", "송태민", "컴퓨터공학과"),
    ("P19", "전하린", "컴퓨터공학과"),
    ("P20", "홍성민", "수학과"),
    ("P21", "김민수", "수학과"),
    ("P22", "유채원", "외부 겸임"),
    ("P23", "고준영", "컴퓨터공학과"),
    ("P24", "문다인", "컴퓨터공학과"),
    ("P25", "양시우", "컴퓨터공학과"),
    ("P26", "배지호", "컴퓨터공학과"),
    ("P27", "백수아", "교양학부"),
    ("P28", "허건우", "컴퓨터공학과"),
];

/// (코드, 이름, 학년)
const GROUPS: [(&str, &str, u8); 6] = [
    ("Y1A", "1학년 A반", 1),
    ("Y1B", "1학년 B반", 1),
    ("Y2A", "2학년 A반", 2),
    ("Y2B", "2학년 B반", 2),
    ("Y3", "3학년", 3),
    ("Y4", "4학년", 4),
];

const COURSES: [(&str, &str); 27] = [
    ("CSE1001", "컴퓨터공학개론"),
    ("CSE1002", "프로그래밍기초"),
    ("MTH1001", "미적분학"),
    ("CSE1003", "이산수학"),
    ("CSE2010", "자료구조"),
    ("CSE2020", "객체지향프로그래밍"),
    ("CSE2030", "컴퓨터구조"),
    ("CSE2040", "디지털논리회로"),
    ("CSE2050", "확률과통계"),
    ("ENG2001", "공학영어"),
    ("CSE3010", "운영체제"),
    ("CSE3020", "알고리즘"),
    ("CSE3030", "데이터베이스"),
    ("CSE3040", "컴퓨터네트워크"),
    ("CSE3050", "소프트웨어공학"),
    ("CSE3060", "인공지능"),
    ("CSE3070", "웹프로그래밍"),
    ("CSE3080", "컴파일러"),
    ("GEN3001", "공학윤리"),
    ("CSE4010", "캡스톤디자인"),
    ("CSE4020", "정보보호"),
    ("CSE4030", "기계학습"),
    ("CSE4040", "클라우드컴퓨팅"),
    ("CSE4050", "졸업세미나"),
    ("CSE4060", "모바일프로그래밍"),
    ("CSE4070", "컴퓨터그래픽스"),
    ("CSE4080", "분산시스템"),
];

/// 분반 한 줄
pub(crate) struct Sec {
    pub course: &'static str,
    pub section: &'static str,
    pub instructors: &'static [&'static str],
    /// (수강 집단, 필수과목 여부)
    pub groups: &'static [(&'static str, bool)],
    pub enrollment: u32,
    pub provisional: bool,
    pub kind: &'static str,
    pub room_kind: Option<&'static str>,
    pub split: &'static [u8],
    /// (장비 코드, 수량)
    pub equipment: &'static [(&'static str, u32)],
}

// 표를 한 줄에 한 분반씩 읽기 쉽게 인자를 풀어 둔다.
#[allow(clippy::too_many_arguments)]
const fn sec(
    course: &'static str,
    section: &'static str,
    instructors: &'static [&'static str],
    groups: &'static [(&'static str, bool)],
    enrollment: u32,
    kind: &'static str,
    split: &'static [u8],
    equipment: &'static [(&'static str, u32)],
) -> Sec {
    let room_kind = match kind.as_bytes() {
        b"lecture" => Some("lecture"),
        _ => Some("lab"),
    };
    Sec {
        course,
        section,
        instructors,
        groups,
        enrollment,
        provisional: false,
        kind,
        room_kind,
        split,
        equipment,
    }
}

const fn provisional(mut s: Sec) -> Sec {
    s.provisional = true;
    s
}

const fn room(mut s: Sec, room_kind: &'static str) -> Sec {
    s.room_kind = Some(room_kind);
    s
}

const PC: &[(&str, u32)] = &[("PC", 1)];
const KIT: &[(&str, u32)] = &[("KIT", 1)];
const BEAM: &[(&str, u32)] = &[("BEAM", 1)];
const NONE: &[(&str, u32)] = &[];

pub(crate) const SECTIONS: [Sec; 42] = [
    // 1학년: A반·B반이 같은 필수과목을 분반으로 나눠 듣는다
    sec(
        "CSE1001",
        "01",
        &["P01"],
        &[("Y1A", true)],
        55,
        "lecture",
        &[2],
        BEAM,
    ),
    sec(
        "CSE1001",
        "02",
        &["P02"],
        &[("Y1B", true)],
        55,
        "lecture",
        &[2],
        BEAM,
    ),
    sec(
        "CSE1002",
        "01",
        &["P04"],
        &[("Y1A", true)],
        30,
        "lab",
        &[2, 1],
        PC,
    ),
    sec(
        "CSE1002",
        "02",
        &["P05"],
        &[("Y1B", true)],
        30,
        "lab",
        &[2, 1],
        PC,
    ),
    sec(
        "MTH1001",
        "01",
        &["P21"],
        &[("Y1A", true)],
        55,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "MTH1001",
        "02",
        &["P20"],
        &[("Y1B", true)],
        55,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE1003",
        "01",
        &["P06"],
        &[("Y1A", true)],
        55,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE1003",
        "02",
        &["P07"],
        &[("Y1B", true)],
        55,
        "lecture",
        &[2, 1],
        NONE,
    ),
    // 2학년
    sec(
        "CSE2010",
        "01",
        &["P03"],
        &[("Y2A", true)],
        28,
        "lab",
        &[2, 1],
        PC,
    ),
    sec(
        "CSE2010",
        "02",
        &["P08"],
        &[("Y2B", true)],
        30,
        "lab",
        &[2, 1],
        PC,
    ),
    provisional(sec(
        "CSE2010",
        "03",
        &["P09"],
        &[],
        20,
        "lecture",
        &[2, 1],
        NONE,
    )),
    sec(
        "CSE2020",
        "01",
        &["P09"],
        &[("Y2A", true)],
        30,
        "lab",
        &[2, 1],
        PC,
    ),
    sec(
        "CSE2020",
        "02",
        &["P10"],
        &[("Y2B", true)],
        30,
        "lab",
        &[2, 1],
        PC,
    ),
    sec(
        "CSE2030",
        "01",
        &["P11"],
        &[("Y2A", true)],
        50,
        "lecture",
        &[3],
        NONE,
    ),
    sec(
        "CSE2030",
        "02",
        &["P11"],
        &[("Y2B", true)],
        50,
        "lecture",
        &[3],
        NONE,
    ),
    sec(
        "CSE2040",
        "01",
        &["P12"],
        &[("Y2A", false)],
        24,
        "lab",
        &[3],
        KIT,
    ),
    sec(
        "CSE2040",
        "02",
        &["P12"],
        &[("Y2B", false)],
        24,
        "lab",
        &[3],
        KIT,
    ),
    sec(
        "CSE2050",
        "01",
        &["P20"],
        &[("Y2A", false), ("Y2B", false)],
        80,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "ENG2001",
        "01",
        &["P22"],
        &[("Y2A", false)],
        30,
        "lecture",
        &[2],
        NONE,
    ),
    sec(
        "ENG2001",
        "02",
        &["P22"],
        &[("Y2B", false)],
        30,
        "lecture",
        &[2],
        NONE,
    ),
    // 3학년
    sec(
        "CSE3010",
        "01",
        &["P13"],
        &[("Y3", true)],
        45,
        "lecture",
        &[2, 1],
        NONE,
    ),
    provisional(sec(
        "CSE3010",
        "02",
        &["P14"],
        &[],
        30,
        "lecture",
        &[2, 1],
        NONE,
    )),
    sec(
        "CSE3020",
        "01",
        &["P15"],
        &[("Y3", true)],
        45,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE3030",
        "01",
        &["P16"],
        &[("Y3", false)],
        40,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE3030",
        "02",
        &["P16"],
        &[("Y3", false)],
        30,
        "lab",
        &[3],
        PC,
    ),
    sec(
        "CSE3040",
        "01",
        &["P17"],
        &[("Y3", false)],
        40,
        "lecture",
        &[2, 1],
        BEAM,
    ),
    sec(
        "CSE3050",
        "01",
        &["P18"],
        &[("Y3", true)],
        45,
        "lecture",
        &[3],
        NONE,
    ),
    sec(
        "CSE3060",
        "01",
        &["P19"],
        &[("Y3", false), ("Y4", false)],
        40,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE3060",
        "02",
        &["P19"],
        &[("Y4", false)],
        30,
        "lab",
        &[3],
        PC,
    ),
    sec(
        "CSE3070",
        "01",
        &["P23"],
        &[("Y3", false)],
        30,
        "lab",
        &[3],
        PC,
    ),
    sec(
        "CSE3080",
        "01",
        &["P24"],
        &[("Y3", false)],
        35,
        "lecture",
        &[2, 1],
        NONE,
    ),
    provisional(sec(
        "GEN3001",
        "01",
        &["P27"],
        &[("Y3", false), ("Y4", false)],
        85,
        "lecture",
        &[2],
        NONE,
    )),
    // 4학년
    sec(
        "CSE4010",
        "01",
        &["P01", "P05"],
        &[("Y4", false)],
        20,
        "mixed",
        &[2, 2],
        PC,
    ),
    sec(
        "CSE4010",
        "02",
        &["P25"],
        &[("Y4", false)],
        20,
        "mixed",
        &[2, 2],
        PC,
    ),
    room(
        sec(
            "CSE4010",
            "03",
            &["P26"],
            &[("Y4", false)],
            20,
            "mixed",
            &[2, 2],
            NONE,
        ),
        "seminar",
    ),
    sec(
        "CSE4020",
        "01",
        &["P28"],
        &[("Y4", false)],
        40,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE4030",
        "01",
        &["P02"],
        &[("Y4", false)],
        40,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE4040",
        "01",
        &["P23"],
        &[("Y4", false)],
        30,
        "lab",
        &[3],
        PC,
    ),
    sec(
        "CSE4050",
        "01",
        &["P18"],
        &[("Y4", true)],
        60,
        "lecture",
        &[1],
        NONE,
    ),
    sec(
        "CSE4060",
        "01",
        &["P26"],
        &[("Y4", false)],
        30,
        "lab",
        &[3],
        PC,
    ),
    sec(
        "CSE4070",
        "01",
        &["P28"],
        &[("Y4", false)],
        35,
        "lecture",
        &[2, 1],
        NONE,
    ),
    sec(
        "CSE4080",
        "01",
        &["P14"],
        &[("Y4", false)],
        35,
        "lecture",
        &[2, 1],
        NONE,
    ),
];

/// 강의실 한 줄
pub(crate) struct RoomRow {
    pub building: &'static str,
    pub number: &'static str,
    pub kind: &'static str,
    pub department: &'static str,
    pub seats: u32,
    /// (장비 코드, 보유, 정상)
    pub equipment: &'static [(&'static str, u32, u32)],
    pub shared: bool,
    pub schedule_checked: Option<&'static str>,
}

const ENG_OFFICE: &str = "공과대학 행정실";
const ACADEMIC: &str = "교무처 학사지원팀";

pub(crate) const ROOMS: [RoomRow; 12] = [
    RoomRow {
        building: "공학관",
        number: "301",
        kind: "lab",
        department: ENG_OFFICE,
        seats: 30,
        equipment: &[("PC", 30, 26), ("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "302",
        kind: "lab",
        department: ENG_OFFICE,
        seats: 32,
        equipment: &[("PC", 32, 30), ("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "305",
        kind: "lab",
        department: ENG_OFFICE,
        seats: 36,
        equipment: &[("PC", 36, 36), ("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "본관",
        number: "201",
        kind: "lab",
        department: ACADEMIC,
        seats: 35,
        equipment: &[("PC", 32, 32), ("BEAM", 1, 1)],
        shared: true,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "201",
        kind: "lecture",
        department: ENG_OFFICE,
        seats: 60,
        equipment: &[("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "202",
        kind: "lecture",
        department: ENG_OFFICE,
        seats: 60,
        equipment: &[("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "203",
        kind: "lecture",
        department: ENG_OFFICE,
        seats: 45,
        equipment: &[("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "401",
        kind: "lecture",
        department: ENG_OFFICE,
        seats: 90,
        equipment: &[("BEAM", 2, 2)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "402",
        kind: "seminar",
        department: ENG_OFFICE,
        seats: 24,
        equipment: &[("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "403",
        kind: "lecture",
        department: ENG_OFFICE,
        seats: 40,
        equipment: &[("BEAM", 1, 0)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "공학관",
        number: "B101",
        kind: "lab",
        department: ENG_OFFICE,
        seats: 24,
        equipment: &[("KIT", 24, 24), ("BEAM", 1, 1)],
        shared: false,
        schedule_checked: None,
    },
    RoomRow {
        building: "본관",
        number: "305",
        kind: "lecture",
        department: ACADEMIC,
        seats: 50,
        equipment: &[("BEAM", 1, 1)],
        shared: true,
        schedule_checked: Some("2027-02-20"),
    },
];

pub(crate) fn instructor_ref(code: &str) -> InstructorRefInput {
    InstructorRefInput {
        code: Some(code.into()),
        name: None,
    }
}

pub(crate) fn room_ref(building: &str, number: &str) -> RoomRefInput {
    RoomRefInput {
        building: Some(building.into()),
        number: Some(number.into()),
        text: None,
    }
}

pub(crate) fn periods_window(day: &str, first: &str, last: &str) -> WindowInput {
    WindowInput {
        day: day.into(),
        first_period: Some(first.into()),
        last_period: (first != last).then(|| last.into()),
        start: None,
        end: None,
    }
}

pub(crate) fn clock_window(day: &str, start: &str, end: &str) -> WindowInput {
    WindowInput {
        day: day.into(),
        first_period: None,
        last_period: None,
        start: Some(start.into()),
        end: Some(end.into()),
    }
}

pub(crate) fn avoid(windows: Vec<WindowInput>) -> Option<RuleInput> {
    Some(RuleInput {
        kind: "avoid_times".into(),
        windows,
    })
}

pub(crate) fn constraint(key: &str, target: TargetInput, text: &str) -> ConstraintInput {
    ConstraintInput {
        key: Some(key.into()),
        target,
        rule: None,
        strength: None,
        weight: None,
        status: "pending".into(),
        from: None,
        until: None,
        text: Some(text.into()),
        confirmed_by: None,
        note: None,
        source: None,
    }
}

pub(crate) fn confirmed(
    mut c: ConstraintInput,
    strength: &str,
    rule: Option<RuleInput>,
) -> ConstraintInput {
    c.status = "confirmed".into();
    c.strength = Some(strength.into());
    c.rule = rule;
    c.confirmed_by = Some("학과 조교".into());
    c
}

pub(crate) fn instructor_target(code: &str) -> TargetInput {
    TargetInput {
        instructor: Some(instructor_ref(code)),
        ..Default::default()
    }
}

pub(crate) fn room_target(building: &str, number: &str) -> TargetInput {
    TargetInput {
        room: Some(room_ref(building, number)),
        ..Default::default()
    }
}

/// 학기 설정: 월~금, 09:00부터 50분 수업·10분 쉬는 9교시, 휴게시간 없음
pub fn setup() -> SetupInput {
    SetupInput {
        start: Some("2027-03-02".into()),
        end: Some("2027-06-15".into()),
        days: ["mon", "tue", "wed", "thu", "fri"]
            .map(String::from)
            .to_vec(),
        periods: (0..9)
            .map(|i| PeriodInput {
                label: CodeValue::text((i + 1).to_string()),
                start: format!("{:02}:00", 9 + i),
                end: format!("{:02}:50", 9 + i),
            })
            .collect(),
        breaks: Vec::new(),
        rules: Some(RulesInput {
            required_course_overlap: Some("hard".into()),
            block_may_cross_break: Some(false),
            room_turnaround_minutes: Some(0),
        }),
    }
}

pub(crate) fn equipment() -> Vec<EquipmentInput> {
    [
        ("PC", "실습용 PC", "per_person"),
        ("KIT", "임베디드 실습 키트", "per_person"),
        ("BEAM", "빔프로젝터", "per_room"),
    ]
    .into_iter()
    .map(|(code, name, scope)| EquipmentInput {
        code: code.into(),
        name: name.into(),
        scope: scope.into(),
        source: None,
    })
    .collect()
}

pub(crate) fn room_input(r: &RoomRow) -> RoomInput {
    RoomInput {
        building: r.building.into(),
        number: r.number.into(),
        kind: r.kind.into(),
        department: Some(r.department.into()),
        total_seats: r.seats,
        available_seats: None,
        equipment: r
            .equipment
            .iter()
            .map(|&(code, owned, available)| RoomEquipmentInput {
                code: code.into(),
                owned,
                available: (available != owned).then_some(available),
                inspection_from: None,
                inspection_until: None,
            })
            .collect(),
        shared: r.shared,
        schedule_checked: r.schedule_checked.map(String::from),
        info_source: Some("시설팀 2027-1 강의실 현황".into()),
        source: None,
    }
}

pub(crate) fn section_input(s: &Sec) -> SectionInput {
    SectionInput {
        course: s.course.into(),
        section: s.section.into(),
        instructors: s.instructors.iter().map(|c| instructor_ref(c)).collect(),
        groups: s
            .groups
            .iter()
            .map(|&(g, required)| GroupLinkInput {
                group: g.into(),
                required,
            })
            .collect(),
        enrollment: s.enrollment,
        provisional: s.provisional,
        kind: s.kind.into(),
        room_kind: s.room_kind.map(String::from),
        weekly_periods: s.split.iter().sum(),
        split: (s.split.len() > 1).then(|| s.split.to_vec()),
        equipment: s
            .equipment
            .iter()
            .map(|&(code, quantity)| EquipmentNeedInput {
                code: code.into(),
                quantity,
            })
            .collect(),
        source: None,
    }
}

/// 기준 학과의 편성 조건
pub(crate) fn constraints() -> Vec<ConstraintInput> {
    let mut list = vec![
        confirmed(
            constraint(
                "C01",
                instructor_target("P03"),
                "수요일 13~16시는 가능하면 비워 주세요.",
            ),
            "soft",
            avoid(vec![clock_window("wed", "13:00", "16:00")]),
        ),
        confirmed(
            constraint(
                "C02",
                instructor_target("P03"),
                "담당 교원은 목요일 1교시에 수업할 수 없습니다.",
            ),
            "hard",
            avoid(vec![periods_window("thu", "1", "1")]),
        ),
        // 강도와 범위가 확인되지 않은 요청: 확인 대기로 두고 적용하지 않는다
        constraint(
            "C03",
            instructor_target("P03"),
            "오전에 수업하기 어렵습니다.",
        ),
        confirmed(
            constraint(
                "C04",
                room_target("공학관", "305"),
                "목요일 오후 전자공학과 실습",
            ),
            "hard",
            avoid(vec![clock_window("thu", "13:00", "17:00")]),
        ),
        confirmed(
            constraint("C05", room_target("본관", "305"), "월·수 오전 교양 강의"),
            "hard",
            avoid(vec![
                periods_window("mon", "1", "3"),
                periods_window("wed", "1", "3"),
            ]),
        ),
        confirmed(
            constraint(
                "C06",
                instructor_target("P22"),
                "겸임교원, 금요일에만 출강 가능",
            ),
            "hard",
            avoid(
                ["mon", "tue", "wed", "thu"]
                    .map(|d| periods_window(d, "1", "9"))
                    .to_vec(),
            ),
        ),
        confirmed(
            constraint(
                "C08",
                instructor_target("P15"),
                "금요일 오후는 가급적 비워 주세요.",
            ),
            "soft",
            avoid(vec![clock_window("fri", "13:00", "18:00")]),
        ),
        confirmed(
            constraint(
                "C09",
                TargetInput {
                    group: Some("Y4".into()),
                    ..Default::default()
                },
                "4학년 수요일 15~17시 취업 특강",
            ),
            "soft",
            avoid(vec![clock_window("wed", "15:00", "17:00")]),
        ),
    ];
    let mut april = confirmed(
        constraint(
            "C07",
            instructor_target("P10"),
            "4월 한 달간 화요일 오전 학과 회의",
        ),
        "hard",
        avoid(vec![periods_window("tue", "1", "3")]),
    );
    april.from = Some("2027-04-01".into());
    april.until = Some("2027-04-30".into());
    list.insert(6, april);
    list[0].weight = Some(3);
    list[0].note = Some("교수 면담 때 구두로 받은 요청".into());
    list
}

/// 미리 잠근 배치: 캡스톤디자인 01 첫 수업(월 6~7교시 공학관 301호), 졸업세미나(금 7교시 공학관 202호)
pub(crate) fn placements() -> Vec<PlacementInput> {
    let place =
        |course: &str, section: &str, day: &str, start: &str, b: &str, n: &str| PlacementInput {
            course: course.into(),
            section: section.into(),
            part: Some(1),
            day: day.into(),
            start_period: start.into(),
            room: Some(room_ref(b, n)),
            locked: true,
            source: None,
        };
    vec![
        place("CSE4010", "01", "mon", "6", "공학관", "301"),
        place("CSE4050", "01", "fri", "7", "공학관", "202"),
    ]
}

/// 기준 학과 전체 문서
pub fn reference() -> InputDocument {
    let mut doc = InputDocument::new(YEAR, SEASON);
    doc.setup = Some(setup());
    doc.equipment = equipment();
    doc.rooms = ROOMS.iter().map(room_input).collect();
    doc.instructors = INSTRUCTORS
        .iter()
        .map(|&(code, name, dept)| InstructorInput {
            code: code.into(),
            name: name.into(),
            department: Some(dept.into()),
            source: None,
        })
        .collect();
    doc.groups = GROUPS
        .iter()
        .map(|&(code, name, grade)| GroupInput {
            code: code.into(),
            name: name.into(),
            grade: Some(grade),
            track: None,
            source: None,
        })
        .collect();
    doc.courses = COURSES
        .iter()
        .map(|&(code, name)| CourseInput {
            code: code.into(),
            name: name.into(),
            source: None,
        })
        .collect();
    doc.sections = SECTIONS.iter().map(section_input).collect();
    doc.constraints = constraints();
    doc.placements = placements();
    doc
}

/// 교원 코드로 이름 찾기 (원본 엑셀 만들 때 쓴다)
pub(crate) fn instructor_name(code: &str) -> &'static str {
    INSTRUCTORS
        .iter()
        .find(|t| t.0 == code)
        .map(|t| t.1)
        .expect("기준 학과에 있는 교원 코드")
}

pub(crate) fn instructor_dept(code: &str) -> &'static str {
    INSTRUCTORS
        .iter()
        .find(|t| t.0 == code)
        .map(|t| t.2)
        .expect("기준 학과에 있는 교원 코드")
}

pub(crate) fn course_name(code: &str) -> &'static str {
    COURSES
        .iter()
        .find(|c| c.0 == code)
        .map(|c| c.1)
        .expect("기준 학과에 있는 학수번호")
}

pub(crate) fn group_name(code: &str) -> &'static str {
    GROUPS
        .iter()
        .find(|g| g.0 == code)
        .map(|g| g.1)
        .expect("기준 학과에 있는 수강 집단")
}
