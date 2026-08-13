//! PGN import and export — the only place `pgn-reader` is used.
//!
//! Export writes `children[0]` as the mainline and `children[1..]` as
//! parenthesised variations.

use kibitz_core::tree::{GameTree, NodeId};
use pgn_reader::{RawTag, Reader, SanPlus, Skip, Visitor};
use shakmaty::{CastlingMode, Chess, Outcome, fen::Fen};
use std::collections::HashMap;
use std::ops::ControlFlow;

/// Tags written first, in the order of the Seven Tag Roster, with the value used
/// when the imported game did not carry one.
const SEVEN_TAG_ROSTER: [(&str, &str); 7] = [
    ("Event", "?"),
    ("Site", "?"),
    ("Date", "????.??.??"),
    ("Round", "?"),
    ("White", "?"),
    ("Black", "?"),
    ("Result", "*"),
];

/// Soft wrap column for the movetext, as most PGN writers use.
const WRAP_COLUMN: usize = 80;

#[derive(Debug, Clone)]
pub struct ImportedGame {
    pub tree: GameTree,
    pub headers: HashMap<String, String>,
}

/// Read the first game out of a PGN string.
pub fn import(pgn: &str) -> Result<ImportedGame, PgnError> {
    let mut reader = Reader::new(std::io::Cursor::new(pgn.as_bytes()));
    let mut visitor = TreeVisitor;
    match reader.read_game(&mut visitor) {
        Ok(Some(result)) => result,
        Ok(None) => Err(PgnError::Empty),
        Err(e) => Err(PgnError::Parse(e.to_string())),
    }
}

pub fn export(tree: &GameTree, headers: &HashMap<String, String>) -> String {
    let mut out = String::new();

    for (name, default) in SEVEN_TAG_ROSTER {
        let value = headers.get(name).map(String::as_str).unwrap_or(default);
        out.push_str(&format!("[{} \"{}\"]\n", name, escape_tag(value)));
    }

    // A non-standard starting position has to survive the round trip.
    let start_fen = &tree.nodes[tree.root].fen;
    if !is_standard_start(start_fen) {
        out.push_str("[SetUp \"1\"]\n");
        out.push_str(&format!("[FEN \"{}\"]\n", escape_tag(start_fen)));
    }

    // Anything else the source PGN carried, in a stable order.
    let mut extra: Vec<(&String, &String)> = headers
        .iter()
        .filter(|(k, _)| {
            !SEVEN_TAG_ROSTER.iter().any(|(n, _)| *n == k.as_str())
                && k.as_str() != "SetUp"
                && k.as_str() != "FEN"
        })
        .collect();
    extra.sort_by(|a, b| a.0.cmp(b.0));
    for (name, value) in extra {
        out.push_str(&format!("[{} \"{}\"]\n", name, escape_tag(value)));
    }
    out.push('\n');

    let mut tokens = Vec::new();
    render_line(tree, tree.root, &mut tokens, true);
    tokens.push(
        headers
            .get("Result")
            .map(String::as_str)
            .unwrap_or("*")
            .to_string(),
    );

    out.push_str(&wrap(&tokens, WRAP_COLUMN));
    out.push('\n');
    out
}

/// Render the mainline starting at `parent`, inlining variations as `( ... )`.
fn render_line(tree: &GameTree, parent: NodeId, tokens: &mut Vec<String>, mut need_number: bool) {
    let mut parent = parent;
    loop {
        let children = &tree.nodes[parent].children;
        let Some(&main) = children.first() else { return };

        tokens.push(move_token(tree, parent, main, need_number));
        need_number = false;

        for &variation in &children[1..] {
            tokens.push("(".to_string());
            tokens.push(move_token(tree, parent, variation, true));
            render_line(tree, variation, tokens, false);
            tokens.push(")".to_string());
            // After a closing paren a black move must repeat its number as `12...`.
            need_number = true;
        }

        parent = main;
    }
}

fn move_token(tree: &GameTree, parent: NodeId, child: NodeId, need_number: bool) -> String {
    let san = san_plus(tree, parent, child);
    let san = san.as_deref().unwrap_or("--");
    let (white_to_move, fullmove) = fen_turn_and_number(&tree.nodes[parent].fen);
    if white_to_move {
        format!("{fullmove}. {san}")
    } else if need_number {
        format!("{fullmove}... {san}")
    } else {
        san.to_string()
    }
}

