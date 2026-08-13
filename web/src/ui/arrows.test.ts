import { describe, expect, it } from 'vitest';
import {
  ARROW_BRUSHES,
  BRUSH,
  EQUAL_WIN_PROB,
  MAX_ARROWS,
  WEIGHT_BRUSHES,
  positionShapes,
  previewShapes,
  weightLevels,
} from './arrows.ts';
import type { Candidate, PositionAnalysis, PlayedMove } from '../api/types.ts';

/** After 1.e4 e5 2.Nf3 d6 3.d4 Bg4 — White to move, Black just played Bg4. */
const AFTER_BG4 = 'rn1qkbnr/ppp2ppp/3p4/4p3/3PP1b1/5N2/PPP2PPP/RNBQKB1R w KQkq - 1 4';
/** The position Bg4 was played from. */
const BEFORE_BG4 = 'rnbqkbnr/ppp2ppp/3p4/4p3/3PP3/5N2/PPP2PPP/RNBQKB1R b KQkq - 0 3';

const candidate = (san: string, uci: string, pv: string[], winProb = 0.52): Candidate => ({
  san,
  uci,
  score: { kind: 'cp', value: 20 },
  win_prob: winProb,
  pv,
});

/** Legal White moves in `AFTER_BG4`, best first, with the given win probs. */
const ranked = (...winProbs: number[]): Candidate[] =>
  [
    candidate('Nc3', 'b1c3', ['Nc3']),
    candidate('dxe5', 'd4e5', ['dxe5']),
    candidate('Be2', 'f1e2', ['Be2']),
    candidate('c3', 'c2c3', ['c3']),
    candidate('h3', 'h2h3', ['h3']),
    candidate('a3', 'a2a3', ['a3']),
  ]
    .slice(0, winProbs.length)
    .map((entry, index) => ({ ...entry, win_prob: winProbs[index]! }));

const played = (san: string, uci: string, classification: PlayedMove['classification']): PlayedMove => ({
  san,
  uci,
  win_prob_before: 0.5,
  win_prob_after: 0.3,
  delta: -0.2,
  classification,
  accuracy: 40,
});

const analysis = (over: Partial<PositionAnalysis> = {}): PositionAnalysis => ({
  fen: AFTER_BG4,
  depth: 18,
  candidates: [candidate('Nc3', 'b1c3', ['Nc3', 'Nc6', 'Be3'])],
  context: null,
  explanations: {},
  ...over,
});

describe('positionShapes', () => {
  it('draws nothing without an analysis', () => {
    expect(positionShapes(null)).toEqual([]);
  });

  it('draws one green arrow for the best move in the position', () => {
    expect(positionShapes(analysis())).toEqual([
      { orig: 'b1', dest: 'c3', brush: BRUSH.best },
    ]);
  });

  it('leaves a mated position alone', () => {
    expect(positionShapes(analysis({ candidates: [] }))).toEqual([]);
  });

  it('draws three arrows by default, heaviest first', () => {
    const shapes = positionShapes(analysis({ candidates: ranked(0.6, 0.5, 0.4, 0.3) }));
    expect(shapes).toEqual([
      { orig: 'b1', dest: 'c3', brush: WEIGHT_BRUSHES[0] },
      { orig: 'd4', dest: 'e5', brush: WEIGHT_BRUSHES[1] },
      { orig: 'f1', dest: 'e2', brush: WEIGHT_BRUSHES[2] },
    ]);
  });

  it('draws nothing at all when the user asks for no arrows', () => {
    expect(positionShapes(analysis({ candidates: ranked(0.6, 0.5, 0.4) }), 0)).toEqual([]);
    // Including the mistake pair: zero arrows means a bare board.
    const shapes = positionShapes(
      analysis({
        context: {
          played: played('Bg4', 'c8g4', 'blunder'),
          candidates: [candidate('Nf6', 'g8f6', ['Nf6'])],
          counterfactual: null,
        },
      }),
      0,
    );
    expect(shapes).toEqual([]);
  });

  it('never draws more arrows than the engine returns candidates', () => {
    const shapes = positionShapes(analysis({ candidates: ranked(0.6, 0.5, 0.4, 0.3, 0.2) }), 99);
    expect(shapes).toHaveLength(MAX_ARROWS);
  });

  it('draws every candidate asked for, whatever the ramp is able to distinguish', () => {
    // The setting's ceiling is the server's MultiPV width, not the ramp length:
    // every candidate asked for is drawn, and the ramp saturates if it runs out
    // rather than dropping arrows.
    const shapes = positionShapes(analysis({ candidates: ranked(0.9, 0.7, 0.5, 0.3, 0.1) }), MAX_ARROWS);
    expect(shapes).toHaveLength(MAX_ARROWS);
    expect(shapes.map((shape) => shape.brush)).toEqual(
      [0, 1, 2, 3, 4].map((level) => WEIGHT_BRUSHES[Math.min(level, WEIGHT_BRUSHES.length - 1)]),
    );
  });

  it('stops at the number of candidates the engine actually returned', () => {
    expect(positionShapes(analysis({ candidates: ranked(0.6, 0.5) }), 5)).toHaveLength(2);
  });
});

