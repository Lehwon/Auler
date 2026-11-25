use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use auler_domain::model::*;
use auler_domain::resolve::resolve;
use auler_domain::time::{PeriodTable, Weekday};
use auler_engine::{
    Evaluator, ImprovementStatus, LocalSolver, Reason, RequiredResource, Resource, SolveOptions,
    SolvePhase, SolveStatus, Solver, UnassignedCause,
};

fn dataset(sections: usize, period_count: usize) -> TermData {
    let mut data = resolve(&auler_samples::reference()).unwrap();
    data.sections.truncate(sections);
    data.rooms.truncate(1);
    data.placements.clear();
    data.constraints.clear();
    data.setup.days = vec![Weekday::Mon];
    data.setup.periods = PeriodTable::new(
        data.setup.periods.periods()[..period_count].to_vec(),
        Vec::new(),
    )
    .unwrap();
    for (index, section) in data.sections.iter_mut().enumerate() {
        section.instructors = vec![data.instructors[index].id];
        section.groups.clear();
        section.split = vec![1];
        section.equipment.clear();
        section.room_kind = None;
        section.enrollment.count = 1;
    }
    data
}

#[test]
fn reference_solving_is_repeatable_and_preserves_every_lock() {
    let data = resolve(&auler_samples::reference()).unwrap();
    let options = SolveOptions {
        time_limit: Duration::from_secs(8),
        seed: 71,
        ..SolveOptions::default()
    };
    let cancelled = AtomicBool::new(false);
    let mut progress = Vec::new();
    let first = LocalSolver.solve(&data, options, &cancelled, &mut |update| {
        progress.push(update)
    });
    let second = LocalSolver.solve(&data, options, &cancelled, &mut |_| {});
    assert_eq!(first.status, SolveStatus::Solved);
    assert_eq!(first.placements, second.placements);
    assert_eq!(first.preferences.cost, second.preferences.cost);
    assert!(first.unassigned.is_empty());
    for locked in &data.placements {
        assert!(first.placements.contains(locked));
    }
    assert!(
        Evaluator::new(&data)
            .audit(&first.placements)
            .can_finalize()
    );
    assert_eq!(progress[0].phase, SolvePhase::Preparing);
    assert!(
        progress
            .iter()
            .any(|update| update.phase == SolvePhase::Searching)
    );
    assert!(
        progress
            .iter()
            .any(|update| update.phase == SolvePhase::Improving)
    );
    assert!(progress.windows(2).all(
        |next| next[0].best_placed <= next[1].best_placed && next[0].elapsed <= next[1].elapsed
    ));
}

#[test]
fn selected_scope_preserves_fixed_placements_and_leaves_other_meetings_untouched() {
    let mut data = dataset(3, 3);
    let meetings: Vec<_> = data.meetings().collect();
    let fixed = Placement {
        meeting: meetings[0],
        day: Weekday::Mon,
        start: 0,
        room: Some(data.rooms[0].id),
        locked: true,
    };
    data.placements.push(fixed.clone());
    let result = LocalSolver.solve_scope(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
        Some(&[meetings[1]].into_iter().collect()),
    );
    assert_eq!(result.status, SolveStatus::Solved);
    assert_eq!(result.placements.len(), 2);
    assert!(result.placements.contains(&fixed));
    assert!(
        result
            .placements
            .iter()
            .any(|placed| placed.meeting == meetings[1] && placed.start != fixed.start)
    );
    assert!(result.unassigned.is_empty());
}

