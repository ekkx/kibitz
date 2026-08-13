import type { GameTree, Node, PositionAnalysis } from '../api/types.ts';
import { isMistake } from './format.ts';

/**
 * "Take me to the next thing that went wrong."
 *
 * This is the question the whole app exists to answer, and until now answering
 * it meant scrolling the move list looking for a `??` by eye. The jump is a
 * *query over the annotated move list*, not another way to step, so it lives
 * here — beside the glyphs it hunts for (`CLASSIFICATION_GLYPH`) and on the same
 * definition of "mistake" the board draws in red (`isMistake`). There is exactly
 * one definition of a mistake in this app and this file does not add a second.
 *
 * Three decisions are worth stating, because each has an obvious wrong answer:
 *
 *   1. **The line, not the tree.** The walk follows the line the *current node
 *      is on*: up its parents to the root, then forward along `children[0]`.
 *      On the mainline that is exactly the mainline, which is what the feature
 *      is for; inside a variation it is that variation, which is the only
 *      reading that keeps "next" meaning "the move after this one". Jumping
 *      into a sibling branch would move the user somewhere they were not
 *      looking and could not walk back to with →.
 *
 *   2. **No wrapping.** At the last mistake the control is dead, not looped
 *      round to the first. A user who cannot tell "there are none left" from
 *      "it went back to the start" has to verify every jump by reading the move
 *      number, which costs more than the feature saves.
 *
 *   3. **Unanalysed is not "clean".** A move is only known to be a mistake once
 *      its node has an analysis, so a game the sweep has not reached has no
 *      mistakes *findable*, which is a different fact from having none. That
 *      difference is reported (`pending`) rather than being flattened into a
 *      silently disabled button, so the interface can say which of the two it
 *      means.
 */

export type MistakeDirection = 'next' | 'prev';

export interface MistakeJump {
  /** The node to select, or null when there is nothing to jump to. */
  target: number | null;
  /**
   * Whether the answer could still change as analysis arrives — true when the
   * walk crossed a move with no analysis before it found anything.
   *
   * With a target this means "there may be a nearer mistake we cannot see yet",
   * and the jump is still worth offering: a known mistake is a known mistake.
   * Without one it is the whole answer — "not analysed that far", never "your
   * game is clean from here".
   */
  pending: boolean;
}

const NOTHING: MistakeJump = { target: null, pending: false };

/**
 * The next / previous move on the current line whose analysis calls it a
 * mistake, walking outward from `currentId` and stopping at the end of the line.
 */
export function findMistake(
  tree: GameTree | null,
  analyses: ReadonlyMap<number, PositionAnalysis>,
  currentId: number,
  direction: MistakeDirection,
): MistakeJump {
  if (!tree) return NOTHING;
  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const line = lineThrough(byId, tree.root, currentId);
  const from = line.indexOf(currentId);
  if (from < 0) return NOTHING;

  const step = direction === 'next' ? 1 : -1;
  let pending = false;

  for (let index = from + step; index >= 0 && index < line.length; index += step) {
    const nodeId = line[index]!;
    // The root is a position, not a move, so it can be neither a mistake nor a
    // gap in what we know. Skipping it here is what lets the backward walk run
    // all the way to the start of the game without reporting the opening
    // position as un-analysed.
    if (nodeId === tree.root) continue;

    const played = analyses.get(nodeId)?.context?.played;
    if (!played) {
      pending = true;
      continue;
    }
    if (isMistake(played.classification)) return { target: nodeId, pending };
  }

  return { target: null, pending };
}

/**
 * The line `nodeId` sits on, root first: its ancestors in order, then the
 * mainline continuation from it.
 *
 * `seen` guards the forward walk against a tree that points at itself. The
 * server does not produce one, but this loop is the only place in the app that
 * follows `children[0]` without a bound, and a malformed tree should give a
 * short line rather than a hung tab.
 */
function lineThrough(byId: ReadonlyMap<number, Node>, rootId: number, nodeId: number): number[] {
  const node = byId.get(nodeId);
  if (!node) return [];

  const ancestors: number[] = [];
  for (let walker = node; ; ) {
    ancestors.push(walker.id);
    if (walker.id === rootId || walker.parent === null) break;
    const parent = byId.get(walker.parent);
    if (!parent) break;
    walker = parent;
  }
  ancestors.reverse();

  const seen = new Set(ancestors);
  for (let walker = node; ; ) {
    const nextId = walker.children[0];
    if (nextId === undefined || seen.has(nextId)) break;
    const next = byId.get(nextId);
    if (!next) break;
    ancestors.push(nextId);
    seen.add(nextId);
    walker = next;
  }

  return ancestors;
}
