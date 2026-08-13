import { describe, expect, it } from 'vitest';
import { deepestOpening } from './opening.ts';
import type { GameTree, Node, OpeningInfo } from '../api/types.ts';

function node(id: number, parent: number | null, opening: OpeningInfo | null): Node {
  return {
    id,
    parent,
    children: parent === null ? [] : [],
    san: parent === null ? null : `m${id}`,
    uci: parent === null ? null : 'e2e4',
    fen: '',
    analysis: null,
    opening,
  };
}

const opening = (eco: string, name: string, matched_plies: number): OpeningInfo => ({
  eco,
  name,
  matched_plies,
});

/**
 * A line that is named for three plies and then stops — the shape of every real
 * game, and the one the caption has to keep rendering after the names run out.
 */
const philidor = opening('C41', 'Philidor Defense', 3);
const namedLine: GameTree = {
  root: 0,
  nodes: [
    node(0, null, null),
    node(1, 0, opening('B00', 'King’s Pawn Game', 1)),
    node(2, 1, opening('C20', 'King’s Pawn Game', 2)),
    node(3, 2, philidor),
    node(4, 3, null),
    node(5, 4, null),
    node(6, 5, null),
  ],
};

describe('deepestOpening', () => {
  it('uses the node’s own opening while the line is still named', () => {
    expect(deepestOpening(namedLine, 3)).toEqual(philidor);
    expect(deepestOpening(namedLine, 1)).toEqual(opening('B00', 'King’s Pawn Game', 1));
  });

  it('keeps the last named ancestor once the names stop', () => {
    // Three plies past the end of the table, and still the same opening.
    expect(deepestOpening(namedLine, 4)).toEqual(philidor);
    expect(deepestOpening(namedLine, 5)).toEqual(philidor);
    expect(deepestOpening(namedLine, 6)).toEqual(philidor);
  });

  it('is null at the root, which no table names', () => {
    expect(deepestOpening(namedLine, 0)).toBeNull();
  });

  it('is null when nothing on the path is named at all', () => {
    const unnamed: GameTree = {
      root: 0,
      nodes: [node(0, null, null), node(1, 0, null), node(2, 1, null)],
    };
    expect(deepestOpening(unnamed, 2)).toBeNull();
  });

  it('walks the selected branch, not the mainline', () => {
    const sicilian = opening('B20', 'Sicilian Defense', 2);
    const branching: GameTree = {
      root: 0,
      nodes: [
        node(0, null, null),
        node(1, 0, opening('B00', 'King’s Pawn Game', 1)),
        node(2, 1, opening('C20', 'King’s Pawn Game', 2)), // 1... e5
        node(3, 1, sicilian), // 1... c5, a variation
        node(4, 3, null), // unnamed continuation of the variation
      ],
    };
    expect(deepestOpening(branching, 4)).toEqual(sicilian);
  });

  it('is null for a missing tree or no selection', () => {
    expect(deepestOpening(null, 3)).toBeNull();
    expect(deepestOpening(namedLine, null)).toBeNull();
    expect(deepestOpening(namedLine, 99)).toBeNull();
  });

  it('does not hang on a tree whose parent links form a cycle', () => {
    const cyclic: GameTree = {
      root: 0,
      nodes: [node(0, 2, null), node(1, 0, null), node(2, 1, null)],
    };
    expect(deepestOpening(cyclic, 2)).toBeNull();
  });
});
