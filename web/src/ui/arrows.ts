import type { DrawBrush, DrawShape } from 'chessground/draw';
import type { Key } from 'chessground/types';
import type { Candidate, PositionAnalysis } from '../api/types.ts';
import { expandSanLine, uciToSquares } from '../chess/rules.ts';
import { isMistake } from './format.ts';

/**
 * Engine arrows on the board (chessground `drawable.autoShapes`).
 *
 * Two visual languages share this board and must not be confused for each
 * other, so they never appear at the same time:
 *
 *   1. **The game frame** — these arrows. They describe the position on the
 *      board right now and the move that produced it. Green is the engine's
 *      ranking, red is a move that lost material or the game.
 *   2. **The counterfactual replay** — the circles and arrows drawn by `App`
 *      once a replayed line reaches its end (DESIGN §8.5). Those describe a
 *      *hypothetical* position that is not the game, and they own the board for
 *      as long as the replay is running.
 *
 * `App` picks exactly one of the two; nothing here draws during a replay.
 *
 * Arrow density is bounded and *weighted*. Several arrows of equal thickness
 * answer no question at all, so the ranking is carried by the drawing itself:
 * the engine's move is drawn heavy and fully saturated, and each step down the
 * ranking is thinner, fainter and greyer (`WEIGHTS`). Rank 1 is meant to be
 * legible as the answer from across the room, with the alternatives visible as
 * alternatives rather than as competitors.
 *
 * A position shows either that ranking, or — when the move that led here was a
 * mistake — the pair "you played this / this was the move". Never both: those
 * two live in different frames (the ranking is of moves *from* this position,
 * the pair is of moves from the previous one) and green arrows in two frames at
 * once is exactly the confusion this avoids. On a mistake the ranking is
 * suppressed entirely rather than merely trimmed, because red-versus-green is a
 * *comparison* and a comparison with three green options is a list; the
 * runners-up stay in the panel, one hover away.
 */

/** Chessground's own green and red, kept as the anchor of the weight ramp. */
const GREEN = '#15781b';
const RED = '#882020';

/**
 * The weight ramp, best first. Thickness, opacity and saturation all fall
 * together — one perceptual dimension would be arguable in isolation, three
 * moving as one is not, and the ramp survives being looked at sideways or on a
 * dark board where opacity alone washes out.
 *
 * `lineWidth` is chessground's own unit: it is divided by 64 to get a stroke
 * width in board squares, so 13 is roughly a fifth of a square.
 *
 * Five rungs, one per candidate the server can return. Each step is the same
 * proportional drop (about ×0.78 in opacity, a similar fall in width), so the
 * ramp reads as one gesture rather than as five arbitrary styles, and the
 * bottom rung is still a visible arrow rather than a hairline. Levels saturate
 * at the last rung (`weightLevels`), so a position that somehow produces more
 * clusters than there are rungs still draws every arrow it was asked for.
 */
const WEIGHTS: readonly Omit<DrawBrush, 'key'>[] = [
  { color: GREEN, opacity: 1, lineWidth: 13 },
  { color: '#2b6d2e', opacity: 0.78, lineWidth: 10 },
  { color: '#3f6b42', opacity: 0.6, lineWidth: 8 },
  { color: '#536956', opacity: 0.47, lineWidth: 6 },
  { color: '#656766', opacity: 0.38, lineWidth: 5 },
];

/** Brush name per weight level; index 0 is the engine's move. */
export const WEIGHT_BRUSHES: readonly string[] = WEIGHTS.map((_, level) => `kibitzWeight${level}`);

/**
 * The ceiling on the arrow-count setting, and so on how many candidates are
 * drawn. Tied to the server's MultiPV width (`docs/API.md`: `candidates` holds
 * up to five moves), *not* to the length of the weight ramp. The two are equal
 * today, but nothing depends on that: several arrows sharing a weight is the
 * normal outcome of clustering, so a ramp shorter than this setting is a
 * supported state and `weightLevels` saturates rather than running off the end.
 */
export const MAX_ARROWS = 5;

/** Enough to see the shape of the position, few enough to read at a glance. */
export const DEFAULT_ARROWS = 3;

