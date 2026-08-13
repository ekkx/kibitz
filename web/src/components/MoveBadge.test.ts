import { describe, expect, it } from 'vitest';
import type { Key } from 'chessground/types';
import type { Classification } from '../api/types.ts';
import { BADGE_MARK, BADGE_SIZE, GRID, badgePlacement } from './MoveBadge.tsx';

/**
 * Two things are worth testing here and neither needs a DOM: the arithmetic
 * that turns a square into two offsets, and the invariants of the drawn mark
 * set — that every classification has a mark, that the family shares one stroke
 * system, and that no mark has grown out of the disc it is drawn in. How a mark
 * *looks* is not assertable and is not attempted; these guard the properties
 * that would silently break it.
 */

/** Half a badge, the amount the disc overhangs the corner it is centred on. */
const HALF = BADGE_SIZE / 2;

const ALL_CLASSIFICATIONS: Classification[] = [
  'book',
  'great',
  'best',
  'excellent',
  'good',
  'inaccuracy',
  'mistake',
  'blunder',
  'miss',
];

describe('badgePlacement', () => {
  it('centres the disc on the top-right corner of the square, White at the bottom', () => {
    // e4 is the 5th column from the left and the 5th row from the top, so its
    // top-right corner is at 62.5% / 50% and the disc straddles it.
    expect(badgePlacement('e4', 'white')).toEqual({ left: 62.5 - HALF, top: 50 - HALF });
    expect(badgePlacement('e4', 'white')).toEqual({ left: 60.25, top: 47.75 });
  });

  it('follows the picture, not the board, when the orientation flips', () => {
    // Same square, mirrored in both axes: e4 becomes the 4th column and the 4th
    // row, and the badge moves to what is now that square's top-right corner.
    expect(badgePlacement('e4', 'black')).toEqual({ left: 50 - HALF, top: 37.5 - HALF });
    expect(badgePlacement('e4', 'black')).toEqual({ left: 47.75, top: 35.25 });
  });

  it('puts a1 at the bottom-left for White and the top-right for Black', () => {
    expect(badgePlacement('a1', 'white')).toEqual({ left: 12.5 - HALF, top: 87.5 - HALF });
    // Flipped, a1 is the far corner and both offsets clamp — see below.
    expect(badgePlacement('a1', 'black')).toEqual({ left: 100 - BADGE_SIZE, top: 0 });
  });

  /**
   * `.board-wrap` is `overflow: hidden`, so the far corner is the case that
   * would otherwise be drawn as a quarter-disc with a clipped glyph.
   */
  describe('at the edges the board would clip', () => {
    it('pulls the corner square’s badge fully inside the board', () => {
      // h8 unclamped would be left 97.75 (half of it past the right edge) and
      // top −2.25 (half of it above the board).
      expect(badgePlacement('h8', 'white')).toEqual({ left: 100 - BADGE_SIZE, top: 0 });
      expect(badgePlacement('h8', 'white')).toEqual({ left: 95.5, top: 0 });
    });

    it('clamps only the axis that overflows', () => {
      // The h-file overhangs to the right at every rank but the 8th; the 8th
      // rank overhangs upwards on every file but the h-file.
      expect(badgePlacement('h4', 'white')).toEqual({ left: 95.5, top: 47.75 });
      expect(badgePlacement('d8', 'white')).toEqual({ left: 50 - HALF, top: 0 });
    });

    it('clamps the mirrored corner when the board is flipped', () => {
      // Flipped, h8 is the near-left corner and needs no clamping at all, while
      // a1 has taken over as the square whose corner leaves the board.
      expect(badgePlacement('h8', 'black')).toEqual({ left: 12.5 - HALF, top: 87.5 - HALF });
      expect(badgePlacement('a1', 'black')).toEqual({ left: 95.5, top: 0 });
    });

    it('never lets any part of the disc leave the board', () => {
      const files = 'abcdefgh';
      for (const orientation of ['white', 'black'] as const) {
        for (const file of files) {
          for (let rank = 1; rank <= 8; rank += 1) {
            const placement = badgePlacement(`${file}${rank}` as Key, orientation);
            expect(placement).not.toBeNull();
            expect(placement!.left).toBeGreaterThanOrEqual(0);
            expect(placement!.top).toBeGreaterThanOrEqual(0);
            expect(placement!.left + BADGE_SIZE).toBeLessThanOrEqual(100);
            expect(placement!.top + BADGE_SIZE).toBeLessThanOrEqual(100);
          }
        }
      }
    });

    it('moves a clamped badge by less than half its width, so it stays on its square', () => {
      // The clamp is a nudge, not a relocation: at most half a disc — about a
      // sixth of a square — so the badge still overlaps the square it labels.
      const clamped = badgePlacement('h8', 'white')!;
      const unclamped = { left: 8 * 12.5 - HALF, top: 0 - HALF };
      expect(unclamped.left - clamped.left).toBeLessThanOrEqual(HALF);
      expect(clamped.top - unclamped.top).toBeLessThanOrEqual(HALF);
    });
  });

  it('mirrors exactly between the two orientations', () => {
    // Away from the clamped edges, flipping the board reflects the square in
    // both axes, so the two offsets for one square always sum to a constant.
    // The two constants differ because the anchors do: `left` is measured to a
    // square's *right* edge and `top` to its *top* edge, which is one square
    // apart — hence 9 squares' worth horizontally and 7 vertically.
    for (const square of ['e4', 'd5', 'c3', 'f6'] as const) {
      const white = badgePlacement(square, 'white')!;
      const black = badgePlacement(square, 'black')!;
      expect(white.left + black.left).toBe(112.5 - BADGE_SIZE);
      expect(white.top + black.top).toBe(87.5 - BADGE_SIZE);
    }
  });

  it('renders nothing for chessground’s off-board sentinel', () => {
    // `Key` includes 'a0', which chessground uses for "not a square".
    expect(badgePlacement('a0', 'white')).toBeNull();
    expect(badgePlacement('a0', 'black')).toBeNull();
  });
});

