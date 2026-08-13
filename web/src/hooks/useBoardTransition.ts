import { useState } from 'react';
import type { GameTree, Node } from '../api/types.ts';

/**
 * Whether the board should slide or jump, and what ply it just crossed.
 *
 * Navigating a game tree is two different gestures wearing one control. Pressing
 * ▶ asks "what is the next move?", and the answer is a piece moving — if the
 * board simply repaints, the user has to diff two positions in their head to
 * find out what happened, which is exactly the work the board exists to do for
 * them. Clicking move 34 from move 6 asks "show me that position", and there is
 * no move to show: sliding a dozen pieces along a dozen unrelated paths is not
 * an answer, it is a smear that hides the position that was actually asked for.
 *
 * So the rule is narrow and structural rather than aesthetic: **animate exactly
 * one ply of the tree, in either direction, and nothing else.** The new node is
 * a child of the old one (▶, →, playing a move, clicking the next move in the
 * list), or the old one is a child of the new (◀, ←, clicking the move above).
 * Everything else — ⏮ / ⏭ / Home / End, a distant node in the move list,
 * switching to a sibling variation, opening a game, resuming one on reload — is
 * a jump, because between those two positions there is no single movement to
 * draw.
 *
 * The hook owns that decision for the whole app, and reports the ply it found
 * alongside the duration. The `move` field has no consumer here — `App` only
 * needs `animationMs` — and exists so that everything keyed on "a single ply
 * just happened" (move sounds, next) reads it off the same detector rather than
 * growing a second, subtly different answer to the same question.
 */
export interface BoardTransition {
  /** ms to give chessground for this render; 0 means jump. */
  animationMs: number;
  /**
   * The move traversed by a single-ply transition, if there was one. Null on a
   * jump, and on every render that is not a transition at all.
   *
   * `san` is the SAN of the ply that was *crossed*, which is the new node's SAN
   * going forward and the old node's SAN coming back — the same move either
   * way, so stepping onto a move and back off it names it identically.
   * `forward` says which way it was crossed.
   *
   * `landedFen` is the position now on the board. It is what a consumer wants
   * whenever the interesting fact is the state arrived at rather than the move
   * itself: a SAN describes a move as it was played *forwards*, so reading it
   * on the way back gets the tense wrong — stepping back off `Rd8#` is not a
   * checkmate, it is the position ceasing to be one.
   */
  move: { san: string; forward: boolean; landedFen: string } | null;
}

/**
 * How long chessground is given to slide the piece for one step of navigation.
 *
 * Short enough that the board never feels like it is being waited for — someone
 * walking a game with the arrow key is reading, and a step that outlasts the
 * decision to take the next one turns reading into queueing. Long enough that
 * the piece is seen travelling rather than teleporting: under about 150ms the
 * eye takes the movement as a cut and the animation buys nothing, and past about
 * 250ms navigation acquires a drag that gets worse with every press.
 *
 * Deliberately well under `REPLAY_MOVE_MS` (420ms). The replay is a
 * presentation — it plays a line *at* the user, who is not doing anything else,
 * and it wants each move to land. Navigation is the user's own hand moving, and
 * it should feel like the board keeping up with them.
 */
export const NAV_MOVE_MS = 180;

/** No movement to draw: repaint the board and say nothing about a move. */
const STILL: BoardTransition = { animationMs: 0, move: null };

/** What the hook last reported, and the state it reported it for. */
interface Seen {
  /** Null before any game is on the board — see the `tree === null` reset. */
  id: number | null;
  enabled: boolean;
  transition: BoardTransition;
}

const BLANK: Seen = { id: null, enabled: false, transition: STILL };

/**
 * The decision itself, as a pure function of the tree and the two node ids.
 *
 * Adjacency is tested through `parent` alone rather than by also scanning
 * `children`: the tree is a tree, so `child.parent === parent.id` and
 * `parent.children.includes(child.id)` are the same fact, and asking the cheap
 * half twice is both the complete answer and O(1) per node.
 */
