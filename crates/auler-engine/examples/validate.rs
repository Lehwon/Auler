use std::hint::black_box;
use std::time::{Duration, Instant};

use auler_domain::model::{Placement, TermData};
use auler_domain::resolve::resolve;
use auler_engine::Evaluator;

const ITERATIONS: usize = 300;

fn timed(mut measure: impl FnMut()) -> (Duration, Duration, Duration) {
    let mut times: Vec<_> = (0..ITERATIONS)
        .map(|_| {
            let start = Instant::now();
            measure();
            start.elapsed()
        })
        .collect();
    times.sort_unstable();
    (
        times[ITERATIONS / 2],
        times[ITERATIONS * 95 / 100],
        times[ITERATIONS - 1],
    )
}

fn run(name: &str, data: TermData) {
    let board: Vec<_> =
        data.meetings()
            .enumerate()
            .map(|(idx, meeting)| {
                data.placement(meeting)
                    .cloned()
                    .unwrap_or_else(|| Placement {
                        meeting,
                        day: data.setup.days[idx % data.setup.days.len()],
                        start: (idx / data.setup.days.len()
                            % data.setup.periods.len().saturating_sub(
                                usize::from(data.meeting_span(meeting).unwrap()) - 1,
                            )) as u8,
                        room: Some(data.rooms[idx % data.rooms.len()].id),
                        locked: false,
                    })
            })
            .collect();
    let engine = Evaluator::new(&data);
    let candidate = board.iter().find(|placement| !placement.locked).unwrap();
    let room_candidates = timed(|| {
        black_box(engine.room_candidates(black_box(candidate), black_box(&board)));
    });
    let audit = timed(|| {
        black_box(engine.audit(black_box(&board)));
    });
    println!(
        "{name}: {} sections, {} meetings, {} rooms, {ITERATIONS} iterations",
        data.sections.len(),
        board.len(),
        data.rooms.len()
    );
    for (label, (median, p95, max)) in [("room candidates", room_candidates), ("audit", audit)] {
        println!("{label}: median {median:?}, p95 {p95:?}, max {max:?}");
    }
}

fn main() {
    run("reference", resolve(&auler_samples::reference()).unwrap());
    run("stress", resolve(&auler_samples::stress()).unwrap());
}
