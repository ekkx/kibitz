//! Turning a stream of `info` lines into *completed depths*.
//!
//! Stockfish narrates its search: at MultiPV 5 a single `go depth 12` prints
//! sixty-odd usable `info` lines, five per iteration. A caller that wants to show
//! the search as it happens wants those grouped, because one iteration is one
//! answer — "at depth 8 the ranking is this" — and the five lines that make it up
//! are not five answers.
//!
//! So nothing is reported per line. Lines are buffered under the depth they
//! belong to, and a depth is handed over only once it is **closed**: either the
//! engine has moved on to a deeper iteration, or the search has ended. That is
//! also the only definition that works in practice — "a full MultiPV set" cannot
//! mean "`multipv` lines", because Stockfish caps MultiPV at the number of legal
//! moves, so a position with three replies never prints a fifth line at any
//! depth.

use std::collections::BTreeMap;

use crate::uci::InfoLine;

/// One finished iteration of a search.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompletedDepth {
    pub depth: u8,
    /// One entry per MultiPV rank, ascending — the same order and the same shape
    /// as [`crate::driver::SearchOutcome::infos`], so a partial and the final
    /// result are converted to candidates by the identical code.
    pub infos: Vec<InfoLine>,
}

/// Buffers `info` lines by depth and closes a depth when the next one opens.
#[derive(Debug, Default)]
pub(crate) struct DepthTracker {
    /// The depth currently being filled, or `None` before the first usable line.
    depth: Option<u8>,
    /// Best line seen per MultiPV rank *within* [`Self::depth`]. A `BTreeMap`
    /// because rank order is the output order, and rank 1 has to come first.
    lines: BTreeMap<usize, InfoLine>,
}

impl DepthTracker {
    pub(crate) fn new() -> DepthTracker {
        DepthTracker::default()
    }

    /// Take one line. Returns the depth this line just *closed* — never the depth
    /// the line itself belongs to, which is still being filled.
    pub(crate) fn push(&mut self, info: InfoLine) -> Option<CompletedDepth> {
        match self.depth {
            // Backwards. Stockfish emits a stale line or two after `stop`, and a
            // set for depth 8 arriving after depth 9 was already reported would
            // walk the board's arrows backwards for no reason. Depth reported to
            // a caller is monotone by construction.
            Some(current) if info.depth < current => None,
            Some(current) if info.depth == current => {
                // Last writer wins within an iteration: an aspiration re-search
                // re-prints a rank at the same depth, and the later line is the
                // one that survived the window.
                self.lines.insert(info.multipv, info);
                None
            }
            // A deeper line: whatever was buffered is now final.
            Some(_) => {
                let closed = self.take();
                self.depth = Some(info.depth);
                self.lines.insert(info.multipv, info);
                closed
            }
            None => {
                self.depth = Some(info.depth);
                self.lines.insert(info.multipv, info);
                None
            }
        }
    }

    /// Close the depth still being filled, because the search has ended.
    ///
    /// The driver calls this **only for a search that ran to `bestmove`**, never
    /// for one that was superseded. A stopped search is cut off mid-iteration, so
    /// its buffer holds however many ranks happened to have been printed — a set
    /// that was never true of any depth. A completed search's last iteration, by
    /// contrast, is finished, and reporting it is worth real time: it is the same
    /// ranking the final result carries, delivered as soon as the engine has it
    /// rather than after whatever else the caller still has to do.
    pub(crate) fn flush(&mut self) -> Option<CompletedDepth> {
        let closed = self.take();
        self.depth = None;
        closed
    }

