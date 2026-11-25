mod search;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use auler_domain::model::*;
use auler_domain::time::Weekday;

use crate::{CheckedPlacement, Evaluation, Evaluator, Finding, Level, Reason, Resource};
use search::Search;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolvePhase {
    Preparing,
    Searching,
    Improving,
}

impl SolvePhase {
    pub fn code(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Searching => "searching",
            Self::Improving => "improving",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub phase: SolvePhase,
    pub elapsed: Duration,
    pub best_placed: usize,
    pub meetings: usize,
    pub preference_cost: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveStatus {
    Solved,
    NoSolution,
    TimedOut,
    Cancelled,
}

impl SolveStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Solved => "solved",
            Self::NoSolution => "no_solution",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SolveOptions {
    pub time_limit: Duration,
    pub seed: u64,
    pub improve_passes: usize,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            time_limit: Duration::from_secs(3),
            seed: 0,
            improve_passes: 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RejectionCount {
    pub example: Finding,
    pub occurrences: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnassignedCause {
    NoCandidate,
    Conflicts,
    BlockedByLocks,
    TimeLimit,
    Cancelled,
}

impl UnassignedCause {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoCandidate => "no_candidate",
            Self::Conflicts => "conflicts",
            Self::BlockedByLocks => "blocked_by_locks",
            Self::TimeLimit => "time_limit",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Unassigned {
    pub meeting: MeetingRef,
    pub cause: UnassignedCause,
    pub candidate_count: Option<usize>,
    pub checked_against_preview: usize,
    /// Rejections observed while preparing this meeting or checking possible locations.
    pub rejections: Vec<RejectionCount>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequiredResource {
    Instructor(InstructorId),
    Group(GroupId),
}

impl RequiredResource {
    pub fn code(self) -> &'static str {
        match self {
            Self::Instructor(_) => "instructor",
            Self::Group(_) => "required_group",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityShortfall {
    pub resource: RequiredResource,
    pub needed_periods: usize,
    pub available_periods: usize,
}

#[derive(Clone, Debug)]
pub struct SolveResult {
    pub status: SolveStatus,
    pub placements: Vec<Placement>,
    pub unassigned: Vec<Unassigned>,
    pub blocked_locks: Vec<CheckedPlacement>,
    pub capacity_shortfalls: Vec<CapacityShortfall>,
    pub preferences: PreferenceSummary,
    pub improvement: ImprovementStatus,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImprovementStatus {
    Skipped,
    Completed,
    TimedOut,
    Cancelled,
}

impl ImprovementStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Completed => "completed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PreferenceSummary {
    pub cost: u64,
    pub warnings: Vec<(MeetingRef, Finding)>,
}

pub trait Solver {
    fn solve(
        &self,
        data: &TermData,
        options: SolveOptions,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> SolveResult;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalSolver;

struct Budget<'a> {
    started: Instant,
    limit: Duration,
    cancelled: &'a AtomicBool,
    meetings: usize,
}

impl Budget<'_> {
    fn stop(&self) -> Option<SolveStatus> {
        if self.cancelled.load(Ordering::Relaxed) {
            Some(SolveStatus::Cancelled)
        } else if self.started.elapsed() >= self.limit {
            Some(SolveStatus::TimedOut)
        } else {
            None
        }
    }

    fn progress(&self, phase: SolvePhase, best_placed: usize) -> Progress {
        Progress {
            phase,
            elapsed: self.started.elapsed(),
            best_placed,
            meetings: self.meetings,
            preference_cost: None,
        }
    }

    fn optimization_stop(&self) -> Option<ImprovementStatus> {
        match self.stop() {
            Some(SolveStatus::Cancelled) => Some(ImprovementStatus::Cancelled),
            Some(SolveStatus::TimedOut) => Some(ImprovementStatus::TimedOut),
            _ => None,
        }
    }
}

struct Choice {
    placement: Placement,
    score: u32,
    tie: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RejectionDetail {
    Resource(Resource),
    Target(Target),
    Equipment(EquipmentId),
}

type Reasons = BTreeMap<(&'static str, Option<RejectionDetail>), RejectionCount>;

struct Domain {
    meeting: MeetingRef,
    choices: Vec<Choice>,
    rejected: Reasons,
}

impl Solver for LocalSolver {
    fn solve(
        &self,
        data: &TermData,
        options: SolveOptions,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
    ) -> SolveResult {
        self.solve_scope(data, options, cancelled, progress, None)
    }
}

impl LocalSolver {
    /// Schedule only the selected meeting units while retaining locked placements in the board.
    pub fn solve_scope(
        &self,
        data: &TermData,
        options: SolveOptions,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(Progress),
        scope: Option<&BTreeSet<MeetingRef>>,
    ) -> SolveResult {
        let started = Instant::now();
        let evaluator = Evaluator::new(data);
        let mut board: Vec<_> = data
            .placements
            .iter()
            .filter(|p| p.locked)
            .cloned()
            .collect();
        board.sort_by_key(|p| p.meeting);
        let fixed = board.len();
        let locked_ids: HashSet<_> = board.iter().map(|p| p.meeting).collect();
        let meetings: Vec<_> = data
            .meetings()
            .filter(|m| scope.is_none_or(|scope| scope.contains(m)))
            .collect();
        let budget = Budget {
            started,
            limit: options.time_limit,
            cancelled,
            meetings: fixed + meetings.iter().filter(|m| !locked_ids.contains(m)).count(),
        };
        progress(budget.progress(SolvePhase::Preparing, fixed));
        let audit = evaluator.audit(&board);
        let blocked_locks: Vec<_> = audit
            .checks
            .into_iter()
            .chain(audit.missing_rooms)
            .filter(|checked| !checked.evaluation.can_assign())
            .collect();
        let mut domains = Vec::new();
        let mut stopped = budget.stop();
        if blocked_locks.is_empty() && stopped.is_none() {
            for meeting in meetings.iter().copied().filter(|m| !locked_ids.contains(m)) {
                match build_domain(data, &evaluator, meeting, &board, &budget, options.seed) {
                    Ok(domain) => domains.push(domain),
                    Err(status) => {
                        stopped = Some(status);
                        break;
                    }
                }
            }
        }
        let mut capacity_shortfalls = Vec::new();
        if stopped.is_none() && blocked_locks.is_empty() {
            match count_capacity(data, &domains, &board, &budget) {
                Ok(shortfalls) => capacity_shortfalls = shortfalls,
                Err(status) => stopped = Some(status),
            }
        }
        let infeasible = !blocked_locks.is_empty()
            || !capacity_shortfalls.is_empty()
            || domains.iter().any(|domain| domain.choices.is_empty());

        let status = if !blocked_locks.is_empty() {
            if stopped == Some(SolveStatus::Cancelled) {
                SolveStatus::Cancelled
            } else {
                SolveStatus::NoSolution
            }
        } else if let Some(status) = stopped {
            status
        } else if infeasible {
            progress(budget.progress(SolvePhase::Searching, fixed));
            fill_preview(&evaluator, &domains, &mut board, &budget, progress);
            progress(budget.progress(SolvePhase::Searching, board.len()));
            if budget.stop() == Some(SolveStatus::Cancelled) {
                SolveStatus::Cancelled
            } else {
                SolveStatus::NoSolution
            }
        } else {
            progress(budget.progress(SolvePhase::Searching, fixed));
            let prepared: Vec<_> = domains
                .iter()
                .map(|domain| {
                    domain
                        .choices
                        .iter()
                        .map(|choice| {
                            evaluator
                                .prepare(&choice.placement)
                                .expect("candidate passed evaluation")
                        })
                        .collect()
                })
                .collect();
            let (status, selected) =
                Search::new(&evaluator, &prepared, &budget, progress, fixed).run();
            board.extend(
                selected
                    .into_iter()
                    .enumerate()
                    .filter_map(|(domain, choice)| {
                        choice.map(|choice| domains[domain].choices[choice].placement.clone())
                    }),
            );
            status
        };
        let improvement = if status == SolveStatus::Solved && options.improve_passes > 0 {
            if budget.stop().is_none() {
                let mut phase = budget.progress(SolvePhase::Improving, board.len());
                phase.preference_cost = Some(preferences(&evaluator, &board).cost);
                progress(phase);
            }
            budget.optimization_stop().unwrap_or_else(|| {
                improve(
                    &evaluator,
                    &domains,
                    &mut board,
                    fixed,
                    options.improve_passes,
                    &budget,
                )
            })
        } else {
            ImprovementStatus::Skipped
        };
        board.sort_by_key(|p| p.meeting);
        let unassigned = describe_unassigned(
            meetings,
            &evaluator,
            &board,
            &domains,
            status,
            !blocked_locks.is_empty(),
        );
        let preferences = preferences(&evaluator, &board);
        SolveResult {
            status,
            placements: board,
            unassigned,
            blocked_locks,
            capacity_shortfalls,
            preferences,
            improvement,
            elapsed: budget.started.elapsed(),
        }
    }
}

fn build_domain(
    data: &TermData,
    evaluator: &Evaluator<'_>,
    meeting: MeetingRef,
    fixed: &[Placement],
    budget: &Budget<'_>,
    seed: u64,
) -> Result<Domain, SolveStatus> {
    let mut domain = Domain {
        meeting,
        choices: Vec::new(),
        rejected: Reasons::new(),
    };
    if data.rooms.is_empty() {
        add_rejections(
            &mut domain.rejected,
            [Finding {
                level: Level::Hard,
                reason: Reason::RoomRequired,
                weight: 0,
            }],
        );
    }
    let old = data.placement(meeting);
    for &day in &data.setup.days {
        for start in 0..data.setup.periods.len().min(usize::from(u8::MAX) + 1) {
            if let Some(stop) = budget.stop() {
                return Err(stop);
            }
            for room in &data.rooms {
                let placement = Placement {
                    meeting,
                    day,
                    start: start as u8,
                    room: Some(room.id),
                    locked: false,
                };
                let evaluation = evaluator.evaluate(&placement, fixed);
                if evaluation.can_assign() {
                    let tie = tie_break(&placement, seed);
                    domain.choices.push(Choice {
                        placement,
                        score: evaluation.preference_cost(),
                        tie,
                    });
                } else {
                    add_rejections(&mut domain.rejected, evaluation.findings);
                }
            }
        }
    }
    domain.choices.sort_unstable_by_key(|choice| {
        (
            choice.score,
            old != Some(&choice.placement),
            choice.tie,
            choice.placement.day,
            choice.placement.start,
            choice.placement.room,
        )
    });
    if !domain.choices.is_empty() {
        domain.rejected.clear();
    }
    Ok(domain)
}

fn tie_break(p: &Placement, seed: u64) -> u64 {
    if seed == 0 {
        return 0;
    }
    let mut value = seed
        ^ p.room.map_or(0, |room| room.0 as u64).rotate_left(17)
        ^ (u64::from(p.start) << 8)
        ^ p.day.index() as u64;
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn add_rejections(out: &mut Reasons, findings: impl IntoIterator<Item = Finding>) {
    for finding in findings {
        if matches!(finding.level, Level::Hard | Level::ExternalCheck) {
            let detail = match finding.reason {
                Reason::Overlap { resource, .. } => Some(RejectionDetail::Resource(resource)),
                Reason::RoomTurnaround { room, .. } => {
                    Some(RejectionDetail::Resource(Resource::Room(room)))
                }
                Reason::Constraint { target, .. } => Some(RejectionDetail::Target(target)),
                Reason::NotEnoughEquipment { equipment, .. }
                | Reason::EquipmentInspection { equipment, .. }
                | Reason::UnknownEquipment(equipment) => {
                    Some(RejectionDetail::Equipment(equipment))
                }
                _ => None,
            };
            let count = out
                .entry((finding.reason.code(), detail))
                .or_insert(RejectionCount {
                    example: finding,
                    occurrences: 0,
                });
            count.occurrences += 1;
        }
    }
}

#[derive(Default)]
struct Capacity {
    periods: usize,
    slots: BTreeSet<(Weekday, u8)>,
}

fn count_capacity(
    data: &TermData,
    domains: &[Domain],
    fixed: &[Placement],
    budget: &Budget<'_>,
) -> Result<Vec<CapacityShortfall>, SolveStatus> {
    let mut resources: BTreeMap<RequiredResource, Capacity> = BTreeMap::new();
    let candidates: HashMap<_, _> = domains
        .iter()
        .map(|domain| (domain.meeting, domain))
        .collect();
    let locked: HashMap<_, _> = fixed
        .iter()
        .map(|placement| (placement.meeting, placement))
        .collect();
    for section in &data.sections {
        if let Some(status) = budget.stop() {
            return Err(status);
        }
        let targets: BTreeSet<_> = section
            .instructors
            .iter()
            .copied()
            .map(RequiredResource::Instructor)
            .chain(
                section
                    .groups
                    .iter()
                    .filter(|group| {
                        group.required && data.setup.rules.required_course_overlap == Strength::Hard
                    })
                    .map(|group| RequiredResource::Group(group.group)),
            )
            .collect();
        for meeting in section.meetings() {
            let domain = candidates.get(&meeting).copied();
            let fixed = locked.get(&meeting).copied();
            if fixed.is_none() && domain.is_none_or(|domain| domain.choices.is_empty()) {
                continue;
            }
            let span = usize::from(section.split[usize::from(meeting.part)]);
            let slots = fixed
                .into_iter()
                .chain(
                    domain
                        .into_iter()
                        .flat_map(|domain| domain.choices.iter().map(|choice| &choice.placement)),
                )
                .flat_map(|p| {
                    (0..span).filter_map(move |offset| {
                        Some((p.day, p.start.checked_add(u8::try_from(offset).ok()?)?))
                    })
                })
                .collect::<BTreeSet<_>>();
            for &target in &targets {
                let capacity = resources.entry(target).or_default();
                capacity.periods += span;
                capacity.slots.extend(slots.iter().copied());
            }
        }
    }
    Ok(resources
        .into_iter()
        .filter_map(|(resource, capacity)| {
            (capacity.slots.len() < capacity.periods).then_some(CapacityShortfall {
                resource,
                needed_periods: capacity.periods,
                available_periods: capacity.slots.len(),
            })
        })
        .collect())
}

fn fill_preview(
    evaluator: &Evaluator<'_>,
    domains: &[Domain],
    board: &mut Vec<Placement>,
    budget: &Budget<'_>,
    progress: &mut dyn FnMut(Progress),
) {
    let mut by_size: Vec<_> = domains.iter().filter(|d| !d.choices.is_empty()).collect();
    by_size.sort_by_key(|d| (d.choices.len(), d.meeting));
    for domain in by_size {
        for choice in &domain.choices {
            if budget.stop().is_some() {
                return;
            }
            if evaluator.evaluate(&choice.placement, board).can_assign() {
                board.push(choice.placement.clone());
                if board.len().is_multiple_of(8) {
                    progress(budget.progress(SolvePhase::Searching, board.len()));
                }
                break;
            }
        }
    }
}

fn describe_unassigned(
    meetings: Vec<MeetingRef>,
    evaluator: &Evaluator<'_>,
    board: &[Placement],
    domains: &[Domain],
    status: SolveStatus,
    blocked_locks: bool,
) -> Vec<Unassigned> {
    let placed: HashSet<_> = board.iter().map(|placement| placement.meeting).collect();
    let domains: HashMap<_, _> = domains
        .iter()
        .map(|domain| (domain.meeting, domain))
        .collect();
    let mut remaining_checks = 128;
    meetings
        .into_iter()
        .filter(|m| !placed.contains(m))
        .map(|meeting| {
            let domain = domains.get(&meeting).copied();
            let candidate_count = domain.map(|domain| domain.choices.len());
            let cause = match (domain, status) {
                (Some(_), _) if candidate_count == Some(0) => UnassignedCause::NoCandidate,
                (_, SolveStatus::Cancelled) => UnassignedCause::Cancelled,
                (_, SolveStatus::TimedOut) => UnassignedCause::TimeLimit,
                (_, SolveStatus::NoSolution) if blocked_locks => UnassignedCause::BlockedByLocks,
                _ => UnassignedCause::Conflicts,
            };
            let mut reasons = Reasons::new();
            let mut checked = 0;
            if let Some(domain) = domain {
                if cause == UnassignedCause::NoCandidate {
                    reasons = domain.rejected.clone();
                } else if status == SolveStatus::NoSolution || !board.is_empty() {
                    let limit = if status == SolveStatus::NoSolution {
                        16
                    } else {
                        4
                    };
                    for choice in domain.choices.iter().take(limit.min(remaining_checks)) {
                        add_rejections(
                            &mut reasons,
                            evaluator.evaluate(&choice.placement, board).findings,
                        );
                        checked += 1;
                    }
                    remaining_checks -= checked;
                }
            }
            Unassigned {
                meeting,
                cause,
                candidate_count,
                checked_against_preview: checked,
                rejections: reasons.into_values().collect(),
            }
        })
        .collect()
}

fn preferences(evaluator: &Evaluator<'_>, board: &[Placement]) -> PreferenceSummary {
    let mut summary = PreferenceSummary::default();
    for placement in board {
        for finding in evaluator.evaluate(placement, board).findings {
            if finding.level == Level::Preference
                && !matches!(finding.reason, Reason::Overlap { other, .. } if other < placement.meeting)
            {
                summary.cost += u64::from(finding.weight);
                summary.warnings.push((placement.meeting, finding));
            }
        }
    }
    summary
}

fn improve(
    evaluator: &Evaluator<'_>,
    domains: &[Domain],
    board: &mut [Placement],
    fixed: usize,
    passes: usize,
    budget: &Budget<'_>,
) -> ImprovementStatus {
    let by_meeting: HashMap<_, _> = domains
        .iter()
        .map(|domain| (domain.meeting, domain))
        .collect();
    for _ in 0..passes {
        if let Some(stopped) = budget.optimization_stop() {
            return stopped;
        }
        let mut changed = false;
        for idx in fixed..board.len() {
            let current = affected_score(&evaluator.evaluate(&board[idx], board), None);
            if current == 0 {
                continue;
            }
            for choice in &by_meeting[&board[idx].meeting].choices {
                if let Some(stopped) = budget.optimization_stop() {
                    return stopped;
                }
                let result = evaluator.evaluate(&choice.placement, board);
                if result.can_assign() && affected_score(&result, None) < current {
                    board[idx] = choice.placement.clone();
                    changed = true;
                    break;
                }
            }
        }
        let with_warnings: Vec<_> = (fixed..board.len())
            .filter(|&idx| evaluator.evaluate(&board[idx], board).preference_cost() > 0)
            .collect();
        for left in with_warnings {
            for right in (fixed..board.len()).filter(|&idx| idx != left) {
                if let Some(stopped) = budget.optimization_stop() {
                    return stopped;
                }
                if improve_swap(evaluator, board, left, right) {
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }
    ImprovementStatus::Completed
}

fn improve_swap(
    evaluator: &Evaluator<'_>,
    board: &mut [Placement],
    left: usize,
    right: usize,
) -> bool {
    let lhs = board[left].clone();
    let rhs = board[right].clone();
    let previous = affected_score(&evaluator.evaluate(&lhs, board), None)
        + affected_score(&evaluator.evaluate(&rhs, board), Some(lhs.meeting));
    board[left] = Placement {
        meeting: lhs.meeting,
        ..rhs.clone()
    };
    board[right] = Placement {
        meeting: rhs.meeting,
        ..lhs.clone()
    };
    let new_lhs = evaluator.evaluate(&board[left], board);
    let new_rhs = evaluator.evaluate(&board[right], board);
    if new_lhs.can_assign()
        && new_rhs.can_assign()
        && affected_score(&new_lhs, None) + affected_score(&new_rhs, Some(lhs.meeting)) < previous
    {
        true
    } else {
        board[left] = lhs;
        board[right] = rhs;
        false
    }
}

/// A swap checks both meetings, so only one of their evaluations counts the mutual preference.
fn affected_score(evaluation: &Evaluation, other_move: Option<MeetingRef>) -> u64 {
    evaluation.findings.iter().filter(|finding| finding.level == Level::Preference
        && !matches!(finding.reason, Reason::Overlap { other, .. } if Some(other) == other_move))
        .map(|finding| u64::from(finding.weight)).sum()
}
