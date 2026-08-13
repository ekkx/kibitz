import { describe, expect, it } from 'vitest';
import { needsPromotion, playMove } from './rules.ts';

/**
 * Promotion, which is the one move the board cannot finish on its own: the
 * question of *what* the pawn becomes has to come back from the user, so the
 * two pieces the picker is built on — "does this move need asking about" and
 * "play it as the piece that was chosen" — are pinned down here.
 */

/** White pawn on b7, a black rook on a8 and c8 to capture into. */
const WHITE_SEVENTH = 'r1r3k1/1P6/8/8/8/8/6PP/6K1 w - - 0 1';
/** Black pawn on g2, one square from the first rank, with g1 empty. */
const BLACK_SECOND = '6k1/8/8/8/8/8/6p1/R4K2 b - - 0 1';
/**
 * A pawn on the seventh that cannot move at all: c7 is pinned *along the rank*
 * by the rook on h7, so pushing it would expose the king on a7. A file pin
 * would not do — a pawn pinned down its own file may still push.
 */
const PINNED = '8/K1P4r/8/8/4k3/8/8/8 w - - 0 1';

describe('needsPromotion', () => {
  it('is true for a pawn pushed to the last rank', () => {
    expect(needsPromotion(WHITE_SEVENTH, 'b7', 'b8')).toBe(true);
  });

  it('is true for a capture onto the last rank', () => {
    expect(needsPromotion(WHITE_SEVENTH, 'b7', 'a8')).toBe(true);
    expect(needsPromotion(WHITE_SEVENTH, 'b7', 'c8')).toBe(true);
  });

  it('is true for Black arriving on the first rank', () => {
    expect(needsPromotion(BLACK_SECOND, 'g2', 'g1')).toBe(true);
  });

  it('is false for every ordinary move', () => {
    expect(needsPromotion(WHITE_SEVENTH, 'g2', 'g4')).toBe(false);
    expect(needsPromotion(WHITE_SEVENTH, 'g1', 'f1')).toBe(false);
  });

  it('is false for a move that is not legal, promotion-shaped or not', () => {
    // The pawn is on the seventh and the square in front is empty, but moving
    // it would expose the king — so there is nothing to ask about.
    expect(needsPromotion(PINNED, 'c7', 'c8')).toBe(false);
    expect(needsPromotion(WHITE_SEVENTH, 'b7', 'b6')).toBe(false);
  });

  it('is false on a FEN that is not a position', () => {
    expect(needsPromotion('not a fen', 'b7', 'b8')).toBe(false);
  });
});

describe('playMove with a promotion piece', () => {
  it('still auto-queens when nobody says otherwise', () => {
    const move = playMove(WHITE_SEVENTH, 'b7', 'b8');
    expect(move?.san).toBe('b8=Q');
    expect(move?.uci).toBe('b7b8q');
  });

  it('plays the underpromotion it is given', () => {
    expect(playMove(WHITE_SEVENTH, 'b7', 'b8', 'n')?.san).toBe('b8=N');
    expect(playMove(WHITE_SEVENTH, 'b7', 'b8', 'r')?.uci).toBe('b7b8r');
    expect(playMove(WHITE_SEVENTH, 'b7', 'b8', 'b')?.uci).toBe('b7b8b');
  });

  it('carries the piece through a capture, and into the resulting position', () => {
    const move = playMove(WHITE_SEVENTH, 'b7', 'c8', 'n');
    expect(move?.san).toBe('bxc8=N');
    expect(move?.fen.split(' ')[0]).toBe('r1N3k1/8/8/8/8/8/6PP/6K1');
  });

  it('promotes for Black too', () => {
    expect(playMove(BLACK_SECOND, 'g2', 'g1', 'n')?.san).toBe('g1=N');
  });

  it('ignores the piece on a move that is not a promotion', () => {
    const move = playMove(WHITE_SEVENTH, 'g2', 'g4', 'n');
    expect(move?.san).toBe('g4');
    expect(move?.uci).toBe('g2g4');
  });

  it('refuses an illegal move whatever piece is asked for', () => {
    expect(playMove(PINNED, 'c7', 'c8', 'q')).toBeNull();
  });
});
