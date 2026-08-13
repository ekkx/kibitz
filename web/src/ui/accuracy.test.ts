import { describe, expect, it } from 'vitest';
import { accuracySummary, mainlineNodes } from './accuracy.ts';
import type { GameTree, Node, PositionAnalysis } from '../api/types.ts';

const node = (id: number, parent: number | null, children: number[]): Node => ({
  id,
  parent,
  children,
  san: id === 0 ? null : `m${id}`,
  uci: null,
  fen: '',
  analysis: null,
  opening: null,
});

/** 0 → 1 → 2, with 1 also having a side branch 3 → 4. */
const TREE: GameTree = {
  root: 0,
  nodes: [node(0, null, [1]), node(1, 0, [2, 3]), node(2, 1, []), node(3, 1, [4]), node(4, 3, [])],
};

describe('mainlineNodes', () => {
  it('follows children[0] and leaves branches out', () => {
    expect(mainlineNodes(TREE).map((n) => n.id)).toEqual([1, 2]);
  });

  it('returns nothing for a session opened at a position', () => {
    expect(mainlineNodes({ root: 0, nodes: [node(0, null, [])] })).toEqual([]);
  });

  it('terminates on a tree that points at itself', () => {
    const cyclic: GameTree = { root: 0, nodes: [node(0, null, [0])] };
    expect(mainlineNodes(cyclic).length).toBeLessThanOrEqual(cyclic.nodes.length + 1);
  });
});

/** `fen` decides who played the move: "b" to move means White just played. */
const analysed = (fenSide: 'w' | 'b', accuracy: number, classification: 'best' | 'blunder') =>
  ({
    fen: `8/8/8/8/8/8/8/8 ${fenSide} - - 0 1`,
    depth: 18,
    candidates: [],
    explanations: {},
    context: {
      played: {
        san: 'x',
        uci: 'a1a2',
        win_prob_before: 0.5,
        win_prob_after: 0.5,
        delta: 0,
        classification,
        accuracy,
      },
      counterfactual: null,
    },
  }) satisfies PositionAnalysis;

describe('accuracySummary', () => {
  it('averages each side separately and counts classifications', () => {
    const summary = accuracySummary([
      analysed('b', 90, 'best'), // White
      analysed('w', 50, 'blunder'), // Black
      analysed('b', 80, 'best'), // White
    ]);
    expect(summary).toEqual({
      whiteAccuracy: 85,
      blackAccuracy: 50,
      moves: 3,
      counts: { best: 2, blunder: 1 },
    });
  });

  it('ignores the root, which has no played move', () => {
    const root: PositionAnalysis = {
      fen: '8/8/8/8/8/8/8/8 w - - 0 1',
      depth: 18,
      candidates: [],
      context: null,
      explanations: {},
    };
    expect(accuracySummary([root])).toEqual({
      whiteAccuracy: 0,
      blackAccuracy: 0,
      moves: 0,
      counts: {},
    });
  });
});
