use auler_domain::model::TermData;
use auler_engine::{Evaluator, Level, Reason, ScheduleAudit};

/// The required checks and items that need acknowledgment before a timetable is finalized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinalizationChecks {
    pub sections: usize,
    pub placed: usize,
    pub locked: usize,
    pub unassigned: usize,
    pub hard_conflicts: usize,
    pub external_checks: usize,
    pub missing_locks: usize,
    pub pending_constraints: usize,
    pub preference_warnings: usize,
    pub provisional_placements: usize,
}

impl FinalizationChecks {
    pub fn inspect(data: &TermData) -> Self {
        Self::from_audit(data, &Evaluator::new(data).audit(&data.placements))
    }

    pub fn from_audit(data: &TermData, audit: &ScheduleAudit) -> Self {
        let mut checks = Self {
            sections: data.sections.len(),
            placed: data
                .placements
                .iter()
                .filter(|placement| placement.room.is_some())
                .count(),
            locked: data
                .placements
                .iter()
                .filter(|placement| placement.locked)
                .count(),
            unassigned: audit.unassigned.len() + audit.missing_rooms.len(),
            missing_locks: audit.removed_locks.len(),
            pending_constraints: audit.pending_constraints.len(),
            ..Self::default()
        };
        for row in &audit.checks {
            let mut outside = false;
            for finding in &row.evaluation.findings {
                let first_in_pair = match finding.reason {
                    Reason::Overlap { other, .. } | Reason::RoomTurnaround { other, .. } => {
                        row.placement.meeting <= other
                    }
                    _ => true,
                };
                match finding.level {
                    Level::Hard => checks.hard_conflicts += usize::from(first_in_pair),
                    Level::ExternalCheck => outside = true,
                    Level::Preference => checks.preference_warnings += usize::from(first_in_pair),
                    Level::Notice if matches!(finding.reason, Reason::ProvisionalEnrollment(_)) => {
                        checks.provisional_placements += 1;
                    }
                    _ => {}
                }
            }
            checks.external_checks += usize::from(outside);
        }
        checks
    }

    pub fn required_passed(self) -> bool {
        self.sections > 0
            && self.unassigned == 0
            && self.hard_conflicts == 0
            && self.external_checks == 0
            && self.missing_locks == 0
    }

    pub fn needs_acknowledgment(self) -> bool {
        self.pending_constraints + self.preference_warnings + self.provisional_placements > 0
    }
}
