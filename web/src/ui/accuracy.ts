import type { Classification, GameTree, Node, PositionAnalysis } from '../api/types.ts';

/**
 * The accuracy summary shown under the move list.
 *
 * It is derived from a list of `PositionAnalysis`, and deliberately not from
 * the sweep's SSE events alone: a session the server has already analysed
 * carries the same analyses in `tree.nodes[*].analysis`, and re-opening it must
 * produce the same summary without running the engine again.
 */
export interface SweepSummary {
  /** Mean accuracy over the moves each side played, 0..100. */
  whiteAccuracy: number;
  blackAccuracy: number;
  moves: number;
  counts: Partial<Record<Classification, number>>;
}

/** The mainline, root first, following `children[0]` (API.md). Excludes the root. */
export function mainlineNodes(tree: GameTree): Node[] {
  const byId = new Map<number, Node>();
  for (const node of tree.nodes) byId.set(node.id, node);

  const line: Node[] = [];
  let walker = byId.get(tree.root);
  // Bounded by the node count so a malformed tree cannot spin here forever.
  for (let steps = 0; walker && steps <= tree.nodes.length; steps++) {
    const nextId = walker.children[0];
    if (nextId === undefined) break;
    const next = byId.get(nextId);
    if (!next) break;
    line.push(next);
    walker = next;
  }
  return line;
}

/**
 * An accumulator so the sweep can fold analyses in as they stream, and a
 * finished session can fold in all of them at once. Same arithmetic either way.
 */
export interface AccuracyAccumulator {
  white: number[];
  black: number[];
  counts: Partial<Record<Classification, number>>;
}

export const emptyAccumulator = (): AccuracyAccumulator => ({ white: [], black: [], counts: {} });

export function accumulate(accumulator: AccuracyAccumulator, analysis: PositionAnalysis): void {
  const played = analysis.context?.played;
  if (!played) return;
  // The move was made by whoever is *not* to move in the resulting FEN.
  const byWhite = analysis.fen.split(' ')[1] === 'b';
  (byWhite ? accumulator.white : accumulator.black).push(played.accuracy);
  accumulator.counts[played.classification] =
    (accumulator.counts[played.classification] ?? 0) + 1;
}

export function summarize(accumulator: AccuracyAccumulator): SweepSummary {
  return {
    whiteAccuracy: mean(accumulator.white),
    blackAccuracy: mean(accumulator.black),
    moves: accumulator.white.length + accumulator.black.length,
    counts: accumulator.counts,
  };
}

/** One-shot form, for a session whose analyses were already on the tree. */
export function accuracySummary(analyses: readonly PositionAnalysis[]): SweepSummary {
  const accumulator = emptyAccumulator();
  for (const analysis of analyses) accumulate(accumulator, analysis);
  return summarize(accumulator);
}

function mean(values: number[]): number {
  if (values.length === 0) return 0;
  return Math.round((values.reduce((sum, value) => sum + value, 0) / values.length) * 10) / 10;
}
