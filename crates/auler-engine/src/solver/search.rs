use crate::Evaluator;
use crate::evaluation::Prepared;

use super::{Budget, Progress, SolvePhase, SolveStatus};

#[derive(Clone, Copy)]
struct Removal {
    domain: usize,
    position: usize,
    choice: usize,
}

pub(super) struct Search<'a> {
    evaluator: &'a Evaluator<'a>,
    candidates: &'a [Vec<Prepared<'a>>],
    budget: &'a Budget<'a>,
    progress: &'a mut dyn FnMut(Progress),
    fixed: usize,
    remaining: Vec<Vec<usize>>,
    assigned: Vec<Option<usize>>,
    best: Vec<Option<usize>>,
    best_count: usize,
    trail: Vec<Removal>,
}

impl<'a> Search<'a> {
    pub fn new(
        evaluator: &'a Evaluator<'a>,
        candidates: &'a [Vec<Prepared<'a>>],
        budget: &'a Budget<'a>,
        progress: &'a mut dyn FnMut(Progress),
        fixed: usize,
    ) -> Self {
        Self {
            evaluator,
            candidates,
            budget,
            progress,
            fixed,
            remaining: candidates
                .iter()
                .map(|choices| (0..choices.len()).collect())
                .collect(),
            assigned: vec![None; candidates.len()],
            best: vec![None; candidates.len()],
            best_count: 0,
            trail: Vec::new(),
        }
    }

    pub fn run(mut self) -> (SolveStatus, Vec<Option<usize>>) {
        let status = self.visit(0);
        (status, self.best)
    }

    fn visit(&mut self, depth: usize) -> SolveStatus {
        if depth == self.assigned.len() {
            return SolveStatus::Solved;
        }
        if let Some(stop) = self.budget.stop() {
            return stop;
        }
        let Some(domain) = (0..self.assigned.len())
            .filter(|&idx| self.assigned[idx].is_none())
            .min_by_key(|&idx| (self.remaining[idx].len(), idx))
        else {
            return SolveStatus::Solved;
        };
        let mut choices = self.remaining[domain].clone();
        choices.sort_unstable();
        for choice in choices {
            if let Some(stop) = self.budget.stop() {
                return stop;
            }
            self.assigned[domain] = Some(choice);
            let assigned = depth + 1;
            if assigned > self.best_count {
                self.best_count = assigned;
                self.best.clone_from(&self.assigned);
                if assigned.is_multiple_of(8) || assigned == self.assigned.len() {
                    (self.progress)(
                        self.budget
                            .progress(SolvePhase::Searching, self.fixed + assigned),
                    );
                }
            }
            let mark = self.trail.len();
            let status = match self.forward_check(self.candidates[domain][choice]) {
                Ok(true) => self.visit(assigned),
                Ok(false) => SolveStatus::NoSolution,
                Err(status) => status,
            };
            self.restore(mark);
            self.assigned[domain] = None;
            if status != SolveStatus::NoSolution {
                return status;
            }
        }
        SolveStatus::NoSolution
    }

    /// Remove conflicting options from the remaining domains and roll them back on backtracking.
    fn forward_check(&mut self, placed: Prepared<'a>) -> Result<bool, SolveStatus> {
        for domain in 0..self.assigned.len() {
            if self.assigned[domain].is_some() {
                continue;
            }
            if let Some(stop) = self.budget.stop() {
                return Err(stop);
            }
            let options = &mut self.remaining[domain];
            let mut position = 0;
            while position < options.len() {
                let choice = options[position];
                if self
                    .evaluator
                    .compatible(placed, self.candidates[domain][choice])
                {
                    position += 1;
                } else {
                    options.swap_remove(position);
                    self.trail.push(Removal {
                        domain,
                        position,
                        choice,
                    });
                }
            }
            if options.is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn restore(&mut self, mark: usize) {
        while self.trail.len() > mark {
            let entry = self.trail.pop().expect("trail contains a removal");
            let options = &mut self.remaining[entry.domain];
            options.push(entry.choice);
            let last = options.len() - 1;
            options.swap(entry.position, last);
        }
    }
}