/// SAN with its `+` / `#` suffix.
///
/// `GameTree` stores `San::from_move`, which carries no check or mate marker, so
/// the suffix is recovered here from the position rather than exporting bare
/// `Qb8` and `Rd8` for a check and a mate. Falls back to the stored SAN if the
/// node cannot be replayed.
fn san_plus(tree: &GameTree, parent: NodeId, child: NodeId) -> Option<String> {
    let stored = tree.nodes[child].san.clone();
    let recovered = (|| {
        let pos = tree.position(parent).ok()?;
        let mv = tree.nodes[child]
            .uci
            .as_deref()?
            .parse::<shakmaty::uci::UciMove>()
            .ok()?
            .to_move(&pos)
            .ok()?;
        Some(shakmaty::san::SanPlus::from_move(pos, mv).to_string())
    })();
    recovered.or(stored)
}

/// Side to move and fullmove number, read straight off the FEN so no position
/// has to be reconstructed.
fn fen_turn_and_number(fen: &str) -> (bool, u32) {
    let mut fields = fen.split_whitespace();
    let white_to_move = fields.nth(1) != Some("b");
    let fullmove = fields.nth(3).and_then(|f| f.parse().ok()).unwrap_or(1);
    (white_to_move, fullmove)
}

fn wrap(tokens: &[String], width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for token in tokens {
        // Parentheses hug their variation: `(2. f4 exf4)`, never `( 2. f4 exf4 )`.
        let glued = line.is_empty() || token == ")" || line.ends_with('(');
        if !glued && line.len() + 1 + token.len() > width {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        } else if !glued {
            line.push(' ');
        }
        line.push_str(token);
    }
    out.push_str(&line);
    out
}

