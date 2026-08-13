//! Arbitration for the one Stockfish process.
//!
//! `docs/DESIGN.md` §10 makes the engine driver "latest only": a new search stops
//! the one in flight. That is the right policy for a user rapidly trying moves on
//! the board, and the wrong one for a whole-game sweep — a sweep is a job of
//! minutes, and a single `POST /analyze` (which the frontend issues on every move
//! the user selects) would destroy it.
//!
//! The server owns the engine, so the server arbitrates. Requests are sorted into
//! two lanes:
//!
//! | | May start while … | Cancelled by … |
//! |---|---|---|
//! | [`Lane::Interactive`] | no sweep search is at the engine | a newer interactive search |
//! | [`Lane::Sweep`] | nothing at all is at the engine | nothing |
//!
//! Two consequences follow, and they are the whole point:
//!
//! 1. **Latest-only survives inside the interactive lane.** Interactive requests
//!    are deliberately *not* serialised against each other — several may sit at
//!    the driver at once, and the driver stops the older ones exactly as before.
//!    Rapidly clicking through moves still abandons stale searches, and the older
//!    request still answers `409 cancelled`.
//! 2. **A sweep can never be cancelled.** While a sweep search is at the engine
//!    nothing else is, so the driver has nothing to supersede it with.
//!
//! The gate is taken around a *single* engine search, not around a whole sweep, so
//! an interactive request waits at most one position — and, because the pipeline
//! consults its cache before asking for the gate, a position the sweep has already
//! done costs no wait at all.
//!
//! Arbitration is per **engine**, not per session: there is one Stockfish process
//! behind every session, so a per-session queue would not stop two sessions from
//! cancelling each other.

use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Which kind of work is asking for the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// A user action on the board — `POST /analyze`. Subject to latest-only.
    Interactive,
    /// One position of a whole-game sweep — `POST /analyze-game`. Never cancelled.
    Sweep,
}

/// Admission control in front of [`kibitz_engine::Engine`].
#[derive(Debug, Default)]
pub struct EngineGate {
    state: Mutex<State>,
    /// Woken every time a holder leaves, so waiters re-check.
    wake: Notify,
}

#[derive(Debug, Default)]
struct State {
    /// Interactive searches currently at the engine. More than one is allowed on
    /// purpose — that is what keeps latest-only working.
    interactive: usize,
    /// Whether a sweep search is currently at the engine.
    sweep: bool,
}

impl State {
    fn try_enter(&mut self, lane: Lane) -> bool {
        match lane {
            Lane::Interactive if !self.sweep => {
                self.interactive += 1;
                true
            }
            Lane::Sweep if !self.sweep && self.interactive == 0 => {
                self.sweep = true;
                true
            }
            _ => false,
        }
    }

    fn leave(&mut self, lane: Lane) {
        match lane {
            Lane::Interactive => self.interactive = self.interactive.saturating_sub(1),
            Lane::Sweep => self.sweep = false,
        }
    }
}

impl EngineGate {
    pub fn new() -> Arc<EngineGate> {
        Arc::new(EngineGate::default())
    }

    /// Wait until `lane` may use the engine. The permit lasts until the returned
    /// guard is dropped, which must be as soon as the single search is done.
    pub async fn acquire(self: &Arc<Self>, lane: Lane) -> Permit {
        loop {
            // Registered before the check, or a release landing between the two
            // would be a lost wakeup and this would sleep forever.
            let wake = self.wake.notified();
            tokio::pin!(wake);
            wake.as_mut().enable();

            if self.state.lock().expect("engine gate poisoned").try_enter(lane) {
                return Permit {
                    gate: Arc::clone(self),
                    lane,
                };
            }

            wake.await;
        }
    }
}

/// Permission to run one search. Releases on drop, including on cancellation of
/// the future that holds it.
#[derive(Debug)]
pub struct Permit {
    gate: Arc<EngineGate>,
    lane: Lane,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.gate
            .state
            .lock()
            .expect("engine gate poisoned")
            .leave(self.lane);
        // `notify_waiters` and not `notify_one`: the waiters are not
        // interchangeable, since a sweep and an interactive request are admitted
        // under different conditions, so every one of them has to re-check.
        self.gate.wake.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn interactive_requests_do_not_block_each_other() {
        // Latest-only lives in the engine driver, and it only works if the gate
        // lets more than one interactive search reach it.
        let gate = EngineGate::new();
        let first = gate.acquire(Lane::Interactive).await;
        let second = tokio::time::timeout(
            Duration::from_millis(100),
            gate.acquire(Lane::Interactive),
        )
        .await
        .expect("a second interactive request must not wait");
        drop((first, second));
    }

    #[tokio::test]
    async fn a_sweep_waits_for_interactive_work_to_finish() {
        let gate = EngineGate::new();
        let interactive = gate.acquire(Lane::Interactive).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), gate.acquire(Lane::Sweep))
                .await
                .is_err(),
            "a sweep must not join an interactive search at the engine"
        );
        drop(interactive);
        tokio::time::timeout(Duration::from_millis(100), gate.acquire(Lane::Sweep))
            .await
            .expect("the sweep proceeds once the engine is free");
    }

    /// The defect this module exists for: a sweep position holds the engine
    /// exclusively, so nothing can arrive to cancel it, and the interactive
    /// request waits for that one position instead of killing the sweep.
    #[tokio::test]
    async fn an_interactive_request_waits_out_one_sweep_position() {
        let gate = EngineGate::new();
        let position = gate.acquire(Lane::Sweep).await;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                gate.acquire(Lane::Interactive)
            )
            .await
            .is_err(),
            "nothing may sit at the engine beside a sweep search"
        );

        // The sweep finishes that position and the interactive request goes
        // through — without the sweep having been disturbed.
        drop(position);
        tokio::time::timeout(
            Duration::from_millis(100),
            gate.acquire(Lane::Interactive),
        )
        .await
        .expect("the interactive request runs between two sweep positions");
    }

    #[tokio::test]
    async fn two_sweeps_take_turns() {
        let gate = EngineGate::new();
        let first = gate.acquire(Lane::Sweep).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), gate.acquire(Lane::Sweep))
                .await
                .is_err()
        );
        drop(first);
        tokio::time::timeout(Duration::from_millis(100), gate.acquire(Lane::Sweep))
            .await
            .expect("the second sweep proceeds");
    }

    /// A waiter parked before the holder leaves must still be woken.
    #[tokio::test]
    async fn a_waiter_is_woken_when_the_holder_leaves() {
        let gate = EngineGate::new();
        let held = gate.acquire(Lane::Sweep).await;

        let waiting = tokio::spawn({
            let gate = Arc::clone(&gate);
            async move { gate.acquire(Lane::Interactive).await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());

        drop(held);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("the waiter was never woken")
            .expect("task panicked");
    }
}
