/**
 * The mark for "turn the board around", drawn here rather than picked out of an
 * icon set.
 *
 * Three lucide glyphs have now been tried in this one button and all three were
 * misread. `FlipVertical2` is a dashed axis between two arrows — a mark for
 * mirroring an *object in an editor*, which is a concept the person in front of
 * a chessboard does not have. `RotateCcw` is worse, because it is not ambiguous
 * at all: a circular arrow is the universal glyph for reload/repeat, in every
 * toolbar the user has ever seen, and no amount of neighbouring context
 * overrides a symbol that already means something else. The remaining
 * candidates were variations on those two.
 *
 * The problem is not that the right lucide icon had not been found yet. A
 * general icon set has marks for general ideas, and "flip the board" is not one:
 * it is a specific, physical, *drawable* thing — a board that gets turned
 * around, so that the end that was far is near. So it is drawn, the way
 * `MoveBadge` draws its nine classification marks and for the same reason: a
 * mark this app needs and no library has is cheaper to author than to shop for.
 *
 * **What is drawn.** A board, and two arrows running past it in opposite
 * directions — one over the top going right, one under the bottom going left.
 * That pair is a half-turn about the board's centre, which is exactly what
 * flipping a chessboard is: not a mirror, but the same board seen from the other
 * side of the table. Neither arrow is an arc, which is the whole point — an arc
 * is a circle is a reload — and the two are 180°-rotationally symmetric about
 * the centre of the grid, so the drawing performs the operation it names.
 *
 * The board is drawn wider than it is tall. A chessboard is square, but a square
 * outline with arrows past it reads as "some box", whereas a foreshortened one
 * reads as a board seen across a table, which is the view this button changes.
 * A checkered fill was tried and abandoned: two filled quadrants inside a 16px
 * icon are four pixels each, and they turn the mark into a blot rather than
 * saying "chess". A horizontal midline was tried too, and made it a table.
 *
 * **Sized and stroked to lucide's system, not to `MoveBadge`'s.** Its four
 * neighbours in that toolbar are lucide chevrons, so this mark is authored in
 * lucide's `0 0 24 24` box at stroke width 2 with round caps and joins, and
 * takes its size from the `[&_svg]:size-4` rule on `Button` like any other icon
 * there. `MoveBadge` has its own stroke system because its marks sit alone on a
 * coloured disc; this one has to belong to a row.
 */
export function FlipBoardIcon({ className }: { className?: string }): React.JSX.Element {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {/* The board. `rx` matches the softness of the chevrons' joins. */}
      <rect x="3" y="6.5" width="18" height="11" rx="2" />
      {/* Over the top, going right. */}
      <path d="M 8 2.5 H 14.5" />
      <path d="M 12.5 0.5 L 14.5 2.5 L 12.5 4.5" />
      {/* Under the bottom, going left: the same arrow turned through 180°
          about (12, 12), which is the motion the whole mark is about. */}
      <path d="M 16 21.5 H 9.5" />
      <path d="M 11.5 19.5 L 9.5 21.5 L 11.5 23.5" />
    </svg>
  );
}
