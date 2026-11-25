use auler_domain::model::{DateRange, Placement, Section, TermSetup, TimeWindow};
use auler_domain::time::{ClockTime, PeriodTable, Weekday};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occupancy {
    pub day: Weekday,
    pub start: ClockTime,
    pub end: ClockTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccupancyError {
    DayOutsideTerm,
    UnknownPart,
    PeriodSpanOutsideDay,
}

impl Occupancy {
    /// 시작 포함, 종료 제외. 한 수업의 끝과 다음 수업의 시작이 같아도 된다.
    pub fn overlaps(self, other: Self) -> bool {
        self.day == other.day && self.start < other.end && other.start < self.end
    }

    pub(crate) fn room_gap(self, other: Self) -> u16 {
        const DAY: i32 = 24 * 60;
        const WEEK: i32 = 7 * DAY;
        let offset = |slot: Self| slot.day.index() as i32 * DAY;
        let gap = |first: Self, next: Self| {
            (offset(next) + i32::from(next.start.minutes())
                - offset(first)
                - i32::from(first.end.minutes()))
            .rem_euclid(WEEK)
        };
        gap(self, other).min(gap(other, self)) as u16
    }

    pub(crate) fn from_window(window: TimeWindow, table: &PeriodTable) -> Option<Self> {
        let (start, end) = match window {
            TimeWindow::Periods { first, last, .. } => table.span_time(
                usize::from(first),
                usize::from(last.checked_sub(first)?) + 1,
            )?,
            TimeWindow::Clock { start, end, .. } if start < end => (start, end),
            _ => return None,
        };
        Some(Self {
            day: window.day(),
            start,
            end,
        })
    }
}

pub fn occupancy(
    setup: &TermSetup,
    section: &Section,
    placement: &Placement,
) -> Result<Occupancy, OccupancyError> {
    if !setup.days.contains(&placement.day) {
        return Err(OccupancyError::DayOutsideTerm);
    }
    let span = section
        .split
        .get(usize::from(placement.meeting.part))
        .ok_or(OccupancyError::UnknownPart)?;
    let (start, end) = setup
        .periods
        .span_time(usize::from(placement.start), usize::from(*span))
        .ok_or(OccupancyError::PeriodSpanOutsideDay)?;
    Ok(Occupancy {
        day: placement.day,
        start,
        end,
    })
}

pub(crate) fn active_on(setup: &TermSetup, range: Option<DateRange>, day: Weekday) -> bool {
    let from = setup
        .start
        .into_iter()
        .chain(range.map(|range| range.from))
        .max();
    let until = setup
        .end
        .into_iter()
        .chain(range.map(|range| range.until))
        .min();
    match (from, until) {
        (Some(from), Some(until)) if from <= until => {
            let first = day.index() as i32 - from.weekday().index() as i32;
            until.ordinal() - from.ordinal() >= first.rem_euclid(7)
        }
        (Some(_), Some(_)) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use auler_domain::input::InputDocument;
    use auler_domain::resolve::resolve;
    use auler_domain::time::Date;

    #[test]
    fn repeating_day_must_occur_in_the_intersection() {
        let mut input = InputDocument::new(2027, "1");
        input.setup = Some(auler_samples::reference::setup());
        let setup = resolve(&input).unwrap().setup;
        let day = Date::parse("2027-04-01").unwrap();
        let range = Some(DateRange {
            from: day,
            until: day,
        });
        assert!(active_on(&setup, range, Weekday::Thu));
        assert!(!active_on(&setup, range, Weekday::Fri));
    }
}
