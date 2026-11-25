use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use auler_domain::model::*;
use auler_domain::resolve::resolve;
use auler_domain::time::Weekday;
use auler_engine::{Evaluator, LocalSolver, SolveOptions, Solver};
use good_lp::{Expression, ProblemVariables, Solution, SolverModel, Variable, highs, variable};

const LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Resource {
    Room(RoomId),
    Instructor(InstructorId),
    Section(SectionId),
    Group(GroupId),
}

fn compare(name: &str, data: &TermData) {
    assert_eq!(data.setup.rules.room_turnaround_minutes, 0);
    assert_eq!(data.setup.rules.required_course_overlap, Strength::Hard);
    let evaluator = Evaluator::new(data);
    let start = Instant::now();
    let local = LocalSolver.solve(
        data,
        SolveOptions {
            time_limit: LIMIT,
            ..SolveOptions::default()
        },
        &AtomicBool::new(false),
        &mut |_| {},
    );
    println!(
        "{name} local: {:?}, {:?}, placed {}, valid {}, cost {:?}, shortfalls {:?}",
        local.status,
        start.elapsed(),
        local.placements.len(),
        evaluator.audit(&local.placements).can_finalize(),
        local.preferences.cost,
        local.capacity_shortfalls
    );

    let start = Instant::now();
    let mut vars = ProblemVariables::new();
    let mut choices: Vec<(Variable, Placement)> = Vec::new();
    let mut meetings: BTreeMap<MeetingRef, Expression> = BTreeMap::new();
    let mut spaces: BTreeMap<(Resource, Weekday, u8), Expression> = BTreeMap::new();
    let mut cost = Expression::from(0);
    let fixed: Vec<_> = data
        .placements
        .iter()
        .filter(|p| p.locked)
        .cloned()
        .collect();
    for section in &data.sections {
        for meeting in section.meetings() {
            meetings
                .entry(meeting)
                .or_insert_with(|| Expression::from(0));
            let mut locations = Vec::new();
            if let Some(locked) = fixed.iter().find(|p| p.meeting == meeting) {
                locations.push(locked.clone());
            } else {
                for &day in &data.setup.days {
                    for start_period in 0..data.setup.periods.len() {
                        for room in &data.rooms {
                            locations.push(Placement {
                                meeting,
                                day,
                                start: start_period as u8,
                                room: Some(room.id),
                                locked: false,
                            });
                        }
                    }
                }
            }
            for p in locations {
                let check = evaluator.evaluate(&p, &fixed);
                let Some(room) = p.room.filter(|_| check.can_assign()) else {
                    continue;
                };
                let choice = vars.add(variable().binary());
                let resources = [Resource::Room(room), Resource::Section(section.id)]
                    .into_iter()
                    .chain(
                        section
                            .instructors
                            .iter()
                            .copied()
                            .map(Resource::Instructor),
                    )
                    .chain(
                        section
                            .groups
                            .iter()
                            .filter(|g| g.required)
                            .map(|g| Resource::Group(g.group)),
                    );
                let span = section.split[usize::from(meeting.part)];
                for resource in resources {
                    for period in p.start..p.start + span {
                        spaces
                            .entry((resource, p.day, period))
                            .or_insert_with(|| Expression::from(0))
                            .add_mul(1, choice);
                    }
                }
                meetings.get_mut(&meeting).unwrap().add_mul(1, choice);
                cost.add_mul(f64::from(check.preference_cost()), choice);
                choices.push((choice, p));
            }
        }
    }
    let mut model = vars.minimise(cost).using(highs).set_threads(1);
    for value in meetings.into_values() {
        model.add_constraint(value.eq(1));
    }
    for value in spaces.into_values() {
        model.add_constraint(value.leq(1));
    }
    let remaining = LIMIT.saturating_sub(start.elapsed());
    let optimum = model.set_time_limit(remaining.as_secs_f64()).solve();
    match optimum {
        Ok(solution) => {
            let board: Vec<_> = choices
                .iter()
                .filter(|(id, _)| solution.value(*id) > 0.5)
                .map(|(_, value)| value.clone())
                .collect();
            let audit = evaluator.audit(&board);
            let cost: u32 = audit
                .checks
                .iter()
                .map(|p| p.evaluation.preference_cost())
                .sum();
            println!(
                "{name} HiGHS: {:?}, {:?}, {} binary variables, placed {}, valid {}, cost {}",
                solution.status(),
                start.elapsed(),
                choices.len(),
                board.len(),
                audit.can_finalize(),
                cost
            );
        }
        Err(error) => println!(
            "{name} HiGHS: {error:?}, {:?}, {} binary variables",
            start.elapsed(),
            choices.len()
        ),
    }
}

fn main() {
    let reference = resolve(&auler_samples::reference()).unwrap();
    let stress = resolve(&auler_samples::stress()).unwrap();
    compare("reference", &reference);
    compare("stress", &stress);
    let mut impossible = reference.clone();
    impossible.placements.clear();
    impossible.constraints.clear();
    impossible.sections.truncate(2);
    let instructor = impossible.instructors[0].id;
    for section in &mut impossible.sections {
        section.instructors = vec![instructor];
        section.split = vec![1];
        section.room_kind = None;
        section.equipment.clear();
        section.enrollment.count = 1;
    }
    impossible.setup.days = vec![Weekday::Mon];
    impossible.setup.periods = auler_domain::time::PeriodTable::new(
        vec![reference.setup.periods.periods()[0].clone()],
        Vec::new(),
    )
    .unwrap();
    compare("impossible", &impossible);
}
