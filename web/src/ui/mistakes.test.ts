import { describe, expect, it } from 'vitest';
import type { Classification, GameTree, Node, PositionAnalysis } from '../api/types.ts';
import { findMistake } from './mistakes.ts';

/**
 * A linear game: node 0 is the root, node n is the nth ply. Only the fields the
 * walk reads are filled in — this is a test of tree traversal, not of the wire
 * format.
 */
function mainline(plies: number): GameTree {
  const nodes: Node[] = [];
  for (let id = 0; id <= plies; id += 1) {
    nodes.push(node(id, id === 0 ? null : id - 1, id < plies ? [id + 1] : []));
  }
  return { nodes, root: 0 };
}

function node(id: number, parent: number | null, children: number[]): Node {
  return {
    id,
    parent,
    children,
    san: parent === null ? null : `m${id}`,
    uci: parent === null ? null : 'e2e4',
    fen: '8/8/8/8/8/8/8/8 w - - 0 1',
    analysis: null,
    opening: null,
  };
}

/** `{ 3: 'blunder' }` — every other node is left un-analysed. */
function analysed(byNode: Record<number, Classification>): Map<number, PositionAnalysis> {
  const map = new Map<number, PositionAnalysis>();
  for (const [id, classification] of Object.entries(byNode)) {
    map.set(Number(id), {
      fen: '8/8/8/8/8/8/8/8 w - - 0 1',
      depth: 12,
      candidates: [],
      context: {
        played: {
          san: 'x',
          uci: 'e2e4',
          win_prob_before: 0.5,
          win_prob_after: 0.4,
          delta: -0.1,
          classification,
          accuracy: 50,
        },
        counterfactual: null,
      },
      explanations: {},
    });
  }
  return map;
}

/** Every ply analysed, so nothing in a test is "pending" by accident. */
function allAnalysed(plies: number, mistakes: Record<number, Classification>): Map<number, PositionAnalysis> {
  const byNode: Record<number, Classification> = {};
  for (let id = 1; id <= plies; id += 1) byNode[id] = mistakes[id] ?? 'good';
  return analysed(byNode);
}

describe('finding the next mistake', () => {
  const tree = mainline(8);
  const analyses = allAnalysed(8, { 3: 'inaccuracy', 6: 'blunder' });

  it('walks forward from the current move', () => {
    expect(findMistake(tree, analyses, 0, 'next')).toEqual({ target: 3, pending: false });
    expect(findMistake(tree, analyses, 3, 'next')).toEqual({ target: 6, pending: false });
  });

  it('does not offer the move you are already on', () => {
    // Standing on the blunder, "next" is the *next* one, not this one again.
    expect(findMistake(tree, analyses, 6, 'next').target).toBeNull();
  });

  it('stops at the end of the game rather than wrapping to the start', () => {
    expect(findMistake(tree, analyses, 7, 'next')).toEqual({ target: null, pending: false });
  });

  it('counts every classification that lost something, and nothing else', () => {
    const kinds = allAnalysed(4, { 1: 'great', 2: 'best', 3: 'miss', 4: 'mistake' });
    expect(findMistake(mainline(4), kinds, 0, 'next').target).toBe(3);
  });
});

describe('finding the previous mistake', () => {
  const tree = mainline(8);
  const analyses = allAnalysed(8, { 3: 'inaccuracy', 6: 'blunder' });

  it('walks backward from the current move', () => {
    expect(findMistake(tree, analyses, 8, 'prev')).toEqual({ target: 6, pending: false });
    expect(findMistake(tree, analyses, 6, 'prev')).toEqual({ target: 3, pending: false });
  });

  it('stops at the start of the game rather than wrapping to the end', () => {
    expect(findMistake(tree, analyses, 3, 'prev')).toEqual({ target: null, pending: false });
    expect(findMistake(tree, analyses, 0, 'prev')).toEqual({ target: null, pending: false });
  });
});

describe('moves the sweep has not reached', () => {
  const tree = mainline(6);

  it('reports "not known yet" rather than "there are none"', () => {
    // Nothing analysed at all: the honest answer is that we cannot say, and the
    // interface says exactly that instead of a dead button with no reason.
    expect(findMistake(tree, new Map(), 0, 'next')).toEqual({ target: null, pending: true });
  });

  it('still offers a mistake it can see, and admits the gap before it', () => {
    // Move 5 is a blunder; moves 1–4 are unknown. Jumping to 5 is useful and
    // true — but it is not necessarily the *first* mistake, so `pending` says so.
    expect(findMistake(tree, analysed({ 5: 'blunder' }), 0, 'next')).toEqual({
      target: 5,
      pending: true,
    });
  });

  it('is not pending when every move in the way has a verdict', () => {
    const analyses = allAnalysed(6, { 5: 'blunder' });
    expect(findMistake(tree, analyses, 0, 'next')).toEqual({ target: 5, pending: false });
  });

  it('does not count the root as a gap: it is a position, not a move', () => {
    const analyses = allAnalysed(6, {});
    expect(findMistake(tree, analyses, 2, 'prev')).toEqual({ target: null, pending: false });
  });
});

describe('inside a variation', () => {
  /*
   * root ─ 1 ─ 2 ─ 3   (mainline, 3 is a blunder)
   *        └── 4 ─ 5   (variation, 5 is a blunder)
   */
  const tree: GameTree = {
    root: 0,
    nodes: [
      node(0, null, [1]),
      node(1, 0, [2, 4]),
      node(2, 1, [3]),
      node(3, 2, []),
      node(4, 1, [5]),
      node(5, 4, []),
    ],
  };
  const analyses = analysed({ 1: 'good', 2: 'good', 3: 'blunder', 4: 'good', 5: 'blunder' });

  it('follows the line the current node is on, not the mainline', () => {
    expect(findMistake(tree, analyses, 4, 'next').target).toBe(5);
    expect(findMistake(tree, analyses, 2, 'next').target).toBe(3);
  });

  it('walks back out of the variation through its own ancestors', () => {
    expect(findMistake(tree, analyses, 5, 'prev').target).toBeNull();
    expect(findMistake(tree, analyses, 3, 'prev').target).toBeNull();
  });
});

describe('degenerate input', () => {
  it('has nothing to say without a tree', () => {
    expect(findMistake(null, new Map(), 0, 'next')).toEqual({ target: null, pending: false });
  });

  it('has nothing to say about a node the tree does not contain', () => {
    expect(findMistake(mainline(3), new Map(), 99, 'next')).toEqual({
      target: null,
      pending: false,
    });
  });
});
