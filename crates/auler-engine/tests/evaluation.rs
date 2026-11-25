use auler_domain::code::{CourseCode, RoomKey, SectionCode};
use auler_domain::model::*;
use auler_domain::resolve::resolve;
use auler_domain::time::{Break, ClockTime, Date, Period, PeriodTable, Weekday};
use auler_engine::{Evaluation, Evaluator, Level, Reason, Resource, RoomStatus, occupancy};

fn fixture() -> TermData {
    resolve(&auler_samples::reference()).unwrap()
}

fn placement(data: &TermData, day: Weekday, start: u8) -> Placement {
    let section = data
        .find_section(
            &CourseCode::new("CSE2010").unwrap(),
            &SectionCode::new("01").unwrap(),
        )
        .unwrap();
    let room = data
        .find_room(&RoomKey::new("공학관", "302").unwrap())
        .unwrap();
    Placement {
        meeting: section.meetings().last().unwrap(),
        day,
        start,
        room: Some(room.id),
        locked: false,
    }
}

fn finding(result: &Evaluation, reason: &str) -> Option<Level> {
    result
        .findings
        .iter()
        .find(|finding| finding.reason.code() == reason)
        .map(|finding| finding.level)
}

#[test]
fn reference_rooms_distinguish_capacity_external_schedule_and_unavailability() {
    let data = fixture();
    let candidate = placement(&data, Weekday::Thu, 5);
    let engine = Evaluator::new(&data);
    let rooms = engine.room_candidates(&candidate, &data.placements);
    let status = |building, number| {
        let room_id = data
            .find_room(&RoomKey::new(building, number).unwrap())
            .unwrap()
            .id;
        rooms
            .iter()
            .find(|room| room.room == room_id)
            .unwrap()
            .evaluation
            .room_status()
    };
    assert_eq!(status("공학관", "301"), RoomStatus::ConditionsNotMet);
    assert_eq!(status("공학관", "302"), RoomStatus::Available);
    assert_eq!(status("공학관", "305"), RoomStatus::Unavailable);
    assert_eq!(status("본관", "201"), RoomStatus::ExternalCheck);
    assert_eq!(
        engine.evaluate(&candidate, &data.placements).room_status(),
        RoomStatus::Available
    );
}

#[test]
fn pending_requests_are_ignored_and_preferences_keep_their_weight() {
    let mut data = fixture();
    let pending = data
        .constraints
        .iter_mut()
        .find(|condition| condition.status == ConstraintStatus::Pending)
        .unwrap();
    pending.rule = Some(Rule::AvoidTimes {
        windows: vec![TimeWindow::Periods {
            day: Weekday::Mon,
            first: 0,
            last: 0,
        }],
    });
    pending.strength = Some(Strength::Hard);
    let pending_id = pending.id;
    let engine = Evaluator::new(&data);
    let pending_only = engine.evaluate(&placement(&data, Weekday::Mon, 0), &[]);
    assert!(pending_only.can_assign());
    assert_eq!(finding(&pending_only, "constraint"), None);
    assert!(pending_only.pending_constraints.contains(&pending_id));
    assert!(engine.audit(&[]).pending_constraints.contains(&pending_id));
    let soft = engine.evaluate(&placement(&data, Weekday::Wed, 5), &[]);
    assert!(soft.can_assign());
    assert_eq!(soft.preference_cost(), 3);
    assert_eq!(soft.room_status(), RoomStatus::Available);
    let hard = engine.evaluate(&placement(&data, Weekday::Thu, 0), &[]);
    assert!(!hard.can_assign());
    assert_eq!(finding(&hard, "constraint"), Some(Level::Hard));
}

