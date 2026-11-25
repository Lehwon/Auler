use std::sync::atomic::AtomicBool;

use auler_domain::input::InputDocument;
use auler_domain::resolve::resolve;
use auler_engine::{Evaluator, LocalSolver, SolveOptions, Solver};

fn run(name: &str, input: InputDocument) {
    let data = resolve(&input).expect("sample data is valid");
    let result = LocalSolver.solve(
        &data,
        SolveOptions::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    let audit = Evaluator::new(&data).audit(&result.placements);
    println!(
        "{name}: {:?}, {:?}, {}/{} placements, {} locked, cost {:?}, finalized eligible {}",
        result.status,
        result.elapsed,
        result.placements.len(),
        data.meetings().count(),
        result.placements.iter().filter(|p| p.locked).count(),
        result.preferences.cost,
        audit.can_finalize(),
    );
    for reason in &result.capacity_shortfalls {
        println!("{reason:?}");
    }
    for missing in result.unassigned.iter().take(3) {
        let reasons: Vec<_> = missing
            .rejections
            .iter()
            .map(|item| item.example.reason.code())
            .collect();
        println!("{:?}: {:?} {:?}", missing.meeting, missing.cause, reasons);
    }
}

fn main() {
    run("reference", auler_samples::reference());
    run("stress", auler_samples::stress());
}
