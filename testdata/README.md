# testdata

Sample PGNs used by the tests and for manual runs of `kibitz analyze`.

- `opera_game.pgn` — Paul Morphy vs. Duke Karl of Brunswick and Count Isouard,
  casual game, Paris 1858 (the "Opera Game"). A 17-move miniature that leaves
  book early and contains several obvious blunders by Black, ending in mate.
- `ruy_lopez_chigorin.pgn` — the main line of the Ruy Lopez, Closed Defence,
  Chigorin Variation (ECO C97) as given in standard opening references. Not a
  single historical game: it exists to run 15+ moves of book so the opening-book
  path in the analysis pipeline is exercised.

Both files are checked for legality and for PGN round-tripping by
`crates/server/src/pgn.rs::tests::testdata_pgns_are_legal`.
