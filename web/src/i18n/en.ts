/**
 * The English UI catalogue, and the shape every other catalogue is checked
 * against — `UiMessageKey` is derived from this object, so a key that exists
 * here and nowhere else falls back to English rather than disappearing.
 *
 * These strings are the *interface* language. The user picks it and the
 * explanation language with one control (`state/useLanguages.ts`), so in
 * practice the language of this catalogue is also the language the server is
 * asked to write explanations in.
 *
 * Server-provided text is never routed through here: opening names, SAN, ECO
 * codes, engine names and the explanation body are rendered as they arrive.
 */
export const en = {
  'app.name': 'kibitz',
  'app.tagline': 'Interactive chess analysis that explains why',

  'health.checking': 'Checking engine…',
  'health.engine': 'Engine',
  'health.ok': '{engine} ready',
  'health.noEngine':
    'The analysis engine did not start. Stockfish is missing or could not be launched, so no analysis is possible. Install Stockfish and restart the kibitz server.',
  'health.unreachable':
    'Cannot reach the kibitz server at 127.0.0.1:7777. Start it with `cargo run`, or run the frontend with VITE_MOCK=1 to use fixture data.',
  'health.engineMissing': 'Engine missing',
  'health.offline': 'Server offline',
  'health.retry': 'Retry',
  'health.mockBadge': 'mock data',

  'import.title': 'Load a game',
  'import.subtitle': 'Paste a PGN, or start from a position and branch from there.',
  'import.tab.pgn': 'PGN',
  'import.tab.position': 'Position',
  'import.pgnPlaceholder': '[Event "…"]\n1. e4 e5 2. Nf3 …',
  'import.fenLabel': 'FEN',
  'import.fenPlaceholder': 'Leave empty for the starting position',
  'import.submit': 'Open',
  'import.loading': 'Opening…',
  'import.sample': 'Use the sample game',
  'import.emptyPgn': 'Paste a PGN first.',

  // The other half of the first screen. Three sentences about what happens
  // after the game is loaded — not a pitch, an answer to "what is this for",
  // which is the question an empty import form leaves standing.
  'import.feature.sweep.title': 'Every move, classified',
  'import.feature.sweep.body':
    'A whole-game analysis marks each inaccuracy, mistake and blunder in the move list, and jumps you straight to them.',
  'import.feature.explain.title': 'Why, not just what',
  'import.feature.explain.body':
    'Every position gets the engine’s best lines and an explanation written for that position — with the punishment replayed on the board.',
  'import.feature.branch.title': 'Try your own idea',
  'import.feature.branch.body':
    'Play a move from anywhere in the game to branch off, and the analysis follows you into the variation.',

  'board.flip': 'Flip board',
  'board.first': 'First move',
  'board.prev': 'Previous move',
  'board.next': 'Next move',
  'board.last': 'Last move',
  'board.startingPosition': 'Starting position',

  // The four pieces a pawn may become, and the picker that asks which.
  'promotion.choose': 'Promote to',
  'promotion.cancel': 'Cancel the promotion',
  'piece.queen': 'Queen',
  'piece.rook': 'Rook',
  'piece.bishop': 'Bishop',
  'piece.knight': 'Knight',

  'tree.title': 'Moves',
  'tree.empty': 'No moves yet — play one on the board.',
  'tree.variation': 'variation',
  // "Mistake" here is every classification that lost something — inaccuracy,
  // mistake, blunder, miss — which is what the board draws in red.
  'tree.mistakePrev': 'Previous mistake',
  'tree.mistakeNext': 'Next mistake',
  'tree.mistakeNone': 'No more mistakes in this direction',
  'tree.mistakeUnknown': 'Not analysed that far yet — run “Analyse whole game”',

  // Names the position only. Never phrase this as book / theory / out of book:
  // an ECO table cannot support that claim (see `OpeningInfo` in api/types.ts).
  'opening.label': 'Opening',
  'opening.eco': 'ECO {eco}',

  'eval.title': 'Evaluation',
  'eval.winProb': 'Win probability',
  'eval.mateIn': 'M{n}',
  'eval.depth': 'depth {n}',
  'eval.forWhite': 'White',
  'eval.forBlack': 'Black',

  'analysis.title': 'Analysis',
  'analysis.candidates': 'Best moves here',
  'analysis.playedMove': 'Move played',
  'analysis.rank': 'rank #{n}',
  'analysis.notInTop': 'outside the engine’s top moves',
  'analysis.accuracyLabel': 'Accuracy',
  'analysis.analyzing': 'Analysing…',
  'analysis.idle': 'Select a move, or play one on the board, to analyse this position.',
  'analysis.cancelled': 'Superseded by a newer analysis.',
  'analysis.failed': 'Analysis failed: {message}',
  'analysis.analyzeNow': 'Analyse this position',
  'analysis.motifs': 'Tactics in the line',

  'counterfactual.refutation': 'How it gets punished',
  'counterfactual.alternative_collapse': 'What happens otherwise',
  'counterfactual.replay': 'Replay line',
  'counterfactual.stop': 'Stop',
  'counterfactual.playing': 'Replaying on the board',
  'counterfactual.returned': 'Board restored to the game position.',

  'explain.title': 'Explanation',
  'explain.streaming': 'Writing…',
  'explain.idle': 'No explanation for this move.',
  'explain.notAnalyzed': 'Analyse this position first.',
  'explain.failed': 'Could not fetch the explanation: {message}',
  'explain.cached': 'cached',
  'explain.regenerate': 'Explain again',
  'explain.request': 'Explain this move',

  'sweep.run': 'Analyse whole game',
  'sweep.running': 'Analysing {done} / {total}',
  'sweep.cancel': 'Stop',
  'sweep.done': 'Swept {total} positions',
  'sweep.failed': 'Sweep failed: {message}',
  'sweep.accuracyFor': 'Accuracy · {color}',
  // The number is coloured by classification, so this is rendered with
  // `tParts`: the template decides where it goes, the component styles it.
  'sweep.count': '{n} {label}',
  'sweep.summaryTitle': 'Game summary',

  'ask.title': 'Ask a follow-up',
  'ask.placeholder': 'What happens after Bxf7+?',
  'ask.submit': 'Ask',
  'ask.busy': 'Thinking…',

  'lang.label': 'Language',
  'lang.hint': 'The language of the interface and of the explanations written for you.',

  'settings.title': 'Settings',
  'settings.open': 'Settings',
  'settings.close': 'Close',

  'settings.arrows.label': 'Board arrows',
  'settings.arrows.hint':
    'The engine’s top moves, drawn heaviest first. Equally good moves get equal weight.',
  'settings.arrows.none': 'Off',

  'settings.sound.label': 'Board sounds',
  'settings.sound.on': 'On',
  'settings.sound.off': 'Off',
  // Just what the setting does. It briefly carried an instruction to go and
  // supply sample files, which was worse than useless once they shipped: it told
  // users to fix something that was not broken.
  'settings.sound.hint':
    'A sound as each move lands, with its own for captures, castling, check and mate.',

  'settings.depth.label': 'Analysis depth',
  // Named for what the user gets, with the ply count kept visible for anyone
  // who thinks in plies. Times are for a whole 34-move game.
  'settings.depth.10': 'Fast',
  'settings.depth.12': 'Balanced',
  'settings.depth.14': 'Deep',
  'settings.depth.16': 'Deepest',
  'settings.depth.option': '{label} · depth {n}',
  'settings.depth.hint':
    'Deeper search catches more, and costs much more time: every two ply roughly triples a whole-game analysis. Applies to the next analysis — use “Analyse whole game” to redo this one.',

  'theme.toggle': 'Toggle light / dark',

  'session.newGame': 'New game',
  'session.white': 'White',
  'session.black': 'Black',
  'session.result': 'Result',

  'class.book': 'Book',
  'class.great': 'Great',
  'class.best': 'Best',
  'class.excellent': 'Excellent',
  'class.good': 'Good',
  'class.inaccuracy': 'Inaccuracy',
  'class.mistake': 'Mistake',
  'class.blunder': 'Blunder',
  'class.miss': 'Miss',

  'motif.fork': 'Fork',
  'motif.pin': 'Pin',
  'motif.skewer': 'Skewer',
  'motif.discovered_attack': 'Discovered attack',
  'motif.hanging': 'Hanging piece',
  'motif.back_rank': 'Back rank',
} as const;

export type UiMessageKey = keyof typeof en;
