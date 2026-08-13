/**
 * The "Use the sample game" PGN offered on the import screen.
 *
 * It lives here rather than in `mock/fixtures.ts` because the import screen is
 * production UI: anything it imports is bundled into a real build, and the mock
 * fixtures must never be (see `api/transport.ts`). The mock re-uses this
 * constant, not the other way round.
 *
 * Morphy's Opera Game — short, famous, and it contains everything the analysis
 * UI has to render: a quiet inaccuracy, an outright blunder, two piece
 * sacrifices, and a forced mate at the end.
 */
export const SAMPLE_PGN = `[Event "Paris Opera"]
[Site "Paris FRA"]
[Date "1858.11.02"]
[White "Paul Morphy"]
[Black "Duke Karl / Count Isouard"]
[Result "1-0"]

1. e4 e5 2. Nf3 d6 3. d4 Bg4 4. dxe5 Bxf3 5. Qxf3 dxe5 6. Bc4 Nf6 7. Qb3 Qe7
8. Nc3 c6 9. Bg5 b5 10. Nxb5 cxb5 11. Bxb5+ Nbd7 12. O-O-O Rd8 13. Rxd7 Rxd7
14. Rd1 Qe6 15. Bxd7+ Nxd7 16. Qb8+ Nxb8 17. Rd8# 1-0`;
