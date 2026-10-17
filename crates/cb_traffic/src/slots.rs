//! Deterministic routing budget (ADR-0067): requests queue FIFO into 8 search slots; each tick
//! every busy slot settles up to its share of the budget, and finished results are delivered
//! in slot order. Results depend only on the requests and the graph — never on the budget
//! split or thread count (slots are independent and their results are merged in order).

use std::collections::VecDeque;

use crate::cache::Key;
use crate::graph::Graph;
use crate::router::{Costs, Landmarks, Route, Search};

/// Search slots.
pub const SLOTS: usize = 8;

/// A routing request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// Caller's ticket.
    pub ticket: u64,
    /// What to route.
    pub key: Key,
}

/// The slot scheduler.
#[derive(Clone, Debug, Default)]
pub struct Scheduler {
    queue: VecDeque<Request>,
    slots: Vec<Option<(Request, Search)>>,
}

impl Scheduler {
    /// An idle scheduler.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            slots: vec![None; SLOTS],
        }
    }

    /// Queues a request.
    pub fn push(&mut self, r: Request) {
        self.queue.push_back(r);
    }

    /// Requests waiting or in progress.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len() + self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// One tick: fill free slots from the queue, advance each by `budget_per_slot` settled
    /// links, and return the finished results in slot order.
    pub fn step(
        &mut self,
        g: &Graph,
        lm: &Landmarks,
        costs: &Costs,
        budget_per_slot: u64,
    ) -> Vec<(Request, Option<Route>)> {
        if self.slots.len() != SLOTS {
            self.slots = vec![None; SLOTS];
        }
        for slot in &mut self.slots {
            if slot.is_none()
                && let Some(r) = self.queue.pop_front()
            {
                let s = Search::new(g, lm, costs, r.key.origin, r.key.dest, r.key.variant, true);
                *slot = Some((r, s));
            }
        }
        let mut out = Vec::new();
        for slot in &mut self.slots {
            let finished = slot.as_mut().and_then(|(r, s)| {
                s.advance(g, lm, costs, budget_per_slot)
                    .map(|res| (*r, res))
            });
            if let Some(f) = finished {
                out.push(f);
                *slot = None;
            }
        }
        out
    }
}