export const BRUSH = {
  /** The engine's move — weight level 0. */
  best: WEIGHT_BRUSHES[0]!,
  /** The move actually played, when it was a mistake. */
  mistake: 'kibitzMistake',
  /** A line the user is pointing at in the panel. */
  preview: 'blue',
  /** The reply inside a previewed line. */
  previewReply: 'paleBlue',
} as const;

/**
 * The brushes above, in the shape chessground wants them in `drawable.brushes`.
 * chessground deep-merges this over its built-in palette, so `blue` and
 * `paleBlue` — which the preview still uses — survive untouched.
 *
 * The mistake red is given the same weight as level 0 on purpose: the pair only
 * reads as a comparison if neither half looks like the lesser claim.
 */
export const ARROW_BRUSHES: Record<string, DrawBrush> = {
  ...Object.fromEntries(
    WEIGHTS.map((weight, level) => [
      WEIGHT_BRUSHES[level]!,
      { key: `kw${level}`, ...weight } satisfies DrawBrush,
    ]),
  ),
  [BRUSH.mistake]: { key: 'kmis', color: RED, opacity: 1, lineWidth: 13 },
};

/**
 * How close two candidates have to be, in win probability, before the board
 * stops claiming one is better than the other.
 *
 * One point of win probability: small enough that the ramp almost always has
 * something to say, which is the whole reason the arrows are weighted at all.
 *
 * This was briefly widened to 0.05, on the argument that reorderings between
 * search depths are almost all under 0.03, so any gap narrower than that is the
 * search being arbitrary rather than the position being decided. That argument
 * is correct about *search noise* and wrong about *what these arrows are for*.
 * At 0.05 nearly every position collapses its whole candidate list into a single
 * cluster: all three arrows come out identical, and the ranking the feature
 * exists to show is simply not drawn. A visible ordering that is sometimes noise
 * is more useful here than no ordering at all — and the panel beside the board
 * carries the exact evaluations, so anyone who wants to know whether a gap is
 * real can read the numbers rather than infer them from a stroke width.
 *
 * So the board draws the ranking the engine returned, at the resolution the
 * engine returned it, and reserves "these are equal" for moves that really are
 * within a rounding error of each other.
 */
export const EQUAL_WIN_PROB = 0.01;

/** How many plies of a previewed line to draw: the move, and the answer to it. */
const PREVIEW_PLIES = 2;

function arrow(uci: string | null | undefined, brush: string): DrawShape | null {
  const squares = uciToSquares(uci);
  if (!squares) return null;
  return { orig: squares[0], dest: squares[1], brush };
}

/**
 * The weight level of each candidate — the heart of the ranking.
 *
 * Weight follows the **evaluation gap**, not the rank number. A rank-driven
 * ramp would draw the second-best move thin even when it is worth exactly as
 * much as the first, which is a claim the engine never made and the one thing
 * the panel's numbers would immediately contradict. So candidates are clustered
 * instead: a candidate keeps the weight of the cluster it is in as long as it
 * is within `EQUAL_WIN_PROB` of the move that opened that cluster, and costs
 * one level of weight as soon as it is not. Three equal moves are drawn equal;
 * one clear best move is drawn alone and heavy, with everything behind it
 * visibly lighter.
 *
 * `win_prob` is the measure rather than `score`, because it is the scale the
 * rest of the app already judges moves on (`PlayedMove.delta`, accuracy) and
 * because centipawns are not linear in the thing being drawn: 30 centipawns
 * decides a level position and is irrelevant at +7.
 *
 * Clustering is measured against the cluster's own head, not the previous
 * candidate, so a long chain of near-equal steps cannot drift arbitrarily far
 * from the best move while still counting as equal to it. The result is
 * non-increasing by construction, so the drawing can never invert the ranking.
 *
 * More clusters than rungs saturates at the lightest rung. Past the bottom of
 * the ramp every remaining move is "clearly worse than the best" and the board
 * has nothing further to say about how they place among themselves — that is
 * the panel's job, where the numbers are.
 */
