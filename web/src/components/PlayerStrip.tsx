import type { CapturedGroup, Color } from '../chess/rules.ts';

export interface PlayerStripProps {
  /** Which side this player has. */
  color: Color;
  /** Their name from the PGN, or the word for the colour when it carried none. */
  name: string;
  /** What this player has taken, grouped by role (`chess/rules.ts`). */
  captured: readonly CapturedGroup[];
  /**
   * This player's material lead in pawns. Only a positive number is drawn — see
   * below — so the side that is behind may be passed its negative lead or zero.
   */
  lead: number;
  /** Placement, so the strip can be laid over exactly the board's width. */
  style?: React.CSSProperties;
}

/**
 * One player, on the edge of the board nearest their own pieces.
 *
 * The topbar used to be the only place the two names appeared, side by side,
 * which answers "whose game is this" and not the question anyone actually has
 * in front of a board: **which of these two am I looking at from below.** Two
 * names in a row cannot answer that, because the row has no top and bottom. A
 * strip does, by being in the place it is talking about — so the strips follow
 * the orientation and swap when the board is flipped.
 *
 * **Nothing here states the colour a second time.** There was a small light or
 * dark disc before the name, on the argument that a name may be unfamiliar and a
 * disc reads before any word does. It is gone: the strip's *position* already
 * says which side this is, and so does the row of captured pieces underneath it,
 * which is the opposite colour by definition. A disc that repeats what the
 * layout has already said is one more mark to look past.
 *
 * **Two rows, not one.** The name and the tally are different kinds of fact —
 * one identifies the player, the other is a running score — and a single row put
 * them in competition for the same horizontal space, with the name truncating to
 * make room for pieces. Stacked, the name gets the whole width and the tally
 * gets its own line. The strip is a caption for the board, not a panel, so both
 * lines are set as tight as they go: `leading-none` on the name, no vertical
 * gap beyond a hairline, and pieces a line of small text tall. The board pays
 * for every pixel spent here out of its own height (`App.tsx`).
 *
 * **The advantage is shown once, on the side that has it**, immediately after
 * the pieces rather than pushed to the far end of the strip — it is the total of
 * the row it sits beside, and a number a board's width away from the thing it
 * counts is a number you have to go and fetch. Two numbers, one of them
 * negative, would be the same fact written twice and then negated; level
 * material shows nothing at all on either strip, because "+0" is a number that
 * has to be read to learn there was nothing to say.
 */
export function PlayerStrip({
  color,
  name,
  captured,
  lead,
  style,
}: PlayerStripProps): React.JSX.Element {
  /** The pieces this player took are the *other* colour's. */
  const takenColor: Color = color === 'white' ? 'black' : 'white';

  return (
    <div className="flex w-full min-w-0 flex-none flex-col gap-px" style={style}>
      <span className="truncate text-sm leading-none font-medium">{name}</span>

      {/* `min-h-4` so the second line is reserved from the opening position
          onwards. Without it the strip would be one line tall until the first
          capture and two lines tall afterwards, and since the board is sized
          from what the strips leave (`App.tsx`), the whole board would jump
          the first time a pawn came off. */}
      <div className="flex min-h-4 items-center gap-1.5">
        {/*
          chessground's embedded piece SVGs, addressed the way its own stylesheet
          addresses them — `.cg-wrap piece.pawn.black` — which is why there is a
          `cg-wrap` here and `<piece>` elements inside it. `PromotionPicker` does
          the same thing for the same reason: it is how these images are reused
          without a second copy of the artwork in the repository.

          `cg-inline` undoes the two things chessground assumes about that
          wrapper, namely that it is the board and therefore fills its parent.
          `cg-captured` is the other half of the borrowing: these are a tally,
          not a position, so they are drawn as flat silhouettes in the theme's
          own ink rather than as board pieces. Both live in `board.css`, where
          the reasoning is.

          Each piece then gets its own small relative box, because
          `.cg-wrap piece` is absolutely positioned at 12.5% — one square of a
          board — and here the box *is* the square.
        */}
        {/* Never allowed to shrink: a squeezed row would clip the pieces rather
            than drop them, and half a bishop is worse than a shorter tally. */}
        <span className="cg-wrap cg-inline cg-captured flex shrink-0 items-center gap-1.5">
          {captured.map((group) => (
            <span key={group.role} className="flex shrink-0 items-center">
              {Array.from({ length: group.count }, (_, index) => (
                <span
                  key={index}
                  /* Overlapped by two fifths, the way a taken-piece row is drawn
                     everywhere: five pawns then cost the width of three, and the
                     row stays a row rather than becoming a second move list. */
                  className="relative block size-4 shrink-0 not-first:-ml-1.5"
                >
                  <PieceTag
                    className={`${group.role} ${takenColor}`}
                    style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }}
                  />
                </span>
              ))}
            </span>
          ))}
        </span>

        {lead > 0 && (
          <span className="shrink-0 font-mono text-xs leading-none font-medium tabular-nums text-muted-foreground">
            +{lead}
          </span>
        )}
      </div>
    </div>
  );
}

/** `piece` is chessground's custom element; React needs telling it is a tag. */
const PieceTag = 'piece' as unknown as React.ElementType<{
  className: string;
  style: React.CSSProperties;
}>;
