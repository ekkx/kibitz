//! Filling in `Node.opening` from the embedded ECO table.
//!
//! `kibitz-core` owns the field but cannot look anything up — the table lives in
//! `kibitz-book`, which depends on core, not the other way round. So every place
//! that creates a node goes through here.
//!
//! **Names only.** A hit gives the position a name for the UI; it says nothing
//! about whether the move that reached it is still theory, and never turns into
//! `Classification::Book`. That verdict comes from the Opening Explorer's game
//! counts (`kibitz_book::Book`), which an ECO table does not carry.
//!
//! A lookup is one hash probe, so nodes are annotated as they are born and
//! nothing is cached.

use kibitz_core::tree::{GameTree, NodeId};
use shakmaty::Chess;

/// A fresh tree whose root already carries its opening name.
///
/// The root is worth looking up: a session started from a mid-game FEN still
/// deserves a name when that position happens to be a known opening.
pub fn new_tree(start: &Chess) -> GameTree {
    let mut tree = GameTree::new(start);
    let root = tree.root;
    annotate(&mut tree, root);
    tree
}

/// Name the position at `id`, or clear the field when it is not a known opening.
///
/// `matched_plies` is the distance in plies from the root of *this tree*, not the
/// length of the ECO line — the position may have been reached by another move
/// order, and the tree may not start from the initial position.
pub fn annotate(tree: &mut GameTree, id: NodeId) {
    if tree.get(id).is_none() {
        return;
    }
    let plies = (tree.path_to(id).len() - 1) as u32;
    let info = kibitz_book::eco::lookup(&tree.nodes[id].fen).map(|o| o.into_info(plies));
    tree.nodes[id].opening = info;
}

#[cfg(test)]
mod tests {
    use super::*;
    use kibitz_core::Classification;

    fn imported(pgn: &str) -> GameTree {
        crate::pgn::import(pgn).expect("valid pgn").tree
    }

    fn opening_at(tree: &GameTree, ply: usize) -> Option<(&str, &str, u32)> {
        let id = tree.mainline()[ply];
        tree.nodes[id]
            .opening
            .as_ref()
            .map(|o| (o.eco.as_str(), o.name.as_str(), o.matched_plies))
    }

    #[test]
    fn the_root_of_a_fresh_game_has_no_opening() {
        let tree = new_tree(&Chess::default());
        assert_eq!(tree.nodes[tree.root].opening, None);
    }

    #[test]
    fn a_session_started_from_a_fen_is_named_when_the_position_is_known() {
        // A FEN start still deserves a name. This is the Ruy Lopez after
        // 3. Bb5, handed in as a bare position with no move history.
        let fen = "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3";
        let pos = kibitz_core::parse_fen(fen).unwrap();
        let tree = new_tree(&pos);
        let opening = tree.nodes[tree.root].opening.as_ref().expect("named");
        assert_eq!(opening.eco, "C60");
        assert_eq!(opening.name, "Ruy Lopez");
        // The root is the root, whatever was played to get there.
        assert_eq!(opening.matched_plies, 0);

        // An ordinary endgame position gets nothing.
        let plain = kibitz_core::parse_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert_eq!(new_tree(&plain).nodes[0].opening, None);
    }

    #[test]
    fn pgn_import_names_the_line() {
        let tree = imported("1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 *");
        assert_eq!(tree.nodes[tree.root].opening, None);
        assert_eq!(opening_at(&tree, 1), Some(("B00", "King's Pawn Game", 1)));
        assert_eq!(opening_at(&tree, 5), Some(("C60", "Ruy Lopez", 5)));
        assert_eq!(
            opening_at(&tree, 6),
            Some(("C65", "Ruy Lopez: Berlin Defense", 6))
        );
    }

    #[test]
    fn variations_are_named_too() {
        let tree = imported("1. e4 e5 2. Nf3 (2. f4 exf4) 2... Nc6 *");
        let e5 = tree.mainline()[2];
        let f4 = tree.nodes[e5].children[1];
        let exf4 = tree.nodes[f4].children[0];
        let named = tree.nodes[exf4]
            .opening
            .as_ref()
            .expect("King's Gambit Accepted");
        assert!(named.name.contains("King's Gambit"), "{}", named.name);
        // Plies are counted from the root through the variation, not along the
        // mainline.
        assert_eq!(named.matched_plies, 4);
    }

    #[test]
    fn the_chigorin_testdata_is_named_deep_into_the_line() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/ruy_lopez_chigorin.pgn");
        let tree = imported(&std::fs::read_to_string(path).expect("testdata"));

