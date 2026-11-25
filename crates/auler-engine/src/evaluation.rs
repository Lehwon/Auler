use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use auler_domain::model::*;
use auler_domain::time::{Break, Weekday};

use crate::occupancy::{Occupancy, OccupancyError, active_on, occupancy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Hard,
    Preference,
    ExternalCheck,
    Notice,
}

impl Level {
    pub fn code(self) -> &'static str {
        match self {
            Self::Hard => "hard",
            Self::Preference => "preference",
            Self::ExternalCheck => "external_check",
            Self::Notice => "notice",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Resource {
    Section(SectionId),
    Instructor(InstructorId),
    Room(RoomId),
    RequiredGroup(GroupId),
}

impl Resource {
    pub fn code(self) -> &'static str {
        match self {
            Self::Section(_) => "section",
            Self::Instructor(_) => "instructor",
            Self::Room(_) => "room",
            Self::RequiredGroup(_) => "required_group",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    UnknownSection(SectionId),
    Occupancy(OccupancyError),
    CrossesBreak(Vec<Break>),
    Locked(Placement),
    UnlockRequired(Placement),
    DuplicateMeeting,
    RoomRequired,
    UnknownRoom(RoomId),
    WrongRoomKind {
        required: RoomKind,
        actual: RoomKind,
    },
    NotEnoughSeats {
        needed: u32,
        available: u32,
    },
    UnknownEquipment(EquipmentId),
    NotEnoughEquipment {
        equipment: EquipmentId,
        needed: u64,
        available: u32,
    },
    EquipmentInspection {
        equipment: EquipmentId,
        period: DateRange,
    },
    ExternalRoomSchedule(RoomId),
    ProvisionalEnrollment(u32),
    Overlap {
        other: MeetingRef,
        other_locked: bool,
        resource: Resource,
    },
    RoomTurnaround {
        other: MeetingRef,
        other_locked: bool,
        room: RoomId,
        needed: u16,
        available: u16,
    },
    Constraint {
        id: ConstraintId,
        target: Target,
    },
}

impl Reason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownSection(_) => "unknown_section",
            Self::Occupancy(OccupancyError::DayOutsideTerm) => "day_outside_term",
            Self::Occupancy(OccupancyError::UnknownPart) => "unknown_part",
            Self::Occupancy(OccupancyError::PeriodSpanOutsideDay) => "period_span_outside_day",
            Self::CrossesBreak(_) => "crosses_break",
            Self::Locked(_) => "locked",
            Self::UnlockRequired(_) => "unlock_required",
            Self::DuplicateMeeting => "duplicate_meeting",
            Self::RoomRequired => "room_required",
            Self::UnknownRoom(_) => "unknown_room",
            Self::WrongRoomKind { .. } => "wrong_room_kind",
            Self::NotEnoughSeats { .. } => "not_enough_seats",
            Self::UnknownEquipment(_) => "unknown_equipment",
            Self::NotEnoughEquipment { .. } => "not_enough_equipment",
            Self::EquipmentInspection { .. } => "equipment_inspection",
            Self::ExternalRoomSchedule(_) => "external_room_schedule",
            Self::ProvisionalEnrollment(_) => "provisional_enrollment",
            Self::Overlap { .. } => "overlap",
            Self::RoomTurnaround { .. } => "room_turnaround",
            Self::Constraint { .. } => "constraint",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub reason: Reason,
    pub weight: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evaluation {
    pub findings: Vec<Finding>,
    pub pending_constraints: Vec<ConstraintId>,
}

impl Evaluation {
    fn report(&mut self, level: Level, reason: Reason, weight: u8) {
        self.findings.push(Finding {
            level,
            reason,
            weight,
        });
    }

    fn hard(&mut self, reason: Reason) {
        self.report(Level::Hard, reason, 0);
    }

    fn apply_strength(&mut self, strength: Strength, reason: Reason, weight: u8) {
        match strength {
            Strength::Hard => self.hard(reason),
            Strength::Soft => self.report(Level::Preference, reason, weight),
        }
    }

    pub fn can_assign(&self) -> bool {
        !self
            .findings
            .iter()
            .any(|finding| matches!(finding.level, Level::Hard | Level::ExternalCheck))
    }

    pub fn preference_cost(&self) -> u32 {
        self.findings
            .iter()
            .filter(|finding| finding.level == Level::Preference)
            .map(|finding| u32::from(finding.weight))
            .sum()
    }

    /// Availability and fit of the room. Check `can_assign()` for the whole placement.
    pub fn room_status(&self) -> RoomStatus {
        let mut requirements = false;
        let mut external = false;
        let mut preference = false;
        for finding in &self.findings {
            match (&finding.reason, finding.level) {
                (
                    Reason::WrongRoomKind { .. }
                    | Reason::NotEnoughSeats { .. }
                    | Reason::NotEnoughEquipment { .. }
                    | Reason::UnknownEquipment(_),
                    Level::Hard,
                ) => requirements = true,
                (
                    Reason::RoomRequired
                    | Reason::UnknownRoom(_)
                    | Reason::UnknownSection(_)
                    | Reason::Occupancy(_)
                    | Reason::EquipmentInspection { .. }
                    | Reason::Overlap {
                        resource: Resource::Room(_),
                        ..
                    }
                    | Reason::RoomTurnaround { .. }
                    | Reason::Constraint {
                        target: Target::Room(_),
                        ..
                    },
                    Level::Hard,
                ) => return RoomStatus::Unavailable,
                (Reason::ExternalRoomSchedule(_), Level::ExternalCheck) => external = true,
                (
                    Reason::Constraint {
                        target: Target::Room(_),
                        ..
                    },
                    Level::Preference,
                ) => preference = true,
                _ => {}
            }
        }
        if requirements {
            RoomStatus::ConditionsNotMet
        } else if external {
            RoomStatus::ExternalCheck
        } else if preference {
            RoomStatus::PreferenceWarning
        } else {
            RoomStatus::Available
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomStatus {
    Available,
    PreferenceWarning,
    ConditionsNotMet,
    Unavailable,
    ExternalCheck,
}

impl RoomStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::PreferenceWarning => "preference_warning",
            Self::ConditionsNotMet => "conditions_not_met",
            Self::Unavailable => "unavailable",
            Self::ExternalCheck => "external_check",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RoomCandidate {
    pub room: RoomId,
    pub evaluation: Evaluation,
}

#[derive(Clone, Debug)]
pub struct CheckedPlacement {
    pub placement: Placement,
    pub evaluation: Evaluation,
}

#[derive(Clone, Debug, Default)]
pub struct ScheduleAudit {
    pub checks: Vec<CheckedPlacement>,
    pub missing_rooms: Vec<CheckedPlacement>,
    /// Meetings with no time placement.
    pub unassigned: Vec<MeetingRef>,
    pub removed_locks: Vec<Placement>,
    pub pending_constraints: Vec<ConstraintId>,
}

impl ScheduleAudit {
    pub fn can_finalize(&self) -> bool {
        self.unassigned.is_empty()
            && self.missing_rooms.is_empty()
            && self.removed_locks.is_empty()
            && self
                .checks
                .iter()
                .all(|checked| checked.evaluation.can_assign())
    }
}

/// Indexes fixed term data once. The placement slice can change on each call, for manual
/// placement previews, a solver's partial schedule, or a read-only final check.
pub struct Evaluator<'a> {
    data: &'a TermData,
    sections: HashMap<SectionId, &'a Section>,
    rooms: HashMap<RoomId, &'a Room>,
    equipment: HashMap<EquipmentId, &'a EquipmentKind>,
    conditions: HashMap<Target, Vec<&'a Constraint>>,
    pending: HashMap<Target, Vec<ConstraintId>>,
    locks: HashMap<MeetingRef, &'a Placement>,
}

impl<'a> Evaluator<'a> {
    pub fn new(data: &'a TermData) -> Self {
        let mut conditions: HashMap<Target, Vec<&Constraint>> = HashMap::new();
        let mut pending: HashMap<Target, Vec<ConstraintId>> = HashMap::new();
        for condition in &data.constraints {
            if condition.is_active() {
                conditions
                    .entry(condition.target)
                    .or_default()
                    .push(condition);
            } else if condition.status == ConstraintStatus::Pending {
                pending
                    .entry(condition.target)
                    .or_default()
                    .push(condition.id);
            }
        }
        Self {
            data,
            sections: data
                .sections
                .iter()
                .map(|section| (section.id, section))
                .collect(),
            rooms: data.rooms.iter().map(|room| (room.id, room)).collect(),
            equipment: data.equipment.iter().map(|kind| (kind.id, kind)).collect(),
            conditions,
            pending,
            locks: data
                .placements
                .iter()
                .filter(|placement| placement.locked)
                .map(|placement| (placement.meeting, placement))
                .collect(),
        }
    }

    pub fn evaluate(&self, candidate: &Placement, placements: &[Placement]) -> Evaluation {
        let mut result = Evaluation::default();
        let current_lock = self.locks.get(&candidate.meeting).copied().or_else(|| {
            placements
                .iter()
                .find(|placement| placement.locked && placement.meeting == candidate.meeting)
        });
        if let Some(current) = current_lock {
            if (current.day, current.start, current.room)
                != (candidate.day, candidate.start, candidate.room)
            {
                result.hard(Reason::Locked(current.clone()));
            } else if !candidate.locked {
                result.hard(Reason::UnlockRequired(current.clone()));
            }
        }
        let Some(section) = self.sections.get(&candidate.meeting.section) else {
            result.hard(Reason::UnknownSection(candidate.meeting.section));
            return result;
        };
        result.pending_constraints.extend(
            targets(section, candidate.room)
                .filter_map(|target| self.pending.get(&target))
                .flatten()
                .copied(),
        );
        let time = match occupancy(&self.data.setup, section, candidate) {
            Ok(time) => Some(time),
            Err(error) => {
                result.hard(Reason::Occupancy(error));
                None
            }
        };
        if section.enrollment.provisional {
            result.report(
                Level::Notice,
                Reason::ProvisionalEnrollment(section.enrollment.count),
                0,
            );
        }
        match candidate.room {
            Some(id) => match self.rooms.get(&id) {
                Some(room) => self.check_room(room, section, candidate.day, &mut result),
                None => result.hard(Reason::UnknownRoom(id)),
            },
            None => result.hard(Reason::RoomRequired),
        }
        if let Some(time) = time {
            let span = section.split[usize::from(candidate.meeting.part)];
            if !self.data.setup.rules.block_may_cross_break {
                let breaks: Vec<_> = self
                    .data
                    .setup
                    .periods
                    .breaks_in_span(usize::from(candidate.start), usize::from(span))
                    .cloned()
                    .collect();
                if !breaks.is_empty() {
                    result.hard(Reason::CrossesBreak(breaks));
                }
            }
            self.check_conditions(candidate, section, time, &mut result);
            let slot = Prepared {
                placement: candidate,
                section,
                time,
            };
            for other in placements
                .iter()
                .filter(|other| other.meeting != candidate.meeting)
            {
                if let Some(other) = self.prepare(other) {
                    let _ = self.pair_issues(slot, other, |level, reason, weight| {
                        result.report(level, reason, weight);
                        ControlFlow::Continue(())
                    });
                }
            }
        }
        result
    }

    pub fn room_candidates(
        &self,
        candidate: &Placement,
        placements: &[Placement],
    ) -> Vec<RoomCandidate> {
        self.data
            .rooms
            .iter()
            .map(|room| {
                let placement = Placement {
                    room: Some(room.id),
                    ..candidate.clone()
                };
                RoomCandidate {
                    room: room.id,
                    evaluation: self.evaluate(&placement, placements),
                }
            })
            .collect()
    }

    pub fn audit(&self, placements: &[Placement]) -> ScheduleAudit {
        let mut present = HashSet::new();
        let mut duplicate = HashSet::new();
        for placement in placements {
            if !present.insert(placement.meeting) {
                duplicate.insert(placement.meeting);
            }
        }
        let mut checks = Vec::new();
        let mut missing_rooms = Vec::new();
        for placement in placements {
            let mut evaluation = self.evaluate(placement, placements);
            if duplicate.contains(&placement.meeting) {
                evaluation.hard(Reason::DuplicateMeeting);
            }
            let checked = CheckedPlacement {
                placement: placement.clone(),
                evaluation,
            };
            if placement.room.is_some() {
                checks.push(checked);
            } else {
                missing_rooms.push(checked);
            }
        }
        let unassigned = self
            .data
            .meetings()
            .filter(|meeting| !present.contains(meeting))
            .collect();
        let mut removed_locks: Vec<_> = self
            .locks
            .values()
            .filter(|placement| !present.contains(&placement.meeting))
            .map(|placement| (*placement).clone())
            .collect();
        removed_locks.sort_unstable_by_key(|placement| placement.meeting);
        let pending_constraints = self
            .data
            .constraints
            .iter()
            .filter(|constraint| constraint.status == ConstraintStatus::Pending)
            .map(|constraint| constraint.id)
            .collect();
        ScheduleAudit {
            checks,
            missing_rooms,
            unassigned,
            removed_locks,
            pending_constraints,
        }
    }

    fn check_room(&self, room: &Room, section: &Section, day: Weekday, result: &mut Evaluation) {
        if let Some(required) = section.room_kind
            && required != room.kind
        {
            result.hard(Reason::WrongRoomKind {
                required,
                actual: room.kind,
            });
        }
        if room.available_seats < section.enrollment.count {
            result.hard(Reason::NotEnoughSeats {
                needed: section.enrollment.count,
                available: room.available_seats,
            });
        }
        if room.needs_external_check() {
            result.report(
                Level::ExternalCheck,
                Reason::ExternalRoomSchedule(room.id),
                0,
            );
        }
        for need in &section.equipment {
            let Some(kind) = self.equipment.get(&need.equipment) else {
                result.hard(Reason::UnknownEquipment(need.equipment));
                continue;
            };
            let required = u64::from(need.quantity)
                * match kind.scope {
                    EquipmentScope::PerPerson => u64::from(section.enrollment.count),
                    EquipmentScope::PerRoom => 1,
                };
            let stock = room
                .equipment
                .iter()
                .find(|stock| stock.equipment == need.equipment);
            let available = stock.map_or(0, |stock| stock.available);
            if u64::from(available) < required {
                result.hard(Reason::NotEnoughEquipment {
                    equipment: need.equipment,
                    needed: required,
                    available,
                });
            }
            if required != 0
                && let Some(period) = stock.and_then(|stock| stock.inspection)
                && active_on(&self.data.setup, Some(period), day)
            {
                result.hard(Reason::EquipmentInspection {
                    equipment: need.equipment,
                    period,
                });
            }
        }
    }

    fn check_conditions(
        &self,
        candidate: &Placement,
        section: &Section,
        time: Occupancy,
        result: &mut Evaluation,
    ) {
        for target in targets(section, candidate.room) {
            let Some(conditions) = self.conditions.get(&target) else {
                continue;
            };
            for condition in conditions {
                let (Some(Rule::AvoidTimes { windows }), Some(strength)) =
                    (&condition.rule, condition.strength)
                else {
                    continue;
                };
                if active_on(&self.data.setup, condition.period, candidate.day)
                    && windows
                        .iter()
                        .filter_map(|window| {
                            Occupancy::from_window(*window, &self.data.setup.periods)
                        })
                        .any(|blocked| time.overlaps(blocked))
                {
                    result.apply_strength(
                        strength,
                        Reason::Constraint {
                            id: condition.id,
                            target,
                        },
                        condition.weight,
                    );
                }
            }
        }
    }

    pub(crate) fn prepare<'b>(&'b self, placement: &'b Placement) -> Option<Prepared<'b>> {
        let section = self.sections.get(&placement.meeting.section)?;
        let time = occupancy(&self.data.setup, section, placement).ok()?;
        Some(Prepared {
            placement,
            section,
            time,
        })
    }

    pub(crate) fn compatible(&self, first: Prepared<'_>, second: Prepared<'_>) -> bool {
        self.pair_issues(first, second, |level, _, _| {
            if level == Level::Hard {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_continue()
    }

    fn pair_issues(
        &self,
        first: Prepared<'_>,
        second: Prepared<'_>,
        mut report: impl FnMut(Level, Reason, u8) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        let overlaps = first.time.overlaps(second.time);
        let other = second.placement.meeting;
        let other_locked = || second.placement.locked || self.locks.contains_key(&other);
        let conflict = |resource| Reason::Overlap {
            other,
            other_locked: other_locked(),
            resource,
        };
        if let Some(room) = first
            .placement
            .room
            .filter(|room| second.placement.room == Some(*room))
        {
            if overlaps {
                report(Level::Hard, conflict(Resource::Room(room)), 0)?;
            } else {
                let needed = self.data.setup.rules.room_turnaround_minutes;
                if needed != 0 {
                    let available = first.time.room_gap(second.time);
                    if available < needed {
                        report(
                            Level::Hard,
                            Reason::RoomTurnaround {
                                other,
                                other_locked: other_locked(),
                                room,
                                needed,
                                available,
                            },
                            0,
                        )?;
                    }
                }
            }
        }
        if !overlaps {
            return ControlFlow::Continue(());
        }
        if first.section.id == second.section.id {
            report(
                Level::Hard,
                conflict(Resource::Section(first.section.id)),
                0,
            )?;
        }
        for &teacher in &first.section.instructors {
            if second.section.instructors.contains(&teacher) {
                report(Level::Hard, conflict(Resource::Instructor(teacher)), 0)?;
            }
        }
        for group in first.section.groups.iter().filter(|group| group.required) {
            if second
                .section
                .groups
                .iter()
                .any(|other| other.required && other.group == group.group)
            {
                let (level, weight) = match self.data.setup.rules.required_course_overlap {
                    Strength::Hard => (Level::Hard, 0),
                    Strength::Soft => (Level::Preference, 1),
                };
                report(
                    level,
                    conflict(Resource::RequiredGroup(group.group)),
                    weight,
                )?;
            }
        }
        ControlFlow::Continue(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Prepared<'a> {
    placement: &'a Placement,
    section: &'a Section,
    time: Occupancy,
}

fn targets(section: &Section, room: Option<RoomId>) -> impl Iterator<Item = Target> {
    std::iter::once(Target::Section(section.id))
        .chain(section.instructors.iter().copied().map(Target::Instructor))
        .chain(
            section
                .groups
                .iter()
                .map(|group| Target::Group(group.group)),
        )
        .chain(room.map(Target::Room))
}
