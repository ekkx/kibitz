//! Parsing of UCI output, kept separate from the process so it can be tested alone.

use kibitz_core::eval::Score;

/// `info depth 20 seldepth 28 multipv 1 score cp 34 ... pv e2e4 e7e5 ...`
#[derive(Debug, Clone, PartialEq)]
pub struct InfoLine {
    pub depth: u8,
    pub multipv: usize,
    pub score: Score,
    /// Moves in UCI notation.
    pub pv: Vec<String>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
}

/// Parse an `info` line. Lines carrying neither a score nor a PV
/// (`info depth 1 currmove ...` and friends) return `None`.
///
/// **Note**: `score` is always from the point of view of the side to move in that
/// position. Lines tagged `lowerbound` or `upperbound` are not trustworthy —
/// return `None` for those.
pub fn parse_info(line: &str) -> Option<InfoLine> {
    let mut tokens = line.split_whitespace();

    // The line must start with `info`. Anything else (`bestmove`, `readyok`,
    // engine banners) is not ours.
    if tokens.next()? != "info" {
        return None;
    }

    let mut depth: Option<u8> = None;
    let mut multipv: Option<usize> = None;
    let mut score: Option<Score> = None;
    let mut pv: Option<Vec<String>> = None;
    let mut nodes: Option<u64> = None;
    let mut nps: Option<u64> = None;

    while let Some(token) = tokens.next() {
        match token {
            // `info string ...` is free-form text; the rest of the line is not
            // structured, so bail out rather than misparse it.
            "string" => return None,
            "depth" => depth = parse_depth(tokens.next()?),
            "multipv" => multipv = Some(tokens.next()?.parse().ok()?),
            "nodes" => nodes = tokens.next()?.parse().ok(),
            "nps" => nps = tokens.next()?.parse().ok(),
            "score" => {
                score = match tokens.next()? {
                    "cp" => Some(Score::Cp(tokens.next()?.parse().ok()?)),
                    "mate" => Some(Score::Mate(tokens.next()?.parse().ok()?)),
                    // `score lowerbound`/`upperbound` without a kind is not
                    // something Stockfish emits, but treat it as untrustworthy.
                    _ => return None,
                };
            }
            // Aspiration-window fail high/low. The value is only a bound, not
            // the real evaluation, so the whole line is discarded.
            "lowerbound" | "upperbound" => return None,
            // `pv` is always last; everything after it belongs to the line.
            "pv" => {
                pv = Some(tokens.map(str::to_owned).collect());
                break;
            }
            _ => {}
        }
    }

    let pv = pv.filter(|moves| !moves.is_empty())?;

    Some(InfoLine {
        depth: depth?,
        multipv: multipv.unwrap_or(1),
        score: score?,
        pv,
        nodes,
        nps,
    })
}

/// Depths above `u8::MAX` are not something Stockfish reaches, but saturate
/// rather than throw the whole line away.
fn parse_depth(token: &str) -> Option<u8> {
    let raw: u32 = token.parse().ok()?;
    Some(raw.min(u8::MAX as u32) as u8)
}

/// `bestmove e2e4 ponder e7e5` -> `Some("e2e4")`. `bestmove (none)` -> `None`.
pub fn parse_bestmove(line: &str) -> Option<String> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "bestmove" {
        return None;
    }
    match tokens.next()? {
        "(none)" | "none" | "0000" => None,
        m => Some(m.to_owned()),
    }
}

/// Whether a line is a `bestmove` line at all, regardless of whether a move was
/// returned. The driver uses this as the end-of-search marker, and a terminal
/// position answers `bestmove (none)`, which [`parse_bestmove`] maps to `None`.
pub(crate) fn is_bestmove(line: &str) -> bool {
    line.split_whitespace().next() == Some("bestmove")
}

#[cfg(test)]
mod tests {
    use super::*;

    // All `info` samples below are verbatim output of Stockfish 18
    // (`/opt/homebrew/bin/stockfish`), except the two `lowerbound`/`upperbound`
    // lines, which were assembled in the documented UCI shape because the
    // aspiration windows did not fail on the positions sampled.