    fn take(&mut self) -> Option<CompletedDepth> {
        let depth = self.depth?;
        let infos: Vec<InfoLine> = std::mem::take(&mut self.lines).into_values().collect();
        if infos.is_empty() {
            return None;
        }
        Some(CompletedDepth { depth, infos })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uci::parse_info;

    /// `info` at one depth and rank, with a PV whose first move names it, so the
    /// assertions below can read a set back as the moves it ranked.
    fn info(depth: u8, multipv: usize, cp: i32, mv: &str) -> InfoLine {
        parse_info(&format!(
            "info depth {depth} seldepth {depth} multipv {multipv} score cp {cp} \
             nodes 1000 nps 100000 time 1 pv {mv}"
        ))
        .expect("valid info line")
    }

    fn moves(closed: &CompletedDepth) -> Vec<&str> {
        closed.infos.iter().map(|i| i.pv[0].as_str()).collect()
    }

    /// MultiPV 1: one line per depth, and each one closes the previous depth.
    #[test]
    fn multipv_1_closes_one_depth_per_line() {
        let mut tracker = DepthTracker::new();

        assert_eq!(tracker.push(info(1, 1, 10, "e2e4")), None, "nothing is closed yet");

        let closed = tracker.push(info(2, 1, 20, "d2d4")).expect("depth 1 closed");
        assert_eq!(closed.depth, 1);
        assert_eq!(moves(&closed), ["e2e4"]);

        let closed = tracker.push(info(3, 1, 30, "g1f3")).expect("depth 2 closed");
        assert_eq!(closed.depth, 2);
        assert_eq!(moves(&closed), ["d2d4"]);

        let closed = tracker.flush().expect("depth 3 closed by the end of the search");
        assert_eq!(closed.depth, 3);
        assert_eq!(moves(&closed), ["g1f3"]);
        assert_eq!(tracker.flush(), None, "and there is nothing left");
    }

    /// MultiPV 5: five lines make one event, not five.
    #[test]
    fn multipv_5_reports_one_event_per_depth() {
        let mut tracker = DepthTracker::new();
        let ranked = ["e2e4", "d2d4", "g1f3", "c2c4", "b1c3"];

        for (rank, mv) in ranked.iter().enumerate() {
            let closed = tracker.push(info(8, rank + 1, 50 - rank as i32 * 10, mv));
            assert_eq!(closed, None, "rank {rank} must not fire on its own");
        }

        // The first line of depth 9 is what closes depth 8, and it closes it whole.
        let closed = tracker.push(info(9, 1, 55, "e2e4")).expect("depth 8 closed");
        assert_eq!(closed.depth, 8);
        assert_eq!(moves(&closed), ranked, "rank order is the output order");
    }

    /// A short set is a real set: MultiPV is capped at the number of legal moves,
    /// so "complete" can never mean "as many lines as the width asked for".
    #[test]
    fn a_depth_with_fewer_lines_than_the_multipv_width_still_closes() {
        let mut tracker = DepthTracker::new();
        tracker.push(info(6, 1, 10, "h7h8"));
        tracker.push(info(6, 2, -50, "h7g8"));

        let closed = tracker.push(info(7, 1, 12, "h7h8")).expect("depth 6 closed");
        assert_eq!(moves(&closed), ["h7h8", "h7g8"], "two of a possible five");
    }

    /// An aspiration re-search reprints a rank at the same depth. The later line
    /// is the one that survived the window, so it replaces the earlier one — and
    /// it still does not close anything.
    #[test]
    fn a_rank_reprinted_at_the_same_depth_replaces_the_earlier_line() {
        let mut tracker = DepthTracker::new();
        tracker.push(info(10, 1, 30, "e2e4"));
        assert_eq!(tracker.push(info(10, 1, 45, "d2d4")), None);

        let closed = tracker.push(info(11, 1, 44, "d2d4")).expect("depth 10 closed");
        assert_eq!(moves(&closed), ["d2d4"]);
        assert_eq!(closed.infos[0].score, kibitz_core::eval::Score::Cp(45));
    }

    /// Stale lines after `stop` must not make the reported depth go backwards.
    #[test]
    fn a_shallower_line_after_a_deeper_one_is_dropped() {
        let mut tracker = DepthTracker::new();
        tracker.push(info(9, 1, 30, "e2e4"));
        let closed = tracker.push(info(10, 1, 33, "e2e4")).expect("depth 9 closed");
        assert_eq!(closed.depth, 9);

        // Two stragglers from the abandoned iteration.
        assert_eq!(tracker.push(info(9, 2, 10, "d2d4")), None);
        assert_eq!(tracker.push(info(8, 1, 5, "g1f3")), None);

        let closed = tracker.flush().expect("depth 10 is still what is buffered");
        assert_eq!(closed.depth, 10);
        assert_eq!(moves(&closed), ["e2e4"], "no straggler joined the set");
    }

    /// A search that produced nothing at all closes nothing — the flush must not
    /// invent an empty depth for a caller to render.
    #[test]
    fn a_tracker_that_saw_no_lines_closes_nothing() {
        let mut tracker = DepthTracker::new();
        assert_eq!(tracker.flush(), None);
        assert_eq!(tracker.flush(), None);
    }
}
