//! Offline ECO opening names.
//!
//! The five ECO volumes from `lichess-org/chess-openings` are embedded in the
//! binary (see `data/README.md`) and replayed at first use into a position →
//! opening map, so a position can be *named* without the network. This exists
//! because the Lichess Opening Explorer has answered 401 to everyone since
//! 2026-02-23 (lichess-org/lila#19610), which took opening names with it.
//!
//! **Scope: names only.** A hit here says "this position has a name", which is a
//! different question from "is this move still theory?". The latter needs the
//! Explorer's game counts — how many masters actually played the move — which an
//! ECO table does not carry. So nothing in this module ever produces
//! [`kibitz_core::Classification::Book`]; that verdict stays with
//! [`crate::BookVerdict::InBook`], and a name is display-only metadata.
//!
//! The index key is the **EPD** (the FEN without the halfmove clock and fullmove
//! number, exactly what [`kibitz_core::normalize_fen`] produces), so a position
//! reached by a different move order resolves to the same opening.

use std::collections::HashMap;
use std::collections::hash_map::Entry as MapEntry;
use std::sync::LazyLock;

use kibitz_core::types::OpeningInfo;
use shakmaty::{Chess, EnPassantMode, Position, fen::Fen, san::San};

/// The vendored volumes, in ECO order. `include_str!` makes them part of the
/// binary, so there is no data file to install or find at runtime, and every
/// `&str` handed out below is a slice of these constants — the table stores no
/// copies of the names.
const VOLUMES: [(&str, &str); 5] = [
    ("a.tsv", include_str!("../data/a.tsv")),
    ("b.tsv", include_str!("../data/b.tsv")),
    ("c.tsv", include_str!("../data/c.tsv")),
    ("d.tsv", include_str!("../data/d.tsv")),
    ("e.tsv", include_str!("../data/e.tsv")),
];

/// A named opening: the ECO code and the name of the line.
///
/// Deliberately carries no move count, no popularity and no verdict — see the
/// module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening {
    /// ECO code, e.g. `"C65"`.
    pub eco: &'static str,
    /// Full name, e.g. `"Ruy Lopez: Berlin Defense"`.
    pub name: &'static str,
}

impl Opening {
    /// Turn this into the wire type.
    ///
    /// `matched_plies` is supplied by the caller because it is a property of the
    /// *game*, not of the table: it is the distance from the root of the
    /// variation tree to this node, which the ECO line's own length does not
    /// know (the same position is reachable by several move orders, and a game
    /// may start from a mid-game FEN).
    pub fn into_info(self, matched_plies: u32) -> OpeningInfo {
        OpeningInfo {
            eco: self.eco.to_string(),
            name: self.name.to_string(),
            matched_plies,
        }
    }
}

/// Look up the named opening for a position. `None` when the position is not
/// a known opening.
///
/// Accepts a full FEN or a bare EPD; the move counters are dropped either way.
/// Costs one hash probe after the table has been built (once per process, on
/// first call — see `eco.rs`'s `building_the_table_is_cheap` test for the
/// measured cost).
pub fn lookup(fen: &str) -> Option<Opening> {
    TABLE
        .get(kibitz_core::normalize_fen(fen).as_str())
        .map(|entry| entry.opening)
}

/// Number of distinct positions in the table. Equal to the number of vendored
/// rows unless two of them name the same position; see the collision policy on
/// `build`.
pub fn position_count() -> usize {
    TABLE.len()
}

/// Built once, on first [`lookup`].
///
/// `LazyLock` rather than a build script or a generated table: replaying all
/// 3,810 lines and hashing the results measures **4.7 ms in release** (80 ms in
/// an unoptimised test build) on an M-series laptop — see the
/// `building_the_table_is_cheap` test, which prints the number. That is far
/// below the cost of maintaining generated code, it is paid once per process,
/// and only by a process that actually names a position.
static TABLE: LazyLock<HashMap<String, Entry>> = LazyLock::new(|| build().entries);

/// The stored value. `plies` is the length of the ECO line that claimed this
/// EPD; it is only used to resolve collisions at build time and is never
/// reported — see [`Opening::into_info`].
#[derive(Debug, Clone, Copy)]
struct Entry {
    opening: Opening,
    plies: u32,
}