    #[test]
    fn parses_a_plain_cp_line() {
        let line = "info depth 12 seldepth 16 multipv 1 score cp 46 nodes 98598 nps 912944 \
                    hashfull 29 tbhits 0 time 108 pv e2e4 c7c5 g1f3 e7e6";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.depth, 12);
        assert_eq!(info.multipv, 1);
        assert_eq!(info.score, Score::Cp(46));
        assert_eq!(info.pv, vec!["e2e4", "c7c5", "g1f3", "e7e6"]);
        assert_eq!(info.nodes, Some(98598));
        assert_eq!(info.nps, Some(912944));
    }

    #[test]
    fn parses_negative_cp() {
        let line = "info depth 1 seldepth 2 multipv 1 score cp -1 nodes 71 nps 71000 \
                    hashfull 0 tbhits 0 time 1 pv e2e4";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.score, Score::Cp(-1));
        assert_eq!(info.pv, vec!["e2e4"]);
    }

    #[test]
    fn parses_multipv_rank() {
        let line = "info depth 12 seldepth 15 multipv 3 score cp 30 nodes 98598 nps 912944 \
                    hashfull 29 tbhits 0 time 108 pv g1f3 d7d5 d2d4";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.multipv, 3);
        assert_eq!(info.score, Score::Cp(30));
    }

    #[test]
    fn parses_mate_score() {
        let line = "info depth 14 seldepth 2 multipv 1 score mate 1 nodes 101394 nps 4055760 \
                    hashfull 34 tbhits 0 time 25 pv a1a8";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.score, Score::Mate(1));
        assert_eq!(info.pv, vec!["a1a8"]);
    }

    #[test]
    fn parses_getting_mated() {
        let line = "info depth 20 seldepth 4 multipv 1 score mate -2 nodes 4321 nps 100000 \
                    hashfull 0 tbhits 0 time 43 pv h7h8 g1g2 d8d2";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.score, Score::Mate(-2));
    }

    #[test]
    fn rejects_lowerbound() {
        let line = "info depth 21 seldepth 30 multipv 1 score cp 128 lowerbound nodes 12345678 \
                    nps 1000000 hashfull 400 tbhits 0 time 12 pv d2d4";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_upperbound() {
        let line = "info depth 21 seldepth 30 multipv 1 score cp -64 upperbound nodes 12345678 \
                    nps 1000000 hashfull 400 tbhits 0 time 12 pv d2d4";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_currmove_lines() {
        let line = "info depth 24 currmove b1c3 currmovenumber 3";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_lines_without_a_pv() {
        let line = "info depth 12 seldepth 16 multipv 1 score cp 46 nodes 98598 nps 912944 \
                    hashfull 29 tbhits 0 time 108";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_lines_without_a_score() {
        let line = "info depth 12 seldepth 16 multipv 1 nodes 98598 nps 912944 time 108 pv e2e4";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_empty_pv() {
        let line = "info depth 12 multipv 1 score cp 46 pv";
        assert_eq!(parse_info(line), None);
    }

    #[test]
    fn rejects_info_string_lines() {
        assert_eq!(
            parse_info("info string NNUE evaluation using nn-c288c895ea92.nnue"),
            None
        );
        assert_eq!(parse_info("info string Using 2 threads"), None);
    }

    #[test]
    fn rejects_non_info_lines() {
        assert_eq!(parse_info("bestmove e2e4 ponder c7c5"), None);
        assert_eq!(parse_info("readyok"), None);
        assert_eq!(parse_info("uciok"), None);
        assert_eq!(parse_info(""), None);
        assert_eq!(parse_info("   "), None);
    }

    #[test]
    fn tolerates_a_missing_multipv_field() {
        // Stockfish always prints `multipv`, but other engines omit it when
        // MultiPV is 1.
        let line = "info depth 8 score cp 22 pv d2d4 d7d5";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.multipv, 1);
    }

    #[test]
    fn parses_bestmove_with_ponder() {
        assert_eq!(
            parse_bestmove("bestmove e2e4 ponder c7c5"),
            Some("e2e4".to_owned())
        );
    }

    #[test]
    fn parses_bestmove_without_ponder() {
        assert_eq!(parse_bestmove("bestmove a1a8"), Some("a1a8".to_owned()));
    }

    #[test]
    fn parses_bestmove_none() {
        assert_eq!(parse_bestmove("bestmove (none)"), None);
        assert_eq!(parse_bestmove("bestmove 0000"), None);
    }

    #[test]
    fn rejects_non_bestmove_lines() {
        assert_eq!(parse_bestmove("info depth 1 score cp 0 pv e2e4"), None);
        assert_eq!(parse_bestmove(""), None);
    }

    #[test]
    fn recognises_bestmove_lines() {
        assert!(is_bestmove("bestmove (none)"));
        assert!(is_bestmove("bestmove e2e4 ponder c7c5"));
        assert!(!is_bestmove("info depth 1 score cp 0 pv e2e4"));
    }
}
