//! Markdown rendering of an analysed game.
//!
//! Pure formatting: it reads what the pipeline produced and never computes a
//! judgement of its own.

use kibitz_core::Classification;
use kibitz_core::types::{Counterfactual, CounterfactualKind, PositionAnalysis};
use std::collections::HashMap;

/// One analysed move of the game, in playing order.
pub struct MoveEntry<'a> {
    pub analysis: &'a PositionAnalysis,
    /// The LLM explanation, when one was generated for this move.
    pub explanation: Option<&'a str>,
}

/// The order classifications are listed in the summary table: best first.
const SUMMARY_ORDER: [Classification; 9] = [
    Classification::Book,
    Classification::Great,
    Classification::Best,
    Classification::Excellent,
    Classification::Good,
    Classification::Inaccuracy,
    Classification::Mistake,
    Classification::Blunder,
    Classification::Miss,
];

pub fn label(classification: Classification) -> &'static str {
    match classification {
        Classification::Book => "book",
        Classification::Great => "great",
        Classification::Best => "best",
        Classification::Excellent => "excellent",
        Classification::Good => "good",
        Classification::Inaccuracy => "inaccuracy",
        Classification::Mistake => "mistake",
        Classification::Blunder => "blunder",
        Classification::Miss => "miss",
    }
}