/// Result of parsing the volumes, with the diagnostics the tests assert on.
///
/// Only `entries` is used at runtime; the rest is what keeps a data refresh
/// honest, so it is dead code outside `cfg(test)` by design.
#[cfg_attr(not(test), allow(dead_code))]
struct Table {
    entries: HashMap<String, Entry>,
    /// Data rows read (header rows excluded).
    rows: usize,
    /// Rows that could not be replayed, as `"file:line: eco name"`. A row that
    /// does not replay is skipped rather than fatal, but it means the vendored
    /// data is broken, so a test asserts this list is empty.
    failed: Vec<String>,
    /// EPDs claimed by more than one row, as `(epd, kept, dropped)`.
    collisions: Vec<(String, &'static str, &'static str)>,
}

/// Parse every volume into the position → opening map.
///
/// **Collision policy: the longest line wins; on a tie the first row in file
/// order (volume A→E, then line order) keeps the slot.** Two rows can name the
/// same EPD when named lines transpose into each other. Longest wins because the
/// deeper line is the more specific description of the position — reaching the
/// same position after eight moves of the Ruy Lopez is better described by the
/// Ruy Lopez variation name than by the two-move line that also happens to
/// arrive there. The tie-break is positional rather than alphabetical so the
/// result depends only on the files, never on hash iteration order: `HashMap`
/// iteration is never used here, and re-running `build()` produces the same map
/// byte for byte.
fn build() -> Table {
    build_from(&VOLUMES)
}

/// [`build`] over an arbitrary set of `(file name, TSV)` pairs, so the tests can
/// exercise the collision policy on data that actually collides.
fn build_from(volumes: &[(&'static str, &'static str)]) -> Table {
    let mut entries: HashMap<String, Entry> = HashMap::with_capacity(4096);
    let mut rows = 0;
    let mut failed = Vec::new();
    let mut collisions = Vec::new();

    for &(file, text) in volumes {
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_end();
            // The header row, and any trailing blank line.
            if index == 0 || line.is_empty() {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(eco), Some(name), Some(pgn)) = (fields.next(), fields.next(), fields.next())
            else {
                failed.push(format!("{file}:{}: malformed row", index + 1));
                continue;
            };
            rows += 1;

            let Some((epd, plies)) = replay(pgn) else {
                failed.push(format!("{file}:{}: {eco} {name}", index + 1));
                continue;
            };
            let entry = Entry {
                opening: Opening { eco, name },
                plies,
            };

            match entries.entry(epd) {
                MapEntry::Vacant(slot) => {
                    slot.insert(entry);
                }
                MapEntry::Occupied(mut slot) => {
                    let kept_is_new = entry.plies > slot.get().plies;
                    let (kept, dropped) = if kept_is_new {
                        (entry.opening.name, slot.get().opening.name)
                    } else {
                        (slot.get().opening.name, entry.opening.name)
                    };
                    collisions.push((slot.key().clone(), kept, dropped));
                    if kept_is_new {
                        slot.insert(entry);
                    }
                }
            }
        }
    }

    Table {
        entries,
        rows,
        failed,
        collisions,
    }
}

/// Replay a `pgn` column value and return `(EPD, plies)`.
///
/// `None` for a line that does not replay — an illegal or unparsable move, or no
/// move at all. The starting position is deliberately not indexed: no moves
/// played is not an opening.
fn replay(pgn: &str) -> Option<(String, u32)> {
    let mut pos = Chess::default();
    let mut plies = 0;

    for token in pgn.split_whitespace() {
        let token = strip_move_number(token);
        if token.is_empty() {
            continue;
        }
        let san: San = token.parse().ok()?;
        let mv = san.to_move(&pos).ok()?;
        // `to_move` already rejected anything illegal.
        pos.play_unchecked(mv);
        plies += 1;
    }

    (plies > 0).then(|| (epd_of(&pos), plies))
}

/// Drop a leading move number: `"1."` on its own, and the `"1.e4"` form some
/// writers use. No SAN token starts with a digit — castling is `O-O` with the
/// letter O — so this cannot eat part of a move. A `"1-0"` result token is
/// returned unchanged and then fails to parse, which surfaces as a broken row
/// rather than being silently skipped.
fn strip_move_number(token: &str) -> &str {
    match token.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(0) => token,
        Some(i) if !token[i..].starts_with('-') => &token[i..],
        Some(_) => token,
        None => "",
    }
}