#[test]
fn required_group_conflicts_follow_school_settings_without_masking_other_conflicts() {
    let mut data = fixture();
    let candidate = placement(&data, Weekday::Mon, 2);
    let group = data.groups[0].id;
    let teacher = data.instructors[0].id;
    let section_index = data
        .sections
        .iter()
        .position(|section| section.id == candidate.meeting.section)
        .unwrap();
    let other_index = if section_index == 0 { 1 } else { 0 };
    for index in [section_index, other_index] {
        data.sections[index].groups = vec![GroupLink {
            group,
            required: true,
        }];
        data.sections[index].instructors = vec![teacher];
        data.sections[index].split.fill(1);
    }
    let other = Placement {
        meeting: data.sections[other_index].meetings().next().unwrap(),
        ..candidate.clone()
    };
    for strength in [Strength::Hard, Strength::Soft] {
        data.setup.rules.required_course_overlap = strength;
        let result = Evaluator::new(&data).evaluate(&candidate, std::slice::from_ref(&other));
        assert!(result.findings.iter().any(|f| matches!(f.reason,
            Reason::Overlap { resource: Resource::Instructor(id), .. } if id == teacher)
            && f.level == Level::Hard));
        assert!(result.findings.iter().any(|f| matches!(
            f.reason,
            Reason::Overlap {
                resource: Resource::Room(_),
                ..
            }
        ) && f.level == Level::Hard));
        assert!(result.findings.iter().any(|f| matches!(f.reason,
            Reason::Overlap { resource: Resource::RequiredGroup(id), .. } if id == group)
            && f.level
                == if strength == Strength::Hard {
                    Level::Hard
                } else {
                    Level::Preference
                }));
    }
}

#[test]
fn precise_clock_overlap_and_room_turnaround_respect_endpoints() {
    let mut data = fixture();
    let candidate = placement(&data, Weekday::Mon, 1);
    let other = Placement {
        meeting: data
            .sections
            .iter()
            .find(|section| section.id != candidate.meeting.section)
            .unwrap()
            .meetings()
            .next()
            .unwrap(),
        start: 0,
        ..candidate.clone()
    };
    for section in &mut data.sections {
        section.split.fill(1);
    }
    let first = occupancy(
        &data.setup,
        data.section(other.meeting.section).unwrap(),
        &other,
    )
    .unwrap();
    let next = occupancy(
        &data.setup,
        data.section(candidate.meeting.section).unwrap(),
        &candidate,
    )
    .unwrap();
    let gap = next.start.minutes() - first.end.minutes();
    assert!(!first.overlaps(next));
    for required in [gap, gap + 1] {
        data.setup.rules.room_turnaround_minutes = required;
        let eval = Evaluator::new(&data).evaluate(&candidate, std::slice::from_ref(&other));
        assert_eq!(
            finding(&eval, "room_turnaround"),
            (required > gap).then_some(Level::Hard)
        );
    }
}

#[test]
fn explicit_break_crossing_does_not_assume_uniform_school_periods() {
    let mut data = fixture();
    let mut candidate = placement(&data, Weekday::Tue, 0);
    candidate.meeting.part = 0;
    let time = |text| ClockTime::parse(text).unwrap();
    let section = data
        .sections
        .iter_mut()
        .find(|section| section.id == candidate.meeting.section)
        .unwrap();
    section.split[0] = 2;
    data.setup.periods = PeriodTable::new(
        vec![
            Period {
                label: "A".into(),
                start: time("11:10"),
                end: time("11:55"),
            },
            Period {
                label: "B".into(),
                start: time("13:05"),
                end: time("14:20"),
            },
        ],
        vec![Break {
            label: "Lunch".into(),
            start: time("12:00"),
            end: time("13:00"),
        }],
    )
    .unwrap();
    let actual = occupancy(
        &data.setup,
        data.section(candidate.meeting.section).unwrap(),
        &candidate,
    )
    .unwrap();
    assert_eq!((actual.start, actual.end), (time("11:10"), time("14:20")));
    data.setup.rules.block_may_cross_break = false;
    assert_eq!(
        finding(
            &Evaluator::new(&data).evaluate(&candidate, &[]),
            "crosses_break"
        ),
        Some(Level::Hard)
    );
    data.setup.rules.block_may_cross_break = true;
    assert_eq!(
        finding(
            &Evaluator::new(&data).evaluate(&candidate, &[]),
            "crosses_break"
        ),
        None
    );
}

