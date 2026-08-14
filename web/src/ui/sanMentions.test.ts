import { describe, expect, it } from 'vitest';
import {
  explanationFens,
  findSanMentions,
  findSanMentionsIn,
  sanFrames,
  type SanMention,
} from './sanMentions.ts';
import type { PositionAnalysis } from '../api/types.ts';

/**
 * The Opera Game again, at the moment the app is built around: 10.Nxb5 has just
 * been played, and the explanation is about it.
 */
/** Before 10.Nxb5 — White to move. */
const BEFORE = 'r3kb1r/p2nqppp/2p2n2/1p2p1B1/2B1P3/1QN5/PPP2PPP/R3K2R w KQkq - 0 10';
/** After 10.Nxb5 — Black to move, and 10…cxb5 is the reply. */
const AFTER = 'r3kb1r/p2nqppp/2p2n2/1N2p1B1/2B1P3/1Q6/PPP2PPP/R3K2R b KQkq - 0 10';

/**
 * Each mention as the whole of what it claims, and so as what the board will
 * draw: "canonical SAN@fromto" for a move, which is an arrow, and "circle:e8"
 * for a bare coordinate that is no move here, which is a circle on that square.
 */
const claim = (mention: SanMention) =>
  mention.kind === 'move'
    ? `${mention.san}@${mention.from}${mention.to}`
    : `circle:${mention.square}`;

const at = (text: string, fen: string) => findSanMentions(text, fen).map(claim);

describe('findSanMentions', () => {
  it('resolves a move written in prose to its two squares', () => {
    expect(at('Nxb5 wins a pawn.', BEFORE)).toEqual(['Nxb5@c3b5']);
    expect(at('Black answers cxb5.', AFTER)).toEqual(['cxb5@c6b5']);
  });

  it('leaves a token that is not legal here as plain text', () => {
    // The difference between a feature and a bug: `Nf3` spells a knight move
    // and no knight can reach f3 in this position, so it is drawn as nothing
    // rather than as a confident, specific, wrong arrow.
    expect(at('Nf3 is not available.', BEFORE)).toEqual([]);
    // And a move of the *other* side does not resolve in this frame either.
    expect(at('cxb5 is the reply.', BEFORE)).toEqual([]);
  });

  it('never matches inside a word', () => {
    // Both of these contain `Nxb5`, which is legal here; neither is a mention.
    expect(at('aNxb5 and Nxb5z are not moves.', BEFORE)).toEqual([]);
    expect(at('Bxb5Nxb5 runs two together.', BEFORE)).toEqual([]);
  });

  it('matches against kana with no space, as Japanese explanations write it', () => {
    expect(at('Nxb5は駒を取ります。', BEFORE)).toEqual(['Nxb5@c3b5']);
  });

  it('keeps the annotation the model wrote as part of the token', () => {
    const [mention] = findSanMentions('Nxb5!? is the try.', BEFORE);
    expect(mention).toMatchObject({ kind: 'move', text: 'Nxb5!?', san: 'Nxb5' });
  });

  it('reads a move number as notation rather than as part of the move', () => {
    expect(at('After 10.Nxb5 the pawn falls.', BEFORE)).toEqual(['Nxb5@c3b5']);
    expect(at('After 10…cxb5 the pawn falls.', AFTER)).toEqual(['cxb5@c6b5']);
  });

  it('finds castling in either spelling, and only where it is legal', () => {
    const castled = 'r3kb1r/p2nqppp/5n2/1B2p1B1/4P3/1Q6/PPP2PPP/R3K2R w KQkq - 1 12';
    expect(at('O-O-O connects the rooks.', castled)).toEqual(['O-O-O@e1c1']);
    expect(at('0-0-0 connects the rooks.', castled)).toEqual(['O-O-O@e1c1']);
    // Black cannot castle here: the king is on e8 with a bishop still on f8.
    expect(at('O-O is not on.', AFTER)).toEqual([]);
  });

  it('reports where in the text each mention sits', () => {
    const text = 'The move Nxb5 wins a pawn.';
    const [mention] = findSanMentions(text, BEFORE);
    expect(text.slice(mention!.start, mention!.end)).toBe('Nxb5');
  });

  it('draws a bare coordinate that really is a legal push as that move', () => {
    expect(at('You should have played a6.', AFTER)).toEqual(['a6@a7a6']);
    // And the residue the rule accepts, documented rather than defended: a6 is
    // empty and playable, so a sentence that meant the *square* gets the arrow
    // anyway. Nothing in the token tells the two apart, and the arrow at least
    // ends on the square being talked about.
    expect(at('a6 is the weak square.', AFTER)).toEqual(['a6@a7a6']);
  });
});