describe('weightLevels', () => {
  it('gives every step down the ranking less weight when the moves differ', () => {
    expect(weightLevels(ranked(0.6, 0.5, 0.4))).toEqual([0, 1, 2]);
  });

  it('gives moves within the equal-enough band the same weight', () => {
    expect(weightLevels(ranked(0.6, 0.596, 0.593))).toEqual([0, 0, 0]);
  });

  it('drops a weight as soon as the gap is a real one', () => {
    // Just under and just over the threshold, so the boundary itself is pinned.
    expect(weightLevels(ranked(0.6, 0.6 - EQUAL_WIN_PROB + 0.001))).toEqual([0, 0]);
    expect(weightLevels(ranked(0.6, 0.6 - EQUAL_WIN_PROB))).toEqual([0, 1]);
  });

  it('measures each candidate against the head of its cluster, not its neighbour', () => {
    // Steps of 0.005 would chain into one cluster if compared pairwise; against
    // the cluster head, the third move has drifted past the band and pays for it.
    expect(weightLevels(ranked(0.6, 0.595, 0.59, 0.585))).toEqual([0, 0, 1, 1]);
  });

  it('is never increasing, so the drawing cannot invert the ranking', () => {
    const levels = weightLevels(ranked(0.6, 0.4, 0.39, 0.2, 0.1));
    expect(levels).toEqual([...levels].sort((a, b) => a - b));
  });

  it('saturates rather than running off the end of the ramp', () => {
    // Six clusters, five rungs: the last two share the lightest weight instead
    // of asking for a rung that does not exist.
    const levels = weightLevels(ranked(0.9, 0.8, 0.7, 0.6, 0.5, 0.4));
    expect(levels).toEqual([0, 1, 2, 3, 4, 4]);
    expect(levels[levels.length - 1]).toBe(WEIGHT_BRUSHES.length - 1);
  });
});

/**
 * The two ends of the threshold, held against numbers the engine actually
 * returned for `testdata/opera_game.pgn` at the default depth 12. A threshold
 * that fails either of these is wrong regardless of how it reads in the
 * abstract: one end is a quiet position, the other is the position the whole
 * sample game turns on. Both have to come out as a *visible ranking* — that is
 * what the weighted arrows are for.
 */
describe('weightLevels on real engine output', () => {
  it('still ranks the opening, where nothing is decided but something is preferred', () => {
    // e4 .5404 / Nf3 .5322 / d4 .5267 / c4 .5248 / e3 .5221. The spread is only
    // 0.02, so this is a soft ranking rather than a verdict — but it is the
    // ranking the engine returned, and drawing it flat would throw it away.
    // The exact evaluations sit in the panel for anyone checking.
    expect(weightLevels(ranked(0.5404, 0.5322, 0.5267, 0.5248, 0.5221))).toEqual([0, 0, 1, 1, 1]);
  });

  it('stands the best move alone when the engine really has one', () => {
    // Before 10.Nxb5: Nxb5 .7532 / Bxf6 .6364 / Bxb5 .6078 / Be2 .5313 / Bd3 .5230.
    // Nxb5 leads by 0.1168 and is drawn alone; the two bishop takes are 0.03
    // apart, so they separate too; the quiet bishop moves are within a rounding
    // error of each other and share the last weight.
    const levels = weightLevels(ranked(0.7532, 0.6364, 0.6078, 0.5313, 0.5230));
    expect(levels).toEqual([0, 1, 2, 3, 3]);
    expect(levels.filter((level) => level === 0)).toHaveLength(1);
  });
});