describe('BADGE_MARK', () => {
  it('has something to draw for every classification', () => {
    // The badge only exists when there is a verdict, so unlike the move list's
    // notation glyphs none of these may be empty.
    for (const classification of ALL_CLASSIFICATIONS) {
      const mark = BADGE_MARK[classification];
      expect(mark.length).toBeGreaterThan(0);
      for (const part of mark) expect(part.d).toBeTruthy();
    }
    expect(Object.keys(BADGE_MARK).sort()).toEqual([...ALL_CLASSIFICATIONS].sort());
  });

  it('tells excellent and good apart by colour alone', () => {
    // A difference of degree, drawn as one. See the table's comment. They share
    // the drawing by *identity*, not by two path strings someone has to keep
    // equal, so this is checking the thing the table promises.
    expect(BADGE_MARK.excellent).toBe(BADGE_MARK.good);
  });

  it('draws geometry, never a typeset character', () => {
    // The whole reason this file exists. Path data made of absolute moves,
    // lines, curves and arcs cannot pick up a stroke weight from the OS's font
    // fallback, which is what the `✓ ★ ?! §` this replaced did on every machine
    // it ran on. Uppercase commands only — the grid check below is only
    // meaningful because every coordinate is absolute.
    for (const mark of Object.values(BADGE_MARK)) {
      for (const part of mark) expect(part.d).toMatch(/^[MLCAZ\d\s.]+$/);
    }
  });

  it('gives the whole family one stroke width and one fill treatment', () => {
    // Nine symbols read as a set because their lines are the same weight. A
    // mark drawn at its own weight would be exactly the inconsistency the font
    // version had, reintroduced by hand.
    const widths = new Set(
      Object.values(BADGE_MARK)
        .flat()
        .map((part) => part.stroke)
        .filter((stroke) => stroke !== 0),
    );
    expect(widths.size).toBe(1);
  });

  it('keeps every mark inside the box it is authored in', () => {
    // Every path is drawn in the same `0 0 24 24` grid; `place` is the only
    // thing allowed to move one out of it, and only ever inwards. A coordinate
    // outside the grid means a mark has grown past the disc that clips it.
    for (const mark of Object.values(BADGE_MARK)) {
      for (const part of mark) {
        for (const number of part.d.match(/[\d.]+/g) ?? []) {
          expect(Number(number)).toBeGreaterThanOrEqual(0);
          expect(Number(number)).toBeLessThanOrEqual(GRID);
        }
      }
    }
  });

  it('builds the two-mark badges out of the single marks, not out of new drawings', () => {
    // `?!` is the `?` and the `!`, scaled and slid apart by a transform; `??` is
    // the `?` twice. Redrawing them separately is how a set drifts — the pair's
    // question mark would slowly stop being the same question mark.
    const data = (mark: readonly { d: string }[]): string[] => mark.map((part) => part.d);

    expect(data(BADGE_MARK.inaccuracy)).toEqual([
      ...data(BADGE_MARK.mistake),
      ...data(BADGE_MARK.great),
    ]);
    expect(data(BADGE_MARK.blunder)).toEqual([
      ...data(BADGE_MARK.mistake),
      ...data(BADGE_MARK.mistake),
    ]);

    // And they are the only marks that are placed rather than drawn as authored.
    for (const classification of ALL_CLASSIFICATIONS) {
      const placed = BADGE_MARK[classification].every((part) => part.transform !== undefined);
      expect(placed).toBe(classification === 'inaccuracy' || classification === 'blunder');
    }
  });
});