        assert_eq!(opening_at(&tree, 5), Some(("C60", "Ruy Lopez", 5)));
        assert_eq!(
            opening_at(&tree, 6),
            Some(("C70", "Ruy Lopez: Morphy Defense", 6))
        );

        // The game's own [ECO "C97"] tag shows up on 11... Qc7, the move that
        // defines the variation.
        assert_eq!(
            opening_at(&tree, 22),
            Some(("C97", "Ruy Lopez: Closed, Chigorin Defense", 22))
        );

        // The deepest named node is what an ECO table buys over a hand-written
        // list: this line is still being named twenty-five plies in.
        let (deepest_ply, deepest) = tree
            .mainline()
            .iter()
            .enumerate()
            .filter_map(|(ply, &id)| tree.nodes[id].opening.clone().map(|o| (ply, o)))
            .next_back()
            .expect("the mainline is named");
        assert_eq!(
            deepest_ply, 25,
            "deepest named node moved: {}",
            deepest.name
        );
        assert_eq!(deepest.eco, "C99");
        assert!(deepest.name.contains("Chigorin"), "{}", deepest.name);
        assert_eq!(deepest.matched_plies as usize, deepest_ply);

        // Coverage is per-position, not a running state: only a position that
        // some ECO row *ends* on is named, so 11. d4 sits unnamed between two
        // named nodes. Nothing here interpolates or inherits.
        assert_eq!(opening_at(&tree, 21), None);
    }

    #[test]
    fn a_move_out_of_theory_is_simply_unnamed() {
        // Nothing inherits: a position the table does not know reports `None`
        // rather than the last name seen. The Opera Game leaves theory at
        // 3... Bg4 and never comes back.
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/opera_game.pgn");
        let tree = imported(&std::fs::read_to_string(path).expect("testdata"));

        assert_eq!(opening_at(&tree, 5), Some(("C41", "Philidor Defense", 5)));
        assert_eq!(opening_at(&tree, 6), None);
        let last = *tree.mainline().last().unwrap();
        assert_eq!(tree.nodes[last].san.as_deref(), Some("Rd8#"));
        assert_eq!(tree.nodes[last].opening, None);
    }

    /// The constraint that matters most: a name is not a verdict.
    ///
    /// `Classification::Book` is produced in exactly one place —
    /// `pipeline::book_context`, reached only from `BookVerdict::InBook`, which
    /// needs the Opening Explorer's game counts. Feeding a name from the ECO
    /// table into the ordinary engine path must leave the classification alone.
    #[test]
    fn a_named_position_never_classifies_a_move_as_book() {
        use kibitz_core::eval::Score;
        use kibitz_core::types::Candidate;
        use kibitz_engine::SearchResult;

        let tree = imported("1. e4 e5 2. Nf3 Nc6 3. Bb5 *");
        let before = tree.position(tree.mainline()[4]).unwrap();
        let after = tree.position(tree.mainline()[5]).unwrap();
        let mv = "Bb5"
            .parse::<shakmaty::san::San>()
            .unwrap()
            .to_move(&before)
            .unwrap();

        // The position is named...
        let opening = kibitz_book::eco::lookup(&tree.nodes[tree.mainline()[5]].fen)
            .expect("Ruy Lopez")
            .into_info(5);
        assert_eq!(opening.name, "Ruy Lopez");

        // ...and the move still goes through `classify()` like any other, which
        // never returns `Book`.
        let search = |uci: &str, san: &str| SearchResult {
            fen: crate::pipeline::fen_of(&before),
            depth: 18,
            candidates: vec![Candidate {
                san: san.to_string(),
                uci: uci.to_string(),
                score: Score::Cp(30),
                win_prob: 0.55,
                pv: vec![san.to_string()],
            }],
            terminal: None,
        };
        let context = crate::pipeline::build_context(
            &before,
            mv,
            &after,
            &search("f1b5", "Bb5"),
            &search("g8f6", "Nf6"),
            18,
            false,
            Some(opening),
        );
        assert_ne!(context.played.classification, Classification::Book);
        assert!(context.opening.is_some(), "the name is still reported");

        // And annotating a tree never invents an analysis, so no classification
        // — `Book` or otherwise — can appear without the engine having run.
        assert!(
            tree.nodes.iter().all(|n| n.analysis.is_none()),
            "naming a position must not fabricate an analysis"
        );
        assert!(tree.nodes.iter().filter(|n| n.opening.is_some()).count() >= 4);
    }
}