export function weightLevels(candidates: readonly Candidate[]): number[] {
  const levels: number[] = [];
  let level = 0;
  let clusterProb = candidates[0]?.win_prob ?? 0;

  candidates.forEach((candidate, index) => {
    if (index > 0 && clusterProb - candidate.win_prob >= EQUAL_WIN_PROB) {
      level = Math.min(level + 1, WEIGHT_BRUSHES.length - 1);
      clusterProb = candidate.win_prob;
    }
    levels.push(level);
  });
  return levels;
}

/**
 * The arrows for a node the user is simply looking at.
 *
 * `count` is the user's arrow-count setting (`state/useSettings.ts`). Zero means
 * a bare board: the ranking and the mistake pair are both part of the game
 * frame, so both go. The replay and the panel preview are a different language
 * and are unaffected — a hover is an explicit request, not board density.
 *
 * A mistake is drawn in the frame of the position *before* it — the played move
 * and the move that should have replaced it both start from squares as they
 * were a ply ago, which is how every review tool draws this, and the two arrows
 * only read as a pair because they share that frame.
 */
export function positionShapes(
  analysis: PositionAnalysis | null,
  count: number = DEFAULT_ARROWS,
): DrawShape[] {
  if (!analysis || count <= 0) return [];
  const played = analysis.context?.played ?? null;

  if (played && isMistake(played.classification)) {
    const shapes: DrawShape[] = [];
    const better = bestAlternative(analysis);
    // A move can be both the engine's first choice and a mistake: a fixed-depth
    // search from the previous position does not always see what the search one
    // ply later does, and some positions are simply lost whatever is played.
    // Drawing the green arrow anyway would stack it under the red one on the
    // same two squares — an invisible arrow making a claim the panel contradicts
    // with "rank #1". Nothing was better, so nothing is drawn as better.
    if (better && better !== played.uci) shapes.push(...compact([arrow(better, BRUSH.best)]));
    shapes.push(...compact([arrow(played.uci, BRUSH.mistake)]));
    if (shapes.length > 0) return shapes;
  }

  return rankedShapes(analysis.candidates, count);
}

/** The top `count` candidates, each drawn at the weight its evaluation earns. */
function rankedShapes(candidates: readonly Candidate[], count: number): DrawShape[] {
  const shown = candidates.slice(0, Math.min(count, MAX_ARROWS));
  const levels = weightLevels(shown);
  return compact(shown.map((candidate, index) => arrow(candidate.uci, WEIGHT_BRUSHES[levels[index]!]!)));
}

/**
 * The move that should have been played instead of the one that was.
 *
 * `context.candidates` is the engine's ranking of the *previous* position, so
 * its first entry is the answer. A server that omits it still leaves one: an
 * `alternative_collapse` counterfactual is by definition the line that starts
 * with the better move, from the position before the mistake.
 */
function bestAlternative(analysis: PositionAnalysis): string | null {
  const context = analysis.context;
  if (!context) return null;

  const ranked = context.candidates?.[0]?.uci;
  if (ranked) return ranked;

  const counterfactual = context.counterfactual;
  if (counterfactual?.kind === 'alternative_collapse') {
    const [first] = expandSanLine(counterfactual.start_fen, counterfactual.pv.slice(0, 1));
    if (first) return `${first.from}${first.to}`;
  }
  return null;
}

/**
 * The arrows for a candidate the user is pointing at in the panel: the move
 * itself and the reply the engine expects, which is the part of a line that is
 * hard to read as text and obvious as an arrow.
 */
export function previewShapes(fen: string, candidate: Candidate): DrawShape[] {
  const line = candidate.pv.length > 0 ? candidate.pv : [candidate.san];
  const steps = expandSanLine(fen, line.slice(0, PREVIEW_PLIES));
  if (steps.length === 0) return compact([arrow(candidate.uci, BRUSH.preview)]);

  return steps.map((step, index) => ({
    orig: step.from as Key,
    dest: step.to as Key,
    brush: index === 0 ? BRUSH.preview : BRUSH.previewReply,
  }));
}

function compact(shapes: (DrawShape | null)[]): DrawShape[] {
  return shapes.filter((shape): shape is DrawShape => shape !== null);
}
