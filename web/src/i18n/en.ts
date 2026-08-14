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

  'health.checking': 'Checking engine…',
  'health.ok': '{engine} ready',
  'health.noEngine':
    'Stockfish is missing or could not be launched. Install it and restart the kibitz server.',
  'health.unreachable':
    'Cannot reach the kibitz server at 127.0.0.1:7777. Start it with `cargo run`, or run the frontend with VITE_MOCK=1 to use fixture data.',
  'health.engineMissing': 'Engine missing',
  'health.offline': 'Server offline',
  'health.retry': 'Retry',
  'health.mockBadge': 'mock data',

  'import.title': 'Load a game',
  'import.tab.pgn': 'PGN',
  'import.tab.position': 'Position',
  'import.pgnPlaceholder': '[Event "…"]\n1. e4 e5 2. Nf3 …',
  'import.fenLabel': 'FEN',
  'import.fenPlaceholder': 'Leave empty for the starting position',
  'import.submit': 'Open',
  'import.loading': 'Opening…',
  'import.sample': 'Use the sample game',
  'import.emptyPgn': 'Paste a PGN first.',

  // The other half of the first screen, and the one place in the app where
  // prose about the app earns its keep: an empty import form leaves "what is
  // this for" standing, and nothing else on this screen answers it. One
  // sentence each, and no more than that.
  'import.feature.sweep.title': 'Every move, classified',
  'import.feature.sweep.body':
    'One pass marks every inaccuracy, mistake and blunder in the move list, and jumps you to them.',
  'import.feature.explain.title': 'Why, not just what',
  'import.feature.explain.body':
    'The engine’s best lines, an explanation written for the position, and the punishment replayed on the board.',
  'import.feature.branch.title': 'Try your own idea',
  'import.feature.branch.body':
    'Play a move anywhere in the game to branch off. The analysis follows you into the variation.',

  'board.flip': 'Flip board',
  'board.first': 'First move',
  'board.prev': 'Previous move',
  'board.next': 'Next move',
  'board.last': 'Last move',

  // The four pieces a pawn may become, and the picker that asks which.
  'promotion.choose': 'Promote to',
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

  'eval.winProb': 'Win probability',
  'eval.mateIn': 'M{n}',
  'eval.depth': 'depth {n}',
  'eval.forWhite': 'White',
  'eval.forBlack': 'Black',

  'analysis.title': 'Analysis',
  'analysis.candidates': 'Best moves here',
  'analysis.rank': 'rank #{n}',
  'analysis.notInTop': 'outside the engine’s top moves',
  'analysis.accuracyLabel': 'Accuracy',
  'analysis.analyzing': 'Analysing…',
  'analysis.failed': 'Analysis failed: {message}',
  'analysis.analyzeNow': 'Analyse this position',

  'counterfactual.refutation': 'How it gets punished',
  'counterfactual.alternative_collapse': 'What happens otherwise',
  'counterfactual.replay': 'Replay line',
  'counterfactual.stop': 'Stop',

  'explain.title': 'Explanation',
  'explain.streaming': 'Writing…',
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

  'settings.title': 'Settings',
  'settings.open': 'Settings',

  // Neither of these two settings has a hint, and the absence is the decision.
  // "Arrows show the engine's top moves" and "a sound plays as each move lands"
  // are both readable off the label and confirmed by the first move played —
  // text that says what the user is about to find out anyway is text they have
  // to read past every time they open this panel to change something else.
  'settings.arrows.label': 'Board arrows',
  'settings.arrows.none': 'Off',

  'settings.sound.label': 'Board sounds',
  'settings.sound.on': 'On',
  'settings.sound.off': 'Off',

  'settings.depth.label': 'Analysis depth',
  // Named for what the user gets, with the ply count kept visible for anyone
  // who thinks in plies. Times are for a whole 34-move game.
  'settings.depth.10': 'Fast',
  'settings.depth.12': 'Balanced',
  'settings.depth.14': 'Deep',
  'settings.depth.16': 'Deepest',
  'settings.depth.option': '{label} · depth {n}',
  // The one hint that survives the trim, because the cost is severe, invisible
  // and impossible to guess from a number — and because a setting that does not
  // touch the analysis already on screen is a surprise worth spending a clause on.
  'settings.depth.hint':
    'Every two ply roughly triples a whole-game analysis. Takes effect on the next one.',

  'theme.toggle': 'Toggle light / dark',

  'session.newGame': 'New game',
  'session.white': 'White',
  'session.black': 'Black',

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