export function boardTransition(
  tree: GameTree | null,
  fromId: number | null,
  toId: number,
): BoardTransition {
  // No previous position (first render of a game), or the board did not move.
  if (!tree || fromId === null || fromId === toId) return STILL;

  const from = findNode(tree, fromId);
  const to = findNode(tree, toId);
  // A node id from a tree that no longer contains it is not a transition we can
  // reason about, so it is a jump — which is also the safe answer.
  if (!from || !to) return STILL;

  // Both directions land on `to`, which is why it is the one FEN reported.
  if (to.parent === from.id) return step(to.san, true, to.fen);
  if (from.parent === to.id) return step(from.san, false, to.fen);

  // Siblings, cousins, or opposite ends of the game: no one movement connects
  // them, whatever the distance.
  return STILL;
}

/**
 * A crossed ply, if it has a name. Only the root has a null SAN and the root can
 * be neither stepped onto forwards nor off backwards, so this never rejects a
 * real transition — but a tree that disagrees gets a jump rather than an
 * animation attached to a move nothing can identify.
 */
function step(san: string | null, forward: boolean, landedFen: string): BoardTransition {
  if (san === null) return STILL;
  return { animationMs: NAV_MOVE_MS, move: { san, forward, landedFen } };
}

function findNode(tree: GameTree, id: number): Node | undefined {
  // Linear, and deliberately not indexed: this runs twice per *navigation*, not
  // per render, over a tree the size of one game.
  return tree.nodes.find((node) => node.id === id);
}

/**
 * `enabled` is false while something else owns the board — today, a running
 * counterfactual replay, which drives its own position at its own duration. A
 * disabled detector still tracks where the user went (the keyboard keeps
 * working underneath a replay), it just reports every step as a jump, and it
 * reports the first step *after* the board is handed back as a jump too: at
 * that moment the board is showing the end of a replayed line, and there is no
 * ply of the game between that position and wherever the user now is.
 */
export function useBoardTransition(
  tree: GameTree | null,
  currentId: number,
  enabled: boolean,
): BoardTransition {
  /**
   * Kept in state rather than in a ref, even though a ref is the obvious shape
   * for "the value I saw last time". This is read back on the same render that
   * writes it, and under `StrictMode` React renders twice and keeps only the
   * second pass — a ref mutated during the first pass would leave the second
   * one comparing `currentId` against itself, concluding nothing had changed,
   * and swallowing every animation in development. A render-phase `setState`
   * re-runs the component before anything is committed, so both passes settle
   * on the same answer.
   */
  const [seen, setSeen] = useState<Seen>(BLANK);

  // No game on the board. Node ids are per-tree, so the id we are holding means
  // nothing once the next game arrives; dropping it here is what guarantees the
  // first render of a game is a jump even when the new root happens to reuse the
  // old node's id. (A tree is only ever replaced wholesale via `null` — `reset`
  // clears it before `open` sets one — so this is the one place a game ends.)
  if (tree === null) {
    if (seen.id !== null) setSeen(BLANK);
    return STILL;
  }

  if (seen.id !== currentId || seen.enabled !== enabled) {
    const moved = seen.id !== currentId;
    // `seen.enabled` as well as `enabled`: the board has to have been ours both
    // before and after the step for the step to be one of ours.
    const transition =
      moved && enabled && seen.enabled ? boardTransition(tree, seen.id, currentId) : STILL;
    const next: Seen = { id: currentId, enabled, transition };
    setSeen(next);
    return next.transition;
  }

  // Re-renders at the same node — an analysis arriving, arrows changing, the
  // board being flipped — keep reporting the transition that got us here. They
  // do not change the position, so chessground has nothing to animate either
  // way, and re-deciding on every render would mean the answer depended on how
  // many times the component happened to render.
  return seen.transition;
}