pub fn render(headers: &HashMap<String, String>, entries: &[MoveEntry<'_>]) -> String {
    let mut out = String::new();
    let white = header(headers, "White");
    let black = header(headers, "Black");
    out.push_str(&format!("# {white} vs {black}\n\n"));

    let event = header(headers, "Event");
    let date = header(headers, "Date");
    let result = headers.get("Result").map(String::as_str).unwrap_or("*");
    out.push_str(&format!("*{event}, {date} — {result}*\n\n"));

    out.push_str("## Summary\n\n");
    out.push_str("| Classification | Count |\n| --- | ---: |\n");
    for classification in SUMMARY_ORDER {
        let count = entries
            .iter()
            .filter(|e| entry_classification(e) == Some(classification))
            .count();
        if count > 0 {
            out.push_str(&format!("| {} | {count} |\n", label(classification)));
        }
    }
    out.push('\n');

    out.push_str("## Moves\n\n");
    for entry in entries {
        let Some(context) = entry.analysis.context.as_ref() else {
            continue;
        };
        let played = &context.played;
        let number = move_number(
            context.position.fullmove_number,
            &context.position.side_to_move,
        );
        out.push_str(&format!(
            "### {number} {}{} — {}\n\n",
            played.san,
            played.classification.glyph(),
            label(played.classification)
        ));

        if played.classification == Classification::Book {
            if let Some(opening) = &context.opening {
                // A `Book` verdict without a name is the normal case, not a
                // fault: since the Explorer outage the verdict comes from
                // `eco::in_theory`, which indexes every position a line passes
                // through, while names come from the positions lines *end* on
                // (`server::opening`). Printing the empty fields anyway leaves
                // "Theory:   (matched 3 plies)" with a hole in the middle.
                if opening.eco.is_empty() && opening.name.is_empty() {
                    out.push_str(&format!(
                        "Opening theory (matched {} plies).\n\n",
                        opening.matched_plies
                    ));
                } else {
                    out.push_str(&format!(
                        "Theory: {} {} (matched {} plies).\n\n",
                        opening.eco, opening.name, opening.matched_plies
                    ));
                }
            } else {
                out.push_str("Opening theory.\n\n");
            }
            continue;
        }

        out.push_str(&format!(
            "- Win probability {:.1}% -> {:.1}% (delta {:+.3}), accuracy {:.1}\n",
            played.win_prob_before * 100.0,
            played.win_prob_after * 100.0,
            played.delta,
            played.accuracy
        ));
        if let Some(rank) = context.played_rank {
            out.push_str(&format!("- Engine rank of the move played: #{}\n", rank + 1));
        } else {
            out.push_str("- The move played was outside the engine's top candidates\n");
        }
        if let Some(best) = context.candidates.first() {
            out.push_str(&format!(
                "- Best: **{}** ({})\n",
                best.san,
                score_text(&best.score)
            ));
        }
        if let Some(opening) = &context.opening {
            out.push_str(&format!(
                "- Left the book here: {} {} (matched {} plies)\n",
                opening.eco, opening.name, opening.matched_plies
            ));
        }
        if let Some(counterfactual) = &context.counterfactual {
            out.push_str(&format!("- {}\n", counterfactual_text(counterfactual)));
        }
        out.push('\n');

        if let Some(text) = entry.explanation {
            for line in text.trim().lines() {
                out.push_str(&format!("> {line}\n"));
            }
            out.push('\n');
        }
    }
    out
}

fn counterfactual_text(counterfactual: &Counterfactual) -> String {
    let heading = match counterfactual.kind {
        CounterfactualKind::Refutation => "How it is punished",
        CounterfactualKind::AlternativeCollapse => "If the alternative is played instead",
    };
    let mut text = format!("{heading}: {}", counterfactual.pv.join(" "));
    if !counterfactual.motifs.is_empty() {
        let motifs: Vec<&str> = counterfactual.motifs.iter().map(|m| m.key()).collect();
        text.push_str(&format!(" ({})", motifs.join(", ")));
    }
    text
}

fn score_text(score: &kibitz_core::Score) -> String {
    match score {
        kibitz_core::Score::Cp(cp) => format!("{:+.2}", *cp as f64 / 100.0),
        kibitz_core::Score::Mate(mate) => format!("#{mate}"),
    }
}

fn entry_classification(entry: &MoveEntry<'_>) -> Option<Classification> {
    entry
        .analysis
        .context
        .as_ref()
        .map(|c| c.played.classification)
}

fn move_number(fullmove: u32, side_to_move: &str) -> String {
    if side_to_move == "black" {
        format!("{fullmove}...")
    } else {
        format!("{fullmove}.")
    }
}

fn header<'a>(headers: &'a HashMap<String, String>, name: &str) -> &'a str {
    headers.get(name).map(String::as_str).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kibitz_core::feature::{ColorPair, Features, KingSafety, PawnStructure, StaticDiff};
    use kibitz_core::types::{AnalysisContext, Candidate, PlayedMove, PositionInfo};

    fn features() -> Features {
        Features {
            material: 0,
            hanging: Vec::new(),
            center_control: ColorPair { white: 0, black: 0 },
            king_safety: ColorPair {
                white: KingSafety::default(),
                black: KingSafety::default(),
            },
            pawn_structure: ColorPair {
                white: PawnStructure::default(),
                black: PawnStructure::default(),
            },
            open_files: Vec::new(),
            mobility: ColorPair { white: 20, black: 20 },
        }
    }

    fn analysis(
        san: &str,
        classification: Classification,
        fullmove: u32,
        side_to_move: &str,
    ) -> PositionAnalysis {
        PositionAnalysis {
            fen: "fen".into(),
            depth: 18,
            candidates: Vec::new(),
            context: Some(AnalysisContext {
                position: PositionInfo {
                    fen: "fen".into(),
                    side_to_move: side_to_move.into(),
                    fullmove_number: fullmove,
                    depth: 18,
                },
                candidates: vec![Candidate {
                    san: "Bb5".into(),
                    uci: "f1b5".into(),
                    score: kibitz_core::Score::Cp(35),
                    win_prob: 0.53,
                    pv: vec!["Bb5".into()],
                }],
                played_rank: Some(2),
                played: PlayedMove {
                    san: san.into(),
                    uci: "g1f3".into(),
                    win_prob_before: 0.62,
                    win_prob_after: 0.21,
                    delta: -0.41,
                    classification,
                    accuracy: 34.2,
                },
                counterfactual: Some(Counterfactual {
                    kind: CounterfactualKind::Refutation,
                    start_fen: "fen".into(),
                    pv: vec!["Qa4+".into(), "Nc6".into()],
                    motifs: Vec::new(),
                }),
                static_diff: StaticDiff {
                    before: features(),
                    after: features(),
                    changes: Vec::new(),
                },
                outlook: None,
                opening: None,
            }),
            explanations: Default::default(),
        }
    }

    #[test]
    fn renders_headers_summary_and_moves() {
        let mut headers = HashMap::new();
        headers.insert("White".to_string(), "Morphy".to_string());
        headers.insert("Black".to_string(), "Duke".to_string());
        headers.insert("Result".to_string(), "1-0".to_string());

        let blunder = analysis("Nf3", Classification::Blunder, 12, "white");
        let good = analysis("Nc6", Classification::Good, 12, "black");
        let entries = vec![
            MoveEntry {
                analysis: &blunder,
                explanation: Some("This drops a piece.\nThe knight is trapped."),
            },
            MoveEntry {
                analysis: &good,
                explanation: None,
            },
        ];

        let markdown = render(&headers, &entries);
        assert!(markdown.starts_with("# Morphy vs Duke\n"), "{markdown}");
        assert!(markdown.contains("*?, ? — 1-0*"), "{markdown}");
        assert!(markdown.contains("| blunder | 1 |"), "{markdown}");
        assert!(markdown.contains("| good | 1 |"), "{markdown}");
        // Classifications with no moves are left out entirely.
        assert!(!markdown.contains("| great |"), "{markdown}");
        assert!(markdown.contains("### 12. Nf3?? — blunder"), "{markdown}");
        assert!(markdown.contains("### 12... Nc6 — good"), "{markdown}");
        assert!(markdown.contains("62.0% -> 21.0% (delta -0.410)"), "{markdown}");
        assert!(markdown.contains("#3"), "{markdown}");
        assert!(markdown.contains("Best: **Bb5** (+0.35)"), "{markdown}");
        assert!(markdown.contains("How it is punished: Qa4+ Nc6"), "{markdown}");
        assert!(markdown.contains("> This drops a piece.\n> The knight is trapped."), "{markdown}");
    }

    #[test]
    fn book_moves_are_reported_as_theory_without_numbers() {
        let mut book = analysis("e4", Classification::Book, 1, "white");
        if let Some(context) = book.context.as_mut() {
            context.opening = Some(kibitz_core::types::OpeningInfo {
                eco: "C97".into(),
                name: "Ruy Lopez".into(),
                matched_plies: 24,
            });
        }
        let entries = vec![MoveEntry {
            analysis: &book,
            explanation: None,
        }];
        let markdown = render(&HashMap::new(), &entries);
        assert!(markdown.contains("### 1. e4 — book"), "{markdown}");
        assert!(markdown.contains("Theory: C97 Ruy Lopez (matched 24 plies)."), "{markdown}");
        // The placeholder win probabilities on a book move must never be printed.
        assert!(!markdown.contains("Win probability"), "{markdown}");
    }

    #[test]
    fn a_book_move_whose_position_has_no_name_does_not_print_an_empty_one() {
        let mut book = analysis("e4", Classification::Book, 1, "white");
        if let Some(context) = book.context.as_mut() {
            context.opening = Some(kibitz_core::types::OpeningInfo {
                eco: String::new(),
                name: String::new(),
                matched_plies: 3,
            });
        }
        let entries = [MoveEntry {
            analysis: &book,
            explanation: None,
        }];
        let markdown = render(&HashMap::new(), &entries);
        assert!(
            markdown.contains("Opening theory (matched 3 plies)."),
            "{markdown}"
        );
        assert!(!markdown.contains("Theory:  "), "{markdown}");
    }
}
