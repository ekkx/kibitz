import { describe, expect, it } from 'vitest';
import { NAV_MOVE_MS, boardTransition } from './useBoardTransition.ts';
import { REPLAY_MOVE_MS } from './useReplay.ts';
import type { GameTree, Node } from '../api/types.ts';

const node = (
  id: number,
  parent: number | null,
  children: number[],
  san: string | null,
): Node => ({
  id,
  parent,
  children,
  san,
  uci: null,
  // Opaque per-node marker: the assertions care that `landedFen` is the FEN
  // of the node arrived at, not that it parses as a position.
  fen: `fen${id}`,
  analysis: null,
  opening: null,
});

/**
 * 1.e4 e5 2.Nf3, with 1...c5 as a sibling variation off the same node — so the
 * fixture carries both kinds of adjacency (parent/child) and both kinds of
 * non-adjacency (sibling, and two plies apart on one line).
 */
const TREE: GameTree = {
  root: 0,
  nodes: [
    node(0, null, [1], null),
    node(1, 0, [2, 4], 'e4'),
    node(2, 1, [3], 'e5'),
    node(3, 2, [], 'Nf3'),
    node(4, 1, [], 'c5'),
  ],
};

const STILL = { animationMs: 0, move: null };

describe('boardTransition', () => {
  it('animates a step onto a child, and names the move stepped onto', () => {
    expect(boardTransition(TREE, 1, 2)).toEqual({
      animationMs: NAV_MOVE_MS,
      move: { san: 'e5', forward: true, landedFen: 'fen2' },
    });
  });

  it('animates a step back to the parent, and names the move stepped off', () => {
    // The same ply as above, crossed the other way: `san` identifies the move,
    // `forward` identifies the direction, so a consumer can tell taking a move
    // back from playing it without a second lookup in the tree. `landedFen` is
    // the parent's — both directions report where the board ended up, which is
    // the half that does not invert when the ply is crossed backwards.
    expect(boardTransition(TREE, 2, 1)).toEqual({
      animationMs: NAV_MOVE_MS,
      move: { san: 'e5', forward: false, landedFen: 'fen1' },
    });
  });

  it('animates the first move of the game, off the root and back onto it', () => {
    expect(boardTransition(TREE, 0, 1)).toEqual({
      animationMs: NAV_MOVE_MS,
      move: { san: 'e4', forward: true, landedFen: 'fen1' },
    });
    expect(boardTransition(TREE, 1, 0)).toEqual({
      animationMs: NAV_MOVE_MS,
      move: { san: 'e4', forward: false, landedFen: 'fen0' },
    });
  });

  it('jumps across more than one ply, however short the hop', () => {
    // ⏭ / End / clicking a distant move: two plies is already two movements,
    // and there is no single one to draw.
    expect(boardTransition(TREE, 1, 3)).toEqual(STILL);
    expect(boardTransition(TREE, 3, 1)).toEqual(STILL);
    expect(boardTransition(TREE, 0, 3)).toEqual(STILL);
  });

  it('jumps between siblings, which are one click but not one move apart', () => {
    // 1...e5 and 1...c5 share a parent. Sliding between them would draw a
    // movement the game never contained.
    expect(boardTransition(TREE, 2, 4)).toEqual(STILL);
    expect(boardTransition(TREE, 4, 2)).toEqual(STILL);
  });

  it('does nothing when the board did not move', () => {
    expect(boardTransition(TREE, 2, 2)).toEqual(STILL);
  });

  it('jumps when there is no previous position — the first render of a game', () => {
    expect(boardTransition(TREE, null, 0)).toEqual(STILL);
    expect(boardTransition(TREE, null, 7)).toEqual(STILL);
  });

  it('jumps without a tree', () => {
    expect(boardTransition(null, 1, 2)).toEqual(STILL);
  });

  it('jumps when either node is not in this tree', () => {
    // A stale id from a previous game, or a node the server has not sent yet.
    expect(boardTransition(TREE, 99, 1)).toEqual(STILL);
    expect(boardTransition(TREE, 1, 99)).toEqual(STILL);
  });

  it('refuses to animate a ply it cannot name', () => {
    // Defensive: only the root has a null SAN and the root is unreachable as a
    // named ply, so a tree that produces one is malformed and gets a jump
    // rather than an animation attached to an unidentifiable move.
    const broken: GameTree = { root: 0, nodes: [node(0, null, [1], null), node(1, 0, [], null)] };
    expect(boardTransition(broken, 0, 1)).toEqual(STILL);
  });
});

describe('NAV_MOVE_MS', () => {
  it('is quick enough to keep up with a hand on the arrow key', () => {
    // The band either side of which the animation stops paying for itself:
    // below it the movement reads as a cut, above it navigation feels held up.
    expect(NAV_MOVE_MS).toBeGreaterThanOrEqual(150);
    expect(NAV_MOVE_MS).toBeLessThanOrEqual(250);
  });

  it('is faster than the replay, which is a presentation rather than navigation', () => {
    expect(NAV_MOVE_MS).toBeLessThan(REPLAY_MOVE_MS);
  });
});
