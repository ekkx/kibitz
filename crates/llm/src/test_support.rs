//! Fixtures shared by the unit tests. Test-only.

use kibitz_core::classify::Classification;
use kibitz_core::eval::Score;
use kibitz_core::feature::{
    ColorPair, FeatureChange, Features, HangingPiece, KingSafety, OpenFile, PawnStructure,
    StaticDiff,
};
use kibitz_core::motif::Motif;
use kibitz_core::outlook::{CandidateConsensus, StrategicOutlook};
use kibitz_core::types::{
    AnalysisContext, Candidate, Counterfactual, CounterfactualKind, OpeningInfo, PlayedMove,
    PositionInfo,
};

fn features(material: i32, white_center: u32, black_center: u32) -> Features {
    Features {
        material,
        hanging: vec![HangingPiece {
            square: "e4".into(),
            role: "knight".into(),
            color: "white".into(),
            see: -300,
        }],
        center_control: ColorPair {
            white: white_center,
            black: black_center,
        },
        king_safety: ColorPair {
            white: KingSafety {
                attackers: 1,
                missing_shield_pawns: 0,
                king_square: Some(6),
            },
            black: KingSafety {
                attackers: 3,
                missing_shield_pawns: 1,
                king_square: Some(62),
            },
        },
        pawn_structure: ColorPair {
            white: PawnStructure {
                isolated: vec!["d4".into()],
                doubled: Vec::new(),
                passed: vec!["a5".into()],
            },
            black: PawnStructure::default(),
        },
        open_files: vec![OpenFile {
            file: "d".into(),
            fully_open: true,
            occupied_by: vec!["d1".into()],
        }],
        mobility: ColorPair {
            white: 34,
            black: 28,
        },
    }
}

fn static_diff() -> StaticDiff {
    StaticDiff {
        before: features(0, 4, 4),
        after: features(-300, 5, 3),
        changes: vec![
            FeatureChange {
                kind: "material".into(),
                side: "white".into(),
                delta: Some(-300.0),
                squares: vec!["e4".into()],
            },
            FeatureChange {
                kind: "center_control".into(),
                side: "both".into(),
                delta: Some(1.0),
                squares: vec!["d4".into(), "e4".into()],
            },
        ],
    }
}

/// A complete `AnalysisContext` with every optional field populated, so tests
/// exercise the whole serialization surface.
pub(crate) fn fixture_context() -> AnalysisContext {
    AnalysisContext {
        position: PositionInfo {
            fen: "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3".into(),
            side_to_move: "black".into(),
            fullmove_number: 3,
            depth: 20,
        },
        candidates: vec![
            Candidate {
                san: "Nf6".into(),
                uci: "g8f6".into(),
                score: Score::Cp(18),
                win_prob: 0.5165,
                pv: vec!["Nf6".into(), "d3".into(), "Bc5".into()],
            },
            Candidate {
                san: "Bc5".into(),
                uci: "f8c5".into(),
                score: Score::Cp(12),
                win_prob: 0.511,
                pv: vec!["Bc5".into(), "c3".into(), "Nf6".into()],
            },
        ],
        played_rank: None,
        played: PlayedMove {
            san: "Nxe4".into(),
            uci: "c6e4".into(),
            win_prob_before: 0.5165,
            win_prob_after: 0.1802,
            delta: -0.3363,
            classification: Classification::Blunder,
            accuracy: 21.4,
        },
        counterfactual: Some(Counterfactual {
            kind: CounterfactualKind::Refutation,
            start_fen: "r1bqkbnr/pppp1ppp/8/4p3/2B1n3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 4".into(),
            pv: vec!["Qa4+".into(), "c6".into(), "Qxe4".into()],
            motifs: vec![
                Motif::Fork {
                    from: "d1".into(),
                    attacker: "a4".into(),
                    targets: vec!["e8".into(), "e4".into()],
                },
                Motif::Hanging {
                    square: "e4".into(),
                    role: "knight".into(),
                    see: -300,
                },
            ],
        }),
        static_diff: static_diff(),
        outlook: Some(StrategicOutlook {
            long_pv: vec![
                "Qa4+".into(),
                "c6".into(),
                "Qxe4".into(),
                "d5".into(),
                "Bxd5".into(),
            ],
            terminal_fen: "r1bqkbnr/pp3ppp/2p5/3Bp3/4Q3/5N2/PPPP1PPP/RNB1K2R b KQkq - 0 6".into(),
            terminal_diff: static_diff(),
            consensus: CandidateConsensus {
                common_piece: Some("queen".into()),
                common_targets: vec!["e4".into()],
            },
        }),
        opening: Some(OpeningInfo {
            eco: "C50".into(),
            name: "Italian Game".into(),
            matched_plies: 5,
        }),
    }
}