fn escape_tag(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn is_standard_start(fen: &str) -> bool {
    kibitz_core::normalize_fen(fen) == kibitz_core::normalize_fen(&fen_of(&Chess::default()))
}

pub(crate) fn fen_of(pos: &Chess) -> String {
    Fen::from_position(pos, shakmaty::EnPassantMode::Legal).to_string()
}

// ─── import visitor ─────────────────────────────────────

struct TreeVisitor;

struct Tags {
    headers: HashMap<String, String>,
    /// From a `[FEN ...]` tag, for games that do not start from the initial position.
    start: Option<Chess>,
    error: Option<PgnError>,
}

struct Movetext {
    tree: GameTree,
    headers: HashMap<String, String>,
    /// Node the next move is played from.
    cursor: NodeId,
    /// Saved cursors, one per open `(`.
    stack: Vec<NodeId>,
    ply: usize,
}

impl Visitor for TreeVisitor {
    type Tags = Tags;
    type Movetext = Movetext;
    type Output = Result<ImportedGame, PgnError>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(Tags {
            headers: HashMap::new(),
            start: None,
            error: None,
        })
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let name = String::from_utf8_lossy(name).into_owned();
        let value = value.decode_utf8_lossy().into_owned();
        if name == "FEN" {
            match value
                .parse::<Fen>()
                .ok()
                .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
            {
                Some(pos) => tags.start = Some(pos),
                None => tags.error = Some(PgnError::Parse(format!("invalid FEN tag: {value}"))),
            }
        }
        tags.headers.insert(name, value);
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        if let Some(err) = tags.error {
            return ControlFlow::Break(Err(err));
        }
        let start = tags.start.unwrap_or_default();
        // Named here rather than after the import, so a game that starts from a
        // `[FEN ...]` tag is named too.
        let tree = crate::opening::new_tree(&start);
        let root = tree.root;
        ControlFlow::Continue(Movetext {
            tree,
            headers: tags.headers,
            cursor: root,
            stack: Vec::new(),
            ply: 0,
        })
    }

    fn san(
        &mut self,
        movetext: &mut Self::Movetext,
        san_plus: SanPlus,
    ) -> ControlFlow<Self::Output> {
        let pos = match movetext.tree.position(movetext.cursor) {
            Ok(pos) => pos,
            Err(e) => return ControlFlow::Break(Err(PgnError::Parse(e.to_string()))),
        };
        let mv = match san_plus.san.to_move(&pos) {
            Ok(mv) => mv,
            Err(_) => {
                return ControlFlow::Break(Err(PgnError::IllegalMove(
                    movetext.ply,
                    san_plus.to_string(),
                )));
            }
        };
        match movetext.tree.play(movetext.cursor, mv) {
            Ok(id) => {
                // Every node, mainline and variation alike, gets its name as it
                // is created: one hash probe into the embedded ECO table.
                crate::opening::annotate(&mut movetext.tree, id);
                movetext.cursor = id;
                movetext.ply += 1;
                ControlFlow::Continue(())
            }
            Err(_) => ControlFlow::Break(Err(PgnError::IllegalMove(
                movetext.ply,
                san_plus.to_string(),
            ))),
        }
    }

    fn begin_variation(
        &mut self,
        movetext: &mut Self::Movetext,
    ) -> ControlFlow<Self::Output, Skip> {
        // A variation is an alternative to the move just played, so it branches
        // off this node's parent.
        movetext.stack.push(movetext.cursor);
        movetext.cursor = movetext.tree.nodes[movetext.cursor]
            .parent
            .unwrap_or(movetext.tree.root);
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, movetext: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        if let Some(saved) = movetext.stack.pop() {
            movetext.cursor = saved;
        }
        ControlFlow::Continue(())
    }

    fn outcome(
        &mut self,
        movetext: &mut Self::Movetext,
        outcome: Outcome,
    ) -> ControlFlow<Self::Output> {
        movetext
            .headers
            .entry("Result".to_string())
            .or_insert_with(|| outcome.known().map(|k| k.as_str()).unwrap_or("*").to_string());
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, movetext: Self::Movetext) -> Self::Output {
        Ok(ImportedGame {
            tree: movetext.tree,
            headers: movetext.headers,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PgnError {
    #[error("no game found in PGN")]
    Empty,
    #[error("invalid move at ply {0}: {1}")]
    IllegalMove(usize, String),
    #[error("parse error: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sans(tree: &GameTree) -> Vec<String> {
        tree.mainline()
            .into_iter()
            .filter_map(|id| tree.nodes[id].san.clone())
            .collect()
    }

    #[test]
    fn imports_a_mainline() {
        let game = import("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1/2-1/2").unwrap();
        assert_eq!(sans(&game.tree), ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]);
        assert_eq!(game.headers.get("Result").map(String::as_str), Some("1/2-1/2"));
    }

    #[test]
    fn imports_headers() {
        let pgn = "[Event \"Test\"]\n[White \"Morphy\"]\n[Black \"Duke\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n";
        let game = import(pgn).unwrap();
        assert_eq!(game.headers["White"], "Morphy");
        assert_eq!(game.headers["Black"], "Duke");
        assert_eq!(game.headers["Event"], "Test");
        assert_eq!(game.headers["Result"], "1-0");
    }

    #[test]
    fn variations_branch_off_the_parent() {
        let game = import("1. e4 e5 2. Nf3 (2. f4 exf4) (2. Bc4) 2... Nc6 *").unwrap();
        let tree = &game.tree;
        // root -> e4 -> e5, and e5 has three children: Nf3, f4, Bc4.
        let e5 = tree.mainline()[2];
        let children = &tree.nodes[e5].children;
        assert_eq!(children.len(), 3);
        assert_eq!(tree.nodes[children[0]].san.as_deref(), Some("Nf3"));
        assert_eq!(tree.nodes[children[1]].san.as_deref(), Some("f4"));
        assert_eq!(tree.nodes[children[2]].san.as_deref(), Some("Bc4"));
        // The variation continues on its own branch.
        let f4 = children[1];
        assert_eq!(
            tree.nodes[tree.nodes[f4].children[0]].san.as_deref(),
            Some("exf4")
        );
        // And the mainline carries on after the variations close.
        assert_eq!(sans(tree), ["e4", "e5", "Nf3", "Nc6"]);
    }

    #[test]
    fn nested_variations() {
        let game = import("1. e4 e5 2. Nf3 (2. f4 exf4 (2... d6) 3. Bc4) 2... Nc6 *").unwrap();
        let tree = &game.tree;
        let e5 = tree.mainline()[2];
        let f4 = tree.nodes[e5].children[1];
        let exf4 = tree.nodes[f4].children[0];
        assert_eq!(tree.nodes[exf4].san.as_deref(), Some("exf4"));
        // `d6` is an alternative to `exf4`, so it hangs off `f4`.
        assert_eq!(tree.nodes[f4].children.len(), 2);
        assert_eq!(tree.nodes[tree.nodes[f4].children[1]].san.as_deref(), Some("d6"));
        // And `3. Bc4` continues after `exf4`.
        assert_eq!(
            tree.nodes[tree.nodes[exf4].children[0]].san.as_deref(),
            Some("Bc4")
        );
    }

    #[test]
    fn rejects_an_illegal_move() {
        let err = import("1. e4 e5 2. Qxd8 *").unwrap_err();
        assert!(matches!(err, PgnError::IllegalMove(2, _)), "{err:?}");
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(matches!(import("").unwrap_err(), PgnError::Empty));
    }

    #[test]
    fn export_numbers_moves_and_variations() {
        let game = import("1. e4 e5 2. Nf3 (2. f4 exf4) 2... Nc6 *").unwrap();
        let pgn = export(&game.tree, &game.headers);
        let movetext = pgn.split("\n\n").nth(1).unwrap().trim().to_string();
        assert_eq!(movetext, "1. e4 e5 2. Nf3 (2. f4 exf4) 2... Nc6 *");
    }

    #[test]
    fn export_writes_the_seven_tag_roster_first() {
        let game = import("[Black \"B\"]\n[White \"W\"]\n[Annotator \"me\"]\n\n1. e4 *").unwrap();
        let pgn = export(&game.tree, &game.headers);
        let tags: Vec<&str> = pgn.lines().take_while(|l| l.starts_with('[')).collect();
        assert_eq!(tags[0], "[Event \"?\"]");
        assert_eq!(tags[4], "[White \"W\"]");
        assert_eq!(tags[5], "[Black \"B\"]");
        assert_eq!(tags[6], "[Result \"*\"]");
        // Extra tags come after the roster.
        assert_eq!(tags[7], "[Annotator \"me\"]");
    }

    #[test]
    fn round_trips_a_game_with_variations_and_headers() {
        let source = "[Event \"Round trip\"]\n[Site \"Nowhere\"]\n[Date \"2026.01.01\"]\n\
                      [Round \"1\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n\
                      1. e4 e5 2. Nf3 (2. f4 exf4 3. Bc4 (3. Nf3 g5)) 2... Nc6 \
                      3. Bb5 (3. Bc4 Bc5) 3... a6 4. Bxc6 dxc6 1-0\n";
        let first = import(source).unwrap();
        let exported = export(&first.tree, &first.headers);
        let second = import(&exported).unwrap();

        assert_eq!(first.headers, second.headers);
        assert_eq!(
            serde_json::to_value(&first.tree).unwrap(),
            serde_json::to_value(&second.tree).unwrap()
        );
        // Exporting the re-imported tree is byte-identical.
        assert_eq!(exported, export(&second.tree, &second.headers));
    }

    #[test]
    fn round_trips_a_non_standard_start_position() {
        let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
        let source = format!("[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n1. e4 Kd7 2. e5 Ke6 *\n");
        let first = import(&source).unwrap();
        let exported = export(&first.tree, &first.headers);
        assert!(exported.contains("[SetUp \"1\"]"), "{exported}");
        assert!(
            exported.contains(&format!("[FEN \"{}\"]", first.tree.nodes[0].fen)),
            "{exported}"
        );
        let second = import(&exported).unwrap();
        assert_eq!(sans(&second.tree), ["e4", "Kd7", "e5", "Ke6"]);
        assert_eq!(first.tree.nodes[0].fen, second.tree.nodes[0].fen);
    }

    #[test]
    fn testdata_pgns_are_legal() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .canonicalize()
            .expect("testdata directory");
        let mut count = 0;
        for entry in std::fs::read_dir(&dir).expect("read testdata") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("pgn") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let game = import(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(
                game.tree.mainline().len() > 10,
                "{} is too short to be useful",
                path.display()
            );
            // And it survives a round trip.
            let again = import(&export(&game.tree, &game.headers)).unwrap();
            assert_eq!(sans(&game.tree), sans(&again.tree), "{}", path.display());
            count += 1;
        }
        assert!(count >= 2, "expected at least two sample PGNs, found {count}");
    }
}