/**
 * The other half of the rule: a coordinate that is not a move here names a
 * square, and is drawn as a circle on it without being checked against
 * anything, because a coordinate is a real square in every position.
 */
describe('findSanMentions on a bare coordinate that is no move', () => {
  it('points at the square the reader was reading about', () => {
    // The case this was reported for. It used to be inert: not a legal pawn
    // move, therefore not a mention, therefore not drawn at all.
    expect(at('Your king on e8 has no escape squares.', AFTER)).toEqual(['circle:e8']);
  });

  it('is sorted from a real push by occupancy, which is why legality works', () => {
    // f6 is exactly one square in front of the f7 pawn, so it would be a legal
    // push — except that the knight the sentence is about is standing on it.
    // That is the whole mechanism: a square named as a location usually has a
    // piece on it, and an occupied square is no pawn's destination.
    expect(at('The knight on f6 holds it together.', AFTER)).toEqual(['circle:f6']);
    // The contrast, in the same position and for the same pawn structure: c5
    // is empty and one square in front of the c6 pawn, so it stays a move.
    expect(at('He should play c5.', AFTER)).toEqual(['c5@c6c5']);
  });

  it('needs no position at all, and so survives having no frames', () => {
    expect(findSanMentionsIn('the e8 square', []).map(claim)).toEqual(['circle:e8']);
  });

  it('does not rescue a token that asserted a move', () => {
    // `+`, `#` and `=Q` describe a position only a move can produce, so these
    // fail the way `Nf3` fails — to nothing — rather than to a circle on d4.
    expect(at('d4+ is not on, nor is e8=Q.', AFTER)).toEqual([]);
  });
});

describe('findSanMentions on a token that asserts a move outright', () => {
  /** A promotion with nothing else on the board to complicate the SAN. */
  const PROMOTION = '8/4P3/8/8/7k/8/8/4K3 w - - 0 1';
  /** d3-d4 checks the king on c5, so the position spells the move `d4+`. */
  const CHECKING_PUSH = '8/8/8/2k5/8/3P4/8/4K3 w - - 0 1';

  it('draws a promotion, which is a coordinate and a claim', () => {
    expect(at('e8=Q ends it.', PROMOTION)).toEqual(['e8=Q@e7e8']);
  });

  it('draws a checking push', () => {
    expect(at('d4+ wins the bishop.', CHECKING_PUSH)).toEqual(['d4+@d3d4']);
  });
});

/**
 * Over-matching, which the legality check used to hide. A bare coordinate no
 * longer has to be a legal move to be drawn, so the pattern's boundaries are
 * now the only thing between `e8` the square and every `e8` that is part of
 * something else. Each of these is a string with a coordinate in it that means
 * something other than that square.
 */
describe('findSanMentions does not find a square inside a longer token', () => {
  it('leaves the coordinate inside a move it already matched', () => {
    // `Ne4` is not legal here, and the `e4` in it must not become a circle as
    // consolation: the text said a knight goes there, not "the e4 square".
    expect(at('Ne4 is impossible.', BEFORE)).toEqual([]);
    // And a move that does resolve is one mention, not a move and a square.
    expect(at('Nxb5 wins a pawn.', BEFORE)).toEqual(['Nxb5@c3b5']);
    expect(at('The reply cxb5 is forced.', AFTER)).toEqual(['cxb5@c6b5']);
  });

  it('leaves the digits of a move number, a percentage and an evaluation', () => {
    expect(at('After 10.Nxb5 the pawn falls.', BEFORE)).toEqual(['Nxb5@c3b5']);
    expect(at('Win probability fell 12% to 54%, or -1.5 pawns.', AFTER)).toEqual([]);
  });

  it('leaves both halves of a UCI move, which is two coordinates run together', () => {
    expect(at('The engine printed c6b5.', AFTER)).toEqual([]);
  });

  it('finds nothing in a FEN', () => {
    expect(at(`The position is ${AFTER}.`, AFTER)).toEqual([]);
    // Not quite airtight, and left that way on purpose: a rank consisting of a
    // lone black bishop, `/b7/`, puts a coordinate between two slashes and
    // nothing else. Refusing a coordinate next to a slash would cost a reading
    // prose really uses — "the e4/d5 tension" — to defend against a string the
    // model is told never to write.
    expect(at('8/8/8/b7/8/8/8/K1k5', AFTER)).toEqual(['circle:b7']);
  });

  it('still matches a coordinate that a non-ASCII word runs straight into', () => {
    expect(at('e8の局面は負けです。', AFTER)).toEqual(['circle:e8']);
  });
});