describe('the weight ramp', () => {
  it('is not what bounds the arrow-count setting', () => {
    // The ceiling is the server's MultiPV width (docs/API.md: five candidates),
    // and the ramp is free to be any length: `weightLevels` saturates, so the
    // two numbers are allowed to disagree in either direction.
    expect(MAX_ARROWS).toBe(5);
    const levels = weightLevels(ranked(0.9, 0.8, 0.7, 0.6, 0.5, 0.4));
    expect(levels).toHaveLength(6);
    expect(Math.max(...levels)).toBeLessThanOrEqual(WEIGHT_BRUSHES.length - 1);
  });

  it('gets thinner and fainter at every step, so rank 1 dominates', () => {
    const ramp = WEIGHT_BRUSHES.map((name) => ARROW_BRUSHES[name]!);
    for (let level = 1; level < ramp.length; level += 1) {
      expect(ramp[level]!.lineWidth).toBeLessThan(ramp[level - 1]!.lineWidth);
      expect(ramp[level]!.opacity).toBeLessThan(ramp[level - 1]!.opacity);
    }
  });

  it('draws the mistake pair at equal weight, so neither half looks lesser', () => {
    const best = ARROW_BRUSHES[BRUSH.best]!;
    const mistake = ARROW_BRUSHES[BRUSH.mistake]!;
    expect(mistake.lineWidth).toBe(best.lineWidth);
    expect(mistake.opacity).toBe(best.opacity);
    expect(mistake.color).not.toBe(best.color);
  });
});

describe('positionShapes on a mistake', () => {
  it('keeps ranking the position when the move played was not a mistake', () => {
    const shapes = positionShapes(
      analysis({
        candidates: ranked(0.6, 0.5),
        context: {
          played: played('Bg4', 'c8g4', 'good'),
          candidates: [candidate('Nf6', 'g8f6', ['Nf6'])],
          counterfactual: null,
        },
      }),
    );
    expect(shapes).toEqual([
      { orig: 'b1', dest: 'c3', brush: WEIGHT_BRUSHES[0] },
      { orig: 'd4', dest: 'e5', brush: WEIGHT_BRUSHES[1] },
    ]);
  });

  /**
   * The pair is a comparison between two moves from the *previous* position;
   * the ranking is of moves from this one. Drawing both would put green arrows
   * in two frames at once, so the ranking is suppressed rather than trimmed.
   */
  it('suppresses the ranking entirely, however many arrows are asked for', () => {
    const shapes = positionShapes(
      analysis({
        candidates: ranked(0.6, 0.5, 0.4, 0.3, 0.2),
        context: {
          played: played('Bg4', 'c8g4', 'blunder'),
          candidates: [candidate('Nf6', 'g8f6', ['Nf6'])],
          counterfactual: null,
        },
      }),
      5,
    );
    expect(shapes).toHaveLength(2);
  });

  it('pairs the mistake with the move that should have been played', () => {
    const shapes = positionShapes(
      analysis({
        context: {
          played: played('Bg4', 'c8g4', 'blunder'),
          candidates: [candidate('Nf6', 'g8f6', ['Nf6'])],
          counterfactual: null,
        },
      }),
    );
    expect(shapes).toEqual([
      { orig: 'g8', dest: 'f6', brush: BRUSH.best },
      { orig: 'c8', dest: 'g4', brush: BRUSH.mistake },
    ]);
    // The two must be told apart by colour, which is the point of the pair.
    expect(BRUSH.best).not.toBe(BRUSH.mistake);
  });

  it('draws the mistake alone when it was also the engine’s first choice', () => {
    const shapes = positionShapes(
      analysis({
        context: {
          played: played('Bg4', 'c8g4', 'blunder'),
          // The engine ranked the played move first from the previous position
          // and the deeper look from here still calls it a blunder.
          candidates: [candidate('Bg4', 'c8g4', ['Bg4'])],
          counterfactual: null,
        },
      }),
    );
    expect(shapes).toEqual([{ orig: 'c8', dest: 'g4', brush: BRUSH.mistake }]);
  });

  it('falls back to the alternative_collapse line when the server sends no ranking', () => {
    const shapes = positionShapes(
      analysis({
        context: {
          played: played('Bg4', 'c8g4', 'mistake'),
          counterfactual: {
            kind: 'alternative_collapse',
            start_fen: BEFORE_BG4,
            pv: ['Nd7', 'c4', 'Ngf6'],
            motifs: [],
          },
        },
      }),
    );
    expect(shapes).toEqual([
      { orig: 'b8', dest: 'd7', brush: BRUSH.best },
      { orig: 'c8', dest: 'g4', brush: BRUSH.mistake },
    ]);
  });
});

describe('previewShapes', () => {
  it('draws the move and the expected reply, and no more', () => {
    const shapes = previewShapes(AFTER_BG4, candidate('dxe5', 'd4e5', ['dxe5', 'Bxf3', 'Qxf3']));
    expect(shapes).toEqual([
      { orig: 'd4', dest: 'e5', brush: BRUSH.preview },
      { orig: 'g4', dest: 'f3', brush: BRUSH.previewReply },
    ]);
  });

  it('still draws the move when the line does not fit the position', () => {
    const shapes = previewShapes(AFTER_BG4, candidate('Nc3', 'b1c3', ['Qxh8', 'Rxa9']));
    expect(shapes).toEqual([{ orig: 'b1', dest: 'c3', brush: BRUSH.preview }]);
  });
});