#[test]
fn preflight_explains_when_an_instructor_needs_more_periods_than_available() {
    let mut data = dataset(2, 1);
    let teacher = data.instructors[0].id;
    data.sections[1].instructors = vec![teacher];
    let result = LocalSolver.solve(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert_eq!(result.status, SolveStatus::NoSolution);
    assert_eq!(result.placements.len(), 1);
    assert_eq!(result.unassigned.len(), 1);
    assert_eq!(
        result.capacity_shortfalls[0].resource,
        RequiredResource::Instructor(teacher)
    );
    assert_eq!(result.capacity_shortfalls[0].needed_periods, 2);
    assert_eq!(result.capacity_shortfalls[0].available_periods, 1);
    let conflicts: Vec<_> = result.unassigned[0]
        .rejections
        .iter()
        .filter_map(|reason| {
            if let Reason::Overlap { resource, .. } = reason.example.reason {
                Some(resource)
            } else {
                None
            }
        })
        .collect();
    assert!(conflicts.contains(&Resource::Instructor(teacher)));
    assert!(conflicts.contains(&Resource::Room(data.rooms[0].id)));
}

#[test]
fn exhaustive_room_conflict_yields_a_valid_partial_and_proves_no_solution() {
    let data = dataset(3, 2);
    let result = LocalSolver.solve(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert_eq!(result.status, SolveStatus::NoSolution);
    assert!(result.capacity_shortfalls.is_empty());
    assert_eq!(result.placements.len(), 2);
    assert_eq!(result.unassigned[0].cause, UnassignedCause::Conflicts);
    assert_eq!(result.unassigned[0].candidate_count, Some(2));
    assert!(
        result.unassigned[0]
            .rejections
            .iter()
            .any(|reason| reason.example.reason.code() == "overlap")
    );
    assert!(
        Evaluator::new(&data)
            .audit(&result.placements)
            .checks
            .iter()
            .all(|check| check.evaluation.can_assign())
    );
}

#[test]
fn zero_candidates_and_lock_conflicts_report_different_recovery_information() {
    let mut data = dataset(2, 2);
    data.sections[0].room_kind = Some(RoomKind::Seminar);
    let options = SolveOptions::default();
    let cancelled = AtomicBool::new(false);
    let result = LocalSolver.solve(&data, options, &cancelled, &mut |_| {});
    assert_eq!(result.status, SolveStatus::NoSolution);
    assert_eq!(result.unassigned[0].cause, UnassignedCause::NoCandidate);
    assert_eq!(result.unassigned[0].candidate_count, Some(0));
    assert!(result.capacity_shortfalls.is_empty());
    assert!(
        result.unassigned[0]
            .rejections
            .iter()
            .any(|reason| reason.example.reason.code() == "wrong_room_kind")
    );
    let locked = Placement {
        meeting: data.sections[0].meetings().next().unwrap(),
        day: Weekday::Mon,
        start: 0,
        room: Some(data.rooms[0].id),
        locked: true,
    };
    data.placements.push(locked.clone());
    let result = LocalSolver.solve(&data, options, &cancelled, &mut |_| {});
    assert_eq!(result.status, SolveStatus::NoSolution);
    assert_eq!(result.blocked_locks[0].placement, locked);
    assert_eq!(result.placements, [locked]);
    assert_eq!(result.unassigned[0].cause, UnassignedCause::BlockedByLocks);
    assert_eq!(result.unassigned[0].candidate_count, None);
    let expired = LocalSolver.solve(
        &data,
        SolveOptions {
            time_limit: Duration::ZERO,
            ..options
        },
        &cancelled,
        &mut |_| {},
    );
    assert_eq!(expired.status, SolveStatus::NoSolution);
    assert_eq!(expired.unassigned[0].cause, UnassignedCause::BlockedByLocks);
}

#[test]
fn time_limit_and_cancellation_are_distinct_and_preserve_best_progress() {
    let data = resolve(&auler_samples::reference()).unwrap();
    let cancel = AtomicBool::new(false);
    let timeout = LocalSolver.solve(
        &data,
        SolveOptions {
            time_limit: Duration::ZERO,
            ..SolveOptions::default()
        },
        &cancel,
        &mut |_| {},
    );
    assert_eq!(timeout.status, SolveStatus::TimedOut);
    assert_eq!(timeout.placements.len(), 2);
    assert!(
        timeout
            .unassigned
            .iter()
            .all(|item| item.cause == UnassignedCause::TimeLimit && item.candidate_count.is_none())
    );
    let mut best_placed = 0;
    let cancelled = LocalSolver.solve(
        &data,
        SolveOptions {
            time_limit: Duration::from_secs(8),
            ..SolveOptions::default()
        },
        &cancel,
        &mut |progress| {
            if progress.phase == SolvePhase::Searching && progress.best_placed >= 8 {
                best_placed = progress.best_placed;
                cancel.store(true, Ordering::Relaxed);
            }
        },
    );
    assert_eq!(cancelled.status, SolveStatus::Cancelled);
    assert_eq!(cancelled.placements.len(), best_placed);
    assert_eq!(cancelled.placements.iter().filter(|p| p.locked).count(), 2);
    assert!(
        cancelled
            .unassigned
            .iter()
            .all(|item| item.cause == UnassignedCause::Cancelled)
    );
    assert!(
        cancelled
            .unassigned
            .iter()
            .any(|item| item.checked_against_preview > 0)
    );
}

#[test]
fn local_improvement_reduces_optional_group_overlap_without_breaking_hard_rules() {
    let mut data = dataset(2, 2);
    let mut another_room = data.rooms[0].clone();
    another_room.id = RoomId(99);
    another_room.key.number = "099".into();
    data.rooms.push(another_room);
    data.setup.rules.required_course_overlap = Strength::Soft;
    for section in &mut data.sections {
        section.groups = vec![GroupLink {
            group: data.groups[0].id,
            required: true,
        }];
    }
    let cancel = AtomicBool::new(false);
    let initial = LocalSolver.solve(
        &data,
        SolveOptions {
            improve_passes: 0,
            ..SolveOptions::default()
        },
        &cancel,
        &mut |_| {},
    );
    let improved = LocalSolver.solve(&data, SolveOptions::default(), &cancel, &mut |_| {});
    assert_eq!(initial.preferences.cost, 1);
    assert_eq!(initial.preferences.warnings.len(), 1);
    assert_eq!(improved.preferences.cost, 0);
    assert!(
        Evaluator::new(&data)
            .audit(&improved.placements)
            .can_finalize()
    );
}

#[test]
fn cancelling_optimization_retains_the_complete_solution_and_preferences() {
    let mut data = dataset(2, 2);
    let section = data.sections[0].id;
    data.constraints.push(Constraint {
        id: ConstraintId(1),
        target: Target::Section(section),
        rule: Some(Rule::AvoidTimes {
            windows: vec![TimeWindow::Periods {
                day: Weekday::Mon,
                first: 0,
                last: 1,
            }],
        }),
        strength: Some(Strength::Soft),
        weight: 4,
        status: ConstraintStatus::Confirmed,
        period: None,
        text: None,
        source: None,
        confirmed_by: None,
        note: None,
    });
    let cancel = AtomicBool::new(false);
    let result = LocalSolver.solve(&data, SolveOptions::default(), &cancel, &mut |phase| {
        if phase.phase == SolvePhase::Improving {
            assert_eq!(phase.preference_cost, Some(4));
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(result.status, SolveStatus::Solved);
    assert_eq!(result.improvement, ImprovementStatus::Cancelled);
    assert_eq!(result.preferences.cost, 4);
    assert_eq!(result.preferences.warnings.len(), 1);
    assert!(
        Evaluator::new(&data)
            .audit(&result.placements)
            .can_finalize()
    );
}

#[test]
fn a_partial_preview_includes_its_remaining_preferences() {
    let mut data = dataset(2, 1);
    let section = data.sections[0].id;
    data.placements.push(Placement {
        meeting: MeetingRef { section, part: 0 },
        day: Weekday::Mon,
        start: 0,
        room: Some(data.rooms[0].id),
        locked: true,
    });
    data.constraints.push(Constraint {
        id: ConstraintId(2),
        target: Target::Section(section),
        rule: Some(Rule::AvoidTimes {
            windows: vec![TimeWindow::Periods {
                day: Weekday::Mon,
                first: 0,
                last: 0,
            }],
        }),
        strength: Some(Strength::Soft),
        weight: 3,
        status: ConstraintStatus::Confirmed,
        period: None,
        text: None,
        source: None,
        confirmed_by: None,
        note: None,
    });
    let result = LocalSolver.solve(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert_eq!(result.status, SolveStatus::NoSolution);
    assert_eq!(result.unassigned.len(), 1);
    assert_eq!(result.preferences.cost, 3);
    assert_eq!(result.preferences.warnings[0].0.section, section);
}
