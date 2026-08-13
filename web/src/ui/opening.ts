import type { GameTree, Node, OpeningInfo } from '../api/types.ts';

/**
 * Which opening name to show for a selected position.
 *
 * The ECO table names a *position*, so a node only carries an `opening` while
 * the exact position is still in the table. A game that is five moves past the
 * end of a named line still belongs to that opening, so the name to show is the
 * one on the deepest ancestor that has one — the selected node itself when it
 * is still named, otherwise the last named node on the path back to the root.
 *
 * Running out of names is not the same as leaving theory, so nothing here (and
 * nothing that renders the result) may frame it that way: when no node on the
 * path has a name, the answer is simply `null` and the UI stays quiet.
 */
export function deepestOpening(tree: GameTree | null, nodeId: number | null): OpeningInfo | null {
  if (!tree || nodeId === null) return null;

  const byId = new Map<number, Node>();
  for (const node of tree.nodes) byId.set(node.id, node);

  // Bounded by the node count so a malformed tree cannot spin here forever.
  let walker = byId.get(nodeId);
  for (let steps = 0; walker && steps <= tree.nodes.length; steps++) {
    if (walker.opening) return walker.opening;
    walker = walker.parent === null ? undefined : byId.get(walker.parent);
  }
  return null;
}