/// FEN minus the move counters, i.e. the EPD used as the index key.
fn epd_of(pos: &Chess) -> String {
    kibitz_core::normalize_fen(&Fen::from_position(pos, EnPassantMode::Legal).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The EPD reached by playing `moves` from the initial position.
    fn epd(moves: &str) -> String {
        replay(moves).expect("test line must replay").0
    }

    fn name_of(moves: &str) -> Option<&'static str> {
        lookup(&epd(moves)).map(|o| o.name)
    }

    #[test]
    fn the_starting_position_is_not_an_opening() {
        // No moves played, so there is nothing to name — and the table never
        // indexes a zero-ply line.
        let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert_eq!(lookup(start), None);
    }

    #[test]
    fn names_the_kings_pawn_game() {
        let opening = lookup(&epd("1. e4")).expect("1. e4 is a named opening");
        assert_eq!(opening.eco, "B00");
        assert_eq!(opening.name, "King's Pawn Game");
    }

    #[test]
    fn names_the_ruy_lopez_and_the_berlin() {
        let ruy = lookup(&epd("1. e4 e5 2. Nf3 Nc6 3. Bb5")).expect("Ruy Lopez");
        assert_eq!(ruy.eco, "C60");
        assert_eq!(ruy.name, "Ruy Lopez");

        let berlin = lookup(&epd("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6")).expect("Berlin Defense");
        assert_eq!(berlin.eco, "C65");
        assert_eq!(berlin.name, "Ruy Lopez: Berlin Defense");
    }

    #[test]
    fn a_transposition_resolves_to_the_same_opening() {
        // The Ruy Lopez by its usual move order and through a Zukertort start.
        // Same position, so the same EPD and the same name.
        let direct = epd("1. e4 e5 2. Nf3 Nc6 3. Bb5");
        let transposed = epd("1. Nf3 Nc6 2. e4 e5 3. Bb5");
        assert_eq!(direct, transposed);
        assert_eq!(lookup(&direct), lookup(&transposed));
        assert_eq!(name_of("1. Nf3 Nc6 2. e4 e5 3. Bb5"), Some("Ruy Lopez"));

        // A second pair where the *last* move differs in kind: the transposed
        // order ends on a double pawn push. The key is built with
        // `EnPassantMode::Legal`, so the en-passant square is only recorded when
        // a capture is actually available — otherwise these two would be
        // different keys and the transposition would go unnamed.
        let direct = epd("1. e4 c5 2. Nf3 d6");
        let transposed = epd("1. Nf3 d6 2. e4 c5");
        assert_eq!(direct, transposed);
        assert!(lookup(&direct).is_some(), "must be a named Sicilian line");
        assert_eq!(lookup(&direct), lookup(&transposed));
    }

    #[test]
    fn a_position_that_is_not_an_opening_returns_none() {
        // A late middlegame from the Opera Game — reachable, legal, and nothing
        // any ECO line names.
        assert_eq!(
            lookup("1n1Rkb1r/p4ppp/4q3/4p1B1/4P3/8/PPP2PPP/2K5 b k - 1 17"),
            None
        );
        // A bare king-and-pawn ending, likewise.
        assert_eq!(lookup("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"), None);
    }

    #[test]
    fn every_row_in_every_volume_replays() {
        let table = build();
        // Silent data loss is worse than a loud failure: a row that does not
        // replay means the vendored TSVs are broken, not that the opening is
        // unknown.
        assert!(
            table.failed.is_empty(),
            "{} of {} rows failed to replay: {:?}",
            table.failed.len(),
            table.rows,
            &table.failed[..table.failed.len().min(10)]
        );
        assert_eq!(table.rows, 3810, "vendored row count changed");
        assert_eq!(table.entries.len(), table.rows - table.collisions.len());
    }

    #[test]
    fn duplicate_epds_keep_the_longest_line_deterministically() {
        let first = build();
        let second = build();

        // As vendored, the upstream data is already free of duplicate EPDs: every
        // row names a distinct position. The policy below is therefore dormant —
        // it exists so that a data refresh which does introduce a transposition
        // resolves it deterministically instead of by hash order. If this ever
        // trips, the rest of the test is what actually gets exercised.
        assert_eq!(
            first.collisions.len(),
            0,
            "upstream gained duplicate EPDs: {:?}",
            &first.collisions[..first.collisions.len().min(5)]
        );

        // Same input, same map — the policy never consults hash iteration order.
        assert_eq!(first.entries.len(), second.entries.len());
        for (epd, entry) in &first.entries {
            let other = second.entries.get(epd).expect("same keys");
            assert_eq!(entry.opening, other.opening, "{epd}");
        }

        // And the survivor of every collision really is a longest line for that
        // position: no row with the same EPD is deeper than the one kept.
        let mut deepest: HashMap<String, u32> = HashMap::new();
        for (_file, text) in VOLUMES {
            for line in text.lines().skip(1) {
                let mut fields = line.trim_end().split('\t');
                let (Some(_eco), Some(_name), Some(pgn)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    continue;
                };
                if let Some((epd, plies)) = replay(pgn) {
                    let slot = deepest.entry(epd).or_insert(0);
                    *slot = (*slot).max(plies);
                }
            }
        }
        for (epd, entry) in &first.entries {
            assert_eq!(
                entry.plies,
                deepest[epd.as_str()],
                "{epd} kept a line shorter than another row for the same position"
            );
        }
    }

    /// The collision policy on data that actually collides. The vendored files do
    /// not (see above), so without this the rule would be untested until the day
    /// a refresh depends on it.
    #[test]
    fn a_duplicated_position_keeps_the_longest_line_whatever_the_file_order() {
        // Two rows for the same position: four plies the direct way, eight with
        // a knight round trip.
        const SHORT: &str = "eco\tname\tpgn\nX01\tShort route\t1. e4 e5 2. Nf3 Nc6\n";
        const LONG: &str =
            "eco\tname\tpgn\nX02\tLong route\t1. Nf3 Nc6 2. Ng1 Nb8 3. Nf3 Nc6 4. e4 e5\n";

        for volumes in [
            [("short.tsv", SHORT), ("long.tsv", LONG)],
            [("long.tsv", LONG), ("short.tsv", SHORT)],
        ] {
            let table = build_from(&volumes);
            assert_eq!(table.rows, 2);
            assert_eq!(table.entries.len(), 1, "both rows name the same position");
            assert_eq!(table.collisions.len(), 1);

            let kept = table.entries.values().next().unwrap().opening;
            assert_eq!(kept.name, "Long route", "file order must not decide");
            assert_eq!(table.collisions[0].1, "Long route");
            assert_eq!(table.collisions[0].2, "Short route");
        }
    }

    #[test]
    fn the_public_table_covers_every_row() {
        // `position_count` forces the `LazyLock`, so this also proves the shared
        // table and a freshly built one agree.
        assert_eq!(position_count(), build().entries.len());
    }

    #[test]
    fn lookup_ignores_the_move_counters() {
        // The same position with different halfmove/fullmove fields is the same
        // opening: that is the whole point of indexing by EPD.
        let a = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2";
        let b = "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 9 40";
        assert!(lookup(a).is_some());
        assert_eq!(lookup(a), lookup(b));

        // A bare EPD works too.
        assert_eq!(
            lookup("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -"),
            lookup(a)
        );
    }

    #[test]
    fn deep_lines_are_named_all_the_way_down() {
        // The reason the table is worth having: it does not stop after a few
        // plies the way a hand-written list would.
        let deep = "1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 \
                    7. Bb3 d6 8. c3 O-O 9. h3 Na5 10. Bc2 c5 11. d4 Qc7";
        let opening = lookup(&epd(deep)).expect("Chigorin main line is named");
        assert_eq!(opening.eco, "C97");
        assert!(opening.name.contains("Chigorin"), "{}", opening.name);
    }

    /// Prints the cost of building the whole table. Kept as an assertion, not a
    /// benchmark: it is what justifies `LazyLock` over a build script.
    #[test]
    fn building_the_table_is_cheap() {
        let started = std::time::Instant::now();
        let table = build();
        let elapsed = started.elapsed();
        println!(
            "eco table: {} rows -> {} positions in {:?}",
            table.rows,
            table.entries.len(),
            elapsed
        );
        // Generous: a debug build on a slow machine. The point of the bound is to
        // catch an accidental O(n²), not to pin down a number.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "building the ECO table took {elapsed:?}; if this is real, move it to a build script"
        );
    }

    /// The constraint from the module docs, asserted rather than trusted: naming
    /// a position must never leak into move classification. `Classification` is
    /// not even reachable from this module — a future change that tries to make a
    /// name imply `Book` has to defeat this test first.
    #[test]
    fn the_eco_table_never_touches_classification() {
        let source = include_str!("eco.rs");
        // Production code only: everything before the test module, so this test's
        // own mention of the names does not count.
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("split yields one");
        let uses = production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| line.contains("Classification") || line.contains("BookVerdict"))
            .count();
        assert_eq!(
            uses, 0,
            "eco.rs must not reference move classification: an ECO name says a \
             position has a name, not that a move is still theory"
        );
    }
}