#[test]
fn inspection_is_date_sensitive_and_locks_survive_new_conflicts() {
    let mut data = fixture();
    let candidate = placement(&data, Weekday::Thu, 5);
    let day = Date::parse("2027-04-01").unwrap();
    let room = data
        .rooms
        .iter_mut()
        .find(|room| Some(room.id) == candidate.room)
        .unwrap();
    room.equipment[0].inspection = Some(DateRange {
        from: day,
        until: day,
    });
    let locked = Placement {
        locked: true,
        ..candidate.clone()
    };
    data.placements.push(locked.clone());
    let engine = Evaluator::new(&data);
    let result = engine.evaluate(&locked, &data.placements);
    assert_eq!(finding(&result, "equipment_inspection"), Some(Level::Hard));
    assert_eq!(finding(&result, "locked"), None);
    let next = Placement {
        day: Weekday::Fri,
        ..locked.clone()
    };
    let moved = engine.evaluate(&next, &data.placements);
    assert_eq!(finding(&moved, "equipment_inspection"), None);
    assert_eq!(finding(&moved, "locked"), Some(Level::Hard));
    assert!(engine.audit(&[]).removed_locks.contains(&locked));
    assert!(!engine.audit(&data.placements).can_finalize());
}

#[test]
fn room_quality_stays_visible_when_a_locked_or_conflicting_slot_cannot_be_used() {
    let mut data = fixture();
    let mut locked = placement(&data, Weekday::Thu, 5);
    locked.locked = true;
    data.placements.push(locked.clone());
    let engine = Evaluator::new(&data);
    let rooms = engine.room_candidates(&locked, &data.placements);
    let short = data
        .find_room(&RoomKey::new("공학관", "301").unwrap())
        .unwrap()
        .id;
    let checked = &rooms
        .iter()
        .find(|room| room.room == short)
        .unwrap()
        .evaluation;
    assert_eq!(checked.room_status(), RoomStatus::ConditionsNotMet);
    assert_eq!(finding(checked, "locked"), Some(Level::Hard));
    let unlocked = Placement {
        locked: false,
        ..locked
    };
    assert_eq!(
        finding(
            &engine.evaluate(&unlocked, &data.placements),
            "unlock_required"
        ),
        Some(Level::Hard)
    );
    let conflict = engine.evaluate(&placement(&data, Weekday::Thu, 0), &[]);
    assert_eq!(conflict.room_status(), RoomStatus::Available);
    assert!(!conflict.can_assign());
}

#[test]
fn audit_separates_missing_rooms_from_unplaced_meetings_and_marks_affected_locks() {
    let mut data = fixture();
    let candidate = placement(&data, Weekday::Wed, 4);
    let mut other = data.placements[0].clone();
    other.day = candidate.day;
    other.start = candidate.start;
    other.room = candidate.room;
    data.placements[0] = other.clone();
    let engine = Evaluator::new(&data);
    let checked = engine.evaluate(&candidate, std::slice::from_ref(&other));
    assert!(checked.findings.iter().any(|f| matches!(
        f.reason, Reason::Overlap { other: id, other_locked: true, resource: Resource::Room(_) } if id == other.meeting
    )));
    let roomless = Placement {
        room: None,
        ..candidate
    };
    let audit = engine.audit(std::slice::from_ref(&roomless));
    assert!(audit.checks.is_empty());
    assert_eq!(audit.missing_rooms.len(), 1);
    assert_eq!(audit.missing_rooms[0].placement, roomless);
    assert!(!audit.unassigned.contains(&roomless.meeting));
    assert!(audit.removed_locks.contains(&other));
    assert!(!audit.can_finalize());
}