describe('findSanMentions while the text is still streaming', () => {
  const frames = sanFrames([BEFORE]);
  const streamed = (text: string) =>
    findSanMentionsIn(text, frames, { streaming: true }).map((m) => m.text);

  it('leaves the last token alone until it is known to be finished', () => {
    // `Nxb5` here may be one delta away from `Nxb5+`, or from a longer word.
    expect(streamed('White played Nxb5')).toEqual([]);
    expect(streamed('White played Nxb5 ')).toEqual(['Nxb5']);
    expect(streamed('White played Nxb5.')).toEqual(['Nxb5']);
  });

  it('holds a bare coordinate back too, because `e8` becomes `e8=Q`', () => {
    expect(streamed('the king on e8')).toEqual([]);
    expect(streamed('the king on e8 ')).toEqual(['e8']);
  });

  it('still matches every earlier token, so text lights up as it arrives', () => {
    expect(streamed('Nxb5 and then Bxb5 and then Nxb')).toEqual(['Nxb5', 'Bxb5']);
  });

  it('takes the last token once the stream is done', () => {
    expect(findSanMentionsIn('White played Nxb5', frames).map((m) => m.text)).toEqual(['Nxb5']);
  });
});

describe('findSanMentionsIn across frames', () => {
  const frames = sanFrames(explanationFens(analysisFixture(), BEFORE));

  it('resolves each mention in the frame it belongs to', () => {
    const found = findSanMentionsIn(
      'Nxb5 was a blunder: after cxb5 Bxb5 the piece is gone.',
      frames,
    );
    expect(found.map(claim)).toEqual([
      'Nxb5@c3b5', // the played move, from the position before it
      'cxb5@c6b5', // the reply, from the position on the board
      'Bxb5@c4b5', // one ply further down the punishment line
    ]);
  });

  it('prefers the position the explanation is about when a token fits two frames', () => {
    // `Bxb5` is legal before the move as well as two plies into the line, and
    // the earlier frame is the one the first paragraph is talking about.
    const [mention] = findSanMentionsIn('Bxb5 was the alternative.', frames);
    expect(mention).toMatchObject({ kind: 'move', fen: BEFORE });
  });

  it('has no move to point at without a position to find one in', () => {
    // Only moves need a frame; a bare coordinate has its own test above.
    expect(findSanMentionsIn('Nxb5 wins a pawn.', [])).toEqual([]);
  });
});

describe('explanationFens', () => {
  it('puts the position before the move first and the board second', () => {
    const fens = explanationFens(analysisFixture(), BEFORE);
    expect(fens[0]).toBe(BEFORE);
    expect(fens[1]).toBe(AFTER);
  });

  it('walks the counterfactual line, a frame per ply', () => {
    // Without these the second half of an explanation — which the prompt tells
    // the model to write by walking exactly this line — would be inert.
    const fens = explanationFens(analysisFixture(), BEFORE);
    expect(fens.length).toBeGreaterThan(2);
  });

  it('survives a line that does not fit the position it claims to start from', () => {
    const analysis = analysisFixture();
    analysis.context!.counterfactual!.pv = ['Qh8', 'Nonsense'];
    expect(() => explanationFens(analysis, BEFORE)).not.toThrow();
    expect(explanationFens(analysis, BEFORE)).toEqual([BEFORE, AFTER]);
  });

  it('has no frames at all without an analysis', () => {
    expect(explanationFens(null, BEFORE)).toEqual([]);
  });
});

function analysisFixture(): PositionAnalysis {
  return {
    fen: AFTER,
    depth: 12,
    candidates: [],
    explanations: {},
    context: {
      played: {
        san: 'Nxb5',
        uci: 'c3b5',
        win_prob_before: 0.6,
        win_prob_after: 0.3,
        delta: -0.3,
        classification: 'blunder',
        accuracy: 20,
      },
      counterfactual: {
        kind: 'refutation',
        start_fen: AFTER,
        pv: ['cxb5', 'Bxb5', 'Qb6'],
        motifs: [],
      },
    },
  };
}
